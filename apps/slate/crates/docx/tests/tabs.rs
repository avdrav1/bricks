//! SLATE-24: tab stops (ADR 0009). A paragraph's `w:tabs` and its style chain's merge,
//! `clear` removes an inherited stop, bar stops are left out, and `w:defaultTabStop` in
//! `word/settings.xml` is the default stop (0.5″ without one). Saving keeps the stops: an
//! edited paragraph keeps its own `w:tabs`, and the settings part is untouched.

use document_edit::{normalize, replace, split, text_len, Container, History, Pos};
use document_model::{Block, Document, DocxInfo, Format, Paragraph, TabAlign, TabLeader, TabStop};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";

/// A package of `body`, with `styles` (the children of `w:styles`) and, when given, a
/// settings part of `settings` (the children of `w:settings`).
fn package(body: &str, styles: &str, settings: Option<&str>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, xml: String| {
        zip.start_file(name, options).unwrap();
        zip.write_all(format!("{HEAD}{xml}").as_bytes()).unwrap();
    };
    let ct = "application/vnd.openxmlformats-officedocument.wordprocessingml";
    put(
        "[Content_Types].xml",
        format!(
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
             <Override PartName=\"/word/document.xml\" ContentType=\"{ct}.document.main+xml\"/>\
             <Override PartName=\"/word/styles.xml\" ContentType=\"{ct}.styles+xml\"/>\
             <Override PartName=\"/word/settings.xml\" ContentType=\"{ct}.settings+xml\"/></Types>"
        ),
    );
    put(
        "_rels/.rels",
        format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"word/document.xml\"/>\
             </Relationships>"
        ),
    );
    put(
        "word/_rels/document.xml.rels",
        format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"{REL}/settings\" Target=\"settings.xml\"/>\
             </Relationships>"
        ),
    );
    put(
        "word/styles.xml",
        format!("<w:styles xmlns:w=\"{W}\">{styles}</w:styles>"),
    );
    if let Some(settings) = settings {
        put(
            "word/settings.xml",
            format!("<w:settings xmlns:w=\"{W}\">{settings}</w:settings>"),
        );
    }
    put(
        "word/document.xml",
        format!("<w:document xmlns:w=\"{W}\"><w:body>{body}</w:body></w:document>"),
    );
    zip.finish().unwrap().into_inner()
}

fn read(bytes: Vec<u8>) -> Document {
    docx::read_from(Cursor::new(bytes)).unwrap()
}

fn stop(position: i32, align: TabAlign, leader: TabLeader) -> TabStop {
    TabStop {
        position,
        align,
        leader,
    }
}

fn left(position: i32) -> TabStop {
    stop(position, TabAlign::Left, TabLeader::None)
}

fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            Block::ListItem(i) => Some(&i.paragraph),
            _ => None,
        })
        .collect()
}

/// A paragraph in style `style` (none: the default) with `ppr` in its `w:pPr`, of `text`.
fn para(style: Option<&str>, ppr: &str, text: &str) -> String {
    let style = style.map_or(String::new(), |s| format!("<w:pStyle w:val=\"{s}\"/>"));
    format!(
        "<w:p><w:pPr>{style}{ppr}</w:pPr><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>"
    )
}

/// Normal; Base (stops at 1440 left and 2880 centre); Derived on Base (clears 1440, adds
/// a right stop at 9360 with a dot leader).
const STYLES: &str = "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\">\
     <w:name w:val=\"Normal\"/></w:style>\
     <w:style w:type=\"paragraph\" w:styleId=\"Base\"><w:name w:val=\"Base\"/>\
     <w:basedOn w:val=\"Normal\"/><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"1440\"/>\
     <w:tab w:val=\"center\" w:pos=\"2880\"/></w:tabs></w:pPr></w:style>\
     <w:style w:type=\"paragraph\" w:styleId=\"Derived\"><w:name w:val=\"Derived\"/>\
     <w:basedOn w:val=\"Base\"/><w:pPr><w:tabs><w:tab w:val=\"clear\" w:pos=\"1440\"/>\
     <w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"9360\"/></w:tabs></w:pPr></w:style>";

