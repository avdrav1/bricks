//! SLATE-3, SLATE-19, SLATE-26, and SLATE-27: every paragraph of the fixtures matches
//! LibreOffice's dump: text, paragraph style (by name), list level, alignment, its spacing,
//! line spacing, indents, contextual spacing, borders, and fill (its style's with its own
//! on top), each run's font, size, bold, italic, underline, strikethrough, and colour, and
//! table cells.
//!
//! Each `X.docx` under `tests/data` (the LibreOffice-built fixture, and the spacing and
//! borders documents written by hand as Word writes them) and `tests/data/poi`
//! (Word-authored samples from Apache POI) has `X-libreoffice.txt`, written by
//! `python3 scripts/lo_docx.py dump X.docx` (format in that script). The reader's blocks
//! are written out in the same shape and compared line by line.
//!
//! Normalization rules, with reasons:
//! - LibreOffice shows some of Word's built-in styles under its own names: the names in
//!   [`LIBREOFFICE_NAMES`] compare as Word's. A document's own style whose name is one of
//!   LibreOffice's built-in display names (the fixture's `Block Quotation`, LibreOffice's
//!   name for its Quotations style) shows with ` (user)` after it: that suffix is dropped.
//! - A list label compares only when it has text: LibreOffice reports `""` as the label of
//!   a bullet (the bullet is a symbol-font glyph, not text), so both sides write `*` for
//!   bullets and empty labels.
//! - Spacing, indents, and border widths and distances compare within a twip: LibreOffice
//!   keeps them in 1/100 mm, so a value in twips comes back rounded.
//! - Border line styles compare as the model draws them: LibreOffice's dot-dash styles as
//!   dashed, its thin-thick ones as double, and embossed, engraved, inset, and outset as
//!   solid. An automatic border colour is LibreOffice's black.
//! - LibreOffice reads the line between paragraphs bordered together (Word's `between`)
//!   as the top border of each but the first, and leaves the bottom border off each but
//!   the last: the model's side gives them so.
//! - Runs compare as the paragraph's text cut where the look changes: both sides join
//!   adjacent runs with the same look, because the two split runs differently where
//!   nothing changes.
//! - Pictures, page breaks, and rules aren't paragraphs in LibreOffice's text: they are
//!   left out of the model's side.

use document_model::{
    Align, Block, Border, BorderStyle, Document, LineSpacing, ListKind, Paragraph,
};
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// LibreOffice's display names for Word's built-in styles it has under names of its own,
/// and Word's names for them.
const LIBREOFFICE_NAMES: &[(&str, &str)] = &[
    ("Default Paragraph Style", "Normal"),
    ("Body Text, Indented", "Body Text Indent"),
];

/// A LibreOffice display name as the model names the style.
fn libreoffice_name(name: &str) -> &str {
    let name = name.strip_suffix(" (user)").unwrap_or(name);
    LIBREOFFICE_NAMES
        .iter()
        .find(|(lo, _)| *lo == name)
        .map_or(name, |(_, word)| word)
}

fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn align(a: Align) -> &'static str {
    match a {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
        Align::Justify => "justify",
    }
}

fn line(l: LineSpacing) -> String {
    match l {
        LineSpacing::Auto(m) => format!("auto:{}", (m * 100.0).round()),
        LineSpacing::Exact(t) => format!("exact:{t}"),
        LineSpacing::AtLeast(t) => format!("atLeast:{t}"),
    }
}

/// A border as the dump writes it: `-`, or its style, whole width and distance from the
/// text in twips, and colour.
fn border(b: &Border) -> String {
    let style = match b.style {
        BorderStyle::None => return "-".into(),
        BorderStyle::Single | BorderStyle::Thick => "solid",
        BorderStyle::Double => "double",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Dashed => "dashed",
    };
    let [r, g, bl] = b.color.unwrap_or_default();
    format!(
        "{style}:{}:{}:{r:02X}{g:02X}{bl:02X}",
        (f64::from(b.line_width()) * 2.5).round(),
        u32::from(b.space) * 20
    )
}

/// A size in half-points as the dump writes points (Python's `:g`).
fn points(half: u16) -> String {
    if half.is_multiple_of(2) {
        (half / 2).to_string()
    } else {
        format!("{}.5", half / 2)
    }
}

/// Paragraphs and runs compared, for the report.
#[derive(Default)]
struct Counts {
    paragraphs: usize,
    runs: usize,
}

