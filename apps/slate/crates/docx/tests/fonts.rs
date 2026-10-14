//! SLATE-25: the font table (ADR 0009). Each `w:font`'s class, pitch, and alternative
//! name read; its embedded faces resolved through `word/_rels/fontTable.xml.rels`, a plain
//! `.ttf` with a zero key as it is and Word's obfuscated `.odttf` de-obfuscated with its
//! `w:fontKey`, both giving back the font file; and saving an edited document keeps the
//! font parts, the font table, and its relationships byte for byte.

use document_edit::{normalize, replace, text_len, Container, History, Pos};
use document_model::{FaceKind, FontFamily, FontPitch, FontTable};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const KEY: &str = "{6C2F5D8A-91E3-4B07-A2C4-3F1E9D8B7A65}";
const ZERO: &str = "{00000000-0000-0000-0000-000000000000}";

/// Cardo, OFL (a Latin subset), as the GUI check embeds it.
fn cardo() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/fonts/Cardo-Regular.ttf");
    std::fs::read(path).unwrap()
}

/// `bytes` obfuscated with GUID `key` as LibreOffice reads it: the key's hex pairs taken
/// from its string last to first, XORed over the first 32 bytes twice.
fn obfuscated(mut bytes: Vec<u8>, key: &str) -> Vec<u8> {
    const POS: [usize; 16] = [35, 33, 31, 29, 27, 25, 22, 20, 17, 15, 12, 10, 7, 5, 3, 1];
    let k: Vec<u8> = POS
        .iter()
        .map(|&p| u8::from_str_radix(&key[p..p + 2], 16).unwrap())
        .collect();
    for i in 0..16 {
        bytes[i] ^= k[i];
        bytes[i + 16] ^= k[i];
    }
    bytes
}

/// A package with a font table: Cardo embedding its regular face as a plain `.ttf` (zero
/// key) and its bold as an obfuscated `.odttf`, an italic whose part is missing, and fonts
/// that embed nothing.
fn package() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, bytes: &[u8]| {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    };
    let ct = "application/vnd.openxmlformats-officedocument.wordprocessingml";
    put(
        "[Content_Types].xml",
        format!(
            "{HEAD}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
             <Default Extension=\"ttf\" ContentType=\"application/x-font-ttf\"/>\
             <Override PartName=\"/word/fonts/font2.odttf\" \
             ContentType=\"application/vnd.openxmlformats-officedocument.obfuscatedFont\"/>\
             <Override PartName=\"/word/document.xml\" ContentType=\"{ct}.document.main+xml\"/>\
             <Override PartName=\"/word/fontTable.xml\" ContentType=\"{ct}.fontTable+xml\"/></Types>"
        )
        .as_bytes(),
    );
    put(
        "_rels/.rels",
        format!(
            "{HEAD}<Relationships xmlns=\"{PKG}\">\
             <Relationship Id=\"rId1\" Type=\"{R}/officeDocument\" Target=\"word/document.xml\"/>\
             </Relationships>"
        )
        .as_bytes(),
    );
    put(
        "word/_rels/document.xml.rels",
        format!(
            "{HEAD}<Relationships xmlns=\"{PKG}\">\
             <Relationship Id=\"rId7\" Type=\"{R}/fontTable\" Target=\"fontTable.xml\"/>\
             </Relationships>"
        )
        .as_bytes(),
    );
    put(
        "word/fontTable.xml",
        format!(
            "{HEAD}<w:fonts xmlns:w=\"{W}\" xmlns:r=\"{R}\">\
             <w:font w:name=\"Cardo\"><w:altName w:val=\"Cardo Text\"/><w:family w:val=\"roman\"/>\
             <w:pitch w:val=\"variable\"/>\
             <w:embedRegular w:fontKey=\"{ZERO}\" r:id=\"rId1\" w:subsetted=\"0\"/>\
             <w:embedBold w:fontKey=\"{KEY}\" r:id=\"rId2\" w:subsetted=\"1\"/>\
             <w:embedItalic w:fontKey=\"{KEY}\" r:id=\"rId3\"/></w:font>\
             <w:font w:name=\"Nosuchfont Serifish\"><w:family w:val=\"roman\"/></w:font>\
             <w:font w:name=\"Nosuchfont Sansish\"><w:family w:val=\"swiss\"/>\
             <w:pitch w:val=\"variable\"/></w:font>\
             <w:font w:name=\"Courier New\"><w:family w:val=\"modern\"/><w:pitch w:val=\"fixed\"/>\
             </w:font></w:fonts>"
        )
        .as_bytes(),
    );
    put(
        "word/_rels/fontTable.xml.rels",
        format!(
            "{HEAD}<Relationships xmlns=\"{PKG}\">\
             <Relationship Id=\"rId1\" Type=\"{R}/font\" Target=\"fonts/font1.ttf\"/>\
             <Relationship Id=\"rId2\" Type=\"{R}/font\" Target=\"fonts/font2.odttf\"/>\
             <Relationship Id=\"rId3\" Type=\"{R}/font\" Target=\"fonts/missing.odttf\"/>\
             </Relationships>"
        )
        .as_bytes(),
    );
    put("word/fonts/font1.ttf", &cardo());
    put("word/fonts/font2.odttf", &obfuscated(cardo(), KEY));
    put(
        "word/document.xml",
        format!(
            "{HEAD}<w:document xmlns:w=\"{W}\"><w:body>\
             <w:p><w:r><w:rPr><w:rFonts w:ascii=\"Cardo\" w:hAnsi=\"Cardo\"/></w:rPr>\
             <w:t>Embedded</w:t></w:r></w:p><w:p><w:r><w:t>Plain</w:t></w:r></w:p>\
             </w:body></w:document>"
        )
        .as_bytes(),
    );
    zip.finish().unwrap().into_inner()
}