#[test]
fn a_paragraphs_own_stops_read_with_their_alignments_and_leaders() {
    let tabs = "<w:tabs><w:tab w:val=\"decimal\" w:pos=\"5040\"/>\
                <w:tab w:val=\"start\" w:pos=\"720\"/><w:tab w:val=\"end\" w:leader=\"hyphen\" w:pos=\"10800\"/>\
                <w:tab w:val=\"bar\" w:pos=\"3000\"/><w:tab w:val=\"center\" w:leader=\"underscore\" w:pos=\"5400\"/>\
                <w:tab w:val=\"left\" w:leader=\"middleDot\" w:pos=\"6000\"/>\
                <w:tab w:val=\"left\" w:leader=\"heavy\" w:pos=\"6500\"/></w:tabs>";
    let doc = read(package(&para(None, tabs, "a\tb"), STYLES, None));
    let p = paragraphs(&doc)[0];
    assert_eq!(
        p.tabs,
        [
            left(720),
            stop(5040, TabAlign::Decimal, TabLeader::None),
            stop(5400, TabAlign::Center, TabLeader::Underscore),
            stop(6000, TabAlign::Left, TabLeader::MiddleDot),
            stop(6500, TabAlign::Left, TabLeader::Heavy),
            stop(10800, TabAlign::Right, TabLeader::Hyphen),
        ],
        "in position order, start and end as left and right, the bar stop left out"
    );
    assert_eq!(doc.styles.tab_stops(p), p.tabs);
    assert_eq!(p.text(), "a\tb");
}

#[test]
fn styles_stops_merge_through_based_on_and_clear_removes_one() {
    let body = para(Some("Base"), "", "base")
        + &para(Some("Derived"), "", "derived")
        + &para(
            Some("Derived"),
            "<w:tabs><w:tab w:val=\"clear\" w:pos=\"2880\"/><w:tab w:val=\"left\" w:pos=\"4320\"/></w:tabs>",
            "own",
        );
    let doc = read(package(&body, STYLES, None));
    let base = doc.styles.get("Base").unwrap();
    assert_eq!(
        base.para.tabs,
        [left(1440), stop(2880, TabAlign::Center, TabLeader::None)]
    );
    let derived = doc.styles.get("Derived").unwrap();
    assert_eq!(
        derived.para.tabs,
        [
            stop(2880, TabAlign::Center, TabLeader::None),
            stop(9360, TabAlign::Right, TabLeader::Dot)
        ],
        "Derived clears Base's 1440 and adds its own"
    );
    let ps = paragraphs(&doc);
    assert!(ps[0].tabs.is_empty() && ps[1].tabs.is_empty());
    assert_eq!(
        ps[2].tabs,
        [stop(2880, TabAlign::Clear, TabLeader::None), left(4320)],
        "the paragraph's own stops keep their clear"
    );
    assert_eq!(
        doc.styles.tab_stops(ps[2]),
        [left(4320), stop(9360, TabAlign::Right, TabLeader::Dot)],
        "its own stops on top of its style's, the cleared one gone"
    );
}

#[test]
fn a_style_clears_a_stop_the_document_defaults_set() {
    let styles = format!(
        "<w:docDefaults><w:pPrDefault><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"1000\"/>\
         <w:tab w:val=\"left\" w:pos=\"2000\"/></w:tabs></w:pPr></w:pPrDefault></w:docDefaults>{STYLES}\
         <w:style w:type=\"paragraph\" w:styleId=\"Clearing\"><w:name w:val=\"Clearing\"/>\
         <w:pPr><w:tabs><w:tab w:val=\"clear\" w:pos=\"1000\"/></w:tabs></w:pPr></w:style>"
    );
    let doc = read(package(&para(Some("Clearing"), "", "x"), &styles, None));
    assert_eq!(
        doc.styles.get("Normal").unwrap().para.tabs,
        [left(1000), left(2000)]
    );
    assert_eq!(doc.styles.get("Clearing").unwrap().para.tabs, [left(2000)]);
    assert_eq!(doc.styles.tab_stops(paragraphs(&doc)[0]), [left(2000)]);
}

