//! Find (SRCH-1, SRCH-2): every cell whose shown value contains some text, then stepping
//! through them from a cell, forwards or backwards, with the match's place among them all.
//!
//! Rules (user decisions, 2026-10-09):
//! - A match is the text anywhere in a cell's value as shown: edits included, quotes
//!   unescaped. "Match case" off ignores ASCII case, like sort and filter.
//! - Order is row by row through the rows shown (a filter's hidden rows don't count), in
//!   their order, wrapping at either end. Scope: the whole table, or one column.
//! - The count is of matching cells shown, and a match's place is its 1-based rank in that
//!   order ("12 of 2,725,768").
//!
//! [`CsvTable::search`] is one parallel pass over the file in file order that counts the
//! matching cells of every row ([`Matches`]). A byte search over each chunk of rows
//! (case-folded when case doesn't matter) picks the rows worth splitting into cells; rows
//! with edits are counted from their cells as shown. [`Matches::step`] then walks the rows
//! shown, a chunk at a time, to the next row with a match, and finds the cell in it.

use crate::{
    CellRef, Col, ColId, CsvTable, Edit, Row, RowBlock, RowId, Run, TableSource, PASTE_MAX_CELLS,
};
use csv_engine::{split_fields, RowIndex};
use memchr::memmem;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// What to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub match_case: bool,
    /// One column to look in (by identity), or `None` for every column shown.
    pub col: Option<ColId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    /// Indexing has not finished, so not every row is known yet.
    NotIndexed,
    Cancelled,
}

/// Where [`Matches::step`] goes from the cell it starts at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// That cell if it matches, else the next match (searching as you type).
    Here,
    Next,
    Previous,
}

/// A match: its cell, and its place among the matches shown (1-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub row: Row,
    pub col: Col,
    pub ordinal: u64,
}

/// A running search's progress, for the UI to read.
#[derive(Debug, Default)]
pub struct SearchProgress {
    /// File rows searched.
    pub rows: AtomicU64,
    /// Matching cells found so far in rows that pass the filters. Provisional: deleted
    /// rows still count until the search ends, and edited rows count only at the end.
    pub found: AtomicU64,
}

/// File rows searched per task, and between cancel checks.
const CHUNK: u64 = 65_536;
/// Table rows per step of a walk through the rows shown, and between cancel checks.
const WALK: u64 = 65_536;
/// A row's count at or past this lives in `Matches::many`.
const MANY: u8 = u8::MAX;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Cells one replace all may change, at most (SRCH-3): the paste limit. Every new value,
/// and its undo, is held as a cell edit until saved (user decision, 2026-10-09).
pub const REPLACE_MAX_CELLS: u64 = PASTE_MAX_CELLS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceError {
    /// More matching cells than [`REPLACE_MAX_CELLS`].
    TooMany(u64),
    /// The table changed since the search.
    Stale,
    Cancelled,
}

/// Every matching cell of a table at one revision, counted by row.
pub struct Matches {
    query: Query,
    needle: Needle,
    revision: u64,
    /// Matching cells per file row; [`MANY`] means the count is in `many`.
    counts: Vec<u8>,
    many: BTreeMap<u64, u32>,
    inserted: HashMap<RowId, u32>,
    /// Matching cells in the rows shown.
    total: u64,
}

impl Matches {
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// Matching cells in the rows shown.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Still true of `table`: nothing it shows changed since the search.
    pub fn is_current(&self, table: &CsvTable) -> bool {
        self.revision == table.revision
    }

    fn file_count(&self, r: u64) -> u64 {
        match self.counts.get(r as usize) {
            Some(&MANY) => u64::from(self.many[&r]),
            Some(&c) => u64::from(c),
            None => 0,
        }
    }

    /// Matching cells in the rows of `run`.
    fn sum(&self, run: Run) -> u64 {
        if run.first.is_inserted() {
            return (0..run.len)
                .map(|i| {
                    self.inserted
                        .get(&RowId(run.first.0 + i))
                        .map_or(0, |&c| u64::from(c))
                })
                .sum();
        }
        let (a, b) = self.file_range(run);
        let plain: u64 = self.counts[a..b].iter().map(|&c| u64::from(c)).sum();
        let extra: u64 = self
            .many
            .range(a as u64..b as u64)
            .map(|(_, &c)| u64::from(c) - u64::from(MANY))
            .sum();
        plain + extra
    }

