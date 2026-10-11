//! SLATE-3: every paragraph of the fixtures matches LibreOffice's dump: text, style, list
//! level, bold, italic, underline, strikethrough, and table cells.
//!
//! Each `X.docx` under `tests/data` (the LibreOffice-built fixture) and `tests/data/poi`
//! (Word-authored samples from Apache POI) has `X-libreoffice.txt`, written by
//! `python3 scripts/lo_docx.py dump X.docx` (format in that script). The reader's blocks
//! are written out in the same shape and compared line by line.
//!
//! Normalization rules, with reasons:
//! - Paragraph styles compare by what the model knows: LibreOffice's display name, minus
//!   the ` (user)` it adds to a style it doesn't recognize, maps to Title, Heading 1-6
//!   (Heading 7-9 show as 6: the model stops at 6), Quote (`Quote`, `Intense Quote`,
//!   `Quotations`, `Block Quotation`), or Normal for every other name.
//! - A list label compares only when it has text: LibreOffice reports `""` as the label of
//!   a bullet (the bullet is a symbol-font glyph, not text), so both sides write `*` for
//!   bullets and empty labels.
//! - Runs compare as the paragraph's text cut where the look changes: both sides join
//!   adjacent runs with the same bold/italic/underline/strike, because the two split runs
//!   differently where nothing changes.
//! - Pictures, page breaks, and rules aren't paragraphs in LibreOffice's text: they are
//!   left out of the model's side.

use document_model::{Block, ListKind, Paragraph, ParagraphStyle, Run};
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn style_name(style: ParagraphStyle) -> String {
    match style {
        ParagraphStyle::Normal => "Normal".into(),
        ParagraphStyle::Title => "Title".into(),
        ParagraphStyle::Heading(n) => format!("Heading {n}"),
        ParagraphStyle::Quote => "Quote".into(),
    }
}

/// LibreOffice's paragraph style display name as the model's style.
fn libreoffice_style(name: &str) -> String {
    let name = name.strip_suffix(" (user)").unwrap_or(name);
    if name == "Title" {
        return name.into();
    }
    if let Some(n) = name.strip_prefix("Heading ") {
        if let Ok(n @ 1..=9) = n.parse::<u8>() {
            return format!("Heading {}", n.min(6));
        }
    }
    match name {
        "Quote" | "Intense Quote" | "Quotations" | "Block Quotation" => "Quote".into(),
        _ => "Normal".into(),
    }
}

fn flags(run: &Run) -> String {
    let s = &run.style;
    let mut f = String::new();
    for (on, c) in [
        (s.bold, 'b'),
        (s.italic, 'i'),
        (s.underline, 'u'),
        (s.strike, 's'),
    ] {
        if on {
            f.push(c);
        }
    }
    if f.is_empty() {
        f.push('-');
    }
    f
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

fn paragraph(p: &Paragraph, list: Option<(u8, &ListKind)>, out: &mut Vec<String>) {
    let (level, label) = match list {
        Some((level, ListKind::Ordered { label, .. })) if !label.is_empty() => {
            (level.to_string(), json(label))
        }
        Some((level, _)) => (level.to_string(), "*".into()),
        None => ("-".into(), "*".into()),
    };
    out.push(format!(
        "para\t{}\t{level}\t{label}\t{}",
        style_name(p.style),
        json(&p.text())
    ));
    let mut runs: Vec<(String, String)> = Vec::new();
    for r in p.runs.iter().filter(|r| !r.text.is_empty()) {
        let f = flags(r);
        match runs.last_mut() {
            Some((last, text)) if *last == f => text.push_str(&r.text),
            _ => runs.push((f, r.text.clone())),
        }
    }
    for (f, text) in runs {
        out.push(format!("run\t{f}\t{}", json(&text)));
    }
}

fn outline(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => paragraph(p, None, out),
            Block::ListItem(item) => {
                paragraph(&item.paragraph, Some((item.level, &item.kind)), out)
            }
            Block::Table(t) => {
                out.push("table".into());
                for row in &t.rows {
                    out.push("row".into());
                    for cell in &row.cells {
                        out.push("cell".into());
                        outline(&cell.blocks, out);
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

/// The dump with styles mapped to the model's, and bullet labels hidden.
fn libreoffice(dump: &str) -> Vec<String> {
    dump.lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f[0] != "para" {
                return line.to_owned();
            }
            let label = if f[2] == "-" || f[3] == "\"\"" {
                "*"
            } else {
                f[3]
            };
            format!(
                "para\t{}\t{}\t{label}\t{}",
                libreoffice_style(f[1]),
                f[2],
                f[4]
            )
        })
        .collect()
}

fn compare(docx: &Path) {
    let dump_path = docx.with_file_name(format!(
        "{}-libreoffice.txt",
        docx.file_stem().unwrap().to_string_lossy()
    ));
    let dump = std::fs::read_to_string(&dump_path).unwrap();
    let expected = libreoffice(&dump);
    let doc = docx::read(docx).unwrap();
    let mut got = Vec::new();
    outline(&doc.blocks, &mut got);
    let first_difference = got
        .iter()
        .zip(&expected)
        .position(|(g, e)| g != e)
        .unwrap_or(got.len().min(expected.len()));
    assert!(
        got == expected,
        "{}: differs from LibreOffice at line {} (of {} vs {}):\n  slate: {:?}\n  lo:    {:?}",
        docx.display(),
        first_difference + 1,
        got.len(),
        expected.len(),
        got.get(first_difference),
        expected.get(first_difference)
    );
}

#[test]
fn the_fixture_matches_libreoffice() {
    compare(&data().join("fixture.docx"));
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
