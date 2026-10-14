//! A Word document's list levels (ADR 0009, decision 5): what each level of a numbering
//! instance in `word/numbering.xml` gives its items. The marker's text (`w:lvlText`, with
//! a symbol font's characters mapped to Unicode, [`symbol_text`]), its alignment
//! (`w:lvlJc`), what follows it (`w:suff`), its run look (the level's `w:rPr`, over the
//! paragraph mark's), and the level's indents and tab stops.
//!
//! A Word list item lays out as Word lays it out: its indents are its style's, then its
//! level's, then its own ([`StyleSheet::list_spacing`](crate::StyleSheet::list_spacing));
//! its marker sits at its first line's start (the left indent less the hanging one, or plus
//! the first-line one), aligned there; after it a tab advances to the next stop (the
//! hanging indent's implicit stop counts when it is past the marker), a space is a space,
//! and nothing is nothing; the text's other lines start at the left indent.
//!
//! Lists Slate starts in a Word document lay out with [`ListLevel::slate`]'s levels, which
//! saving writes for them.

use crate::{RunStyle, TabStop};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// Where a marker sits against its first line's start (`w:lvlJc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MarkerAlign {
    /// It starts there.
    #[default]
    Left,
    /// It centres there.
    Center,
    /// It ends there.
    Right,
}

/// What follows a marker (`w:suff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MarkerSuffix {
    /// A tab, to the next tab stop.
    #[default]
    Tab,
    /// A space.
    Space,
    /// Nothing: the text follows the marker.
    Nothing,
}

/// A level of a Word list (`w:lvl`): its items' marker and indents.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ListLevel {
    /// `w:numFmt`: `bullet`, `decimal`, `lowerLetter`…
    pub format: String,
    /// `w:lvlText` with a symbol font's characters mapped to Unicode: a bullet's marker,
    /// or a number's pattern (`%1.`), whose items' labels fill it in. Empty: no marker.
    pub text: String,
    pub align: MarkerAlign,
    pub suffix: MarkerSuffix,
    /// The level's `w:ind`, in twips: left, and the first line's from it (negative:
    /// hanging).
    pub indent_left: Option<i32>,
    pub indent_first: Option<i32>,
    /// The level's `w:tabs`, clears included, in position order.
    pub tabs: Vec<TabStop>,
    /// The marker's run look over the paragraph mark's (`w:rPr`); without a font when the
    /// level's was a symbol font whose characters were mapped.
    pub look: RunStyle,
}

impl ListLevel {
    /// Whether its items are bulleted (else numbered).
    pub fn bulleted(&self) -> bool {
        self.format == "bullet"
    }

    /// Slate's own level `level` (0 to 8) of a bulleted or numbered list: what a list Slate
    /// starts in a Word document lays out with, and what saving writes for it. Bullets •, ◦,
    /// ▪ and numbers 1., a., i. in turn; each level 0.5″ further in, hanging 0.25″.
    pub fn slate(bullet: bool, level: u8) -> &'static ListLevel {
        &SLATE[usize::from(bullet)][usize::from(level.min(8))]
    }
}

/// Slate's levels, numbered then bulleted.
static SLATE: LazyLock<[Vec<ListLevel>; 2]> = LazyLock::new(|| {
    let level = |bullet: bool, l: u8| {
        let (format, text) = if bullet {
            (
                "bullet",
                ["\u{2022}", "\u{25E6}", "\u{25AA}"][usize::from(l % 3)].to_owned(),
            )
        } else {
            (
                ["decimal", "lowerLetter", "lowerRoman"][usize::from(l % 3)],
                format!("%{}.", l + 1),
            )
        };
        ListLevel {
            format: format.to_owned(),
            text,
            indent_left: Some(720 * (i32::from(l) + 1)),
            indent_first: Some(-360),
            ..ListLevel::default()
        }
    };
    [false, true].map(|b| (0..9).map(|l| level(b, l)).collect())
});

/// A Word document's list levels by numbering instance (`w:numId`) and level.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Numbering {
    levels: HashMap<(u32, u8), ListLevel>,
}

impl Numbering {
    pub fn insert(&mut self, num: u32, level: u8, def: ListLevel) {
        self.levels.insert((num, level), def);
    }

    /// Level `level` of instance `num`, if it has one.
    pub fn level(&self, num: u32, level: u8) -> Option<&ListLevel> {
        self.levels.get(&(num, level))
    }
}

/// A Word list item's numbering: its instance (`w:numId`), whose level its own level
/// picks, and its paragraph mark's run look (`w:pPr/w:rPr`), which its marker's look goes
/// over.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ListNumbering {
    pub num: u32,
    #[serde(default)]
    pub mark: RunStyle,
}

