//! STYLE-6: format edits. A cell's style layers the user's styles over the file's as Excel
//! does; patches restyle ranges, whole rows, and whole columns by runs; borders follow the
//! selection's edges; every edit undoes exactly; the pool only grows and reuses records.

use data_model::{
    BorderPreset, CellRef, Color, CsvTable, DelimiterChoice, Edit, FileLayout, FileStylesBuilder,
    Fill, FormatError, FormatTarget, LineStyle, Pattern, StyleEdit, StyleId, StylePatch, StylePool,
    TableSource, Xf, REPLACE_MAX_CELLS,
};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A 6×4 table with no header row.
fn table(name: &str) -> (Tmp, CsvTable) {
    let p = std::env::temp_dir().join(format!("format-{}-{name}.csv", std::process::id()));
    let rows: String = (0..6).map(|r| format!("{r}0,{r}1,{r}2,{r}3\n")).collect();
    std::fs::write(&p, rows).unwrap();
    let mut t = CsvTable::open(&p, DelimiterChoice::Fixed(b',')).unwrap();
    t.index().build(&AtomicBool::new(false)).unwrap();
    t.set_header(false);
    (Tmp(p), t)
}

fn at(t: &CsvTable, r: u64, c: u32) -> CellRef {
    CellRef {
        row: t.row_id(r),
        col: t.col_id(c),
    }
}

/// A pool with a yellow fill (style 1) and a red fill (style 2), as a file would have.
fn pool() -> StylePool {
    let mut p = StylePool::default();
    for rgb in [0xFFFF00, 0xFF0000] {
        p.fills.push(Fill::Pattern {
            pattern: Pattern::Solid,
            fg: Color::Rgb(rgb),
            bg: Color::Auto,
        });
        let fill = p.fills.len() as u32 - 1;
        p.xfs.push(Xf {
            fill,
            locked: true,
            ..Xf::default()
        });
    }
    p.from_file.fills = p.fills.len() as u32;
    p.from_file.xfs = p.xfs.len() as u32;
    p
}

fn every_style(t: &CsvTable) -> Vec<StyleId> {
    (0..t.row_count())
        .flat_map(|r| (0..5).map(move |c| (r, c)))
        .map(|(r, c)| t.style_of(at(t, r, c)))
        .collect()
}

fn bold() -> StylePatch {
    StylePatch {
        bold: Some(true),
        ..StylePatch::default()
    }
}

fn format(t: &mut CsvTable, pool: &mut StylePool, target: FormatTarget, p: &StylePatch) -> Edit {
    let edit = t.format_edit(pool, target, p).unwrap();
    t.apply(edit)
}

fn cells(rows: std::ops::Range<u64>, cols: std::ops::Range<u32>) -> FormatTarget {
    FormatTarget::Cells { rows, cols }
}

#[test]
fn layers_stack_as_in_excel() {
    let (_f, mut t) = table("layers");
    // The file: column A yellow; row 2 red; cell A3 (row 2) yellow again; B4 red.
    let mut b = FileStylesBuilder::new(vec![(0, 0, StyleId(1))]);
    b.row(2, StyleId(2));
    b.cell(2, 0, StyleId(1));
    b.cell(2, 1, StyleId(2));
    b.cell(3, 1, StyleId(2));
    t.set_file_styles(b.finish(), FileLayout::default());
    let s = |t: &CsvTable, r, c| t.style_of(at(t, r, c));
    assert_eq!(s(&t, 0, 0), StyleId(1), "file column");
    assert_eq!(s(&t, 2, 2), StyleId(2), "file row over file column");
    assert_eq!(s(&t, 2, 0), StyleId(1), "file cell over file row");
    assert_eq!(s(&t, 3, 1), StyleId(2), "file cell");
    assert_eq!(s(&t, 0, 3), StyleId(0), "nothing");

    let (u_cell, u_row, u_col) = (StyleId(7), StyleId(8), StyleId(9));
    let (a, b, c) = (t.col_id(0), t.col_id(1), t.col_id(2));
    t.apply(Edit::Styles(StyleEdit {
        cells: vec![(a, vec![(t.row_id(2).0, t.row_id(2).0 + 1, Some(u_cell))])],
        rows: vec![(t.row_id(4).0, t.row_id(4).0 + 1, Some(u_row))],
        cols: vec![(a, Some(u_col)), (b, Some(u_col)), (c, Some(u_col))],
    }));
    assert_eq!(s(&t, 2, 0), u_cell, "user cell over file cell");
    assert_eq!(s(&t, 3, 1), StyleId(2), "file cell over user column");
    assert_eq!(s(&t, 4, 0), u_row, "user row over user and file columns");
    assert_eq!(s(&t, 2, 2), StyleId(2), "file row over user column");
    assert_eq!(s(&t, 0, 0), u_col, "user column over file column");
    assert_eq!(s(&t, 0, 2), u_col, "user column");
    assert_eq!(s(&t, 0, 3), StyleId(0));
    assert_eq!(t.changes(), 5, "one per range or column");
}

