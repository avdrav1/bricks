//! SLATE-7: text files write back byte for byte; a Markdown file keeps the bytes of every
//! block that didn't change, and what is written fresh reads back as the blocks it was.

use document_model::{
    encode, load_linked_images, read_markdown, read_markdown_source, read_text, write_markdown,
    write_text, Block, Encoding, Format, ImageSource, LineEnding, ListItem, ListKind,
    MarkdownSource, Paragraph, Run, RunStyle, StyleSheet, TextInfo, Unencodable,
};
use std::path::{Path, PathBuf};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn fixtures(extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(data(""))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == extension))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    files
}

fn text_info(doc: &document_model::Document) -> TextInfo {
    match doc.format {
        Format::Text(info) | Format::Markdown(info) => info,
        Format::Docx => panic!("not text"),
    }
}

/// The fixture as read: its bytes, blocks, format, and source.
fn markdown(bytes: &[u8]) -> (Vec<Block>, TextInfo, MarkdownSource) {
    let (doc, source) = read_markdown_source(bytes);
    let info = text_info(&doc);
    (doc.blocks, info, source)
}

fn fixture() -> Vec<u8> {
    std::fs::read(data("fixture.md")).unwrap()
}

fn write(blocks: &[Block], info: &TextInfo, source: Option<&MarkdownSource>) -> Vec<u8> {
    write_markdown(blocks, &StyleSheet::builtin(), info, source)
        .unwrap()
        .0
}

fn paragraph(text: &str) -> Block {
    Block::Paragraph(Paragraph {
        runs: vec![Run {
            text: text.into(),
            style: RunStyle::default(),
        }],
        ..Paragraph::new("Normal")
    })
}

/// Blocks compared one by one, so a failure names the first that differs.
#[track_caller]
fn assert_blocks(got: &[Block], want: &[Block], written: &[u8]) {
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(
            g,
            w,
            "block {i} differs; written:\n{}",
            String::from_utf8_lossy(written)
        );
    }
    assert_eq!(
        got.len(),
        want.len(),
        "block count; written:\n{}",
        String::from_utf8_lossy(written)
    );
}

/// `out` equals `orig` outside `orig[changed]`: it starts with all of `orig` before the
/// range and ends with all of it after.
#[track_caller]
fn assert_only_changed(orig: &[u8], out: &[u8], changed: std::ops::Range<usize>) {
    let (head, tail) = (&orig[..changed.start], &orig[changed.end..]);
    assert!(
        out.len() >= head.len() + tail.len() && out.starts_with(head) && out.ends_with(tail),
        "expected changes only within {changed:?} ({:?}); written:\n{}",
        String::from_utf8_lossy(&orig[changed.clone()]),
        String::from_utf8_lossy(out)
    );
}