    /// Offset in `run` of its first row with a match, or its last one when `last`.
    fn hit_in(&self, run: Run, last: bool) -> Option<u64> {
        if run.first.is_inserted() {
            let has = |i: &u64| self.inserted.contains_key(&RowId(run.first.0 + i));
            return if last {
                (0..run.len).rev().find(has)
            } else {
                (0..run.len).find(has)
            };
        }
        let (a, b) = self.file_range(run);
        let mut rows = self.counts[a..b].iter();
        let i = if last {
            rows.rposition(|&c| c != 0)
        } else {
            rows.position(|&c| c != 0)
        };
        i.map(|i| i as u64)
    }

    fn file_range(&self, run: Run) -> (usize, usize) {
        let len = self.counts.len();
        let a = (run.first.0 as usize).min(len);
        (a, (a + run.len as usize).min(len))
    }

    /// `value` with every match of the text replaced by `with`; `None` if it has none.
    /// Matching ignores case as the search did; `with` goes in as typed.
    pub fn replace_in(&self, value: &str, with: &str) -> Option<String> {
        self.needle.replace(value, with, &mut Vec::new())
    }

    /// Replace all (SRCH-3): one edit giving every matching cell shown its value with
    /// the text replaced, for one undo step. Run it on a snapshot on the job pool; `done`
    /// counts rows done. `None` if there is nothing to replace.
    pub fn replace_all(
        &self,
        table: &CsvTable,
        with: &str,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Result<Option<Edit>, ReplaceError> {
        if !self.is_current(table) {
            return Err(ReplaceError::Stale);
        }
        if self.total > REPLACE_MAX_CELLS {
            return Err(ReplaceError::TooMany(self.total));
        }
        if self.total == 0 {
            return Ok(None);
        }
        // The rows shown with a match, by id.
        let mut rows: Vec<RowId> = Vec::new();
        table
            .walk(0..table.row_count(), false, cancel, |_, run| {
                if run.first.is_inserted() {
                    let ids = (0..run.len).map(|i| RowId(run.first.0 + i));
                    rows.extend(ids.filter(|id| self.inserted.contains_key(id)));
                } else {
                    let (a, b) = self.file_range(run);
                    let hits = self.counts[a..b].iter().enumerate();
                    rows.extend(
                        hits.filter(|(_, &c)| c != 0)
                            .map(|(i, _)| RowId::source((a + i) as u64)),
                    );
                }
                None::<()>
            })
            .map_err(|_| ReplaceError::Cancelled)?;
        rows.sort_unstable();
        let (plain, edited): (Vec<RowId>, Vec<RowId>) = rows
            .into_iter()
            .partition(|&id| !id.is_inserted() && table.overlay.row(id).is_none());

        // Rows as in the file: their spans a chunk of file rows at a time, in parallel.
        let groups: Vec<&[RowId]> = plain.chunk_by(|a, b| a.0 / CHUNK == b.0 / CHUNK).collect();
        let mut cells: Vec<(CellRef, Option<Box<str>>)> = groups
            .into_par_iter()
            .map(|group| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ReplaceError::Cancelled);
                }
                let (first, last) = (group[0].0, group[group.len() - 1].0);
                let mut spans = Vec::new();
                let found = table.index.row_spans(first..last + 1, &mut spans).ok();
                if found != Some((last + 1 - first) as usize) {
                    return Err(ReplaceError::Stale); // the file changed underneath
                }
                let (mut fields, mut scratch, mut out) = (Vec::new(), Vec::new(), Vec::new());
                let mut folded = Vec::new();
                for &id in group {
                    let span = &spans[(id.0 - first) as usize];
                    table.file_row_matches(
                        self,
                        id.0,
                        span,
                        &mut fields,
                        &mut scratch,
                        |col, v| {
                            let v = String::from_utf8_lossy(v);
                            if let Some(new) = self.needle.replace(&v, with, &mut folded) {
                                out.push((CellRef { row: id, col }, Some(new.into_boxed_str())));
                            }
                        },
                    );
                }
                done.fetch_add(group.len() as u64, Ordering::Relaxed);
                Ok(out)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        // Rows with edits, from their cells as shown.
        let mut scratch = Vec::new();
        for id in edited {
            for col in table.edited_row_cols(self, id, table.overlay.row(id)) {
                let Some(v) = table.cell_of(id, col) else {
                    continue;
                };
                if let Some(new) = self.needle.replace(&v, with, &mut scratch) {
                    cells.push((CellRef { row: id, col }, Some(new.into_boxed_str())));
                }
            }
            done.fetch_add(1, Ordering::Relaxed);
        }
        Ok((!cells.is_empty()).then_some(Edit::Cells(cells)))
    }