#[test]
fn bold_on_a_range_keeps_each_cells_other_formats() {
    let (_f, mut t) = table("bold");
    let mut b = FileStylesBuilder::new(Vec::new());
    b.cell(1, 1, StyleId(1)); // B2 yellow
    t.set_file_styles(b.finish(), FileLayout::default());
    let mut p = pool();
    let before = every_style(&t);
    let undo = format(&mut t, &mut p, cells(0..3, 0..2), &bold());
    for r in 0..6 {
        for c in 0..4 {
            let xf = p.xf(t.style_of(at(&t, r, c))).clone();
            let inside = r < 3 && c < 2;
            assert_eq!(p.font(&xf).bold, inside, "bold at {r},{c}");
            assert_eq!(
                xf.fill,
                if (r, c) == (1, 1) { 2 } else { 0 },
                "fill at {r},{c}"
            );
        }
    }
    t.apply(undo);
    assert_eq!(every_style(&t), before);
    assert_eq!(t.changes(), 0);
}

#[test]
fn whole_columns_restyle_only_cells_with_their_own_style() {
    let (_f, mut t) = table("cols");
    let mut b = FileStylesBuilder::new(Vec::new());
    b.row(4, StyleId(2)); // row 5 red
    b.cell(1, 1, StyleId(1)); // B2 yellow
    b.cell(2, 1, StyleId(1)); // B3 yellow
    t.set_file_styles(b.finish(), FileLayout::default());
    let mut p = pool();
    let before = every_style(&t);
    let edit = t
        .format_edit(&mut p, FormatTarget::Cols(1..2), &bold())
        .unwrap();
    let Edit::Styles(e) = &edit else {
        panic!("a style edit")
    };
    assert_eq!(e.cols.len(), 1, "a column style");
    assert_eq!(e.cells.len(), 1);
    assert_eq!(
        e.cells[0].1.len(),
        2,
        "B2:B3's run and the red row's cell, not every row"
    );
    let undo = t.apply(edit);
    let look = |t: &CsvTable, r, c| p.xf(t.style_of(at(t, r, c))).clone();
    for r in 0..6 {
        let xf = look(&t, r, 1);
        assert!(p.font(&xf).bold, "B{} bold", r + 1);
        let fill = match r {
            1 | 2 => 2,
            4 => 3,
            _ => 0,
        };
        assert_eq!(xf.fill, fill, "B{} keeps its fill", r + 1);
        assert!(!p.font(&look(&t, r, 0)).bold, "A{} untouched", r + 1);
    }
    // A cell typed later in the column shows the column's style.
    assert_eq!(t.column_style(t.col_id(1)), t.style_of(at(&t, 5, 1)));
    t.apply(undo);
    assert_eq!(every_style(&t), before);
    assert_eq!(t.changes(), 0);
}

#[test]
fn whole_rows_set_the_row_style_and_keep_styled_columns() {
    let (_f, mut t) = table("rows");
    let mut b = FileStylesBuilder::new(vec![(3, 3, StyleId(2))]); // column D red
    b.cell(1, 0, StyleId(1)); // A2 yellow
    t.set_file_styles(b.finish(), FileLayout::default());
    let mut p = pool();
    let before = every_style(&t);
    let undo = format(
        &mut t,
        &mut p,
        FormatTarget::Rows {
            rows: 1..3,
            cols: 0..4,
        },
        &bold(),
    );
    assert!(t.row_style(t.row_id(1)).is_some() && t.row_style(t.row_id(2)).is_some());
    for r in 0..6 {
        for c in 0..4 {
            let xf = p.xf(t.style_of(at(&t, r, c))).clone();
            assert_eq!(p.font(&xf).bold, (1..3).contains(&r), "bold at {r},{c}");
            let fill = match (r, c) {
                (1, 0) => 2,
                (_, 3) => 3,
                _ => 0,
            };
            assert_eq!(xf.fill, fill, "fill at {r},{c}");
        }
    }
    t.apply(undo);
    assert_eq!(every_style(&t), before);
    assert_eq!(t.changes(), 0);
}

#[test]
fn outside_borders_go_on_the_selections_edges_only() {
    for (preset, line) in [
        (BorderPreset::Outside, LineStyle::Thin),
        (BorderPreset::ThickOutside, LineStyle::Thick),
    ] {
        let (_f, mut t) = table("borders");
        let mut p = pool();
        let patch = StylePatch {
            border: Some(preset),
            ..StylePatch::default()
        };
        format(&mut t, &mut p, cells(1..4, 1..4), &patch);
        for r in 0..6 {
            for c in 0..4 {
                let b = p
                    .border(p.xf(t.style_of(at(&t, r, c))))
                    .cloned()
                    .unwrap_or_default();
                let inside = (1..4).contains(&r) && (1..4).contains(&c);
                let side = |on: bool| if inside && on { line } else { LineStyle::None };
                assert_eq!(b.top.style, side(r == 1), "top at {r},{c}");
                assert_eq!(b.bottom.style, side(r == 3), "bottom at {r},{c}");
                assert_eq!(b.left.style, side(c == 1), "left at {r},{c}");
                assert_eq!(b.right.style, side(c == 3), "right at {r},{c}");
            }
        }
    }
}