/// The byte range of the line holding `needle` in `text`, line break included.
fn line_of(text: &[u8], needle: &str) -> std::ops::Range<usize> {
    let s = std::str::from_utf8(text).unwrap();
    let at = s
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in the text"));
    let start = s[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = s[at..].find('\n').map_or(s.len(), |i| at + i + 1);
    start..end
}

fn index_of(blocks: &[Block], text: &str) -> usize {
    blocks
        .iter()
        .position(|b| match b {
            Block::Paragraph(p) => p.text() == text,
            Block::ListItem(item) => item.paragraph.text() == text,
            _ => false,
        })
        .unwrap_or_else(|| panic!("a block reading {text:?}"))
}

fn set_text(block: &mut Block, text: &str) {
    let p = match block {
        Block::Paragraph(p) => p,
        Block::ListItem(item) => &mut item.paragraph,
        other => panic!("no text in {other:?}"),
    };
    p.runs = vec![Run {
        text: text.into(),
        style: RunStyle::default(),
    }];
}

// 1. Plain text.

#[test]
fn every_text_fixture_writes_back_byte_for_byte() {
    for path in fixtures("txt") {
        let bytes = std::fs::read(&path).unwrap();
        let doc = read_text(&bytes);
        let out = write_text(&doc.blocks, &text_info(&doc)).unwrap();
        assert_eq!(out, bytes, "{}", path.display());
    }
    let cases: [&[u8]; 9] = [
        b"",
        b"\n",
        b"\n\n",
        b"one line, no newline",
        b"\xEF\xBB\xBF",
        b"\xEF\xBB\xBFa\r\n\r\nb",
        b"\xFF\xFEa\0\r\0",
        b"caf\xE9 \x80 \x81\x8D\x8F\x90\x9D\r\n",
        b"tabs\tand trailing spaces  \nlast",
    ];
    for bytes in cases {
        let doc = read_text(bytes);
        let out = write_text(&doc.blocks, &text_info(&doc)).unwrap();
        assert_eq!(out, bytes, "{:?}", String::from_utf8_lossy(bytes));
    }
}

#[test]
fn text_of_other_blocks_is_the_clipboards() {
    let doc = read_markdown(&fixture());
    let info = TextInfo {
        encoding: Encoding::Utf8,
        line_ending: LineEnding::Lf,
        final_newline: true,
    };
    let out = String::from_utf8(write_text(&doc.blocks, &info).unwrap()).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    // A paragraph's hard break stays a line break; list items lose their markers; a
    // table gives a line a cell; a picture its alt text; a rule nothing.
    assert!(lines.contains(&"and this line continues the paragraph."));
    assert!(lines.contains(&"Third level bullet"));
    let table = lines.iter().position(|l| *l == "Name").unwrap();
    assert_eq!(lines[table..table + 3], ["Name", "Count", "Note"]);
    assert_eq!(lines[table + 9], "A blue picture");
    assert_eq!(lines.last(), Some(&"The last paragraph."));
}

// 6. Encodings.

#[test]
fn text_an_encoding_cant_hold_is_an_error_and_utf16_has_its_mark() {
    let info = |encoding| TextInfo {
        encoding,
        line_ending: LineEnding::CrLf,
        final_newline: true,
    };
    let blocks = [paragraph("Café"), paragraph("仮名")];
    let err = write_text(&blocks, &info(Encoding::Windows1252)).unwrap_err();
    assert_eq!(
        err,
        Unencodable {
            ch: '仮',
            encoding: Encoding::Windows1252
        }
    );
    assert_eq!(err.to_string(), "Windows-1252 has no “仮”");
    assert_eq!(
        write_text(&blocks[..1], &info(Encoding::Windows1252)).unwrap(),
        b"Caf\xE9\r\n"
    );
    let le = write_text(&blocks, &info(Encoding::Utf16Le)).unwrap();
    assert!(le.starts_with(b"\xFF\xFEC\0a\0"));
    let be = write_text(&blocks, &info(Encoding::Utf16Be)).unwrap();
    assert!(be.starts_with(b"\xFE\xFF\0C\0a"));
    assert_eq!(encode("x", Encoding::Utf8Bom).unwrap(), b"\xEF\xBB\xBFx");
    assert_eq!(read_text(&le).blocks, blocks);
    assert_eq!(read_text(&be).blocks, blocks);
    // Markdown goes through the same encoder.
    let md = write_markdown(
        &blocks,
        &StyleSheet::builtin(),
        &info(Encoding::Windows1252),
        None,
    )
    .unwrap_err();
    assert_eq!(md.ch, '仮');
}

// 2. Markdown fixtures: unchanged writes the same bytes; fresh reads back the same.

/// Every Markdown fixture, and the big one with CRLF and CR line endings, in UTF-16, and
/// without its final newline.
fn markdown_cases() -> Vec<(String, Vec<u8>)> {
    let mut cases: Vec<(String, Vec<u8>)> = fixtures("md")
        .into_iter()
        .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
        .collect();
    let text = String::from_utf8(fixture()).unwrap();
    cases.push(("fixture.md, CRLF".into(), text.replace('\n', "\r\n").into()));
    cases.push(("fixture.md, CR".into(), text.replace('\n', "\r").into()));
    cases.push((
        "fixture.md, UTF-16".into(),
        encode(&text, Encoding::Utf16Le).unwrap(),
    ));
    cases.push((
        "fixture.md, no final newline".into(),
        text.trim_end().as_bytes().to_vec(),
    ));
    cases
}

#[test]
fn every_markdown_fixture_writes_back_unchanged() {
    for (name, bytes) in markdown_cases() {
        let (blocks, info, source) = markdown(&bytes);
        let (out, written) =
            write_markdown(&blocks, &StyleSheet::builtin(), &info, Some(&source)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&bytes),
            "{name}"
        );
        assert_eq!(out, bytes, "{name}");
        // The source returned serves the next save the same way.
        assert_eq!(write(&blocks, &info, Some(&written)), bytes, "{name}");
    }
}