fn default_tab(settings: Option<&str>) -> u32 {
    match read(package(&para(None, "", "x"), STYLES, settings)).format {
        Format::Docx(info) => info.default_tab,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_default_stop_is_the_settings_or_half_an_inch() {
    assert_eq!(
        default_tab(Some(
            "<w:zoom w:percent=\"100\"/><w:defaultTabStop w:val=\"567\"/>"
        )),
        567
    );
    assert_eq!(
        default_tab(Some("<w:zoom w:percent=\"100\"/>")),
        720,
        "none set"
    );
    assert_eq!(
        default_tab(Some("<w:defaultTabStop w:val=\"0\"/>")),
        720,
        "0"
    );
    assert_eq!(default_tab(None), 720, "no settings part");
    assert_eq!(DocxInfo::default().default_tab, 720);
}

/// A file `name` in a directory of test `test`'s own.
fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("docx-tabs-{}", std::process::id()))
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn part(path: &Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

const SETTINGS: &str = "<w:defaultTabStop w:val=\"360\"/>";

/// The tabbed paragraph: a bar stop the model leaves out, a right stop with a dot
/// leader, and a clear of its style's centre stop.
const OWN: &str =
    "<w:tabs><w:tab w:val=\"clear\" w:pos=\"2880\"/><w:tab w:val=\"bar\" w:pos=\"3000\"/>\
                   <w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"7200\"/></w:tabs>";

/// The tabbed document, opened as Slate opens it.
fn tabbed(test: &str) -> (PathBuf, Document, docx::DocxSource) {
    let body = para(Some("Derived"), OWN, "Name\tPage")
        + &para(None, "", "Plain")
        + "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>";
    let path = temp(test, "tabs.docx");
    std::fs::write(&path, package(&body, STYLES, Some(SETTINGS))).unwrap();
    let (mut doc, mut source) = docx::read_source(&path).unwrap();
    normalize(&mut doc);
    source.set_blocks(doc.blocks.clone());
    (path, doc, source)
}

fn save(path: &Path, dest: &Path, doc: &Document, source: &docx::DocxSource) {
    docx::save(dest, path, &doc.blocks, source, &AtomicBool::new(false)).unwrap();
}

#[test]
fn an_edited_paragraph_keeps_its_own_stops_and_the_settings() {
    let (path, mut doc, source) = tabbed("edited");
    let own = paragraphs(&doc)[0].tabs.clone();
    assert_eq!(
        own,
        [
            stop(2880, TabAlign::Clear, TabLeader::None),
            stop(7200, TabAlign::Right, TabLeader::Dot)
        ]
    );
    let mut history = History::new();
    let at = Pos::new(0, text_len(&doc, Container::Top, 0).unwrap());
    let edit = replace(&doc, at, at, " 12", &document_edit::look(&doc, at)).unwrap();
    history.apply(&mut doc, edit);
    // Enter after "Name": both halves keep the paragraph's stops.
    let edit = split(&doc, Pos::new(0, 4), Pos::new(0, 4)).unwrap();
    history.apply(&mut doc, edit);
    let dest = temp("edited", "saved.docx");
    save(&path, &dest, &doc, &source);
    assert_eq!(
        part(&dest, "word/settings.xml"),
        part(&path, "word/settings.xml")
    );
    let xml = part(&dest, "word/document.xml");
    assert_eq!(
        xml.matches(OWN).count(),
        2,
        "both edited paragraphs keep the w:tabs, bar stop included: {xml}"
    );
    let back = docx::read(&dest).unwrap();
    let ps = paragraphs(&back);
    assert_eq!(ps[0].text(), "Name");
    assert_eq!(ps[1].text(), "\tPage 12");
    assert_eq!((&ps[0].tabs, &ps[1].tabs), (&own, &own));
    assert!(ps[2].tabs.is_empty());
    assert_eq!(back.format.default_tab(), Some(360));
}

#[test]
fn a_paragraph_whose_stops_differ_from_its_templates_writes_its_own() {
    let (path, mut doc, source) = tabbed("differ");
    // Stops aren't edited yet; a paragraph that came with others (pasted from another
    // document) stands for one.
    let Block::Paragraph(p) = &mut doc.blocks[0] else {
        panic!("a paragraph");
    };
    p.tabs = vec![stop(4000, TabAlign::Center, TabLeader::Underscore)];
    p.runs[0].text.push('!');
    let dest = temp("differ", "saved.docx");
    save(&path, &dest, &doc, &source);
    let xml = part(&dest, "word/document.xml");
    assert!(
        xml.contains(
            "<w:tabs><w:tab w:val=\"center\" w:leader=\"underscore\" w:pos=\"4000\"/></w:tabs>"
        ),
        "{xml}"
    );
    assert!(!xml.contains("w:val=\"bar\""), "{xml}");
    let back = docx::read(&dest).unwrap();
    assert_eq!(
        paragraphs(&back)[0].tabs,
        [stop(4000, TabAlign::Center, TabLeader::Underscore)]
    );
}