    /// The match `step` leads to from cell `from` (row, column of the rows shown), going
    /// row by row and wrapping at either end. `hint`, the match the search was at before,
    /// spares counting the matches before the new one when it is `from`. `None` if there
    /// is no match. `table` must be the one searched ([`Self::is_current`]).
    pub fn step(
        &self,
        table: &mut CsvTable,
        from: (Row, Col),
        step: Step,
        hint: Option<Hit>,
        cancel: &AtomicBool,
    ) -> Result<Option<Hit>, SearchError> {
        let n = table.row_count();
        if self.total == 0 || n == 0 {
            return Ok(None);
        }
        let (row, col) = (from.0.min(n - 1), from.1);
        let back = step == Step::Previous;
        let in_row = |table: &mut CsvTable, r: Row, want: &dyn Fn(Col) -> bool| {
            let cols = table.match_cols(self, r);
            let mut it = cols.into_iter().filter(|&c| want(c));
            if back {
                it.next_back()
            } else {
                it.next()
            }
        };
        // The rest of the starting row, then the rows after it (before it, going back),
        // then from the other end, then the starting row's other side.
        let near = match step {
            Step::Here => in_row(table, row, &|c| c >= col),
            Step::Next => in_row(table, row, &|c| c > col),
            Step::Previous => in_row(table, row, &|c| c < col),
        };
        let mut hit = near.map(|c| (row, c, false));
        if hit.is_none() {
            let spans = if back {
                [0..row, row + 1..n]
            } else {
                [row + 1..n, 0..row]
            };
            'spans: for (k, mut span) in spans.into_iter().enumerate() {
                while let Some(r) = table.walk_to_hit(self, span.clone(), back, cancel)? {
                    if let Some(c) = in_row(table, r, &|_| true) {
                        hit = Some((r, c, k == 1));
                        break 'spans;
                    }
                    // Counted but not found: can't happen while the table is current.
                    if back {
                        span.end = r;
                    } else {
                        span.start = r + 1;
                    }
                }
            }
        }
        if hit.is_none() {
            let far = match step {
                Step::Here => in_row(table, row, &|c| c < col),
                Step::Next => in_row(table, row, &|c| c <= col),
                Step::Previous => in_row(table, row, &|c| c >= col),
            };
            hit = far.map(|c| (row, c, true));
        }
        let Some((r, c, wrapped)) = hit else {
            return Ok(None);
        };
        let ordinal = match hint {
            Some(h) if (h.row, h.col) == (row, col) && step != Step::Here => {
                match (back, wrapped) {
                    (false, false) => h.ordinal + 1,
                    (false, true) => 1,
                    (true, false) => h.ordinal - 1,
                    (true, true) => self.total,
                }
            }
            _ => {
                let before = table.count_matches(self, 0..r, cancel)?;
                let in_row = table.match_cols(self, r).iter().filter(|&&x| x < c).count();
                before + in_row as u64 + 1
            }
        };
        Ok(Some(Hit {
            row: r,
            col: c,
            ordinal,
        }))
    }
}

/// The query's text as compared: folded to lower case unless case matters.
struct Needle {
    bytes: Vec<u8>,
    fold: bool,
}

impl Needle {
    fn new(q: &Query) -> Self {
        let mut bytes = q.text.as_bytes().to_vec();
        if !q.match_case {
            bytes.make_ascii_lowercase();
        }
        Self {
            bytes,
            fold: !q.match_case,
        }
    }

    fn in_value(&self, value: &[u8], scratch: &mut Vec<u8>) -> bool {
        let hay = if self.fold {
            scratch.clear();
            scratch.extend(value.iter().map(u8::to_ascii_lowercase));
            &scratch[..]
        } else {
            value
        };
        memmem::find(hay, &self.bytes).is_some()
    }