#[test]
fn every_markdown_fixture_written_fresh_reads_back_the_same() {
    for (name, bytes) in markdown_cases() {
        // pulldown-cmark doesn't end every line at a lone CR (a fence's lines run on), so
        // the CR file's blocks are not what a fresh writer can give back.
        if name.ends_with(", CR") {
            continue;
        }
        let (blocks, info, _) = markdown(&bytes);
        let out = write(&blocks, &info, None);
        let again = read_markdown(&out);
        assert_blocks(&again.blocks, &blocks, &out);
        assert_eq!(again.format, Format::Markdown(info), "{name}");
    }
}

#[test]
fn constructs_written_fresh_read_back_the_same() {
    let cases = [
        "- a\n- b\n  - c\n    1. d\n    2. e\n- f\n",
        "1. one\n   - nested\n2. two\n",
        "1. one\n2. two\n\nbetween\n\n5. five\n6. six\n",
        "3) three\n4) four\n",
        "- item one\n\n  more of item one\n\n  ```\n  code\n  ```\n- item two\n",
        "- [ ] open\n- [x] done\n  - [ ] nested task\n",
        "> quote\n>\n> > deeper\n> > > deepest\n",
        "> - listed in a quote\n",
        "- # heading in an item\n",
        "Setext\n======\n\nOther\n-----\n",
        "#\n\n## \\#hashes\\##\n",
        "````md\n```\ninner fence\n```\n````\n",
        "~~~\ntilde fence\n~~~\n",
        "```\n```\n",
        "```\n\n\nblank lines\n\n```\n",
        "| a | b \\| c | `x \\| y` |\n| --- | :-: | --: |\n| 1 | 2 | 3 |\n| | only two |\n",
        "![alt *text* \\[x\\]](<a b.png>)\n\n![](x(1).png)\n\n![x](a\\(b.png)\n",
        "[link](https://example.com/a_(b)_c) and [x](<a b>) and [y](a\\)b)\n",
        "Text with &amp; and &copy; and \\&#32; and AT&T\n",
        "\\* not a list\n\n\\- not a list\n\n\\+ not a list\n\n1\\. not a list\n\n12\\) neither\n",
        "\\# not a heading\n\n\\> not a quote\n\n\\=== not setext\n",
        "a\\\nb\\\n\\\nc\n",
        "trailing hard break before&#32;\\\nnext\n",
        "&#32;   leading spaces\n",
        "<div>\nhtml\n</div>\n\nafter\n",
        "Footnote[^n] here.\n\n[^n]: The note.\n",
        "[ref]: /url\n\n[used][ref]\n",
        "***\n\n___\n",
        "a <span>raw</span> b <!-- comment --> c\n",
        "snake_case_name and 2*3*4 and a * b and x _ y\n",
        "~one~ ~~two~~ and a ~ b\n",
        "\\`not code\\` and ``code with ` tick`` and `` `edge` ``\n",
        "-\n  ```\n  code in an empty item\n  ```\n",
        "- ![picture](p.png) in an item\n",
        "Tab\there and C:\\path\\to\\file and \\\\server\n",
        "1. a\n1. b\n1. c\n",
        "- a\n\n- b\n",
    ];
    let info = TextInfo {
        encoding: Encoding::Utf8,
        line_ending: LineEnding::Lf,
        final_newline: true,
    };
    for case in cases {
        let blocks = read_markdown(case.as_bytes()).blocks;
        let out = write(&blocks, &info, None);
        let again = read_markdown(&out).blocks;
        assert_blocks(&again, &blocks, &out);
    }
}

