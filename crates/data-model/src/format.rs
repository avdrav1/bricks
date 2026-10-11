//! Editing formats (STYLE-6, ADR 0009): the user's style layers over the file's, the edit
//! that changes them ([`Edit::Styles`]), and building one from a [`StylePatch`] on a
//! selection.
//!
//! A cell shows the first style it finds in: the user's cell style, the file's, the
//! user's row style, the file's, the user's column style, the file's, else 0. The user's
//! layers are keyed by row and column ids, so they follow sorts, inserts, and deletes.
//! Cell and row styles are runs of row id values (inserted rows have the high bit, so one
//! space holds both). Memory follows format actions, not cells: a whole column is one
//! column style plus the cells that carry their own style there, as in Excel.

use crate::style::{Border, Color, Edge, Fill, LineStyle, Pattern, StyleId, StylePool, Xf};
use crate::table::id_ranges;
use crate::{
    CellRef, Col, ColId, CsvTable, Edit, HAlign, Row, RowId, Run, TableSource, Underline, VAlign,
    REPLACE_MAX_CELLS,
};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

/// Rows `start..end` (row id values) given a style, or with `None` back to no user style.
pub type StyleRange = (u64, u64, Option<StyleId>);

/// Rows `start..end` (row id values) that have a style the user gave them.
pub type StyleRun = (u64, u64, StyleId);

/// A change to the user's style layers: each range or column gets its style, in order.
/// [`CsvTable::apply`] returns the exact inverse (the user's entries there before).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleEdit {
    /// Cell styles: row ranges per column.
    pub cells: Vec<(ColId, Vec<StyleRange>)>,
    /// Row styles.
    pub rows: Vec<StyleRange>,
    /// Column styles.
    pub cols: Vec<(ColId, Option<StyleId>)>,
}

impl StyleEdit {
    /// Changes nothing.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|(_, r)| r.is_empty()) && self.rows.is_empty() && self.cols.is_empty()
    }

    /// Heap the edit holds beyond its own size.
    pub fn heap_bytes(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<(ColId, Vec<StyleRange>)>()
            + self
                .cells
                .iter()
                .map(|(_, r)| r.capacity() * std::mem::size_of::<StyleRange>())
                .sum::<usize>()
            + self.rows.capacity() * std::mem::size_of::<StyleRange>()
            + self.cols.capacity() * std::mem::size_of::<(ColId, Option<StyleId>)>()
    }
}

/// Runs of row id values: start → (end, style); sorted, apart, and neighbours with the
/// same style joined, so equal layers are equal maps.
pub(crate) type Runs = BTreeMap<u64, (u64, StyleId)>;

/// The user's style layers of a sheet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UserStyles {
    pub(crate) cells: HashMap<ColId, Runs>,
    pub(crate) rows: Runs,
    pub(crate) cols: HashMap<ColId, StyleId>,
}

fn run_at(runs: &Runs, row: u64) -> Option<StyleId> {
    runs.range(..=row)
        .next_back()
        .filter(|(_, (end, _))| row < *end)
        .map(|(_, (_, s))| *s)
}

/// The runs that meet `rows`, last first.
fn runs_in(runs: &Runs, rows: Range<u64>) -> impl Iterator<Item = (u64, u64, StyleId)> + '_ {
    runs.range(..rows.end)
        .rev()
        .take_while(move |(_, (end, _))| *end > rows.start)
        .map(|(&s, &(e, st))| (s, e, st))
}

/// Give rows `start..end` of `runs` style `style` (`None`: no entry); returns the steps
/// that put back what was there, to apply in order.
fn set_range(runs: &mut Runs, start: u64, end: u64, style: Option<StyleId>) -> Vec<StyleRange> {
    if start >= end {
        return Vec::new();
    }
    let hit: Vec<(u64, u64, StyleId)> = runs_in(runs, start..end).collect();
    let mut undo = Vec::with_capacity(hit.len() + 1);
    undo.push((start, end, None));
    for &(s, e, st) in hit.iter().rev() {
        runs.remove(&s);
        if s < start {
            runs.insert(s, (start, st));
        }
        if e > end {
            runs.insert(end, (e, st));
        }
        undo.push((s.max(start), e.min(end), Some(st)));
    }
    if let Some(st) = style {
        let (mut s, mut e) = (start, end);
        if let Some((&ps, &(pe, pst))) = runs.range(..start).next_back() {
            if pe == start && pst == st {
                runs.remove(&ps);
                s = ps;
            }
        }
        if let Some(&(ne, nst)) = runs.get(&end) {
            if nst == st {
                runs.remove(&end);
                e = ne;
            }
        }
        runs.insert(s, (e, st));
    }
    undo
}