    /// `value` with every match replaced by `with`; `None` if it has none. Folding is
    /// ASCII-only, so offsets in the folded text are offsets in `value`, and a match of
    /// UTF-8 text in UTF-8 text starts and ends on character boundaries.
    fn replace(&self, value: &str, with: &str, scratch: &mut Vec<u8>) -> Option<String> {
        let hay = if self.fold {
            scratch.clear();
            scratch.extend(value.bytes().map(|b| b.to_ascii_lowercase()));
            &scratch[..]
        } else {
            value.as_bytes()
        };
        let mut matches = memmem::find_iter(hay, &self.bytes).peekable();
        matches.peek()?;
        let mut out = String::with_capacity(value.len());
        let mut last = 0;
        for at in matches {
            out.push_str(&value[last..at]);
            out.push_str(with);
            last = at + self.bytes.len();
        }
        out.push_str(&value[last..]);
        Some(out)
    }
}

/// One chunk's share of the pass: counts of its file rows, and those too big for a byte.
struct ChunkCounts {
    counts: Vec<u8>,
    many: Vec<(u64, u32)>,
}

impl CsvTable {
    /// Every cell whose shown value contains the query's text, counted by row: one pass
    /// over the file. Run it on a snapshot on the job pool.
    pub fn search(
        &mut self,
        q: &Query,
        cancel: &AtomicBool,
        progress: &SearchProgress,
    ) -> Result<Matches, SearchError> {
        if !self.index.is_complete() {
            return Err(SearchError::NotIndexed);
        }
        let mut m = Matches {
            query: q.clone(),
            needle: Needle::new(q),
            revision: self.revision,
            counts: Vec::new(),
            many: BTreeMap::new(),
            inserted: HashMap::new(),
            total: 0,
        };
        if q.text.is_empty() {
            return Ok(m);
        }
        let chunks = self.count_file_rows(&m, cancel, progress)?;
        m.counts.reserve_exact(self.index.row_count() as usize);
        for c in chunks {
            m.counts.extend(c.counts);
            m.many.extend(c.many);
        }
        self.count_edited_rows(&mut m);
        m.total = self.count_matches(&m, 0..self.row_count(), cancel)?;
        progress.found.store(m.total, Ordering::Relaxed);
        Ok(m)
    }