/// The paragraph of a paragraph or list item.
fn paragraph_of(block: &Block) -> Option<&Paragraph> {
    match block {
        Block::Paragraph(p) => Some(p),
        Block::ListItem(item) => Some(&item.paragraph),
        _ => None,
    }
}

/// Paragraph `p` (between `prev` and `next`) as the dump writes it.
fn paragraph(
    doc: &Document,
    p: &Paragraph,
    list: Option<(u8, &ListKind)>,
    (prev, next): (Option<&Paragraph>, Option<&Paragraph>),
    out: &mut Vec<String>,
) {
    let (level, label) = match list {
        Some((level, ListKind::Ordered { label, .. })) if !label.is_empty() => {
            (level.to_string(), json(label))
        }
        Some((level, _)) => (level.to_string(), "*".into()),
        None => ("-".into(), "*".into()),
    };
    let style = doc.styles.style_of(&p.style);
    let look = doc.styles.spacing(p);
    let mut borders = doc.styles.borders(p);
    if borders.between.is_drawn() {
        if next.is_some_and(|n| doc.styles.bordered_together(p, n)) {
            borders.bottom = Border::default();
        }
        if prev.is_some_and(|n| doc.styles.bordered_together(n, p)) {
            borders.top = borders.between;
        }
    }
    let fill = doc
        .styles
        .shading(p)
        .fill
        .map_or("-".to_owned(), |[r, g, b]| format!("{r:02X}{g:02X}{b:02X}"));
    out.push(format!(
        "para\t{}\t{level}\t{label}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{fill}\t{}",
        style.name,
        align(p.align),
        look.space_before,
        look.space_after,
        line(look.line),
        look.indent_left,
        look.indent_right,
        look.indent_first,
        u8::from(look.contextual),
        border(&borders.top),
        border(&borders.bottom),
        border(&borders.left),
        border(&borders.right),
        json(&p.text())
    ));
    let mut runs: Vec<(String, String)> = Vec::new();
    for r in p.runs.iter().filter(|r| !r.text.is_empty()) {
        let l = doc.styles.look(&p.style, &r.style);
        let mut flags: String = [
            (l.bold, 'b'),
            (l.italic, 'i'),
            (l.underline, 'u'),
            (l.strike, 's'),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, c)| *c)
        .collect();
        if flags.is_empty() {
            flags.push('-');
        }
        let color = l.color.map_or("auto".to_owned(), |[r, g, b]| {
            format!("{r:02X}{g:02X}{b:02X}")
        });
        let look = format!("{flags}\t{}\t{}\t{color}", l.font, points(l.size));
        match runs.last_mut() {
            Some((last, text)) if *last == look => text.push_str(&r.text),
            _ => runs.push((look, r.text.clone())),
        }
    }
    for (look, text) in runs {
        out.push(format!("run\t{look}\t{}", json(&text)));
    }
}

fn outline(doc: &Document, blocks: &[Block], out: &mut Vec<String>) {
    for (i, b) in blocks.iter().enumerate() {
        let around = (
            i.checked_sub(1).and_then(|j| paragraph_of(&blocks[j])),
            blocks.get(i + 1).and_then(paragraph_of),
        );
        match b {
            Block::Paragraph(p) => paragraph(doc, p, None, around, out),
            Block::ListItem(item) => paragraph(
                doc,
                &item.paragraph,
                Some((item.level, &item.kind)),
                around,
                out,
            ),
            Block::Table(t) => {
                out.push("table".into());
                for row in &t.rows {
                    out.push("row".into());
                    for cell in &row.cells {
                        out.push("cell".into());
                        outline(doc, &cell.blocks, out);
                    }
                }
                out.push("end".into());
            }
            Block::CodeBlock(_)
            | Block::Image(_)
            | Block::PageBreak
            | Block::Rule
            | Block::Raw(_) => {}
        }
    }
}

/// A LibreOffice border line style as the model draws it.
fn drawn_style(style: &str) -> &str {
    match style {
        "dash_dot" | "dash_dot_dot" | "fine_dashed" => "dashed",
        "double_thin" => "double",
        s if s.starts_with("thinthick") || s.starts_with("thickthin") => "double",
        "embossed" | "engraved" | "inset" | "outset" => "solid",
        s => s,
    }
}

