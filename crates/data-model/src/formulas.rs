//! Which cells hold formulas (FML-3): the file's, found by one parallel pass after
//! indexing, plus the overlay's.

use crate::{CellRef, ColId, Computed, CsvTable, RowBlock, RowId};
use csv_engine::{split_fields, RowIndex};
use rayon::prelude::*;
use std::borrow::Cow;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// File rows per task, and between cancel checks.
const CHUNK: u64 = 65_536;

/// The cells of a sheet's file whose text is a formula (raw text starting with `=`, longer
/// than one character), found by [`crate::CsvTable::scan_formulas`] after indexing. Cells
/// edited since are not tracked here: the overlay is consulted for those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaCells {
    /// `(file row, file column)`, sorted by row, then column.
    pub(crate) file: Vec<(u64, u32)>,
    pub(crate) complete: bool,
}

impl FormulaCells {
    /// The formula cells of a whole file.
    pub fn new(mut file: Vec<(u64, u32)>) -> Self {
        file.sort_unstable();
        Self {
            file,
            complete: true,
        }
    }

    /// Not scanned yet.
    pub fn incomplete() -> Self {
        Self {
            file: Vec::new(),
            complete: false,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// The file's formula cells, `(file row, file column)`.
    pub fn file_cells(&self) -> &[(u64, u32)] {
        &self.file
    }
}

impl Default for FormulaCells {
    /// Complete and empty: a table with no file behind it.
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Whether raw text is a formula's: `=` and something after it.
pub fn is_formula(raw: &str) -> bool {
    raw.len() > 1 && raw.as_bytes()[0] == b'='
}

/// Where rows are among all rows (the header and hidden rows included), for many
/// lookups at once: built in one pass over the row order, then O(1) each.
pub struct RowPositions {
    /// By file row; `u64::MAX` for a deleted row. Empty while rows are in file order.
    file: Vec<u64>,
    inserted: std::collections::HashMap<RowId, u64>,
    /// Rows in file order (no row order yet): a file row's position is its number.
    identity: Option<u64>,
}

impl RowPositions {
    /// Position of row `id`; `None` once deleted.
    pub fn get(&self, id: RowId) -> Option<u64> {
        if let Some(rows) = self.identity {
            return (!id.is_inserted() && id.0 < rows).then_some(id.0);
        }
        if id.is_inserted() {
            return self.inserted.get(&id).copied();
        }
        self.file
            .get(id.0 as usize)
            .copied()
            .filter(|&p| p != u64::MAX)
    }
}

impl CsvTable {
    /// The rows at `positions` among all rows (the header included, filters ignored).
    pub fn read_all(&mut self, positions: Range<u64>, out: &mut RowBlock) {
        out.reset(positions.start);
        self.read_positions(positions, out);
    }

    /// Where row `id` is among all rows (the header and hidden rows included); `None`
    /// once deleted. O(runs), or O(rows) once sorted: use [`Self::row_positions`] for many.
    pub fn position_of(&self, id: RowId) -> Option<u64> {
        match &self.rows {
            Some(m) => m.position(id),
            None => (!id.is_inserted() && id.0 < self.index.row_count()).then_some(id.0),
        }
    }

    /// Every row's position, for many lookups.
    pub fn row_positions(&self) -> RowPositions {
        let Some(order) = &self.rows else {
            return RowPositions {
                file: Vec::new(),
                inserted: Default::default(),
                identity: Some(self.index.row_count()),
            };
        };
        let mut file = vec![u64::MAX; self.index.row_count() as usize];
        let mut inserted = std::collections::HashMap::new();
        let mut at = 0;
        order.for_each(|run| {
            for i in 0..run.len {
                let id = RowId(run.first.0 + i);
                if id.is_inserted() {
                    inserted.insert(id, at + i);
                } else if let Some(slot) = file.get_mut(id.0 as usize) {
                    *slot = at + i;
                }
            }
            at += run.len;
        });
        RowPositions {
            file,
            inserted,
            identity: None,
        }
    }

    /// Results for formula cells as jobs see them (ADR 0007): set on a snapshot.
    pub fn set_computed(&mut self, computed: Option<Arc<Computed>>) {
        self.computed = computed;
    }

    pub fn computed(&self) -> Option<&Computed> {
        self.computed.as_deref()
    }

    /// Whether row `id` shows text other than its file row's: edits or formula results.
    pub(crate) fn shows_other_text(&self, id: RowId) -> bool {
        self.overlay.row(id).is_some()
            || self.computed.as_ref().is_some_and(|c| c.row(id).is_some())
    }

    /// The file's formula cells, from [`Self::scan_formulas`].
    pub fn set_formula_cells(&mut self, cells: FormulaCells) {
        self.formulas = Arc::new(cells);
    }

    /// Whether the file's formula cells are known (scanned, or no file).
    pub fn formulas_scanned(&self) -> bool {
        self.formulas.complete
    }

    /// Every formula cell still in the table with its position (row among all rows, view
    /// column): the file's that are not edited, cleared, or deleted, and the overlay's.
    /// `None` until the file is scanned.
    pub fn formula_cells_at(&self) -> Option<Vec<(CellRef, (u64, u32))>> {
        if !self.formulas.complete {
            return None;
        }
        let positions = self.row_positions();
        let mut out = Vec::new();
        for &(row, col) in &self.formulas.file {
            let at = CellRef {
                row: RowId::source(row),
                col: ColId::source(col),
            };
            if self.overlay.get(at).is_some()
                || self.cleared.at(row).is_some_and(|c| c.contains(at.col))
            {
                continue;
            }
            if let (Some(p), Some(c)) = (positions.get(at.row), self.col_of(at.col)) {
                out.push((at, (p, c)));
            }
        }
        for (&row, edits) in self.overlay.iter_rows() {
            let Some(p) = positions.get(row) else {
                continue;
            };
            for (&col, v) in edits {
                if let (true, Some(c)) = (is_formula(v), self.col_of(col)) {
                    out.push((CellRef { row, col }, (p, c)));
                }
            }
        }
        Some(out)
    }

    /// Every formula cell still in the table; `None` until the file is scanned.
    pub fn formula_cells(&self) -> Option<Vec<CellRef>> {
        Some(
            self.formula_cells_at()?
                .into_iter()
                .map(|(at, _)| at)
                .collect(),
        )
    }

    /// Whether the file had formula cells, or an edit wrote one.
    pub fn has_formulas(&self) -> bool {
        !self.formulas.file.is_empty()
            || self
                .overlay
                .iter_rows()
                .any(|(_, e)| e.values().any(|v| is_formula(v)))
    }

    /// Find the file's formula cells: one parallel pass in file order, after indexing;
    /// rows without a `=` byte are not parsed. Run it on a snapshot on the job pool.
    /// `None` once `cancel` is set, or if the file isn't indexed.
    pub fn scan_formulas(&self, cancel: &AtomicBool) -> Option<FormulaCells> {
        if !self.index.is_complete() {
            return None;
        }
        let bytes = self.index.source().bytes();
        let file_rows = self.index.row_count();
        let chunks: Option<Vec<Vec<(u64, u32)>>> = (0..file_rows.div_ceil(CHUNK))
            .into_par_iter()
            .map(|c| {
                if cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let rows = c * CHUNK..file_rows.min((c + 1) * CHUNK);
                let mut spans = Vec::new();
                let found = self.index.row_spans(rows.clone(), &mut spans).ok()?;
                if found != (rows.end - rows.start) as usize {
                    return None; // the file changed underneath
                }
                let (start, end) = match (spans.first(), spans.last()) {
                    (Some(a), Some(b)) => (a.start as usize, b.end as usize),
                    _ => return Some(Vec::new()),
                };
                let (mut out, mut fields, mut next) = (Vec::new(), Vec::new(), 0);
                for at in memchr::memchr_iter(b'=', &bytes[start..end]) {
                    let i = spans.partition_point(|s| s.end <= (start + at) as u64);
                    if i < next || i >= spans.len() {
                        continue;
                    }
                    next = i + 1;
                    let span = &spans[i];
                    let mut line = &bytes[span.start as usize..span.end as usize];
                    if span.start == 0 {
                        line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
                    }
                    split_fields(line, &self.dialect, &mut fields);
                    for (col, f) in fields.iter().enumerate() {
                        let v: Cow<'_, [u8]> = f.value(line, self.dialect.quote);
                        if v.len() > 1 && v[0] == b'=' {
                            out.push((rows.start + i as u64, col as u32));
                        }
                    }
                }
                Some(out)
            })
            .collect();
        let mut file = Vec::new();
        for c in chunks? {
            file.extend(c);
        }
        Some(FormulaCells {
            file,
            complete: true,
        })
    }
}

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
