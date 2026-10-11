//! Formula calculation over a workbook (ADR 0007): lazy, revision-cached, with an explicit
//! work stack for deep chains and a cell-visit budget for the UI thread.

use crate::formulas::is_formula;
use crate::workbook::same_name;
use crate::{CellRef, Computed, RowBlock, RowPositions, SheetId, TableSource, Workbook};
use formula::{
    evaluate, literal, parse, result_text, unknown_functions, Context, ErrorKind, ExactIndex, Expr,
    Interrupt, Value,
};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Cell visits the UI thread spends on formulas per frame; the rest goes to a job.
pub const UI_BUDGET: u64 = 200_000;
/// Formula cells evaluated in place, one inside another, before the driver's work stack
/// takes over.
const MAX_DEPTH: usize = 64;
/// Rows read at a time for a reference area.
const CHUNK: u64 = 65_536;
/// Cell visits between cancel checks.
const CANCEL_EVERY: u64 = 4_096;
/// Columns of a whole-row reference: Excel's sheet width.
const SHEET_COLS: u32 = 16_384;

type Key = (SheetId, CellRef);
/// A lookup area: sheet, rows, columns, and whether offsets count columns.
type AreaKey = (SheetId, Range<u64>, Range<u32>, bool);

/// Cells' values, as a job computes them for [`Workbook::store_values`].
pub type CellValues = Vec<((SheetId, CellRef), Value)>;

/// Results, parses, and exact-lookup indexes, valid for one workbook revision.
#[derive(Default)]
pub(crate) struct Calc {
    revision: u64,
    values: HashMap<Key, Value>,
    parsed: HashMap<Key, Option<Arc<Expr>>>,
    indexes: HashMap<AreaKey, Arc<ExactIndex>>,
}

impl Calc {
    fn sync(&mut self, revision: u64) {
        if self.revision != revision {
            self.values.clear();
            self.parsed.clear();
            self.indexes.clear();
            self.revision = revision;
        }
    }
}

/// How much evaluation may cost: each cell visited costs 1.
pub struct Budget {
    left: u64,
    cancel: Option<Arc<AtomicBool>>,
    since_check: u64,
}

impl Budget {
    /// At most `n` cell visits, then [`Interrupt::Budget`].
    pub fn cells(n: u64) -> Self {
        Self {
            left: n,
            cancel: None,
            since_check: 0,
        }
    }

    /// No limit; [`Interrupt::Cancelled`] once `cancel` is set (checked every 4,096 visits).
    pub fn unlimited(cancel: Arc<AtomicBool>) -> Self {
        Self {
            left: u64::MAX,
            cancel: Some(cancel),
            since_check: 0,
        }
    }

    fn spend(&mut self, n: u64) -> Result<(), Interrupt> {
        if self.left < n {
            self.left = 0;
            return Err(Interrupt::Budget);
        }
        self.left -= n;
        self.since_check += n;
        if self.since_check >= CANCEL_EVERY {
            self.since_check = 0;
            if self
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
            {
                return Err(Interrupt::Cancelled);
            }
        }
        Ok(())
    }
}

/// A formula cell to evaluate, with its position (row among all rows, view column).
#[derive(Clone, Copy)]
struct Work {
    key: Key,
    pos: (u64, u32),
}

impl Workbook {
    /// The value of a cell: a formula's result, or what its raw text reads as.
    pub fn value_of(
        &mut self,
        sheet: SheetId,
        at: CellRef,
        budget: &mut Budget,
    ) -> Result<Value, Interrupt> {
        let Some(s) = self.sheet(sheet) else {
            return Ok(Value::Error(ErrorKind::Ref));
        };
        match (s.table.position_of(at.row), s.table.col_of(at.col)) {
            (Some(row), Some(col)) => self.value_at(sheet, at, (row, col), budget),
            _ => Ok(Value::Empty),
        }
    }

    /// [`Self::value_of`] for a cell whose position is known.
    pub(crate) fn value_at(
        &mut self,
        sheet: SheetId,
        at: CellRef,
        pos: (u64, u32),
        budget: &mut Budget,
    ) -> Result<Value, Interrupt> {
        let revision = self.revision();
        self.calc.sync(revision);
        if let Some(v) = self.calc.values.get(&(sheet, at)) {
            return Ok(v.clone());
        }
        let mut calc = std::mem::take(&mut self.calc);
        let result = drive(
            self,
            &mut calc,
            Work {
                key: (sheet, at),
                pos,
            },
            budget,
        );
        self.calc = calc;
        result
    }