/// Apply `ranges` in order; the steps that undo them, in order.
fn set_ranges(runs: &mut Runs, ranges: Vec<StyleRange>) -> Vec<StyleRange> {
    let steps: Vec<Vec<StyleRange>> = ranges
        .into_iter()
        .map(|(s, e, st)| set_range(runs, s, e, st))
        .collect();
    steps.into_iter().rev().flatten().collect()
}

impl UserStyles {
    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.rows.is_empty() && self.cols.is_empty()
    }

    pub(crate) fn cell(&self, col: ColId, row: u64) -> Option<StyleId> {
        if self.cells.is_empty() {
            return None;
        }
        run_at(self.cells.get(&col)?, row)
    }

    pub(crate) fn row(&self, row: u64) -> Option<StyleId> {
        run_at(&self.rows, row)
    }

    pub(crate) fn col(&self, col: ColId) -> Option<StyleId> {
        if self.cols.is_empty() {
            return None;
        }
        self.cols.get(&col).copied()
    }

    /// Entries in all: what counts as unsaved changes.
    pub(crate) fn len(&self) -> usize {
        self.cells.values().map(BTreeMap::len).sum::<usize>() + self.rows.len() + self.cols.len()
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        let run = std::mem::size_of::<(u64, (u64, StyleId))>() + 16;
        (self.cells.values().map(BTreeMap::len).sum::<usize>() + self.rows.len()) * run
            + self.cols.len() * std::mem::size_of::<(ColId, StyleId)>()
    }

    pub(crate) fn apply(&mut self, edit: StyleEdit) -> StyleEdit {
        let mut inverse = StyleEdit::default();
        for (col, ranges) in edit.cells {
            let runs = self.cells.entry(col).or_default();
            let undo = set_ranges(runs, ranges);
            if runs.is_empty() {
                self.cells.remove(&col);
            }
            inverse.cells.push((col, undo));
        }
        // A column listed twice is undone last-first.
        inverse.cells.reverse();
        inverse.rows = set_ranges(&mut self.rows, edit.rows);
        for (col, style) in edit.cols {
            let before = match style {
                Some(s) => self.cols.insert(col, s),
                None => self.cols.remove(&col),
            };
            inverse.cols.push((col, before));
        }
        inverse.cols.reverse();
        inverse
    }

    /// The whole layers as one edit onto empty ones, each style through `map` (the
    /// recovery journal's restore).
    pub(crate) fn as_edit(&self, map: &dyn Fn(StyleId) -> StyleId) -> StyleEdit {
        let ranges = |runs: &Runs| -> Vec<StyleRange> {
            runs.iter()
                .map(|(&s, &(e, st))| (s, e, Some(map(st))))
                .collect()
        };
        let mut cells: Vec<(ColId, Vec<StyleRange>)> = self
            .cells
            .iter()
            .map(|(&c, runs)| (c, ranges(runs)))
            .collect();
        cells.sort_unstable_by_key(|(c, _)| *c);
        let mut cols: Vec<(ColId, Option<StyleId>)> =
            self.cols.iter().map(|(&c, &s)| (c, Some(map(s)))).collect();
        cols.sort_unstable_by_key(|(c, _)| *c);
        StyleEdit {
            cells,
            rows: ranges(&self.rows),
            cols,
        }
    }
}

/// Borders a preset draws, on each cell by where it sits in the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderPreset {
    /// No border on any side.
    None,
    /// A thin line on every side of every cell.
    All,
    /// A thin line around the selection.
    Outside,
    Top,
    Bottom,
    Left,
    Right,
    /// A thick line around the selection.
    ThickOutside,
}