// 3. One edited paragraph.

#[test]
fn an_edited_paragraph_changes_only_its_lines() {
    let bytes = fixture();
    let (mut blocks, info, source) = markdown(&bytes);
    for needle in [
        "A list that starts at seven:",
        "The last paragraph.",
        "Second bullet",
        "Nested number",
        "A done task",
    ] {
        let i = index_of(&blocks, needle);
        let before = blocks[i].clone();
        set_text(&mut blocks[i], "Edited *text*, with 1. specials #");
        let out = write(&blocks, &info, Some(&source));
        assert_only_changed(&bytes, &out, line_of(&bytes, needle));
        assert_blocks(&read_markdown(&out).blocks, &blocks, &out);
        blocks[i] = before;
    }
    // An edited heading, table, and code block too.
    let table = blocks
        .iter()
        .position(|b| matches!(b, Block::Table(_)))
        .unwrap();
    let code = blocks
        .iter()
        .position(|b| matches!(b, Block::CodeBlock(_)))
        .unwrap();
    type Edit = Box<dyn Fn(&mut Block)>;
    let edits: [(usize, &str, Edit); 3] = [
        (
            index_of(&blocks, "Third level heading"),
            "Third level heading",
            Box::new(|b| set_text(b, "Renamed")),
        ),
        (
            table,
            "| Apples | 3 | *fresh* |",
            Box::new(|b| {
                if let Block::Table(t) = b {
                    set_text(&mut t.rows[1].cells[0].blocks[0], "Plums | prunes");
                }
            }),
        ),
        (
            code,
            "    println!(\"hello\");",
            Box::new(|b| {
                if let Block::CodeBlock(c) = b {
                    c.text = c.text.replace("hello", "edited");
                }
            }),
        ),
    ];
    for (i, needle, edit) in edits {
        let line = line_of(&bytes, needle);
        let before = blocks[i].clone();
        edit(&mut blocks[i]);
        assert_ne!(blocks[i], before);
        let out = write(&blocks, &info, Some(&source));
        let text = std::str::from_utf8(&bytes).unwrap();
        // The block's lines: from the blank line before it to the blank line after it.
        let start = text[..line.start].rfind("\n\n").map_or(0, |p| p + 2);
        let end = text[line.end..]
            .find("\n\n")
            .map_or(text.len(), |p| line.end + p + 1);
        assert_only_changed(&bytes, &out, start..end);
        assert_blocks(&read_markdown(&out).blocks, &blocks, &out);
        blocks[i] = before;
    }
}

// 4. Deleting, inserting, and moving blocks.

#[test]
fn a_deleted_block_takes_only_its_lines() {
    let bytes = fixture();
    let (blocks, info, source) = markdown(&bytes);
    for needle in ["Fourth level heading", "A done task", "The last paragraph."] {
        let mut edited = blocks.clone();
        edited.remove(index_of(&blocks, needle));
        let out = write(&edited, &info, Some(&source));
        let line = line_of(&bytes, needle);
        assert_only_changed(
            &bytes,
            &out,
            line.start - 1..(line.end + 1).min(bytes.len()),
        );
        assert!(out.len() < bytes.len() - needle.len());
        assert_blocks(&read_markdown(&out).blocks, &edited, &out);
    }
    // Deleting the first block drops what came before it with it.
    let mut edited = blocks.clone();
    edited.remove(0);
    let out = write(&edited, &info, Some(&source));
    assert!(out.starts_with(b"A paragraph with *italic*"));
    assert_blocks(&read_markdown(&out).blocks, &edited, &out);
}

