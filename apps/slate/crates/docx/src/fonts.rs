//! The font table (`word/fontTable.xml`, ADR 0009): each `w:font`'s name, class
//! (`w:family`), pitch, and alternative name, and the faces it embeds (`w:embedRegular`,
//! `w:embedBold`, `w:embedItalic`, `w:embedBoldItalic`) through the table's relationships.
//!
//! Word obfuscates the fonts it embeds (ECMA-376 Part 1, § 17.8.1): the first 32 bytes of
//! the file are XORed with the GUID of the face's `w:fontKey`, its bytes in reverse order
//! of their hex digits. Such a part is named `.odttf` with content type
//! `application/vnd.openxmlformats-officedocument.obfuscatedFont`. Google Docs embeds
//! plain `.ttf` files with a zero key. A face is de-obfuscated unless it's a plain part
//! that already starts as a TrueType or OpenType file, and dropped unless it starts as one
//! afterwards. The bytes are never parsed here: only fontconfig and FreeType read them.

use crate::{rel_kind, xml, DocxError, Package, Rel};
use document_model::{EmbeddedFace, FaceKind, Font, FontFamily, FontPitch, FontTable};
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::{Read, Seek};

/// The content type of Word's obfuscated fonts.
const OBFUSCATED: &str = "application/vnd.openxmlformats-officedocument.obfuscatedFont";

impl<R: Read + Seek> Package<R> {
    /// The font table the main part's relationships `rels` name (else
    /// `word/fontTable.xml`), its faces read; empty when the package has none.
    pub(crate) fn font_table(
        &mut self,
        rels: &HashMap<String, Rel>,
    ) -> Result<FontTable, DocxError> {
        let name = Self::part_of(rels, "fontTable", "word/fontTable.xml");
        let Some(xml) = self.text(&name)? else {
            return Ok(FontTable::default());
        };
        let declared = parse(&xml)?;
        let mut table = FontTable::default();
        if declared.iter().all(|f| f.embeds.is_empty()) {
            table.fonts = declared.into_iter().map(|f| f.font).collect();
            return Ok(table);
        }
        let font_rels = self.rels(&name)?;
        let types = self.content_types()?;
        for Declared { mut font, embeds } in declared {
            for embed in embeds {
                let Some(rel) = font_rels.get(&embed.id).filter(|r| !r.external) else {
                    continue;
                };
                if rel_kind(rel) != "font" {
                    continue;
                }
                let Some(bytes) = self.bytes(&rel.target)? else {
                    continue;
                };
                let obfuscated = rel.target.to_ascii_lowercase().ends_with(".odttf")
                    || types.of(&rel.target) == Some(OBFUSCATED);
                if let Some(bytes) = deobfuscate(bytes, embed.key.as_deref(), obfuscated) {
                    font.faces.push(EmbeddedFace {
                        kind: embed.kind,
                        subsetted: embed.subsetted,
                        bytes: bytes.into(),
                    });
                }
            }
            table.fonts.push(font);
        }
        Ok(table)
    }

    /// `[Content_Types].xml`: the content type of each part named, and by extension.
    fn content_types(&mut self) -> Result<ContentTypes, DocxError> {
        let mut types = ContentTypes::default();
        let Some(xml) = self.text("[Content_Types].xml")? else {
            return Ok(types);
        };
        let mut r = xml::reader(&xml);
        loop {
            match r.read_event()? {
                Event::Empty(e) | Event::Start(e) => {
                    let ty = xml::attr(&e, b"ContentType").map(|t| t.into_owned());
                    match (xml::local(&e), ty) {
                        (b"Override", Some(ty)) => {
                            if let Some(part) = xml::attr(&e, b"PartName") {
                                let part = part.trim_start_matches('/').to_ascii_lowercase();
                                types.parts.insert(part, ty);
                            }
                        }
                        (b"Default", Some(ty)) => {
                            if let Some(ext) = xml::attr(&e, b"Extension") {
                                types.extensions.insert(ext.to_ascii_lowercase(), ty);
                            }
                        }
                        _ => {}
                    }
                }
                Event::Eof => return Ok(types),
                _ => {}
            }
        }
    }
}

/// A package's content types (part names and extensions compare ignoring ASCII case, as
/// OPC says).
#[derive(Default)]
struct ContentTypes {
    parts: HashMap<String, String>,
    extensions: HashMap<String, String>,
}

impl ContentTypes {
    fn of(&self, part: &str) -> Option<&str> {
        let part = part.to_ascii_lowercase();
        if let Some(ty) = self.parts.get(&part) {
            return Some(ty);
        }
        let (_, ext) = part.rsplit_once('.')?;
        self.extensions.get(ext).map(String::as_str)
    }
}

/// A `w:font` as the table declares it.
struct Declared {
    font: Font,
    embeds: Vec<Embed>,
}

/// A `w:embed*` element: which face, its relationship, and its key.
struct Embed {
    kind: FaceKind,
    id: String,
    key: Option<String>,
    subsetted: bool,
}