    /// Evaluate `cells` with no budget (a job on a snapshot): `None` once `cancel` is set.
    /// `done` counts cells evaluated.
    pub fn evaluate_all(
        &mut self,
        cells: &[(SheetId, CellRef)],
        cancel: Arc<AtomicBool>,
        done: &AtomicU64,
    ) -> Option<CellValues> {
        let mut budget = Budget::unlimited(cancel.clone());
        let mut positions: HashMap<SheetId, RowPositions> = HashMap::new();
        let mut out = Vec::with_capacity(cells.len());
        for &(sheet, at) in cells {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let Some(s) = self.sheet(sheet) else {
                out.push(((sheet, at), Value::Error(ErrorKind::Ref)));
                continue;
            };
            let rows = positions
                .entry(sheet)
                .or_insert_with(|| s.table.row_positions());
            let v = match (rows.get(at.row), s.table.col_of(at.col)) {
                (Some(row), Some(col)) => match self.value_at(sheet, at, (row, col), &mut budget) {
                    Ok(v) => v,
                    Err(Interrupt::Cancelled) => return None,
                    Err(e) => unreachable!("no budget and the driver resolves {e:?}"),
                },
                _ => Value::Empty,
            };
            out.push(((sheet, at), v));
            done.fetch_add(1, Ordering::Relaxed);
        }
        Some(out)
    }

    /// Keep results a job computed on a snapshot taken at `revision`; ignored if the
    /// workbook changed since.
    pub fn store_values(&mut self, revision: u64, values: CellValues) {
        if revision != self.revision() {
            return;
        }
        self.calc.sync(revision);
        self.calc.values.extend(values);
    }

    /// Result text of every formula cell of `sheet`, for jobs ([`crate::CsvTable::set_computed`]).
    /// Run it on a snapshot; scans the sheet's file for formulas first if that hasn't
    /// been done. `None` once `cancel` is set.
    pub fn computed_for(
        &mut self,
        sheet: SheetId,
        cancel: Arc<AtomicBool>,
        done: &AtomicU64,
    ) -> Option<Computed> {
        let s = self.sheet_mut(sheet)?;
        if !s.table.formulas_scanned() {
            let found = s.table.scan_formulas(&cancel, &AtomicU64::new(0))?;
            s.table.set_formula_cells(found);
        }
        let cells = s.table.formula_cells_at().expect("scanned just now");
        let mut budget = Budget::unlimited(cancel);
        let mut computed = Computed::default();
        for (at, pos) in cells {
            match self.value_at(sheet, at, pos, &mut budget) {
                Ok(v) => computed.insert(at, result_text(&v)),
                Err(Interrupt::Cancelled) => return None,
                Err(e) => unreachable!("no budget and the driver resolves {e:?}"),
            }
            done.fetch_add(1, Ordering::Relaxed);
        }
        Some(computed)
    }
}