#[test]
fn a_deleted_paragraph_between_numbered_lists_keeps_their_numbers() {
    let bytes = fixture();
    let (blocks, info, source) = markdown(&bytes);
    let mut edited = blocks.clone();
    edited.remove(index_of(&blocks, "A list that starts at seven:"));
    let out = write(&edited, &info, Some(&source));
    assert_blocks(&read_markdown(&out).blocks, &edited, &out);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("3. Third number\n\n7) Seven starts this list\n"));
}

#[test]
fn inserted_paragraphs_read_back_and_leave_the_rest_alone() {
    let bytes = fixture();
    let (blocks, info, source) = markdown(&bytes);
    let cases: [(&str, usize, &str); 5] = [
        // Between two items of a tight list: blank lines around it.
        (
            "between items",
            index_of(&blocks, "Second bullet"),
            "- First bullet\n\nInserted between items\n\n- Second bullet\n",
        ),
        // After a tight list.
        (
            "after a list",
            index_of(&blocks, "A list that starts at seven:"),
            "3. Third number\n\nInserted after a list\n\nA list that",
        ),
        (
            "at the start",
            0,
            "Inserted at the start\n\n# Slate Markdown",
        ),
        (
            "at the end",
            blocks.len(),
            "The last paragraph.\n\nInserted at the end\n",
        ),
        // Right after a nested item that ended with one line break.
        (
            "after a nested item",
            index_of(&blocks, "Back to the first level"),
            "    - Third level bullet\n\nInserted after a nested item\n\n- Back to",
        ),
    ];
    for (what, at, expect) in cases {
        let mut edited = blocks.clone();
        edited.insert(at, paragraph(&format!("Inserted {what}")));
        let out = write(&edited, &info, Some(&source));
        let text = String::from_utf8(out.clone()).unwrap();
        assert!(text.contains(expect), "{what}: {expect:?} in\n{text}");
        assert_blocks(&read_markdown(&out).blocks, &edited, &out);
        // The original text is all there, around the new paragraph.
        let new = text.find(&format!("Inserted {what}")).unwrap();
        let orig = std::str::from_utf8(&bytes).unwrap();
        let (head, tail) = (&text[..new], &text[new..]);
        let tail = &tail[tail.find('\n').unwrap_or(tail.len())..];
        let cut = head.trim_end().len();
        assert!(orig.starts_with(&head[..cut]), "{what}: head kept");
        assert!(orig.ends_with(tail.trim_start()), "{what}: tail kept");
    }
}

#[test]
fn a_new_list_item_between_items_keeps_the_list_tight() {
    let bytes = fixture();
    let (blocks, info, source) = markdown(&bytes);
    let mut edited = blocks.clone();
    let at = index_of(&blocks, "Second bullet");
    edited.insert(
        at,
        Block::ListItem(ListItem {
            kind: ListKind::Bullet,
            level: 0,
            paragraph: match paragraph("New bullet") {
                Block::Paragraph(mut p) => {
                    p.indent = 1;
                    p
                }
                _ => unreachable!(),
            },
        }),
    );
    let out = write(&edited, &info, Some(&source));
    let text = String::from_utf8(out.clone()).unwrap();
    assert!(text.contains("- First bullet\n- New bullet\n- Second bullet\n"));
    assert_blocks(&read_markdown(&out).blocks, &edited, &out);
}