impl BorderPreset {
    fn apply(self, b: &mut Border, at: Edges) {
        let line = |style| Edge {
            style,
            color: Color::Auto,
        };
        let thin = line(LineStyle::Thin);
        match self {
            Self::None => {
                for e in [&mut b.left, &mut b.right, &mut b.top, &mut b.bottom] {
                    *e = Edge::default();
                }
            }
            Self::All => {
                for e in [&mut b.left, &mut b.right, &mut b.top, &mut b.bottom] {
                    *e = thin;
                }
            }
            Self::Outside | Self::ThickOutside => {
                let l = if self == Self::Outside {
                    thin
                } else {
                    line(LineStyle::Thick)
                };
                for (on, e) in [
                    (at.top, &mut b.top),
                    (at.bottom, &mut b.bottom),
                    (at.left, &mut b.left),
                    (at.right, &mut b.right),
                ] {
                    if on {
                        *e = l;
                    }
                }
            }
            Self::Top if at.top => b.top = thin,
            Self::Bottom if at.bottom => b.bottom = thin,
            Self::Left if at.left => b.left = thin,
            Self::Right if at.right => b.right = thin,
            Self::Top | Self::Bottom | Self::Left | Self::Right => {}
        }
    }
}

/// The edges of the selection a cell lies on, for border presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

/// What a format action changes; `None` leaves that part as each cell has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StylePatch {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Underline>,
    pub strike: Option<bool>,
    pub font_name: Option<Box<str>>,
    /// Hundredths of a point.
    pub font_size: Option<u32>,
    pub font_color: Option<Color>,
    /// `Some(None)`: no fill; `Some(Some(c))`: a solid fill of `c`.
    pub fill: Option<Option<Color>>,
    pub border: Option<BorderPreset>,
    pub horizontal: Option<HAlign>,
    pub vertical: Option<VAlign>,
    pub wrap: Option<bool>,
    /// An Excel number format code (`General`, `0.00%`, …).
    pub num_fmt: Option<Box<str>>,
    /// Back to the workbook's default style first (Clear Formatting).
    pub clear: bool,
}

impl StylePatch {
    fn touches_font(&self) -> bool {
        self.bold.is_some()
            || self.italic.is_some()
            || self.underline.is_some()
            || self.strike.is_some()
            || self.font_name.is_some()
            || self.font_size.is_some()
            || self.font_color.is_some()
    }
}

/// Number formats the file format defines by id in every locale, used for their codes
/// instead of a custom format (date and time ones differ by locale, so they aren't).
const PLAIN_BUILTINS: [u32; 8] = [1, 2, 3, 4, 9, 10, 11, 49];

pub(crate) fn intern<T: PartialEq>(list: &mut Vec<T>, item: T) -> u32 {
    match list.iter().position(|x| *x == item) {
        Some(i) => i as u32,
        None => {
            list.push(item);
            list.len() as u32 - 1
        }
    }
}

impl StylePool {
    /// Style `id` with `patch` applied, for a cell on selection edges `at`. New fonts,
    /// fills, borders, number formats, and cell formats are appended (an equal record is
    /// reused); the file's records never change, and the pool only grows, so every id
    /// stays valid for undo.
    pub fn apply(&mut self, id: StyleId, patch: &StylePatch, at: Edges) -> StyleId {
        let mut xf: Xf = if patch.clear {
            self.xfs[0].clone()
        } else {
            self.xf(id).clone()
        };
        if patch.touches_font() {
            let mut f = self.font(&xf).clone();
            if let Some(v) = patch.bold {
                f.bold = v;
            }
            if let Some(v) = patch.italic {
                f.italic = v;
            }
            if let Some(v) = patch.underline {
                f.underline = v;
            }
            if let Some(v) = patch.strike {
                f.strike = v;
            }
            if let Some(name) = &patch.font_name {
                f.name = name.clone();
                // A theme font scheme would win over the name in Excel and LibreOffice.
                f.scheme = None;
            }
            if let Some(size) = patch.font_size {
                f.size = size;
            }
            if let Some(c) = patch.font_color {
                f.color = c;
            }
            xf.font = intern(&mut self.fonts, f);
        }
        if let Some(fill) = patch.fill {
            let fill = match fill {
                None => Fill::default(),
                Some(c) => Fill::Pattern {
                    pattern: Pattern::Solid,
                    fg: c,
                    bg: Color::Auto,
                },
            };
            xf.fill = intern(&mut self.fills, fill);
        }
        if let Some(preset) = patch.border {
            let mut b = self.border(&xf).cloned().unwrap_or_default();
            preset.apply(&mut b, at);
            xf.border = intern(&mut self.borders, b);
        }
        if let Some(h) = patch.horizontal {
            xf.align.horizontal = h;
        }
        if let Some(v) = patch.vertical {
            xf.align.vertical = v;
        }
        if let Some(w) = patch.wrap {
            xf.align.wrap = w;
        }
        if let Some(code) = &patch.num_fmt {
            xf.num_fmt = self.num_fmt_for(code);
        }
        self.intern(xf)
    }