/// Evaluate `target` with an explicit work stack: a formula that needs a cell deeper than
/// [`MAX_DEPTH`] interrupts, the driver evaluates that cell first, then tries again. A
/// cell needed while it is already on the stack is a cycle.
fn drive(
    book: &mut Workbook,
    calc: &mut Calc,
    target: Work,
    budget: &mut Budget,
) -> Result<Value, Interrupt> {
    let mut stack = vec![target];
    let mut on_stack: HashSet<Key> = HashSet::from([target.key]);
    let seed = book.revision();
    while let Some(&top) = stack.last() {
        if let Some(v) = calc.values.get(&top.key) {
            if stack.len() == 1 {
                return Ok(v.clone());
            }
            stack.pop();
            on_stack.remove(&top.key);
            continue;
        }
        let mut ev = Eval {
            book: &mut *book,
            calc: &mut *calc,
            budget: &mut *budget,
            depth: 0,
            need: None,
            in_progress: HashSet::new(),
            sheet: top.key.0,
            pos: top.pos,
            rng: seed,
        };
        match ev.eval_cell(top.key.0, top.key.1, top.pos) {
            Ok(v) => {
                if stack.len() == 1 {
                    return Ok(v);
                }
                stack.pop();
                on_stack.remove(&top.key);
            }
            Err(Interrupt::Deeper) => {
                let need = ev.need.take().expect("Deeper names the cell it needs");
                if on_stack.contains(&need.key) {
                    calc.values.insert(need.key, Value::Error(ErrorKind::Circ));
                } else {
                    stack.push(need);
                    on_stack.insert(need.key);
                }
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the loop returns the target's value")
}

/// The context one evaluation sees.
struct Eval<'a> {
    book: &'a mut Workbook,
    calc: &'a mut Calc,
    budget: &'a mut Budget,
    depth: usize,
    /// The cell a [`Interrupt::Deeper`] needs first.
    need: Option<Work>,
    /// Formula cells being evaluated, one inside another: needing one again is a cycle.
    in_progress: HashSet<Key>,
    /// The formula's own sheet and position.
    sheet: SheetId,
    pos: (u64, u32),
    rng: u64,
}

impl Eval<'_> {
    fn sheet_index(&self, name: Option<&str>) -> Option<usize> {
        let sheets = self.book.sheets();
        match name {
            None => sheets.iter().position(|s| s.id == self.sheet),
            Some(n) => sheets.iter().position(|s| same_name(&s.name, n)),
        }
    }

    /// One cell's value; a formula cell is evaluated in place up to [`MAX_DEPTH`] deep.
    fn eval_cell(
        &mut self,
        sheet: SheetId,
        at: CellRef,
        pos: (u64, u32),
    ) -> Result<Value, Interrupt> {
        let key = (sheet, at);
        if let Some(v) = self.calc.values.get(&key) {
            return Ok(v.clone());
        }
        let Some(s) = self.book.sheet(sheet) else {
            return Ok(Value::Error(ErrorKind::Ref));
        };
        let Some(raw) = s.table.cell_of(at.row, at.col) else {
            return Ok(Value::Empty);
        };
        if !is_formula(&raw) {
            return Ok(literal(&raw));
        }
        let parsed = match self.calc.parsed.entry(key) {
            Entry::Occupied(e) => e.get().clone(),
            Entry::Vacant(e) => e.insert(parse(&raw[1..]).ok().map(Arc::new)).clone(),
        };
        let Some(expr) = parsed else {
            // Text that starts with `=` but isn't a formula shows as text.
            let v = Value::text(&*raw);
            self.calc.values.insert(key, v.clone());
            return Ok(v);
        };
        if s.table.overlay().get(at).is_none() && !unknown_functions(&expr).is_empty() {
            if let Some(cached) = s.cached.get(&at) {
                let v = literal(cached);
                self.calc.values.insert(key, v.clone());
                return Ok(v);
            }
        }
        drop(raw);
        if self.in_progress.contains(&key) {
            return Ok(Value::Error(ErrorKind::Circ));
        }
        if self.depth >= MAX_DEPTH {
            self.need = Some(Work { key, pos });
            return Err(Interrupt::Deeper);
        }
        self.in_progress.insert(key);
        let saved = (self.sheet, self.pos, self.rng);
        (self.sheet, self.pos) = (sheet, pos);
        self.rng =
            mix(saved.2 ^ u64::from(sheet.0) ^ at.row.0.rotate_left(17) ^ u64::from(at.col.0));
        self.depth += 1;
        let result = evaluate(&expr, self);
        self.depth -= 1;
        (self.sheet, self.pos, self.rng) = saved;
        self.in_progress.remove(&key);
        if let Ok(v) = &result {
            self.calc.values.insert(key, v.clone());
        }
        result
    }
}

/// SplitMix64's finalizer: a well-mixed nonzero seed.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (x ^ (x >> 31)) | 1
}

