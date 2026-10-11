//! STYLE-7: a copy pasted back into Bricks carries its cells' styles, in the same undo step
//! as the text; a paste past the last row styles the rows it adds.

use commands::{Batch, OnSheet, UndoStack};
use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{
    CellRef, Color, CsvTable, FormatTarget, Origin, StyleId, StylePatch, TableSource, Workbook,
};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

fn book() -> Workbook {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("paste-styles-{}-{n}.csv", std::process::id()));
    let rows: String = (0..6)
        .map(|r| format!("{r},a{r},b{r},c{r},d{r}\n"))
        .collect();
    std::fs::write(&path, rows).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    std::fs::remove_file(path).unwrap();
    Workbook::single(
        "s".into(),
        CsvTable::new(index, Dialect::default()),
        Origin::Csv,
    )
}

fn style(book: &Workbook, row: u64, col: u32) -> StyleId {
    let t = book.active();
    t.style_of(CellRef {
        row: t.row_id(row),
        col: t.col_id(col),
    })
}

fn text(book: &Workbook, row: u64, col: u32) -> String {
    book.active()
        .cell_value(row, col)
        .unwrap_or_default()
        .into_owned()
}

/// Copy rows `rows` × columns `cols` and paste it at `at` as the app does; returns the step.
fn copy_paste(
    book: &mut Workbook,
    rows: std::ops::Range<u64>,
    cols: std::ops::Range<u32>,
    at: (u64, u32),
) -> OnSheet {
    let pool = book.styles().clone();
    let id = book.active_id();
    let t = book.active_mut();
    let copied = t
        .copy_range_styled(
            rows.clone(),
            cols.clone(),
            &pool,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap();
    let block = t.copy_styles(rows, cols);
    let cells = data_model::parse_tsv(&copied.tsv);
    let mut edits = t.paste(at.0, at.1, &cells).unwrap();
    edits.extend(t.paste_styles(&edits, at.0, at.1, &block));
    let focus = CellRef {
        row: t.row_id(at.0.min(t.row_count() - 1)),
        col: t.col_id(at.1),
    };
    OnSheet::new(id, Box::new(Batch::new("Paste", edits, focus)))
}

#[test]
fn a_pasted_cell_takes_its_style_and_undo_takes_both_back() {
    let mut book = book();
    // B2 bold on a yellow fill.
    let patch = StylePatch {
        bold: Some(true),
        fill: Some(Some(Color::Rgb(0xFFFF00))),
        ..StylePatch::default()
    };
    let id = book.active_id();
    let edit = book
        .format_edit(
            id,
            FormatTarget::Cells {
                rows: 1..2,
                cols: 1..2,
            },
            &patch,
        )
        .unwrap();
    book.active_mut().apply(edit);
    let bold = style(&book, 1, 1);
    assert_ne!(bold, StyleId(0));
    let mut undo = UndoStack::default();
    undo.execute(
        Box::new(copy_paste(&mut book, 1..2, 1..2, (4, 3))),
        &mut book,
    );
    assert_eq!(
        (style(&book, 4, 3), text(&book, 4, 3)),
        (bold, "a1".into()),
        "D5 is B2 now"
    );
    assert_eq!(style(&book, 4, 2), StyleId(0), "its neighbours aren't");
    undo.undo(&mut book);
    assert_eq!(
        (style(&book, 4, 3), text(&book, 4, 3)),
        (StyleId(0), "c4".into()),
        "one step back"
    );
    undo.redo(&mut book);
    assert_eq!(style(&book, 4, 3), bold);
}

#[test]
fn a_plain_copy_clears_the_formats_it_lands_on_and_new_rows_get_styles() {
    let mut book = book();
    let patch = StylePatch {
        italic: Some(true),
        ..StylePatch::default()
    };
    let id = book.active_id();
    let edit = book
        .format_edit(
            id,
            FormatTarget::Cells {
                rows: 0..2,
                cols: 0..1,
            },
            &patch,
        )
        .unwrap();
    book.active_mut().apply(edit);
    let italic = style(&book, 0, 0);
    // Plain C1:C2 over italic A1:A2: they're plain now, as in Excel.
    let mut undo = UndoStack::default();
    undo.execute(
        Box::new(copy_paste(&mut book, 0..2, 2..3, (0, 0))),
        &mut book,
    );
    assert_eq!(
        (style(&book, 0, 0), style(&book, 1, 0)),
        (StyleId(0), StyleId(0))
    );
    undo.undo(&mut book);
    assert_eq!(style(&book, 1, 0), italic);
    // Italic A1:A2 pasted at A6: row 6 is the last, so a row is added, and styled.
    let rows = book.active().row_count();
    undo.execute(
        Box::new(copy_paste(&mut book, 0..2, 0..1, (rows - 1, 0))),
        &mut book,
    );
    assert_eq!(book.active().row_count(), rows + 1);
    assert_eq!(
        (style(&book, rows - 1, 0), style(&book, rows, 0)),
        (italic, italic)
    );
    undo.undo(&mut book);
    assert_eq!(book.active().row_count(), rows);
}

#[test]
fn the_html_for_other_applications_carries_the_look() {
    let mut book = book();
    let patch = StylePatch {
        bold: Some(true),
        font_color: Some(Color::Rgb(0xC00000)),
        fill: Some(Some(Color::Rgb(0xFFFF00))),
        ..StylePatch::default()
    };
    let id = book.active_id();
    let edit = book
        .format_edit(
            id,
            FormatTarget::Cells {
                rows: 0..1,
                cols: 0..1,
            },
            &patch,
        )
        .unwrap();
    book.active_mut().apply(edit);
    let pool = book.styles().clone();
    let html = book
        .active_mut()
        .copy_range_styled(
            0..1,
            0..2,
            &pool,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap()
        .html
        .unwrap();
    assert!(
        html.contains("<td style=\"font-weight:bold;color:#c00000;background-color:#ffff00;\">0</td><td>a0</td>"),
        "{html}"
    );
}