#[test]
fn a_moved_picture_keeps_its_url() {
    let bytes = fixture();
    let (blocks, info, mut source) = markdown(&bytes);
    // As the app shows it: the picture loaded from disk.
    let mut shown = blocks.clone();
    load_linked_images(&mut shown, &data(""));
    let i = shown
        .iter()
        .position(|b| matches!(b, Block::Image(_)))
        .unwrap();
    assert!(matches!(
        &shown[i],
        Block::Image(image) if matches!(image.source, ImageSource::Embedded { .. })
    ));
    source.set_blocks(shown.clone());
    let mut edited = shown.clone();
    let image = edited.remove(i);
    edited.insert(1, image);
    let out = write(&edited, &info, Some(&source));
    let text = String::from_utf8(out.clone()).unwrap();
    assert_eq!(text.matches("![A blue picture](picture.png)").count(), 1);
    assert!(text
        .starts_with("# Slate Markdown fixture\n\n![A blue picture](picture.png)\n\nA paragraph"));
    assert!(!text.contains("data:"));
    let mut again = read_markdown(&out).blocks;
    load_linked_images(&mut again, &data(""));
    assert_blocks(&again, &edited, &out);
    // A new picture with no source is written as a data URI.
    let mut edited = shown.clone();
    let mut new = edited[i].clone();
    if let Block::Image(image) = &mut new {
        image.alt = "Pasted".into();
    }
    edited.push(new);
    let out = String::from_utf8(write(&edited, &info, Some(&source))).unwrap();
    assert!(out.ends_with(")\n") && out.contains("![Pasted](data:image/png;base64,iVBORw0KGgo"));
}

#[test]
fn a_picture_inside_a_paragraph_is_copied_with_it_or_rewritten_with_it() {
    let bytes = b"Before ![pic](p.png) after.\n\nNext.\n";
    let (mut blocks, info, source) = markdown(bytes);
    assert_eq!(blocks.len(), 4);
    set_text(&mut blocks[3], "Next, edited.");
    let out = write(&blocks, &info, Some(&source));
    assert_eq!(out, b"Before ![pic](p.png) after.\n\nNext, edited.\n");
    set_text(&mut blocks[0], "Changed ");
    let out = write(&blocks, &info, Some(&source));
    assert_blocks(&read_markdown(&out).blocks, &blocks, &out);
}

// 5. Line endings of fresh text.

#[test]
fn a_crlf_file_gets_crlf_in_fresh_blocks() {
    let text = String::from_utf8(fixture()).unwrap().replace('\n', "\r\n");
    let bytes = text.as_bytes();
    let (mut blocks, info, source) = markdown(bytes);
    assert_eq!(info.line_ending, LineEnding::CrLf);
    let i = index_of(&blocks, "A list that starts at seven:");
    set_text(&mut blocks[i], "Edited\nwith a line break");
    let code = blocks
        .iter()
        .position(|b| matches!(b, Block::CodeBlock(_)))
        .unwrap();
    if let Block::CodeBlock(c) = &mut blocks[code] {
        c.text.push_str("// more\n");
    }
    blocks.push(paragraph("Appended"));
    let out = write(&blocks, &info, Some(&source));
    let out_text = String::from_utf8(out.clone()).unwrap();
    assert!(out_text.contains("Edited\\\r\nwith a line break\r\n"));
    assert!(out_text.contains("// more\r\n```\r\n"));
    assert!(out_text.ends_with("The last paragraph.\r\n\r\nAppended\r\n"));
    let lone = out_text
        .match_indices('\n')
        .filter(|(i, _)| *i == 0 || out_text.as_bytes()[i - 1] != b'\r')
        .count();
    assert_eq!(lone, 0, "no lone line feed in\n{out_text}");
    assert_blocks(&read_markdown(&out).blocks, &blocks, &out);
}

// 7. Inline looks.

fn run(text: &str, look: &str) -> Run {
    Run {
        text: text.into(),
        style: RunStyle {
            bold: look.contains('b').then_some(true),
            italic: look.contains('i').then_some(true),
            strike: look.contains('s').then_some(true),
            code: look.contains('c'),
            raw: look.contains('r'),
            ..RunStyle::default()
        },
    }
}

fn linked(mut run: Run, url: &str) -> Run {
    run.style.link = Some(url.into());
    run
}

fn runs_of(blocks: &[Block]) -> Vec<Run> {
    match blocks {
        [Block::Paragraph(p)] => p.runs.clone(),
        other => panic!("one paragraph, not {other:?}"),
    }
}