impl Context for Eval<'_> {
    fn cell(&mut self, sheet: Option<&str>, row: u64, col: u32) -> Result<Value, Interrupt> {
        let Some(idx) = self.sheet_index(sheet) else {
            return Ok(Value::Error(ErrorKind::Ref));
        };
        self.budget.spend(1)?;
        let s = &self.book.sheets()[idx];
        if row >= s.table.total_rows() {
            return Ok(Value::Empty);
        }
        let at = CellRef {
            row: s.table.id_at(row),
            col: s.table.col_id(col),
        };
        let id = s.id;
        self.eval_cell(id, at, (row, col))
    }

    fn area(
        &mut self,
        sheet: Option<&str>,
        rows: Range<u64>,
        cols: Range<u32>,
        visit: &mut dyn FnMut(u64, u32, &Value) -> bool,
    ) -> Result<(), Interrupt> {
        let Some(idx) = self.sheet_index(sheet) else {
            visit(rows.start, cols.start, &Value::Error(ErrorKind::Ref));
            return Ok(());
        };
        let id = self.book.sheets()[idx].id;
        let end = rows.end.min(self.book.sheets()[idx].table.total_rows());
        let mut block = RowBlock::full_text();
        let mut at = rows.start;
        while at < end {
            let chunk_end = (at + CHUNK).min(end);
            let table = &mut self.book.sheet_mut(id).expect("resolved above").table;
            table.read_all(at..chunk_end, &mut block);
            for r in at..chunk_end {
                let width = block.cells_in_row(r).min(cols.end);
                if width <= cols.start {
                    continue;
                }
                self.budget.spend(u64::from(width - cols.start))?;
                for c in cols.start..width {
                    let text = block.cell(r, c).expect("within the row");
                    if text.is_empty() {
                        continue;
                    }
                    let v = if is_formula(text) {
                        let t = &self.book.sheets()[idx].table;
                        let cell = CellRef {
                            row: t.id_at(r),
                            col: t.col_id(c),
                        };
                        self.eval_cell(id, cell, (r, c))?
                    } else {
                        literal(text)
                    };
                    if !visit(r, c, &v) {
                        return Ok(());
                    }
                }
            }
            at = chunk_end;
        }
        Ok(())
    }

    fn size(&self, sheet: Option<&str>) -> Result<(u64, u32), ErrorKind> {
        let idx = self.sheet_index(sheet).ok_or(ErrorKind::Ref)?;
        Ok((self.book.sheets()[idx].table.total_rows(), SHEET_COLS))
    }

    fn name(&self, name: &str) -> Option<Expr> {
        let (_, def) = self.book.names().iter().find(|(n, _)| same_name(n, name))?;
        parse(def).ok()
    }

    fn position(&self) -> (u64, u32) {
        self.pos
    }

    fn today(&self) -> f64 {
        self.book.now.floor()
    }

    fn now(&self) -> f64 {
        self.book.now
    }

    fn random(&mut self) -> f64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Built once per area and revision from [`Context::area`], so it costs the same
    /// budget as a scan and evaluates formula cells in the area.
    fn exact_index(
        &mut self,
        sheet: Option<&str>,
        rows: Range<u64>,
        cols: Range<u32>,
        by_row: bool,
    ) -> Result<Option<Arc<ExactIndex>>, Interrupt> {
        let Some(idx) = self.sheet_index(sheet) else {
            return Ok(None);
        };
        let key = (
            self.book.sheets()[idx].id,
            rows.clone(),
            cols.clone(),
            by_row,
        );
        if let Some(index) = self.calc.indexes.get(&key) {
            return Ok(Some(index.clone()));
        }
        let mut index = ExactIndex::default();
        let (r0, c0) = (rows.start, cols.start);
        self.area(sheet, rows, cols, &mut |r, c, v| {
            index.insert(v, if by_row { u64::from(c - c0) } else { r - r0 });
            true
        })?;
        let index = Arc::new(index);
        self.calc.indexes.insert(key, index.clone());
        Ok(Some(index))
    }
}

/// The active sheet as the grid shows it: formula cells show their results (ADR 0007).
/// Results cost at most [`UI_BUDGET`] cell visits per [`ShownSheet`]; cells over budget
/// show `…` and are listed in [`Self::into_pending`] for a job.
pub struct ShownSheet<'a> {
    book: &'a mut Workbook,
    budget: Budget,
    pending: Vec<CellRef>,
}

impl<'a> ShownSheet<'a> {
    pub fn new(book: &'a mut Workbook) -> Self {
        Self {
            book,
            budget: Budget::cells(UI_BUDGET),
            pending: Vec::new(),
        }
    }

    /// Formula cells shown as `…`: their results need a job.
    pub fn into_pending(self) -> Vec<CellRef> {
        self.pending
    }
}

/// What a formula cell shows while its result is being computed.
pub const PENDING: &str = "…";

impl TableSource for ShownSheet<'_> {
    fn row_count(&self) -> u64 {
        self.book.active().row_count()
    }

    fn is_complete(&self) -> bool {
        self.book.active().is_complete()
    }

    fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
        self.book.active_mut().read_rows(rows, out);
        if !out.any_cell(is_formula) {
            return;
        }
        let sheet = self.book.active_id();
        let (book, budget, pending) = (&mut *self.book, &mut self.budget, &mut self.pending);
        out.map_cells(|r, c, text| {
            if !is_formula(text) {
                return None;
            }
            let t = book.active();
            let at = CellRef {
                row: t.row_id(r),
                col: t.col_id(c),
            };
            let pos = (t.pos(r), c);
            Some(match book.value_at(sheet, at, pos, budget) {
                Ok(v) => result_text(&v),
                Err(_) => {
                    pending.push(at);
                    PENDING.to_string()
                }
            })
        });
    }
}
