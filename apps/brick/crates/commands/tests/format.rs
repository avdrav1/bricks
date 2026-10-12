//! STYLE-6: every format action is one undo step: it applies, undoes to exactly the styles
//! before, and redoes to exactly the styles after, on cells, whole rows, and whole columns.

use commands::{Batch, OnSheet, UndoStack};
use csv_engine::{Dialect, Source, SparseRowIndex};
use data_model::{
    BorderPreset, CellRef, Color, CsvTable, FileLayout, FileStylesBuilder, FormatTarget, HAlign,
    Origin, StyleId, StylePatch, TableSource, Underline, VAlign, Workbook,
};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn book() -> Workbook {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("cmd-format-{}-{n}.csv", std::process::id()));
    let rows: String = (0..5).map(|r| format!("{r},a{r},b{r},c{r}\n")).collect();
    std::fs::write(&path, rows).unwrap();
    let index = SparseRowIndex::new(Arc::new(Source::open(&path).unwrap()), &Dialect::default());
    index.build(&AtomicBool::new(false)).unwrap();
    std::fs::remove_file(path).unwrap();
    let mut t = CsvTable::new(index, Dialect::default());
    // Some of the file's own styles under the edits: a styled column, row, and cell.
    let mut b = FileStylesBuilder::new(vec![(3, 3, StyleId(0))]);
    b.row(2, StyleId(0));
    b.cell(1, 1, StyleId(0));
    t.set_file_styles(b.finish(), FileLayout::default());
    Workbook::single("s".into(), t, Origin::Csv)
}

/// What every cell looks like: its style's records.
fn looks(book: &Workbook) -> Vec<String> {
    let (t, pool) = (book.active(), book.styles());
    let mut out = Vec::new();
    for r in 0..t.row_count() {
        for c in 0..5 {
            let xf = pool.xf(t.style_of(CellRef {
                row: t.row_id(r),
                col: t.col_id(c),
            }));
            out.push(format!(
                "{:?} {:?} {:?} {:?} {:?}",
                pool.font(xf),
                pool.fill(xf),
                pool.border(xf),
                xf.num_fmt,
                xf.align
            ));
        }
    }
    out
}

fn patches() -> Vec<StylePatch> {
    let p = StylePatch::default;
    let mut all = vec![
        StylePatch {
            bold: Some(true),
            ..p()
        },
        StylePatch {
            italic: Some(true),
            ..p()
        },
        StylePatch {
            underline: Some(Underline::Single),
            ..p()
        },
        StylePatch {
            strike: Some(true),
            ..p()
        },
        StylePatch {
            font_name: Some("DejaVu Serif".into()),
            ..p()
        },
        StylePatch {
            font_size: Some(1800),
            ..p()
        },
        StylePatch {
            font_color: Some(Color::Rgb(0xC00000)),
            ..p()
        },
        StylePatch {
            fill: Some(Some(Color::Rgb(0xFFFF00))),
            ..p()
        },
        StylePatch {
            fill: Some(None),
            ..p()
        },
        StylePatch {
            horizontal: Some(HAlign::Center),
            ..p()
        },
        StylePatch {
            vertical: Some(VAlign::Top),
            ..p()
        },
        StylePatch {
            wrap: Some(true),
            ..p()
        },
        StylePatch {
            num_fmt: Some("[$$-409]#,##0.00".into()),
            ..p()
        },
        StylePatch { clear: true, ..p() },
    ];
    for preset in [
        BorderPreset::All,
        BorderPreset::Outside,
        BorderPreset::Top,
        BorderPreset::Bottom,
        BorderPreset::Left,
        BorderPreset::Right,
        BorderPreset::ThickOutside,
        BorderPreset::None,
    ] {
        all.push(StylePatch {
            border: Some(preset),
            ..p()
        });
    }
    all
}

fn targets() -> [FormatTarget; 3] {
    [
        FormatTarget::Cells {
            rows: 0..3,
            cols: 1..4,
        },
        FormatTarget::Rows {
            rows: 1..3,
            cols: 0..4,
        },
        FormatTarget::Cols(2..4),
    ]
}

/// Run `patch` on `target` as one command, undo it, redo it; the looks before and after.
fn step(
    book: &mut Workbook,
    undo: &mut UndoStack<Workbook>,
    target: &FormatTarget,
    patch: &StylePatch,
) -> (Vec<String>, Vec<String>) {
    let sheet = book.active_id();
    let before = looks(book);
    let edit = book.format_edit(sheet, target.clone(), patch).unwrap();
    let focus = CellRef {
        row: book.active().row_id(0),
        col: book.active().col_id(0),
    };
    let cmd = Batch::new("Format cells", vec![edit], focus);
    undo.execute(Box::new(OnSheet::new(sheet, Box::new(cmd))), book);
    let after = looks(book);
    assert!(undo.undo(book).is_some());
    assert_eq!(looks(book), before, "undo of {patch:?} on {target:?}");
    assert!(undo.redo(book).is_some());
    assert_eq!(looks(book), after, "redo of {patch:?} on {target:?}");
    (before, after)
}

#[test]
fn every_format_action_applies_undoes_and_redoes_exactly() {
    for target in targets() {
        for patch in patches() {
            let mut book = book();
            let (before, after) = step(&mut book, &mut UndoStack::default(), &target, &patch);
            // Taking off what plain cells don't have changes nothing.
            let removes =
                patch.clear || patch.border == Some(BorderPreset::None) || patch.fill == Some(None);
            assert_eq!(after == before, removes, "{patch:?} on {target:?} shows");
        }
    }
}

#[test]
fn a_run_of_format_actions_undoes_back_to_the_file() {
    let mut book = book();
    let mut undo = UndoStack::default();
    let mut steps = 0;
    for target in targets() {
        for patch in patches() {
            step(&mut book, &mut undo, &target, &patch);
            steps += 1;
        }
    }
    let formatted = looks(&book);
    for _ in 0..steps {
        assert!(undo.undo(&mut book).is_some());
    }
    assert_eq!(book.edit_count(), 0);
    for _ in 0..steps {
        assert!(undo.redo(&mut book).is_some());
    }
    assert_eq!(looks(&book), formatted);
}