/// `runs` written as a paragraph, and what reading that back gives.
fn round_trip(runs: Vec<Run>) -> (String, Vec<Run>) {
    let block = Block::Paragraph(Paragraph {
        runs,
        ..Paragraph::default()
    });
    let info = TextInfo {
        encoding: Encoding::Utf8,
        line_ending: LineEnding::Lf,
        final_newline: true,
    };
    let out = write(&[block], &info, None);
    let back = runs_of(&read_markdown(&out).blocks);
    (String::from_utf8(out).unwrap(), back)
}

#[test]
fn inline_looks_read_back_as_written() {
    let cases: Vec<Vec<Run>> = vec![
        // Overlapping looks.
        vec![run("bold ", "b"), run("both", "bi"), run(" bold", "b")],
        vec![run("italic ", "i"), run("both", "bi"), run(" italic", "i")],
        vec![run("a ", ""), run("bold ", "b"), run("and ", "bi"), run("struck", "bis")],
        vec![run("all three", "bis"), run(" then ", ""), run("italic", "i")],
        vec![run("italic", "i"), run("both", "bi"), run("again", "i")],
        vec![run("bold", "b"), run("both", "bi"), run("again", "b")],
        vec![run("struck ", "s"), run("bold struck", "bs"), run(" plain", "")],
        // Specials in plain text.
        vec![run(
            "Stars * and *stars*, _under_ score_d, # hash, [brackets](x), <tag> & &amp; \\ `ticks` ~tilde~ | pipe",
            "",
        )],
        vec![run("# not a heading", "")],
        vec![run("1. not a list", "")],
        vec![run("- not a list", "")],
        vec![run("+ not a list", "")],
        vec![run("> not a quote", "")],
        vec![run("=== not a heading underline", "")],
        vec![run("    four spaces", "")],
        vec![run("line\n# two\n1) three\n   - four", "")],
        vec![run("trailing spaces  \nafter", "")],
        vec![run("ends in a backslash \\", "")],
        vec![run("Footnote[^1] stays", "")],
        // Code spans.
        vec![run("code ", ""), run("with `backticks`", "c")],
        vec![run("``", "c")],
        vec![run(" padded ", "c")],
        vec![run("bold code", "bc"), run(" and ", ""), run("*stars*", "c")],
        // Links, with looks inside.
        vec![
            run("see ", ""),
            linked(run("bold", "b"), "https://example.com/a"),
            linked(run(" link", ""), "https://example.com/a"),
            run(" here", ""),
        ],
        vec![
            linked(run("[brackets]", ""), "x y"),
            linked(run("next", "i"), "(paren"),
        ],
        // Raw inline HTML.
        vec![
            run("press ", ""),
            run("<kbd>", "r"),
            run("Ctrl", "b"),
            run("</kbd>", "r"),
        ],
    ];
    for runs in cases {
        let (written, back) = round_trip(runs.clone());
        assert_eq!(back, runs, "written as {written:?}");
    }
}

#[test]
fn whitespace_at_a_looks_edge_reads_back_outside_it() {
    // CommonMark won't open `**` before a space or close it after one: the spaces are
    // written outside the delimiters, so they read back in the look around.
    let (written, back) = round_trip(vec![run("a", ""), run(" bold ", "b"), run("c", "")]);
    assert_eq!(written, "a **bold** c\n");
    assert_eq!(back, [run("a ", ""), run("bold", "b"), run(" c", "")]);
    let (written, back) = round_trip(vec![run("bold ", "b"), run("italic", "i")]);
    assert_eq!(written, "**bold** *italic*\n");
    assert_eq!(back, [run("bold", "b"), run(" ", ""), run("italic", "i")]);
    // Looks that overlap without nesting close and reopen around the space between.
    let (written, back) = round_trip(vec![
        run("bold ", "b"),
        run("both", "bi"),
        run(" italic", "i"),
    ]);
    assert_eq!(written, "**bold *both*** *italic*\n");
    assert_eq!(
        back,
        [
            run("bold ", "b"),
            run("both", "bi"),
            run(" ", ""),
            run("italic", "i")
        ]
    );
    // Spaces at a paragraph's ends are kept as references.
    let (written, back) = round_trip(vec![run("  both ends\t", "")]);
    assert_eq!(written, "&#32; both ends&#9;\n");
    assert_eq!(back, [run("  both ends\t", "")]);
}

