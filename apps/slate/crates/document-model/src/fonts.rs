//! A Word document's font table (`word/fontTable.xml`, ADR 0009): each font it names, the
//! kind of family it belongs to, and the faces the document carries for it. Slate draws a
//! run in an embedded face when its font isn't installed, and otherwise falls back to the
//! generic family of the font's class, so a serif document stays serif.

use std::sync::Arc;

/// The fonts a Word document's font table lists, in its order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontTable {
    pub fonts: Vec<Font>,
}

impl FontTable {
    /// The font named `name`, as Word matches names: ignoring case.
    pub fn get(&self, name: &str) -> Option<&Font> {
        self.fonts
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// Whether any font carries a face.
    pub fn has_embedded(&self) -> bool {
        self.fonts.iter().any(|f| !f.faces.is_empty())
    }
}

/// One font of the table (`w:font`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    /// The name runs give it (`w:name`).
    pub name: String,
    /// Its class (`w:family`).
    pub family: FontFamily,
    /// `w:pitch`.
    pub pitch: FontPitch,
    /// Another name it goes by (`w:altName`).
    pub alt_name: Option<String>,
    /// The faces the document embeds, de-obfuscated.
    pub faces: Vec<EmbeddedFace>,
}

/// A font's class (`w:family`, ECMA-376 § 17.18.30): which generic family stands in for
/// it when it's neither installed nor embedded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontFamily {
    /// Proportional with serifs.
    Roman,
    /// Proportional without serifs.
    Swiss,
    /// Monospaced.
    Modern,
    Script,
    Decorative,
    /// Unknown: Word's default when the table gives none.
    #[default]
    Auto,
}

impl FontFamily {
    /// `w:family`'s value; anything unknown is `Auto`.
    pub fn parse(value: &str) -> Self {
        match value {
            "roman" => Self::Roman,
            "swiss" => Self::Swiss,
            "modern" => Self::Modern,
            "script" => Self::Script,
            "decorative" => Self::Decorative,
            _ => Self::Auto,
        }
    }

    /// The fontconfig generic family that stands in for a font of this class: sans-serif
    /// for swiss, monospace for modern, serif for the rest.
    pub fn generic(self) -> &'static str {
        match self {
            Self::Swiss => "sans-serif",
            Self::Modern => "monospace",
            Self::Roman | Self::Script | Self::Decorative | Self::Auto => "serif",
        }
    }
}

/// `w:pitch` (ECMA-376 § 17.18.68).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontPitch {
    #[default]
    Default,
    Fixed,
    Variable,
}

impl FontPitch {
    /// `w:pitch`'s value; anything unknown is `Default`.
    pub fn parse(value: &str) -> Self {
        match value {
            "fixed" => Self::Fixed,
            "variable" => Self::Variable,
            _ => Self::Default,
        }
    }
}

/// Which face of its font an embedded font file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceKind {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// A font file the document carries (`w:embedRegular` and the like): TrueType or
/// OpenType, de-obfuscated, not parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedFace {
    pub kind: FaceKind,
    /// `w:subsetted`: the file holds only the glyphs the document uses.
    pub subsetted: bool,
    pub bytes: Arc<[u8]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_map_to_generic_families() {
        let generic = |v: &str| FontFamily::parse(v).generic();
        assert_eq!(generic("roman"), "serif");
        assert_eq!(generic("swiss"), "sans-serif");
        assert_eq!(generic("modern"), "monospace");
        for v in ["script", "decorative", "auto", "", "nonsense"] {
            assert_eq!(generic(v), "serif", "{v}");
        }
        assert_eq!(FontFamily::parse("nonsense"), FontFamily::Auto);
        assert_eq!(FontPitch::parse("fixed"), FontPitch::Fixed);
        assert_eq!(FontPitch::parse("variable"), FontPitch::Variable);
        assert_eq!(FontPitch::parse("x"), FontPitch::Default);
    }

    #[test]
    fn fonts_are_found_by_name_ignoring_case() {
        let font = |name: &str| Font {
            name: name.into(),
            family: FontFamily::Roman,
            pitch: FontPitch::Variable,
            alt_name: None,
            faces: Vec::new(),
        };
        let table = FontTable {
            fonts: vec![font("Cardo"), font("Arial")],
        };
        assert_eq!(table.get("cardo").map(|f| f.name.as_str()), Some("Cardo"));
        assert!(table.get("Cardo Bold").is_none());
        assert!(!table.has_embedded());
    }
}