/// The fonts of a font table part.
fn parse(xml: &str) -> Result<Vec<Declared>, DocxError> {
    let declared = |e: &quick_xml::events::BytesStart<'_>| Declared {
        font: Font {
            name: xml::attr(e, b"name").unwrap_or_default().into_owned(),
            family: FontFamily::Auto,
            pitch: FontPitch::Default,
            alt_name: None,
            faces: Vec::new(),
        },
        embeds: Vec::new(),
    };
    let mut r = xml::reader(xml);
    let mut out: Vec<Declared> = Vec::new();
    let mut open = false;
    loop {
        match r.read_event()? {
            Event::Start(e) if xml::local(&e) == b"font" => {
                out.push(declared(&e));
                open = true;
            }
            Event::Empty(e) if xml::local(&e) == b"font" => out.push(declared(&e)),
            Event::End(e) if e.local_name().as_ref() == b"font" => open = false,
            Event::Empty(e) | Event::Start(e) if open => {
                let Some(last) = out.last_mut() else { continue };
                let kind = match xml::local(&e) {
                    b"family" => {
                        last.font.family = FontFamily::parse(&xml::val(&e).unwrap_or_default());
                        continue;
                    }
                    b"pitch" => {
                        last.font.pitch = FontPitch::parse(&xml::val(&e).unwrap_or_default());
                        continue;
                    }
                    b"altName" => {
                        last.font.alt_name = xml::val(&e).map(|v| v.into_owned());
                        continue;
                    }
                    b"embedRegular" => FaceKind::Regular,
                    b"embedBold" => FaceKind::Bold,
                    b"embedItalic" => FaceKind::Italic,
                    b"embedBoldItalic" => FaceKind::BoldItalic,
                    _ => continue,
                };
                if let Some(id) = xml::attr(&e, b"id") {
                    last.embeds.push(Embed {
                        kind,
                        id: id.into_owned(),
                        key: xml::attr(&e, b"fontKey").map(|k| k.into_owned()),
                        subsetted: xml::attr(&e, b"subsetted").is_some_and(|v| on_off(&v)),
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out.retain(|d| !d.font.name.is_empty());
    Ok(out)
}

/// `w:subsetted`'s on/off value (`xml::on` reads `w:val`, which this attribute isn't).
fn on_off(value: &str) -> bool {
    !matches!(value, "0" | "false" | "off")
}

/// Whether `bytes` start as a TrueType or OpenType file (or collection): the sfnt
/// version's four bytes.
fn is_font(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf")
    )
}

/// The 16 bytes of `key`, a GUID (`{01234567-89AB-CDEF-0123-456789ABCDEF}`, braces and
/// hyphens optional), in the order its hex digits are written; `None` if it isn't one.
fn guid(key: &str) -> Option<[u8; 16]> {
    let hex: Vec<u8> = key
        .bytes()
        .filter(|b| !matches!(b, b'{' | b'}' | b'-'))
        .collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, pair) in hex.chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// XOR the first 32 bytes of `bytes` with `key`'s GUID, its bytes reversed: ECMA-376's
/// obfuscation, which undoes itself.
pub(crate) fn obfuscate(bytes: &mut [u8], key: &[u8; 16]) {
    for (i, b) in bytes.iter_mut().take(32).enumerate() {
        *b ^= key[15 - i % 16];
    }
}

/// A face's font file: `bytes` as they are when they're a plain part that starts as a
/// font, else de-obfuscated with `key`; `None` when that doesn't give a font file either.
pub(crate) fn deobfuscate(
    mut bytes: Vec<u8>,
    key: Option<&str>,
    obfuscated: bool,
) -> Option<Vec<u8>> {
    if !obfuscated && is_font(&bytes) {
        return Some(bytes);
    }
    let key = guid(key?)?;
    if bytes.len() < 32 {
        return None;
    }
    obfuscate(&mut bytes, &key);
    is_font(&bytes).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of a TrueType file: its sfnt version, then bytes 4 to 31 as 4, 5, ... 31.
    fn font() -> Vec<u8> {
        let mut bytes = vec![0, 1, 0, 0];
        bytes.extend(4u8..40);
        bytes
    }

    #[test]
    fn a_guids_bytes_are_read_in_writing_order() {
        let key = guid("{00112233-4455-6677-8899-AABBCCDDEEFF}").unwrap();
        assert_eq!(
            key,
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
                0xEE, 0xFF
            ]
        );
        assert_eq!(guid("00112233445566778899aabbccddeeff"), Some(key));
        assert_eq!(guid("{0011}"), None);
        assert_eq!(guid("{00112233-4455-6677-8899-AABBCCDDEEFG}"), None);
    }

    /// A reference vector worked by hand from § 17.8.1: the key's bytes reversed (FF EE
    /// ... 11 00) XOR the font's first 32 bytes, twice over; the rest untouched.
    #[test]
    fn a_reference_vector_de_obfuscates() {
        let mut plain = vec![0u8; 40];
        plain[1] = 1;
        plain[39] = 0x5A;
        #[rustfmt::skip]
        let obfuscated = vec![
            0xFF, 0xEF, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88,
            0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00,
            0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88,
            0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00,
            0, 0, 0, 0, 0, 0, 0, 0x5A,
        ];
        let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
        assert_eq!(deobfuscate(obfuscated, Some(key), true), Some(plain));
    }

    #[test]
    fn obfuscating_and_de_obfuscating_give_the_font_back() {
        let key = "{6C2F5D8A-91E3-4B07-A2C4-3F1E9D8B7A65}";
        let mut bytes = font();
        obfuscate(&mut bytes, &guid(key).unwrap());
        assert_ne!(bytes[..32], font()[..32]);
        assert_eq!(bytes[32..], font()[32..]);
        // As an .odttf part, and as a plain part whose bytes aren't a font.
        assert_eq!(deobfuscate(bytes.clone(), Some(key), true), Some(font()));
        assert_eq!(deobfuscate(bytes.clone(), Some(key), false), Some(font()));
        // Without its key, or with another, it's no font.
        assert_eq!(deobfuscate(bytes.clone(), None, true), None);
        let other = "{6C2F5D8A-91E3-4B07-A2C4-3F1E9D8B7A66}";
        assert_eq!(deobfuscate(bytes, Some(other), true), None);
    }

    #[test]
    fn a_plain_font_with_a_zero_key_stays_as_it_is() {
        let zero = "{00000000-0000-0000-0000-000000000000}";
        assert_eq!(deobfuscate(font(), Some(zero), false), Some(font()));
        assert_eq!(deobfuscate(font(), None, false), Some(font()));
        // A zero key XORs nothing, even for a part named .odttf.
        assert_eq!(deobfuscate(font(), Some(zero), true), Some(font()));
        let mut otf = font();
        otf[..4].copy_from_slice(b"OTTO");
        assert_eq!(deobfuscate(otf.clone(), None, false), Some(otf));
        // Too short to hold a key's worth, or not a font at all.
        assert_eq!(deobfuscate(vec![1, 2, 3], Some(zero), true), None);
        assert_eq!(
            deobfuscate(b"PK\x03\x04".repeat(10), Some(zero), false),
            None
        );
    }

    #[test]
    fn subsetted_reads_as_on_off() {
        assert!(on_off("1") && on_off("true") && on_off("on"));
        assert!(!on_off("0") && !on_off("false") && !on_off("off"));
    }

    #[test]
    fn the_table_reads_names_classes_pitches_and_embeds() {
        let xml = r#"<w:fonts xmlns:w="w" xmlns:r="r">
            <w:font w:name="Cardo"><w:altName w:val="Cardo Old"/><w:family w:val="roman"/>
              <w:pitch w:val="variable"/>
              <w:embedRegular w:fontKey="{00000000-0000-0000-0000-000000000000}" r:id="rId1" w:subsetted="0"/>
              <w:embedBold w:fontKey="{11111111-0000-0000-0000-000000000000}" r:id="rId2" w:subsetted="1"/>
            </w:font>
            <w:font w:name="Arial"><w:family w:val="swiss"/><w:pitch w:val="variable"/></w:font>
            <w:font w:name="Courier New"><w:family w:val="modern"/><w:pitch w:val="fixed"/></w:font>
            <w:font w:name="Bare"/>
            <w:font w:name="Odd"><w:family w:val="weird"/></w:font>
            </w:fonts>"#;
        let fonts = parse(xml).unwrap();
        let summary: Vec<_> = fonts
            .iter()
            .map(|d| (d.font.name.as_str(), d.font.family, d.font.pitch))
            .collect();
        assert_eq!(
            summary,
            [
                ("Cardo", FontFamily::Roman, FontPitch::Variable),
                ("Arial", FontFamily::Swiss, FontPitch::Variable),
                ("Courier New", FontFamily::Modern, FontPitch::Fixed),
                ("Bare", FontFamily::Auto, FontPitch::Default),
                ("Odd", FontFamily::Auto, FontPitch::Default),
            ]
        );
        assert_eq!(fonts[0].font.alt_name.as_deref(), Some("Cardo Old"));
        let embeds: Vec<_> = fonts[0]
            .embeds
            .iter()
            .map(|e| (e.kind, e.id.as_str(), e.key.as_deref(), e.subsetted))
            .collect();
        assert_eq!(
            embeds,
            [
                (
                    FaceKind::Regular,
                    "rId1",
                    Some("{00000000-0000-0000-0000-000000000000}"),
                    false
                ),
                (
                    FaceKind::Bold,
                    "rId2",
                    Some("{11111111-0000-0000-0000-000000000000}"),
                    true
                ),
            ]
        );
        assert!(fonts[1..].iter().all(|d| d.embeds.is_empty()));
    }
}
