//! Tables (`<a:tbl>`): the grid, cells with spans and merges, and the table style
//! (`tableStyles.xml`) applied part by part — whole table, banded rows and columns,
//! first/last column, last/first row, corner cells — before each cell's own `tcPr`.

use crate::fill::Looks;
use crate::text::{self, FontRef, Styles, TextContext};
use crate::xml::{self, Element};
use presentation_model::{
    Anchor, Borders, Fill, Insets, Outline, Source, Table, TableCell, TableRow,
};
use std::sync::LazyLock;

/// PowerPoint's default table style ("Medium Style 2 - Accent 1"), for decks that name
/// it without carrying its definition (PowerPoint knows it built in).
const MEDIUM_STYLE_2_ACCENT_1: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";
static DEFAULT_STYLE: LazyLock<Element> = LazyLock::new(|| {
    xml::parse(include_bytes!("medium-style-2-accent-1.xml")).expect("built-in table style")
});

/// The region of the table a style part covers, for choosing outer or inside borders.
struct Region {
    rows: (usize, usize),
    cols: (usize, usize),
}

#[derive(Default)]
struct CellLook {
    fill: Option<Option<presentation_model::Color>>,
    borders: Borders,
    font_ref: FontRef,
}

/// Read a table. `base` are the text styles cell text starts from (the presentation's
/// default and the master's other style); `styles` is `tableStyles.xml`'s root.
pub(crate) fn table(
    tbl: &Element,
    looks: &Looks<'_>,
    ctx: &TextContext<'_>,
    base: &[(&Element, Source)],
    styles: Option<&Element>,
) -> Table {
    let columns: Vec<i64> = tbl
        .child("tblGrid")
        .into_iter()
        .flat_map(|g| g.all("gridCol"))
        .map(|c| c.num("w").unwrap_or(0))
        .collect();
    let rows_el: Vec<&Element> = tbl.all("tr").collect();
    let (n_rows, n_cols) = (rows_el.len(), columns.len());
    let pr = tbl.child("tblPr");
    let flag = |name| pr.and_then(|p| p.flag(name)).unwrap_or(false);
    let (first_row, last_row, first_col, last_col) = (
        flag("firstRow"),
        flag("lastRow"),
        flag("firstCol"),
        flag("lastCol"),
    );
    let (band_row, band_col) = (flag("bandRow"), flag("bandCol"));
    let style = pr.and_then(|p| p.child("tableStyle")).or_else(|| {
        let id = pr?.child("tableStyleId")?.text();
        let id = id.trim();
        styles
            .and_then(|s| s.all("tblStyle").find(|t| t.attr("styleId") == Some(id)))
            .or_else(|| (id == MEDIUM_STYLE_2_ACCENT_1).then(|| &*DEFAULT_STYLE))
    });

    let mut rows = Vec::with_capacity(n_rows);
    for (r, tr) in rows_el.iter().enumerate() {
        let mut cells = Vec::new();
        for (c, tc) in tr.all("tc").enumerate() {
            let mut look = CellLook::default();
            if let Some(style) = style {
                let last_r = n_rows.saturating_sub(1);
                let last_c = n_cols.saturating_sub(1);
                let whole = Region {
                    rows: (0, last_r),
                    cols: (0, last_c),
                };
                let row = Region {
                    rows: (r, r),
                    cols: (0, last_c),
                };
                let col = Region {
                    rows: (0, last_r),
                    cols: (c, c),
                };
                let cell = Region {
                    rows: (r, r),
                    cols: (c, c),
                };
                let mut parts: Vec<(&str, &Region)> = vec![("wholeTbl", &whole)];
                let data_row = !(first_row && r == 0) && !(last_row && r == last_r);
                let data_col = !(first_col && c == 0) && !(last_col && c == last_c);
                if band_col && data_col {
                    let i = c - usize::from(first_col);
                    parts.push((if i % 2 == 0 { "band1V" } else { "band2V" }, &col));
                }
                if band_row && data_row {
                    let i = r - usize::from(first_row);
                    parts.push((if i % 2 == 0 { "band1H" } else { "band2H" }, &row));
                }
                if last_col && c == last_c {
                    parts.push(("lastCol", &col));
                }
                if first_col && c == 0 {
                    parts.push(("firstCol", &col));
                }
                if last_row && r == last_r {
                    parts.push(("lastRow", &row));
                }
                if first_row && r == 0 {
                    parts.push(("firstRow", &row));
                }
                let corner = |top: bool, left: bool| {
                    (if top {
                        first_row && r == 0
                    } else {
                        last_row && r == last_r
                    }) && (if left {
                        first_col && c == 0
                    } else {
                        last_col && c == last_c
                    })
                };
                for (name, top, left) in [
                    ("seCell", false, false),
                    ("swCell", false, true),
                    ("neCell", true, false),
                    ("nwCell", true, true),
                ] {
                    if corner(top, left) {
                        parts.push((name, &cell));
                    }
                }
                for (name, region) in parts {
                    if let Some(part) = style.child(name) {
                        apply_part(&mut look, part, region, r, c, looks, ctx);
                    }
                }
            }
            cells.push(cell(tc, look, looks, ctx, base));
        }
        rows.push(TableRow {
            height: tr.num("h").unwrap_or(0),
            cells,
        });
    }
    Table { columns, rows }
}