// Random edits: wherever fresh writing reads back right, so does copying from the source,
// the source returned serves the next save, and a second edit against it reads back too.

/// A small deterministic random number source.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize % n.max(1)
    }
}

fn random_edit(blocks: &mut Vec<Block>, r: &mut Lcg) {
    let n = blocks.len();
    if n == 0 {
        return;
    }
    let i = r.below(n);
    let fresh = |r: &mut Lcg| {
        let Block::Paragraph(p) = paragraph("Fresh text") else {
            unreachable!()
        };
        match r.below(5) {
            0 => Block::Paragraph(p),
            1 => Block::ListItem(ListItem {
                kind: ListKind::Bullet,
                level: r.below(2) as u8,
                paragraph: Paragraph { indent: 1, ..p },
            }),
            2 => Block::ListItem(ListItem {
                kind: ListKind::Ordered {
                    number: 1 + r.below(9) as u64,
                    label: String::new(),
                },
                level: 0,
                paragraph: Paragraph { indent: 1, ..p },
            }),
            3 => Block::CodeBlock(document_model::CodeBlock {
                language: None,
                text: "x\n".into(),
                indent: r.below(2) as u8,
            }),
            _ => Block::Paragraph(Paragraph { indent: 1, ..p }),
        }
    };
    match r.below(5) {
        0 => {
            blocks.remove(i);
        }
        1 => {
            let b = blocks[i].clone();
            blocks.insert(r.below(n + 1), b);
        }
        2 => {
            let b = blocks.remove(i);
            blocks.insert(r.below(n), b);
        }
        3 => {
            let b = fresh(r);
            blocks.insert(r.below(n + 1), b);
        }
        _ => {
            if let Block::Paragraph(p) | Block::ListItem(ListItem { paragraph: p, .. }) =
                &mut blocks[i]
            {
                p.push(" edited", &RunStyle::default());
            }
        }
    }
}

#[test]
fn random_edits_read_back_wherever_fresh_writing_does() {
    let fixture = String::from_utf8(fixture()).unwrap();
    let sources = [
        fixture.clone(),
        fixture.replace('\n', "\r\n"),
        "- a\n- b\n  - c\n    1. d\n    2. e\n- f\n\n1. one\n   - nested\n2. two\n\nbetween\n\n\
         5. five\n6. six\n\n- item one\n\n  more of item one\n\n  ```\n  code\n  ```\n- item two\n\n\
         > quote\n>\n> > deeper\n\nText ![pic](p.png) more.\n\n-\n  ```\n  x\n  ```\n"
            .to_owned(),
    ];
    let mut r = Lcg(7);
    let mut tried = 0;
    for text in &sources {
        let (blocks, info, source) = markdown(text.as_bytes());
        for _ in 0..1_000 {
            let mut edited = blocks.clone();
            for _ in 0..1 + r.below(3) {
                random_edit(&mut edited, &mut r);
            }
            let fresh = write(&edited, &info, None);
            if read_markdown(&fresh).blocks != edited {
                continue;
            }
            tried += 1;
            let (out, written) =
                write_markdown(&edited, &StyleSheet::builtin(), &info, Some(&source)).unwrap();
            assert_blocks(&read_markdown(&out).blocks, &edited, &out);
            assert_eq!(write(&edited, &info, Some(&written)), out);
            let mut again = edited.clone();
            random_edit(&mut again, &mut r);
            if read_markdown(&write(&again, &info, None)).blocks == again {
                let out = write(&again, &info, Some(&written));
                assert_blocks(&read_markdown(&out).blocks, &again, &out);
            }
        }
    }
    assert!(tried > 1_200, "only {tried} edits written");
}