    /// The number format id for `code`: 0 for General, a built-in id for the codes every
    /// locale shares, else a custom format (added if new).
    pub fn num_fmt_for(&mut self, code: &str) -> u32 {
        if code.eq_ignore_ascii_case("General") {
            return 0;
        }
        PLAIN_BUILTINS
            .into_iter()
            .find(|&id| {
                formula::builtin_format(id) == Some(code) && !self.num_fmts.contains_key(&id)
            })
            .unwrap_or_else(|| self.num_fmt_id(code))
    }
}

/// The cells a format action reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatTarget {
    /// Table rows × view columns.
    Cells { rows: Range<Row>, cols: Range<Col> },
    /// Whole table rows; `cols` are the columns shown, where column styles meet them.
    Rows { rows: Range<Row>, cols: Range<Col> },
    /// Whole view columns.
    Cols(Range<Col>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// The range has this many cells, more than [`REPLACE_MAX_CELLS`].
    TooMany(u64),
    /// The number format code doesn't compile.
    BadNumberFormat,
}

impl CsvTable {
    /// The style of the cell `at` (ids, so it follows sorts and inserts): the user's cell
    /// style, the file's, then the row's, then the column's, else 0.
    pub fn style_of(&self, at: CellRef) -> StyleId {
        match self.own_style(at) {
            Some(s) => s,
            None => self
                .row_style(at.row)
                .unwrap_or_else(|| self.column_style(at.col)),
        }
    }

    /// The cell's own style: the user's, else the file's.
    fn own_style(&self, at: CellRef) -> Option<StyleId> {
        self.user_styles
            .cell(at.col, at.row.0)
            .or_else(|| self.file_cell_style(at))
    }

    fn file_cell_style(&self, at: CellRef) -> Option<StyleId> {
        if at.row.is_inserted() || at.col.is_inserted() {
            return None;
        }
        self.file_styles.cell(at.row.0, at.col.0)
    }

    fn file_row_style(&self, id: RowId) -> Option<StyleId> {
        (!id.is_inserted())
            .then(|| self.file_styles.row(id.0))
            .flatten()
    }

    fn file_column_style(&self, id: ColId) -> StyleId {
        if id.is_inserted() {
            StyleId(0)
        } else {
            self.file_styles.column(id.0)
        }
    }

    /// Row `id`'s style (`<row s customFormat>`): the user's, else the file's.
    pub fn row_style(&self, id: RowId) -> Option<StyleId> {
        self.user_styles
            .row(id.0)
            .or_else(|| self.file_row_style(id))
    }

    /// Column `id`'s style (`<col style>`): the user's, else the file's, else 0.
    pub fn column_style(&self, id: ColId) -> StyleId {
        self.user_styles
            .col(id)
            .unwrap_or_else(|| self.file_column_style(id))
    }

    /// What the cell would show without the user's cell style.
    fn style_beneath_user(&self, at: CellRef) -> StyleId {
        self.file_cell_style(at)
            .or_else(|| self.row_style(at.row))
            .unwrap_or_else(|| self.column_style(at.col))
    }

    /// Style runs the sheet holds, the file's and the user's: what working out a whole
    /// row or column format costs.
    pub fn style_runs(&self) -> usize {
        self.file_styles.runs() + self.user_styles.len()
    }

    /// The user changed formats on this sheet (STYLE-6).
    pub fn has_user_styles(&self) -> bool {
        !self.user_styles.is_empty()
    }