#[test]
fn number_formats_and_clear() {
    let (_f, mut t) = table("numfmt");
    let mut b = FileStylesBuilder::new(Vec::new());
    b.cell(0, 0, StyleId(1));
    t.set_file_styles(b.finish(), FileLayout::default());
    let mut p = pool();
    let code = |p: &StylePool, t: &CsvTable, r, c| {
        let id = p.xf(t.style_of(at(t, r, c))).num_fmt;
        p.custom_format(id)
            .map(str::to_owned)
            .or_else(|| formula::builtin_format(id).map(str::to_owned))
    };
    for c in ["0.00%", "yyyy-mm-dd", "#,##0.00", "General"] {
        let patch = StylePatch {
            num_fmt: Some(c.into()),
            ..StylePatch::default()
        };
        format(&mut t, &mut p, cells(0..2, 0..1), &patch);
        assert_eq!(code(&p, &t, 0, 0).as_deref(), Some(c));
        assert_eq!(code(&p, &t, 1, 0).as_deref(), Some(c));
        assert_eq!(p.xf(t.style_of(at(&t, 0, 0))).fill, 2, "A1 keeps its fill");
    }
    assert!(matches!(
        t.format_edit(
            &mut p,
            cells(0..1, 0..1),
            &StylePatch {
                num_fmt: Some("0.0.0\"".into()),
                ..StylePatch::default()
            }
        ),
        Err(FormatError::BadNumberFormat)
    ));
    let clear = StylePatch {
        clear: true,
        ..StylePatch::default()
    };
    format(&mut t, &mut p, cells(0..6, 0..4), &clear);
    assert!(every_style(&t).iter().all(|&s| s == StyleId(0)));
    assert_eq!(t.changes(), 1, "only A1's file style needs overriding");
}

#[test]
fn every_edit_undoes_exactly_after_sorts_and_inserts() {
    let (_f, mut t) = table("undo");
    let mut b = FileStylesBuilder::new(vec![(2, 2, StyleId(1))]);
    b.row(3, StyleId(2));
    b.cell(1, 0, StyleId(1));
    t.set_file_styles(b.finish(), FileLayout::default());
    let e = t.insert_rows(2, 2).unwrap();
    t.apply(e);
    let mut p = pool();
    let patches = [
        bold(),
        StylePatch {
            italic: Some(true),
            fill: Some(Some(Color::Rgb(0x00FF00))),
            ..StylePatch::default()
        },
        StylePatch {
            border: Some(BorderPreset::All),
            ..StylePatch::default()
        },
        StylePatch {
            clear: true,
            ..StylePatch::default()
        },
        StylePatch {
            bold: Some(false),
            ..StylePatch::default()
        },
    ];
    let targets = [
        cells(1..5, 0..3),
        FormatTarget::Rows {
            rows: 2..6,
            cols: 0..4,
        },
        FormatTarget::Cols(0..3),
        cells(0..8, 1..2),
    ];
    let mut history = Vec::new();
    let mut states = vec![every_style(&t)];
    for target in &targets {
        for patch in &patches {
            history.push(format(&mut t, &mut p, target.clone(), patch));
            states.push(every_style(&t));
        }
    }
    while let Some(undo) = history.pop() {
        states.pop();
        t.apply(undo);
        assert_eq!(&every_style(&t), states.last().unwrap());
    }
    assert_eq!(t.changes(), 2, "the inserted rows only");
}

#[test]
fn the_pool_reuses_records_and_only_grows() {
    let (_f, mut t) = table("pool");
    let mut p = pool();
    let (fonts, xfs) = (p.fonts.len(), p.xfs.len());
    format(&mut t, &mut p, cells(0..1, 0..1), &bold());
    format(&mut t, &mut p, cells(3..4, 2..3), &bold());
    assert_eq!(t.style_of(at(&t, 0, 0)), t.style_of(at(&t, 3, 2)));
    assert_eq!((p.fonts.len(), p.xfs.len()), (fonts + 1, xfs + 1));
    let unbold = StylePatch {
        bold: Some(false),
        ..StylePatch::default()
    };
    format(&mut t, &mut p, cells(0..1, 0..1), &unbold);
    assert_eq!(
        t.style_of(at(&t, 0, 0)),
        StyleId(0),
        "back to the file's own record"
    );
    assert_eq!(t.changes(), 1, "A1 has no user style left");
    assert_eq!((p.fonts.len(), p.xfs.len()), (fonts + 1, xfs + 1));
}

#[test]
fn big_ranges_are_refused() {
    let (_f, t) = table("cap");
    let mut p = pool();
    // The cap counts the cells there are: rows past the end aren't any.
    let rows = REPLACE_MAX_CELLS / 4 + 1;
    let r = t.format_edit(&mut p, cells(0..rows, 0..4), &bold());
    assert!(r.is_ok(), "cut to six rows");
    let wide = t.format_edit(
        &mut p,
        cells(0..6, 0..(REPLACE_MAX_CELLS / 6 + 1) as u32),
        &bold(),
    );
    assert!(matches!(wide, Err(FormatError::TooMany(_))));
}
