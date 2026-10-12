//! Commands over a workbook (ADR 0008): table commands run on the sheet they were made
//! for, and sheet structure changes are commands too.

use crate::Command;
use data_model::{BookEdit, CellRef, CsvTable, SheetId, Workbook};

/// A table command run on one sheet of the workbook: undo finds its sheet again however
/// the user moved between sheets since.
pub struct OnSheet {
    sheet: SheetId,
    inner: Box<dyn Command<CsvTable>>,
}

impl OnSheet {
    pub fn new(sheet: SheetId, inner: Box<dyn Command<CsvTable>>) -> Self {
        Self { sheet, inner }
    }

    fn table<'a>(&self, book: &'a mut Workbook) -> &'a mut CsvTable {
        &mut book
            .sheet_mut(self.sheet)
            .expect("a sheet in history exists")
            .table
    }
}

impl Command<Workbook> for OnSheet {
    fn apply(&mut self, book: &mut Workbook) {
        let table = self.table(book);
        self.inner.apply(table);
    }

    fn revert(&mut self, book: &mut Workbook) {
        let table = self.table(book);
        self.inner.revert(table);
    }

    fn label(&self) -> &str {
        self.inner.label()
    }

    fn focus(&self) -> Option<CellRef> {
        self.inner.focus()
    }

    fn sheet(&self) -> Option<SheetId> {
        Some(self.sheet)
    }

    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.inner.heap_bytes()
    }
}

/// Changes to the workbook's sheets, as one step: each edit runs in order, and undo runs
/// their inverses in reverse.
pub struct SheetEdit {
    edits: Vec<BookEdit>,
    label: &'static str,
    show: Option<SheetId>,
    focus: Option<CellRef>,
}

impl SheetEdit {
    /// `show`: the sheet to show after the step runs or is undone.
    pub fn new(label: &'static str, edits: Vec<BookEdit>, show: Option<SheetId>) -> Self {
        Self {
            edits,
            label,
            show,
            focus: None,
        }
    }

    /// The cell to show after the step, on sheet `show`.
    pub fn with_focus(mut self, focus: CellRef) -> Self {
        self.focus = Some(focus);
        self
    }

    fn swap(&mut self, book: &mut Workbook) {
        let mut inverse: Vec<BookEdit> = self.edits.drain(..).map(|e| book.apply(e)).collect();
        inverse.reverse();
        self.edits = inverse;
    }
}

impl Command<Workbook> for SheetEdit {
    fn apply(&mut self, book: &mut Workbook) {
        self.swap(book);
    }

    fn revert(&mut self, book: &mut Workbook) {
        self.swap(book);
    }

    fn label(&self) -> &str {
        self.label
    }

    fn focus(&self) -> Option<CellRef> {
        self.focus
    }

    fn sheet(&self) -> Option<SheetId> {
        self.show
    }

    fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.edits.capacity() * std::mem::size_of::<BookEdit>()
            + self.edits.iter().map(BookEdit::heap_bytes).sum::<usize>()
    }
}