    /// The user's cell styles of the file's columns, as `(source column, runs)`, sorted:
    /// the cells a patched save restyles.
    pub fn user_cell_runs(&self) -> Vec<(ColId, Vec<StyleRun>)> {
        let mut out: Vec<(ColId, Vec<StyleRun>)> = self
            .user_styles
            .cells
            .iter()
            .map(|(&c, runs)| (c, runs.iter().map(|(&s, &(e, st))| (s, e, st)).collect()))
            .collect();
        out.sort_unstable_by_key(|(c, _)| *c);
        out
    }

    /// The user's row styles, sorted.
    pub fn user_row_runs(&self) -> Vec<StyleRun> {
        self.user_styles
            .rows
            .iter()
            .map(|(&s, &(e, st))| (s, e, st))
            .collect()
    }

    /// Column `id` has a style the user gave it.
    pub fn has_user_column_style(&self, id: ColId) -> bool {
        self.user_styles.col(id).is_some()
    }

    /// View columns up to which some cell has its own style (the file's or the user's):
    /// past them, a cell shows its row's or column's.
    pub fn styled_width(&self) -> Col {
        let file = (0..self.file_styles.width())
            .rev()
            .find_map(|c| self.col_of(ColId::source(c)))
            .map_or(0, |c| c + 1);
        let user = self
            .user_styles
            .cells
            .keys()
            .filter_map(|&c| self.col_of(c))
            .max()
            .map_or(0, |c| c + 1);
        file.max(user)
    }

    /// View columns up to which some column has a style the user gave it.
    pub fn user_column_style_width(&self) -> Col {
        self.user_styles
            .cols
            .keys()
            .filter_map(|&c| self.col_of(c))
            .max()
            .map_or(0, |c| c + 1)
    }

    /// The row ids of table rows `rows`, as sorted, merged ranges of id values.
    fn row_id_ranges(&self, rows: Range<Row>) -> Vec<Range<u64>> {
        let ranges = self.view_ranges(rows);
        let runs: Vec<Run> = match &self.rows {
            Some(order) => ranges.into_iter().flat_map(|r| order.runs_in(r)).collect(),
            None => ranges
                .into_iter()
                .map(|r| Run {
                    first: RowId::source(r.start),
                    len: r.end - r.start,
                })
                .collect(),
        };
        id_ranges(&runs)
    }

    /// The edit that applies `patch` to `target`, as one undoable step; new style records
    /// go into `pool` (the workbook's). Cell ranges are walked in runs of equal styles
    /// and are capped at [`REPLACE_MAX_CELLS`] cells. Whole columns get a column style,
    /// and only the cells with their own style there (or a row style) are restyled, as
    /// in Excel; whole rows likewise. An empty [`StyleEdit`] when nothing changes.
    pub fn format_edit(
        &self,
        pool: &mut StylePool,
        target: FormatTarget,
        patch: &StylePatch,
    ) -> Result<Edit, FormatError> {
        if let Some(code) = &patch.num_fmt {
            if !code.eq_ignore_ascii_case("General") && formula::NumberFormat::parse(code).is_err()
            {
                return Err(FormatError::BadNumberFormat);
            }
        }
        let mut b = Builder {
            t: self,
            pool,
            patch,
            memo: HashMap::new(),
        };
        let edit = match target {
            FormatTarget::Cells { rows, cols } => b.cells(rows, cols)?,
            FormatTarget::Rows { rows, cols } => b.rows(rows, cols),
            FormatTarget::Cols(cols) => b.cols(cols),
        };
        Ok(Edit::Styles(edit))
    }
}

/// Append `(start, end, value)` to `ops`, joining it to the last one when they touch.
fn push_op(ops: &mut Vec<StyleRange>, start: u64, end: u64, value: Option<StyleId>) {
    if let Some(last) = ops.last_mut() {
        if last.1 == start && last.2 == value {
            last.1 = end;
            return;
        }
    }
    ops.push((start, end, value));
}

struct Builder<'a> {
    t: &'a CsvTable,
    pool: &'a mut StylePool,
    patch: &'a StylePatch,
    /// Each style's restyled id per edge position: runs repeat styles.
    memo: HashMap<(StyleId, Edges), StyleId>,
}

