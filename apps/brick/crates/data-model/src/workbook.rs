//! A workbook: the sheets of one window, each a [`CsvTable`] (ADR 0006), with defined
//! names and the formula calculation cache (ADR 0007). Sheet structure changes are
//! operations with inverses ([`BookEdit`]), like table edits (ADR 0008).

use crate::calc::Calc;
use crate::table::next_revision;
use crate::{CellRef, CsvTable, Edit, FormatError, FormatTarget, StylePatch, StylePool};
use std::collections::HashMap;
use std::sync::Arc;

/// The result text a file stored for each formula cell (XLSX).
pub type CachedResults = HashMap<CellRef, Box<str>>;

/// Stable identity of a sheet: names change, positions change, ids don't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SheetId(pub u32);

/// Where a sheet's rows come from: the CSV file, sheet `k` of the .xlsx file (among its
/// worksheets), or nothing (added in this session).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Csv,
    Xlsx(u32),
    New,
}

pub struct Sheet {
    pub id: SheetId,
    pub name: String,
    pub table: CsvTable,
    pub origin: Origin,
    /// The result text the file stored for each formula cell (XLSX); empty for CSV. Shown
    /// for formulas calling functions Brick doesn't implement, until the cell is edited.
    pub cached: Arc<CachedResults>,
}

impl Sheet {
    fn snapshot(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            table: self.table.snapshot(),
            origin: self.origin,
            cached: self.cached.clone(),
        }
    }
}

/// A change to the workbook's sheets, as an operation: [`Workbook::apply`] returns its
/// inverse.
pub enum BookEdit {
    /// Put this sheet at position `at`.
    Insert {
        at: usize,
        sheet: Box<Sheet>,
    },
    /// Take out the sheet at position `at`; the inverse holds it whole.
    Remove {
        at: usize,
    },
    Rename {
        id: SheetId,
        name: String,
    },
    /// Move the sheet at position `from` to position `to`.
    Move {
        from: usize,
        to: usize,
    },
    /// Table edits on several sheets, in order (FML-10, SHEET-2 formula rewrites).
    Sheets(Vec<(SheetId, Edit)>),
}

impl BookEdit {
    /// Heap the edit holds beyond its own size. A removed sheet counts what its edits
    /// hold, not its file.
    pub fn heap_bytes(&self) -> usize {
        match self {
            BookEdit::Insert { sheet, .. } => {
                std::mem::size_of::<Sheet>() + sheet.name.capacity() + sheet.table.edit_bytes()
            }
            BookEdit::Remove { .. } | BookEdit::Move { .. } => 0,
            BookEdit::Rename { name, .. } => name.capacity(),
            BookEdit::Sheets(edits) => {
                edits.capacity() * std::mem::size_of::<(SheetId, Edit)>()
                    + edits.iter().map(|(_, e)| e.heap_bytes()).sum::<usize>()
            }
        }
    }
}

pub struct Workbook {
    sheets: Vec<Sheet>,
    active: SheetId,
    next_id: u32,
    /// Defined names and their formulas, without `=`.
    names: Vec<(String, String)>,
    /// Sheet ids and names, in order, as last saved.
    saved_layout: Vec<(SheetId, String)>,
    /// Changes when sheets are added, removed, renamed, or moved.
    layout_revision: u64,
    /// The clock `TODAY()` and `NOW()` read: a local serial date with the time of day as
    /// the fraction. Set by the app; data-model has no clock.
    pub(crate) now: f64,
    pub(crate) calc: Calc,
    /// Every sheet's styles live in this one pool (ADR 0009).
    styles: Arc<StylePool>,
}

impl Default for Workbook {
    fn default() -> Self {
        Self::single("Sheet1".to_string(), CsvTable::untitled(), Origin::New)
    }
}

impl Workbook {
    /// A workbook of one sheet.
    pub fn single(name: String, table: CsvTable, origin: Origin) -> Self {
        Self::from_sheets(vec![(name, table, origin, HashMap::new())], Vec::new())
    }