/// `text` in symbol font `font` (Symbol or Wingdings) as Unicode characters: each of the
/// font's codes, or its private-use form (U+F020 to U+F0FF, as Word writes them), as the
/// character it draws; others kept. `None` when `font` isn't one of them.
pub fn symbol_text(font: &str, text: &str) -> Option<String> {
    let table: fn(u8) -> Option<char> = match font.trim() {
        f if f.eq_ignore_ascii_case("symbol") => symbol,
        f if f.eq_ignore_ascii_case("wingdings") => wingdings,
        _ => return None,
    };
    Some(
        text.chars()
            .map(|c| {
                let code = match u32::from(c) {
                    n @ 0xF020..=0xF0FF => n - 0xF000,
                    n @ 0x20..=0xFF => n,
                    _ => return c,
                };
                table(code as u8).unwrap_or(c)
            })
            .collect(),
    )
}

/// The Symbol font's code `code` as Unicode (Adobe's Symbol encoding).
fn symbol(code: u8) -> Option<char> {
    const GREEK_UPPER: &str = "ΑΒΧΔΕΦΓΗΙϑΚΛΜΝΟΠΘΡΣΤΥςΩΞΨΖ";
    const GREEK_LOWER: &str = "αβχδεφγηιϕκλμνοπθρστυϖωξψζ";
    const HIGH: &str = "€ϒ′≤⁄∞ƒ♣♦♥♠↔←↑→↓°±″≥×∝∂•÷≠≡≈…⏐⎯↵ℵℑℜ℘⊗⊕∅∩∪⊃⊇⊄⊂⊆∈∉∠∇®©™∏√⋅¬∧∨⇔⇐⇑⇒⇓◊〈®©™∑⎛⎜⎝⎡⎢⎣⎧⎨⎩⎪\u{F8FF}〉∫⌠⎮⌡⎞⎟⎠⎤⎥⎦⎫⎬⎭";
    let c = match code {
        0x22 => '∀',
        0x24 => '∃',
        0x27 => '∋',
        0x2A => '∗',
        0x2D => '−',
        0x40 => '≅',
        0x41..=0x5A => GREEK_UPPER.chars().nth(usize::from(code - 0x41))?,
        0x5C => '∴',
        0x5E => '⊥',
        0x60 => '‾',
        0x61..=0x7A => GREEK_LOWER.chars().nth(usize::from(code - 0x61))?,
        0x7E => '∼',
        0xA0..=0xFE => HIGH.chars().nth(usize::from(code - 0xA0))?,
        0x20..=0x7F => char::from(code),
        _ => return None,
    };
    Some(c)
}