    /// Matching cells per file row, from the file alone (rows with edits are left at 0
    /// for [`Self::count_edited_rows`]).
    fn count_file_rows(
        &self,
        m: &Matches,
        cancel: &AtomicBool,
        progress: &SearchProgress,
    ) -> Result<Vec<ChunkCounts>, SearchError> {
        let file_rows = self.index.row_count();
        let needle = &m.needle;
        // Quotes are doubled in the file, so a quote in the text can't be looked for in
        // the raw bytes: then every row is checked cell by cell.
        let prefilter = !needle.bytes.contains(&self.dialect.quote);
        let shown = |row: u64| {
            row >= self.first_row()
                && self
                    .filters
                    .iter()
                    .all(|(_, set)| set.passes(RowId::source(row)))
        };
        (0..file_rows.div_ceil(CHUNK))
            .into_par_iter()
            .map(|c| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(SearchError::Cancelled);
                }
                let rows = c * CHUNK..file_rows.min((c + 1) * CHUNK);
                let mut spans = Vec::new();
                let found = self.index.row_spans(rows.clone(), &mut spans).ok();
                if found != Some((rows.end - rows.start) as usize) {
                    return Err(SearchError::Cancelled); // the file changed underneath
                }
                let mut out = ChunkCounts {
                    counts: vec![0; spans.len()],
                    many: Vec::new(),
                };
                let (mut fields, mut scratch) = (Vec::new(), Vec::new());
                let mut shown_found = 0;
                let mut count = |i: usize| {
                    let row = rows.start + i as u64;
                    if self.overlay.row(RowId::source(row)).is_some() {
                        return; // counted from its edits
                    }
                    let n = self.count_in_file_row(m, row, &spans[i], &mut fields, &mut scratch);
                    if n >= u32::from(MANY) {
                        out.counts[i] = MANY;
                        out.many.push((row, n));
                    } else {
                        out.counts[i] = n as u8;
                    }
                    if n > 0 && shown(row) {
                        shown_found += u64::from(n);
                    }
                };
                if prefilter {
                    let bytes = self.index.source().bytes();
                    let (start, end) = match (spans.first(), spans.last()) {
                        (Some(a), Some(b)) => (a.start as usize, b.end as usize),
                        _ => (0, 0),
                    };
                    let chunk = &bytes[start..end];
                    let folded: Vec<u8>;
                    let hay = if needle.fold {
                        folded = chunk.iter().map(u8::to_ascii_lowercase).collect();
                        &folded[..]
                    } else {
                        chunk
                    };
                    let mut next_row = 0usize;
                    for at in memmem::find_iter(hay, &needle.bytes) {
                        let at = (start + at) as u64;
                        let i = spans.partition_point(|s| s.end <= at);
                        if i >= next_row && i < spans.len() {
                            count(i);
                            next_row = i + 1;
                        }
                    }
                } else {
                    (0..spans.len()).for_each(&mut count);
                }
                progress
                    .rows
                    .fetch_add(rows.end - rows.start, Ordering::Relaxed);
                progress.found.fetch_add(shown_found, Ordering::Relaxed);
                Ok(out)
            })
            .collect()
    }

    /// Matching cells of file row `row` (at `span`, without edits).
    fn count_in_file_row(
        &self,
        m: &Matches,
        row: u64,
        span: &Range<u64>,
        fields: &mut Vec<csv_engine::Field>,
        scratch: &mut Vec<u8>,
    ) -> u32 {
        let mut n = 0;
        self.file_row_matches(m, row, span, fields, scratch, |_, _| n += 1);
        n
    }

    /// Each matching cell of file row `row` (at `span`, without edits), with its value.
    fn file_row_matches(
        &self,
        m: &Matches,
        row: u64,
        span: &Range<u64>,
        fields: &mut Vec<csv_engine::Field>,
        scratch: &mut Vec<u8>,
        mut each: impl FnMut(ColId, &[u8]),
    ) {
        let bytes = self.index.source().bytes();
        let mut line = &bytes[span.start as usize..span.end as usize];
        if span.start == 0 {
            line = line.strip_prefix(UTF8_BOM).unwrap_or(line);
        }
        split_fields(line, &self.dialect, fields);
        let cleared = self.cleared.at(row);
        for (i, f) in fields.iter().enumerate() {
            let id = ColId::source(i as Col);
            if m.query.col.is_none_or(|c| c == id)
                && (self.cols.is_none() || self.col_of(id).is_some())
                && !cleared.is_some_and(|c| c.contains(id))
            {
                let value = f.value(line, self.dialect.quote);
                if m.needle.in_value(&value, scratch) {
                    each(id, &value);
                }
            }
        }
    }

    /// The columns in scope of row `id`, which has the edits `edits` (or none).
    fn edited_row_cols(
        &self,
        m: &Matches,
        id: RowId,
        edits: Option<&BTreeMap<ColId, Box<str>>>,
    ) -> Vec<ColId> {
        if let Some(c) = m.query.col {
            return vec![c];
        }
        let fields = if id.is_inserted() {
            0
        } else {
            let mut f = Vec::new();
            self.index
                .row_fields(id.0, &mut f)
                .map_or(0, |_| f.len() as Col)
        };
        let edited = || edits.into_iter().flat_map(|e| e.keys().copied());
        let width = match &self.cols {
            Some(map) => map.width_with(fields, edited()),
            None => fields.max(edited().next_back().map_or(0, |c| c.0 + 1)),
        };
        (0..width).map(|p| self.col_id(p)).collect()
    }

    /// Count every row with edits (source rows the file pass skipped, and inserted rows)
    /// from its cells as shown.
    fn count_edited_rows(&self, m: &mut Matches) {
        let mut scratch = Vec::new();
        for (&id, edits) in self.overlay.iter_rows() {
            let n = self
                .edited_row_cols(m, id, Some(edits))
                .into_iter()
                .filter(|&c| {
                    self.cell_of(id, c)
                        .is_some_and(|v| m.needle.in_value(v.as_bytes(), &mut scratch))
                })
                .count() as u32;
            if id.is_inserted() {
                if n > 0 {
                    m.inserted.insert(id, n);
                }
            } else if let Some(slot) = m.counts.get_mut(id.0 as usize) {
                m.many.remove(&id.0);
                if n >= u32::from(MANY) {
                    *slot = MANY;
                    m.many.insert(id.0, n);
                } else {
                    *slot = n as u8;
                }
            }
        }
    }

    /// Visit the rows shown at table rows `rows` as runs of consecutive ids, each with
    /// the table row it starts at; last first when `back` (each run still low to high).
    /// Stops at the first `Some` that `f` returns.
    fn walk<T>(
        &self,
        rows: Range<Row>,
        back: bool,
        cancel: &AtomicBool,
        mut f: impl FnMut(Row, Run) -> Option<T>,
    ) -> Result<Option<T>, SearchError> {
        let steps = rows.end.saturating_sub(rows.start).div_ceil(WALK);
        let mut runs: Vec<(Row, Run)> = Vec::new();
        for k in 0..steps {
            if cancel.load(Ordering::Relaxed) {
                return Err(SearchError::Cancelled);
            }
            let k = if back { steps - 1 - k } else { k };
            let chunk = rows.start + k * WALK..rows.end.min(rows.start + (k + 1) * WALK);
            runs.clear();
            let mut at = chunk.start;
            for p in self.view_ranges(chunk) {
                match &self.rows {
                    None => {
                        let len = p.end - p.start;
                        runs.push((
                            at,
                            Run {
                                first: RowId::source(p.start),
                                len,
                            },
                        ));
                        at += len;
                    }
                    Some(order) => {
                        for run in order.runs_in(p) {
                            runs.push((at, run));
                            at += run.len;
                        }
                    }
                }
            }
            let found = if back {
                runs.iter().rev().find_map(|&(r, run)| f(r, run))
            } else {
                runs.iter().find_map(|&(r, run)| f(r, run))
            };
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    /// The first row in `rows` (of the rows shown) with a match; the last when `back`.
    fn walk_to_hit(
        &self,
        m: &Matches,
        rows: Range<Row>,
        back: bool,
        cancel: &AtomicBool,
    ) -> Result<Option<Row>, SearchError> {
        self.walk(rows, back, cancel, |r, run| {
            m.hit_in(run, back).map(|i| r + i)
        })
    }

    /// Matching cells in table rows `rows`.
    fn count_matches(
        &self,
        m: &Matches,
        rows: Range<Row>,
        cancel: &AtomicBool,
    ) -> Result<u64, SearchError> {
        let mut total = 0;
        self.walk(rows, false, cancel, |_, run| {
            total += m.sum(run);
            None::<()>
        })?;
        Ok(total)
    }

    /// The columns of table row `row` whose values contain the text, in order.
    fn match_cols(&mut self, m: &Matches, row: Row) -> Vec<Col> {
        let id = self.row_id(row);
        let counted = if id.is_inserted() {
            m.inserted.contains_key(&id)
        } else {
            m.file_count(id.0) > 0
        };
        if !counted {
            return Vec::new();
        }
        let only = match m.query.col {
            Some(c) => match self.col_of(c) {
                Some(p) => Some(p),
                None => return Vec::new(),
            },
            None => None,
        };
        let mut block = RowBlock::full_text();
        self.read_rows(row..row + 1, &mut block);
        let mut scratch = Vec::new();
        (0..block.cells_in_row(row))
            .filter(|&c| only.is_none_or(|o| o == c))
            .filter(|&c| {
                let v = block.cell(row, c).unwrap_or("");
                m.needle.in_value(v.as_bytes(), &mut scratch)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needle(text: &str, match_case: bool) -> Needle {
        Needle::new(&Query {
            text: text.into(),
            match_case,
            col: None,
        })
    }

    #[test]
    fn case_folds_unless_asked_not_to() {
        let mut s = Vec::new();
        assert!(needle("LAND", false).in_value(b"Portland", &mut s));
        assert!(!needle("LAND", true).in_value(b"Portland", &mut s));
        assert!(needle("land", true).in_value(b"Portland", &mut s));
        assert!(!needle("x", false).in_value(b"", &mut s));
    }
}