fn apply_part(
    look: &mut CellLook,
    part: &Element,
    region: &Region,
    r: usize,
    c: usize,
    looks: &Looks<'_>,
    ctx: &TextContext<'_>,
) {
    if let Some(tc_style) = part.child("tcStyle") {
        if let Some(f) = tc_style.child("fill") {
            if let Some(fill) = looks.fill_in(f, None, &Fill::None) {
                look.fill = Some(solid(&fill));
            }
        } else if let Some(r) = tc_style.child("fillRef") {
            if let Some(fill) = looks.style_fill(r) {
                look.fill = Some(solid(&fill));
            }
        }
        if let Some(bdr) = tc_style.child("tcBdr") {
            let edge = |outer: &str, inner: &str, at_edge: bool| {
                bdr.child(if at_edge { outer } else { inner })
                    .map(|side| border(side, looks))
            };
            let b = &mut look.borders;
            if let Some(v) = edge("top", "insideH", r == region.rows.0) {
                b.top = v;
            }
            if let Some(v) = edge("bottom", "insideH", r == region.rows.1) {
                b.bottom = v;
            }
            if let Some(v) = edge("left", "insideV", c == region.cols.0) {
                b.left = v;
            }
            if let Some(v) = edge("right", "insideV", c == region.cols.1) {
                b.right = v;
            }
        }
    }
    if let Some(tx) = part.child("tcTxStyle") {
        let fr = &mut look.font_ref;
        let on = |v: &str| matches!(v, "on" | "1" | "true");
        if let Some(b) = tx.attr("b") {
            fr.bold = Some(on(b));
        }
        if let Some(i) = tx.attr("i") {
            fr.italic = Some(on(i));
        }
        if let Some(f) = tx.child("fontRef") {
            match f.attr("idx") {
                Some("major") => fr.font = Some(ctx.fonts.major.clone()),
                Some("minor") => fr.font = Some(ctx.fonts.minor.clone()),
                _ => {}
            }
            if let Some(col) = ctx.palette.first(f, None) {
                fr.color = Some(col);
            }
        }
        if let Some(face) = tx.path(&["font", "latin"]).and_then(|l| l.attr("typeface")) {
            fr.font = Some(ctx.fonts.resolve(face));
        }
        // A colour directly in the text style wins over the font reference's.
        if let Some(col) = tx
            .elements()
            .filter(|e| e.name != "fontRef" && e.name != "font")
            .find_map(|e| ctx.palette.color(e, None))
        {
            fr.color = Some(col);
        }
    }
}

/// A border side of a style part: `<a:ln>`, or `<a:lnRef>` into the theme.
fn border(side: &Element, looks: &Looks<'_>) -> Option<Outline> {
    if let Some(ln) = side.child("ln") {
        return looks.border(ln);
    }
    let r = side.child("lnRef")?;
    let idx: usize = r.num("idx")?;
    let ln = looks
        .fmt?
        .child("lnStyleLst")?
        .elements()
        .nth(idx.checked_sub(1)?)?;
    let mut o = looks.border(ln)?;
    if let Some(c) = looks.palette.first(r, None) {
        o.color = c;
    }
    Some(o)
}

fn solid(fill: &Fill) -> Option<presentation_model::Color> {
    match fill {
        Fill::Solid(c) => Some(*c),
        Fill::Unsupported { approx, .. } => Some(*approx),
        Fill::None | Fill::Picture(_) => None,
    }
}

fn cell(
    tc: &Element,
    mut look: CellLook,
    looks: &Looks<'_>,
    ctx: &TextContext<'_>,
    base: &[(&Element, Source)],
) -> TableCell {
    let pr = tc.child("tcPr");
    if let Some(pr) = pr {
        if let Some(fill) = looks.fill_in(pr, None, &Fill::None) {
            look.fill = Some(solid(&fill));
        }
        let b = &mut look.borders;
        for (name, slot) in [
            ("lnT", &mut b.top),
            ("lnB", &mut b.bottom),
            ("lnL", &mut b.left),
            ("lnR", &mut b.right),
        ] {
            if let Some(ln) = pr.child(name) {
                *slot = looks.border(ln);
            }
        }
    }
    let tx_body = tc.child("txBody");
    let styles = Styles {
        base: base.to_vec(),
        font_ref: look.font_ref,
        shape: tx_body
            .and_then(|b| b.child("lstStyle"))
            .map(|s| (s, Source::Own))
            .into_iter()
            .collect(),
        body: Vec::new(),
    };
    // Cell text isn't edited (yet), so its markup isn't kept: no source.
    let mut text = text::body(tx_body, &[], &styles, ctx).unwrap_or_else(|| {
        let empty = xml::parse(b"<txBody/>").expect("empty body");
        text::body(Some(&empty), &[], &styles, ctx).expect("a body")
    });
    // Cell margins and anchoring come from `tcPr`, with PowerPoint's defaults.
    let m = |name, default| pr.and_then(|p| p.num(name)).unwrap_or(default);
    let d = Insets::default();
    text.insets = Insets {
        left: m("marL", d.left),
        top: m("marT", d.top),
        right: m("marR", d.right),
        bottom: m("marB", d.bottom),
    };
    text.anchor = match pr.and_then(|p| p.attr("anchor")) {
        Some("ctr") => Anchor::Middle,
        Some("b") => Anchor::Bottom,
        _ => Anchor::Top,
    };
    TableCell {
        text,
        fill: look.fill.flatten(),
        borders: look.borders,
        col_span: tc.num("gridSpan").unwrap_or(1).max(1),
        row_span: tc.num("rowSpan").unwrap_or(1).max(1),
        merged: tc.flag("hMerge") == Some(true) || tc.flag("vMerge") == Some(true),
    }
}