/// The Wingdings font's code `code` as Unicode: Unicode's mapping, with the arrowhead
/// bullet (0xD8) and the shadowed square (0x71) as Word's lists show them.
fn wingdings(code: u8) -> Option<char> {
    const TABLE: [u32; 0xDF] = [
        0x20, 0x1F589, 0x2702, 0x2701, 0x1F453, 0x1F56D, 0x1F56E, 0x1F56F, // 0x20
        0x1F57F, 0x2706, 0x1F582, 0x1F583, 0x1F4EA, 0x1F4EB, 0x1F4EC, 0x1F4ED, // 0x28
        0x1F4C1, 0x1F4C2, 0x1F4C4, 0x1F5CF, 0x1F5D0, 0x1F5C4, 0x231B, 0x1F5AE, // 0x30
        0x1F5B0, 0x1F5B2, 0x1F5B3, 0x1F5B4, 0x1F5AB, 0x1F5AC, 0x2707, 0x270D, // 0x38
        0x1F58E, 0x270C, 0x1F44C, 0x1F44D, 0x1F44E, 0x261C, 0x261E, 0x261D, // 0x40
        0x261F, 0x1F590, 0x263A, 0x1F610, 0x2639, 0x1F4A3, 0x2620, 0x1F3F3, // 0x48
        0x1F3F1, 0x2708, 0x263C, 0x1F4A7, 0x2744, 0x1F546, 0x271E, 0x1F548, // 0x50
        0x2720, 0x2721, 0x262A, 0x262F, 0x0950, 0x2638, 0x2648, 0x2649, // 0x58
        0x264A, 0x264B, 0x264C, 0x264D, 0x264E, 0x264F, 0x2650, 0x2651, // 0x60
        0x2652, 0x2653, 0x1F670, 0x1F675, 0x25CF, 0x1F53E, 0x25A0, 0x25A1, // 0x68
        0x1F790, 0x274F, 0x2752, 0x2B27, 0x29EB, 0x25C6, 0x2756, 0x2B25, // 0x70
        0x2327, 0x2BB9, 0x2318, 0x1F3F5, 0x1F3F6, 0x1F676, 0x1F677, 0, // 0x78
        0x24EA, 0x2460, 0x2461, 0x2462, 0x2463, 0x2464, 0x2465, 0x2466, // 0x80
        0x2467, 0x2468, 0x2469, 0x24FF, 0x2776, 0x2777, 0x2778, 0x2779, // 0x88
        0x277A, 0x277B, 0x277C, 0x277D, 0x277E, 0x277F, 0x1F662, 0x1F660, // 0x90
        0x1F661, 0x1F663, 0x1F65E, 0x1F65C, 0x1F65D, 0x1F65F, 0x00B7, 0x2022, // 0x98
        0x25AA, 0x26AA, 0x1F786, 0x1F788, 0x25C9, 0x25CE, 0x1F53F, 0x25AA, // 0xA0
        0x25FB, 0x1F7C2, 0x2726, 0x2605, 0x2736, 0x2734, 0x2739, 0x2735, // 0xA8
        0x2BD0, 0x2316, 0x27E1, 0x2311, 0x2BD1, 0x272A, 0x2730, 0x1F550, // 0xB0
        0x1F551, 0x1F552, 0x1F553, 0x1F554, 0x1F555, 0x1F556, 0x1F557, 0x1F558, // 0xB8
        0x1F559, 0x1F55A, 0x1F55B, 0x2BB0, 0x2BB1, 0x2BB2, 0x2BB3, 0x2BB4, // 0xC0
        0x2BB5, 0x2BB6, 0x2BB7, 0x1F66A, 0x1F66B, 0x1F655, 0x1F654, 0x1F657, // 0xC8
        0x1F656, 0x1F650, 0x1F651, 0x1F652, 0x1F653, 0x232B, 0x2326, 0x2B98, // 0xD0
        0x27A2, 0x2B99, 0x2B9B, 0x2B88, 0x2B8A, 0x2B89, 0x2B8B, 0x1F868, // 0xD8
        0x1F86A, 0x1F869, 0x1F86B, 0x1F86C, 0x1F86D, 0x1F86F, 0x1F86E, 0x1F878, // 0xE0
        0x1F87A, 0x1F879, 0x1F87B, 0x1F87C, 0x1F87D, 0x1F87F, 0x1F87E, 0x21E6, // 0xE8
        0x21E8, 0x21E7, 0x21E9, 0x2B04, 0x21F3, 0x2B00, 0x2B01, 0x2B03, // 0xF0
        0x2B02, 0x1F8AC, 0x1F8AD, 0x1F5F6, 0x2714, 0x1F5F7, 0x1F5F9, // 0xF8
    ];
    let n = *TABLE.get(usize::from(code.checked_sub(0x20)?))?;
    char::from_u32(n).filter(|_| n != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_fonts_map_to_unicode() {
        // Word's Symbol bullet, as Word writes it (private use) and as the font's code.
        assert_eq!(symbol_text("Symbol", "\u{F0B7}").as_deref(), Some("•"));
        assert_eq!(symbol_text(" symbol ", "\u{B7}").as_deref(), Some("•"));
        assert_eq!(
            symbol_text("Symbol", "\u{F061}\u{F0D7}").as_deref(),
            Some("α⋅")
        );
        assert_eq!(symbol_text("Symbol", "\u{F0A7}").as_deref(), Some("♣"));
        // Wingdings' list bullets.
        for (code, want) in [
            (0xA7, '▪'),
            (0xD8, '➢'),
            (0xFC, '✔'),
            (0x76, '❖'),
            (0x71, '❏'),
            (0xA8, '◻'),
            (0x6E, '■'),
            (0x6C, '●'),
            (0x9F, '•'),
            (0xFE, '🗹'),
        ] {
            let private = char::from_u32(0xF000 + code).unwrap().to_string();
            let mapped = symbol_text("Wingdings", &private).unwrap();
            assert_eq!(mapped, want.to_string(), "Wingdings {code:#X}");
        }
        // Other fonts aren't mapped; text outside the fonts' codes stays.
        assert_eq!(symbol_text("Courier New", "o"), None);
        assert_eq!(
            symbol_text("Wingdings", "●x\u{F07F}").as_deref(),
            Some("●⌧\u{F07F}")
        );
    }

    #[test]
    fn symbols_high_table_lines_up() {
        // The high half's last code is the right brace's bottom, its first the euro.
        assert_eq!(symbol(0xA0), Some('€'));
        assert_eq!(symbol(0xFE), Some('⎭'));
        assert_eq!(symbol(0xFF), None);
        assert_eq!(symbol(0x5A), Some('Ζ'));
        assert_eq!(symbol(0x7A), Some('ζ'));
    }

    #[test]
    fn slates_levels() {
        let l = ListLevel::slate(true, 4);
        assert_eq!(
            (l.text.as_str(), l.indent_left, l.indent_first),
            ("◦", Some(3600), Some(-360))
        );
        let n = ListLevel::slate(false, 2);
        assert_eq!((n.format.as_str(), n.text.as_str()), ("lowerRoman", "%3."));
        assert!(ListLevel::slate(true, 0).bulleted() && !n.bulleted());
        assert_eq!(ListLevel::slate(true, 20), ListLevel::slate(true, 8));
    }
}