fn table(doc: &document_model::Document) -> &FontTable {
    doc.format
        .fonts()
        .expect("a Word document has a font table")
}

#[test]
fn the_font_table_reads_classes_and_embedded_faces() {
    let doc = docx::read_from(Cursor::new(package())).unwrap();
    let table = table(&doc);
    let names: Vec<_> = table
        .fonts
        .iter()
        .map(|f| (f.name.as_str(), f.family, f.pitch))
        .collect();
    assert_eq!(
        names,
        [
            ("Cardo", FontFamily::Roman, FontPitch::Variable),
            ("Nosuchfont Serifish", FontFamily::Roman, FontPitch::Default),
            ("Nosuchfont Sansish", FontFamily::Swiss, FontPitch::Variable),
            ("Courier New", FontFamily::Modern, FontPitch::Fixed),
        ]
    );
    let cardo_font = table.get("cardo").unwrap();
    assert_eq!(cardo_font.alt_name.as_deref(), Some("Cardo Text"));
    // The plain .ttf as it is, the .odttf de-obfuscated: both the font file; the italic's
    // part is missing, so it has none.
    let faces: Vec<_> = cardo_font
        .faces
        .iter()
        .map(|f| (f.kind, f.subsetted, f.bytes.as_ref() == cardo().as_slice()))
        .collect();
    assert_eq!(
        faces,
        [
            (FaceKind::Regular, false, true),
            (FaceKind::Bold, true, true)
        ]
    );
    assert!(table.fonts[1..].iter().all(|f| f.faces.is_empty()));
    assert!(table.has_embedded());
}

#[test]
fn a_package_without_a_font_table_has_an_empty_one() {
    let doc = docx::read_from(Cursor::new(docx::blank().to_vec())).unwrap();
    assert!(!table(&doc).has_embedded());
    let doc = document_model::read_markdown(b"# x\n");
    assert!(doc.format.fonts().is_none());
}

/// A file `name` in a directory of test `test`'s own.
fn temp(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("docx-fonts-{}", std::process::id()))
        .join(test);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn part(path: &Path, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

#[test]
fn saving_keeps_the_font_parts_byte_for_byte() {
    let path = temp("save", "fonts.docx");
    std::fs::write(&path, package()).unwrap();
    let (mut doc, mut source) = docx::read_source(&path).unwrap();
    normalize(&mut doc);
    source.set_blocks(doc.blocks.clone());
    let at = Pos::new(1, text_len(&doc, Container::Top, 1).unwrap());
    let edit = replace(&doc, at, at, " edited", &document_edit::look(&doc, at)).unwrap();
    History::new().apply(&mut doc, edit);
    let dest = temp("save", "saved.docx");
    docx::save(&dest, &path, &doc.blocks, &source, &AtomicBool::new(false)).unwrap();
    for name in [
        "word/fontTable.xml",
        "word/_rels/fontTable.xml.rels",
        "word/fonts/font1.ttf",
        "word/fonts/font2.odttf",
        "[Content_Types].xml",
    ] {
        assert!(part(&dest, name) == part(&path, name), "{name} changed");
    }
    let xml = String::from_utf8(part(&dest, "word/document.xml")).unwrap();
    assert!(xml.contains("Plain edited"), "{xml}");
    let back = docx::read(&dest).unwrap();
    assert_eq!(table(&back), table(&doc));
}