    /// A workbook of these sheets (ids 0..n, the first active) and defined names.
    pub fn from_sheets(
        sheets: Vec<(String, CsvTable, Origin, CachedResults)>,
        names: Vec<(String, String)>,
    ) -> Self {
        assert!(!sheets.is_empty(), "a workbook has a sheet");
        let sheets: Vec<Sheet> = sheets
            .into_iter()
            .enumerate()
            .map(|(i, (name, table, origin, cached))| Sheet {
                id: SheetId(i as u32),
                name,
                table,
                origin,
                cached: Arc::new(cached),
            })
            .collect();
        let mut book = Self {
            next_id: sheets.len() as u32,
            active: SheetId(0),
            sheets,
            names,
            saved_layout: Vec::new(),
            layout_revision: next_revision(),
            now: 0.0,
            calc: Calc::default(),
            styles: Arc::default(),
        };
        book.mark_saved();
        book
    }

    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }

    /// The workbook's style pool (ADR 0009).
    pub fn styles(&self) -> &Arc<StylePool> {
        &self.styles
    }

    /// The pool read from the file (STYLE-1).
    pub fn set_styles(&mut self, styles: StylePool) {
        self.styles = Arc::new(styles);
    }

    /// The pool, to add records to (STYLE-6): a new copy when snapshots share it, so a
    /// job's snapshot keeps the pool it was taken with. Records are only ever appended.
    pub fn styles_mut(&mut self) -> &mut StylePool {
        Arc::make_mut(&mut self.styles)
    }

    /// The edit that applies `patch` to `target` on sheet `id`, its new style records
    /// added to the pool ([`CsvTable::format_edit`]).
    pub fn format_edit(
        &mut self,
        id: SheetId,
        target: FormatTarget,
        patch: &StylePatch,
    ) -> Result<Edit, FormatError> {
        let sheet = self
            .sheets
            .iter()
            .find(|s| s.id == id)
            .expect("a formatted sheet exists");
        sheet
            .table
            .format_edit(Arc::make_mut(&mut self.styles), target, patch)
    }

    /// Some sheet has formatting a CSV file can't keep: formats the user set, or merged
    /// cells other than the file's (STYLE-6).
    pub fn has_formatting(&self) -> bool {
        self.sheets
            .iter()
            .any(|s| s.table.has_user_styles() || s.table.merges() != s.table.layout().merges)
    }

    pub fn sheet(&self, id: SheetId) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.id == id)
    }

    pub fn sheet_mut(&mut self, id: SheetId) -> Option<&mut Sheet> {
        self.sheets.iter_mut().find(|s| s.id == id)
    }

    pub fn index_of(&self, id: SheetId) -> Option<usize> {
        self.sheets.iter().position(|s| s.id == id)
    }

    /// The sheet named `name`, ignoring case.
    pub fn sheet_named(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| same_name(&s.name, name))
    }

    pub fn active(&self) -> &CsvTable {
        &self.active_sheet().table
    }

    pub fn active_mut(&mut self) -> &mut CsvTable {
        let id = self.active;
        &mut self.sheet_mut(id).expect("the active sheet exists").table
    }

    pub fn active_sheet(&self) -> &Sheet {
        self.sheet(self.active).expect("the active sheet exists")
    }

    pub fn active_id(&self) -> SheetId {
        self.active
    }

    /// Show sheet `id`; ignored if there is no such sheet.
    pub fn set_active(&mut self, id: SheetId) {
        if self.sheet(id).is_some() {
            self.active = id;
        }
    }

    /// Defined names and their formulas (without `=`).
    pub fn names(&self) -> &[(String, String)] {
        &self.names
    }

    /// The workbook as it is now, for a worker thread: each sheet's table snapshot, the
    /// names and the clock; an empty calculation cache.
    pub fn snapshot(&self) -> Workbook {
        Workbook {
            sheets: self.sheets.iter().map(Sheet::snapshot).collect(),
            active: self.active,
            next_id: self.next_id,
            names: self.names.clone(),
            saved_layout: self.saved_layout.clone(),
            layout_revision: self.layout_revision,
            now: self.now,
            calc: Calc::default(),
            styles: self.styles.clone(),
        }
    }

    /// Changes whenever any sheet's table changes or sheets are added, removed, renamed,
    /// or moved: formula results cached under an older revision are stale.
    pub fn revision(&self) -> u64 {
        self.sheets
            .iter()
            .map(|s| s.table.revision())
            .max()
            .unwrap_or(0)
            .max(self.layout_revision)
    }

    pub fn layout_revision(&self) -> u64 {
        self.layout_revision
    }

    /// Unsaved changes: every table's, plus one if sheets were added, removed, renamed,
    /// or moved since the last save.
    pub fn edit_count(&self) -> usize {
        let tables: usize = self.sheets.iter().map(|s| s.table.changes()).sum();
        tables + usize::from(self.layout() != self.saved_layout)
    }

    /// The sheet layout as it is now is the saved one.
    pub fn mark_saved(&mut self) {
        self.saved_layout = self.layout();
    }

    fn layout(&self) -> Vec<(SheetId, String)> {
        self.sheets.iter().map(|s| (s.id, s.name.clone())).collect()
    }

    /// Set the clock `TODAY()` and `NOW()` read (local serial date and time). A new
    /// value drops cached results, so volatile formulas show the new time. Whether it
    /// changed.
    pub fn set_now(&mut self, now: f64) -> bool {
        if now == self.now {
            return false;
        }
        self.now = now;
        self.calc = Calc::default();
        true
    }

    /// A new sheet with a fresh id, for [`BookEdit::Insert`].
    pub fn new_sheet(&mut self, name: String, table: CsvTable, origin: Origin) -> Sheet {
        let id = SheetId(self.next_id);
        self.next_id += 1;
        Sheet {
            id,
            name,
            table,
            origin,
            cached: Arc::new(HashMap::new()),
        }
    }

    /// The smallest unused `SheetN`, N ≥ 1.
    pub fn new_sheet_name(&self) -> String {
        (1..)
            .map(|n| format!("Sheet{n}"))
            .find(|name| self.sheet_named(name).is_none())
            .expect("some SheetN is free")
    }

    /// Whether `name` may name a sheet (Excel's rules), not counting sheet `except`.
    pub fn check_sheet_name(
        &self,
        name: &str,
        except: Option<SheetId>,
    ) -> Result<(), &'static str> {
        if name.trim().is_empty() {
            return Err("A sheet name can't be empty");
        }
        if name.chars().count() > 31 {
            return Err("A sheet name has at most 31 characters");
        }
        if name.contains(['[', ']', ':', '*', '?', '/', '\\']) {
            return Err("A sheet name can't contain [ ] : * ? / \\");
        }
        if name.starts_with('\'') || name.ends_with('\'') {
            return Err("A sheet name can't start or end with '");
        }
        if self
            .sheets
            .iter()
            .any(|s| Some(s.id) != except && same_name(&s.name, name))
        {
            return Err("Another sheet has that name");
        }
        Ok(())
    }

    /// Change the sheets (ADR 0008); returns the inverse.
    pub fn apply(&mut self, edit: BookEdit) -> BookEdit {
        match edit {
            BookEdit::Insert { at, sheet } => {
                self.next_id = self.next_id.max(sheet.id.0 + 1);
                self.sheets.insert(at.min(self.sheets.len()), *sheet);
                self.layout_revision = next_revision();
                BookEdit::Remove { at }
            }
            BookEdit::Remove { at } => {
                assert!(self.sheets.len() > 1, "a workbook keeps one sheet");
                let sheet = self.sheets.remove(at);
                if sheet.id == self.active {
                    let next = at.min(self.sheets.len() - 1);
                    self.active = self.sheets[next].id;
                }
                self.layout_revision = next_revision();
                BookEdit::Insert {
                    at,
                    sheet: Box::new(sheet),
                }
            }
            BookEdit::Rename { id, name } => {
                let sheet = self.sheet_mut(id).expect("a renamed sheet exists");
                let before = std::mem::replace(&mut sheet.name, name);
                self.layout_revision = next_revision();
                BookEdit::Rename { id, name: before }
            }
            BookEdit::Move { from, to } => {
                let sheet = self.sheets.remove(from);
                self.sheets.insert(to, sheet);
                self.layout_revision = next_revision();
                BookEdit::Move { from: to, to: from }
            }
            BookEdit::Sheets(edits) => {
                let mut inverse: Vec<(SheetId, Edit)> = edits
                    .into_iter()
                    .map(|(id, edit)| {
                        let table = &mut self.sheet_mut(id).expect("an edited sheet exists").table;
                        (id, table.apply(edit))
                    })
                    .collect();
                inverse.reverse();
                BookEdit::Sheets(inverse)
            }
        }
    }
}

/// Sheet names compare ignoring case, as Excel's do.
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}