impl Builder<'_> {
    fn restyle(&mut self, cur: StyleId, at: Edges) -> StyleId {
        // Edges matter only to borders.
        let at = if self.patch.border.is_some() {
            at
        } else {
            Edges::default()
        };
        let (pool, patch) = (&mut *self.pool, self.patch);
        *self
            .memo
            .entry((cur, at))
            .or_insert_with(|| pool.apply(cur, patch, at))
    }

    /// `rows` (row id values) cut wherever a row style changes, or a cell style of
    /// column `col`, and at `extra`: every cell between two cuts has the same layers.
    fn cuts(&self, col: Option<ColId>, rows: Range<u64>, extra: &[u64]) -> Vec<u64> {
        let t = self.t;
        let mut out = vec![rows.start, rows.end];
        let mut add = |x: u64| {
            if x > rows.start && x < rows.end {
                out.push(x);
            }
        };
        if let Some(col) = col {
            if let Some(runs) = t.user_styles.cells.get(&col) {
                for (s, e, _) in runs_in(runs, rows.clone()) {
                    add(s);
                    add(e);
                }
            }
            if !col.is_inserted() {
                for r in t.file_styles.cell_runs(col.0, rows.clone()) {
                    add(r.0);
                    add(r.1);
                }
            }
        }
        for (s, e, _) in runs_in(&t.user_styles.rows, rows.clone()) {
            add(s);
            add(e);
        }
        for r in t.file_styles.row_runs(rows.clone()) {
            add(r.0);
            add(r.1);
        }
        for &x in extra {
            add(x);
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The first and last selected rows' ids, and the cuts that give each its own run.
    fn ends(&self, rows: &Range<Row>) -> (u64, u64, [u64; 4]) {
        let top = self.t.row_id(rows.start).0;
        let bottom = self.t.row_id(rows.end - 1).0;
        (
            top,
            bottom,
            [top, top.saturating_add(1), bottom, bottom.saturating_add(1)],
        )
    }

    fn cells(&mut self, rows: Range<Row>, cols: Range<Col>) -> Result<StyleEdit, FormatError> {
        let t = self.t;
        let rows = rows.start..rows.end.min(t.row_count());
        let mut edit = StyleEdit::default();
        if rows.is_empty() || cols.is_empty() {
            return Ok(edit);
        }
        let n = (rows.end - rows.start).saturating_mul(u64::from(cols.end - cols.start));
        if n > REPLACE_MAX_CELLS {
            return Err(FormatError::TooMany(n));
        }
        let ids = t.row_id_ranges(rows.clone());
        let (top, bottom, extra) = self.ends(&rows);
        for c in cols.clone() {
            let col = t.col_id(c);
            let side = Edges {
                left: c == cols.start,
                right: c + 1 == cols.end,
                ..Edges::default()
            };
            let mut ops = Vec::new();
            for r in &ids {
                for w in self.cuts(Some(col), r.clone(), &extra).windows(2) {
                    let at = CellRef {
                        row: RowId(w[0]),
                        col,
                    };
                    let edges = Edges {
                        top: w[0] == top,
                        bottom: w[0] == bottom,
                        ..side
                    };
                    let new = self.restyle(t.style_of(at), edges);
                    let value = (new != t.style_beneath_user(at)).then_some(new);
                    if value != t.user_styles.cell(col, w[0]) {
                        push_op(&mut ops, w[0], w[1], value);
                    }
                }
            }
            if !ops.is_empty() {
                edit.cells.push((col, ops));
            }
        }
        Ok(edit)
    }

    fn rows(&mut self, rows: Range<Row>, cols: Range<Col>) -> StyleEdit {
        let t = self.t;
        let rows = rows.start..rows.end.min(t.row_count());
        let mut edit = StyleEdit::default();
        if rows.is_empty() {
            return edit;
        }
        let ids = t.row_id_ranges(rows.clone());
        let (top, bottom, extra) = self.ends(&rows);
        let edges = |row: u64| Edges {
            top: row == top,
            bottom: row == bottom,
            ..Edges::default()
        };
        // The rows' own styles.
        for r in &ids {
            for w in self.cuts(None, r.clone(), &extra).windows(2) {
                let id = RowId(w[0]);
                let new = self.restyle(t.row_style(id).unwrap_or_default(), edges(w[0]));
                let value = (Some(new) != t.file_row_style(id)).then_some(new);
                if value != t.user_styles.row(w[0]) {
                    push_op(&mut edit.rows, w[0], w[1], value);
                }
            }
        }
        // Cells with their own style, cells of styled columns in rows that had no style
        // (they showed the column's; a row style would hide it), and for borders the
        // cells of the first and last columns shown (the rows' left and right edges).
        let border = self.patch.border.is_some() && !cols.is_empty();
        let (first, last) = (t.col_id(cols.start), t.col_id(cols.end.saturating_sub(1)));
        let mut meeting: Vec<ColId> = t
            .user_styles
            .cells
            .keys()
            .copied()
            .filter(|&c| t.col_of(c).is_some())
            .collect();
        meeting.extend((0..t.file_styles.width()).map(ColId::source).filter(|&c| {
            t.col_of(c).is_some() && !t.file_styles.cell_runs(c.0, 0..u64::MAX).is_empty()
        }));
        meeting.extend(
            cols.map(|c| t.col_id(c))
                .filter(|&c| t.column_style(c) != StyleId(0)),
        );
        if border {
            meeting.extend([first, last]);
        }
        meeting.sort_unstable();
        meeting.dedup();
        for col in meeting {
            let side = Edges {
                left: border && col == first,
                right: border && col == last,
                ..Edges::default()
            };
            let mut ops = Vec::new();
            for r in &ids {
                for w in self.cuts(Some(col), r.clone(), &extra).windows(2) {
                    let at = CellRef {
                        row: RowId(w[0]),
                        col,
                    };
                    let (cur, own) = match t.own_style(at) {
                        Some(_) => (t.style_of(at), true),
                        None if side.left || side.right => (t.style_of(at), false),
                        None if t.row_style(at.row).is_none()
                            && t.column_style(col) != StyleId(0) =>
                        {
                            (t.column_style(col), false)
                        }
                        None => continue,
                    };
                    let new = self.restyle(
                        cur,
                        Edges {
                            top: w[0] == top,
                            bottom: w[0] == bottom,
                            ..side
                        },
                    );
                    if own && new == cur {
                        continue;
                    }
                    if Some(new) != t.user_styles.cell(col, w[0]) {
                        push_op(&mut ops, w[0], w[1], Some(new));
                    }
                }
            }
            if !ops.is_empty() {
                edit.cells.push((col, ops));
            }
        }
        edit
    }

    fn cols(&mut self, cols: Range<Col>) -> StyleEdit {
        let t = self.t;
        let mut edit = StyleEdit::default();
        // For borders, the first and last table rows are the columns' top and bottom.
        let n = t.row_count();
        let ends = (self.patch.border.is_some() && n > 0).then(|| self.ends(&(0..n)));
        let extra = ends.map_or_else(Vec::new, |(.., cuts)| cuts.to_vec());
        for c in cols.clone() {
            let col = t.col_id(c);
            let side = Edges {
                left: c == cols.start,
                right: c + 1 == cols.end,
                ..Edges::default()
            };
            let new = self.restyle(t.column_style(col), side);
            let value = (new != t.file_column_style(col)).then_some(new);
            if value != t.user_styles.col(col) {
                edit.cols.push((col, value));
            }
            // Cells with their own style, and cells in styled rows (the row's style wins
            // over the column's there): every row, by runs.
            let mut ops = Vec::new();
            for w in self.cuts(Some(col), 0..u64::MAX, &extra).windows(2) {
                let at = CellRef {
                    row: RowId(w[0]),
                    col,
                };
                let (top, bottom) = ends.map_or((false, false), |(a, b, _)| (w[0] == a, w[0] == b));
                let (cur, plain) = match t.own_style(at) {
                    Some(_) => (t.style_of(at), false),
                    None => match t.row_style(at.row) {
                        Some(s) => (s, false),
                        None if top || bottom => (t.column_style(col), true),
                        None => continue,
                    },
                };
                let new = self.restyle(
                    cur,
                    Edges {
                        top,
                        bottom,
                        ..side
                    },
                );
                // A plain cell on the top or bottom edge shows the new column style unless
                // it gets its own.
                if (new != cur || plain) && Some(new) != t.user_styles.cell(col, w[0]) {
                    push_op(&mut ops, w[0], w[1], Some(new));
                }
            }
            if !ops.is_empty() {
                edit.cells.push((col, ops));
            }
        }
        edit
    }
}