/// The dump with LibreOffice's names for Word's styles mapped back, bullet labels hidden,
/// and border styles as the model draws them.
fn libreoffice(dump: &str) -> Vec<String> {
    dump.lines()
        .map(|line| {
            let mut f: Vec<String> = line.split('\t').map(str::to_owned).collect();
            if f[0] != "para" {
                return line.to_owned();
            }
            f[1] = libreoffice_name(&f[1]).to_owned();
            if f[2] == "-" || f[3] == "\"\"" {
                f[3] = "*".into();
            }
            for side in &mut f[12..16] {
                if let Some((style, rest)) = side.split_once(':') {
                    *side = format!("{}:{rest}", drawn_style(style));
                }
            }
            f.join("\t")
        })
        .collect()
}

/// Whether two outline lines match: equal, but for spacing, indents, and border widths and
/// distances within a twip.
fn matches(slate: &str, lo: &str) -> bool {
    if slate == lo {
        return true;
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (slate.split('\t').collect(), lo.split('\t').collect());
    if a.len() != b.len() || a[0] != "para" {
        return false;
    }
    let near = |x: &str, y: &str| match (x.parse::<i64>(), y.parse::<i64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() <= 1,
        _ => x == y,
    };
    a.iter().zip(&b).enumerate().all(|(i, (x, y))| match i {
        5 | 6 | 8..=10 => near(x, y),
        7 => match (x.split_once(':'), y.split_once(':')) {
            (Some((m, v)), Some((n, w))) if m == n && m != "auto" => near(v, w),
            _ => x == y,
        },
        12..=15 => {
            let (x, y): (Vec<&str>, Vec<&str>) = (x.split(':').collect(), y.split(':').collect());
            x.len() == y.len()
                && x.iter().zip(&y).enumerate().all(|(j, (u, v))| {
                    if j == 1 || j == 2 {
                        near(u, v)
                    } else {
                        u == v
                    }
                })
        }
        _ => x == y,
    })
}

fn compare(docx: &Path) -> Counts {
    let dump_path = docx.with_file_name(format!(
        "{}-libreoffice.txt",
        docx.file_stem().unwrap().to_string_lossy()
    ));
    let dump = std::fs::read_to_string(&dump_path).unwrap();
    let expected = libreoffice(&dump);
    let doc = docx::read(docx).unwrap();
    let mut got = Vec::new();
    outline(&doc, &doc.blocks, &mut got);
    let first_difference = got
        .iter()
        .zip(&expected)
        .position(|(g, e)| !matches(g, e))
        .unwrap_or(got.len().min(expected.len()));
    assert!(
        got.len() == expected.len() && first_difference == got.len(),
        "{}: differs from LibreOffice at line {} (of {} vs {}):\n  slate: {:?}\n  lo:    {:?}",
        docx.display(),
        first_difference + 1,
        got.len(),
        expected.len(),
        got.get(first_difference),
        expected.get(first_difference)
    );
    let count = |kind: &str| got.iter().filter(|l| l.starts_with(kind)).count();
    let counts = Counts {
        paragraphs: count("para\t"),
        runs: count("run\t"),
    };
    eprintln!(
        "{}: {} paragraphs, {} runs",
        docx.display(),
        counts.paragraphs,
        counts.runs
    );
    counts
}

#[test]
fn the_fixture_matches_libreoffice() {
    let counts = compare(&data().join("fixture.docx"));
    assert!(counts.paragraphs > 30 && counts.runs > 30);
}

/// SLATE-26: a paragraph's own spacing and indents over its style's, written as Word and
/// Google Docs write them (`python3 scripts/lo_docx.py spacing`).
#[test]
fn the_spacing_document_matches_libreoffice() {
    let counts = compare(&data().join("spacing.docx"));
    assert_eq!(counts.paragraphs, 21);
}

/// SLATE-27: paragraph borders and shading, a paragraph's own over its style's, written as
/// Word and Google Docs write them (`python3 scripts/lo_docx.py borders`).
#[test]
fn the_borders_document_matches_libreoffice() {
    let counts = compare(&data().join("borders.docx"));
    assert_eq!(counts.paragraphs, 22);
}

#[test]
fn word_authored_samples_match_libreoffice() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(data().join("poi"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 3,
        "the Apache POI samples are in tests/data/poi"
    );
    for f in files {
        compare(&f);
    }
}
