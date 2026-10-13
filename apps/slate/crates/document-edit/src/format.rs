//! The format bar's commands (SLATE-21): run looks set over a selection as one text edit,
//! or on the look typed next; and what the bar shows, the looks at the caret or across a
//! selection.

use crate::text::{paragraphs_in, range, restyle, runs_within, style_at};
use crate::{blocks, look, paragraph, Edit, Pos};
use document_model::{Align, Document, Look, Rgb, RunStyle};

/// The smallest and largest font sizes, in half-points (1 pt and 400 pt).
pub const MIN_SIZE: u16 = 2;
pub const MAX_SIZE: u16 = 800;

/// A font size change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeChange {
    /// This size, in half-points.
    Set(u16),
    /// Back to the paragraph style's.
    Clear,
    /// Each run's size shown, larger (or smaller when negative) by this many half-points.
    Step(i16),
}

/// Run looks to set; each `None` leaves that look as it is. For the font and the colour,
/// `Some(None)` takes the paragraph style's back (the colour then is automatic unless the
/// style sets one).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookPatch {
    pub font: Option<Option<String>>,
    pub size: Option<SizeChange>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<Option<Rgb>>,
}

impl LookPatch {
    /// Apply the patch to `run`, direct formatting in a paragraph of style `style`.
    fn apply(&self, doc: &Document, style: &str, run: &mut RunStyle) {
        if let Some(font) = &self.font {
            run.font.clone_from(font);
        }
        match self.size {
            Some(SizeChange::Set(size)) => run.size = Some(size.clamp(MIN_SIZE, MAX_SIZE)),
            Some(SizeChange::Clear) => run.size = None,
            Some(SizeChange::Step(by)) => {
                let shown = i32::from(doc.styles.look(style, run).size);
                let size = (shown + i32::from(by)).clamp(i32::from(MIN_SIZE), i32::from(MAX_SIZE));
                run.size = Some(size as u16);
            }
            None => {}
        }
        for (set, to) in [
            (&mut run.bold, self.bold),
            (&mut run.italic, self.italic),
            (&mut run.underline, self.underline),
            (&mut run.strike, self.strike),
        ] {
            if to.is_some() {
                *set = to;
            }
        }
        if let Some(color) = self.color {
            run.color = color;
        }
    }
}

/// Set `patch`'s looks on the text of paragraphs and list items in `from..to`, as their
/// runs' direct formatting: one undoable edit that leaves the range selected. `None` when
/// the range is empty or already looks so.
pub fn format(doc: &Document, from: Pos, to: Pos, patch: &LookPatch) -> Option<Edit> {
    restyle(doc, from, to, |style, run| patch.apply(doc, style, run))
}

/// The look typed text takes at `at` once `patch` is set on `look`, what typing there
/// would take.
pub fn formatted(doc: &Document, at: Pos, look: &RunStyle, patch: &LookPatch) -> RunStyle {
    let mut look = look.clone();
    patch.apply(doc, style_at(doc, at), &mut look);
    look
}

/// The looks the format bar shows: each `None` where the selection mixes them (or there is
/// no text to have them). `color`'s inner `None` is automatic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looks {
    /// The paragraph style's id.
    pub style: Option<String>,
    pub font: Option<String>,
    /// In half-points.
    pub size: Option<u16>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<Option<Rgb>>,
    pub align: Option<Align>,
}

/// One value seen so far, or several.
enum Seen<T> {
    Nothing,
    One(T),
    Mixed,
}

impl<T: PartialEq> Seen<T> {
    fn see(&mut self, value: T) {
        match self {
            Seen::Nothing => *self = Seen::One(value),
            Seen::One(v) if *v == value => {}
            _ => *self = Seen::Mixed,
        }
    }

    fn get(self) -> Option<T> {
        match self {
            Seen::One(v) => Some(v),
            _ => None,
        }
    }
}

/// The looks over `from..to`: the paragraph styles and alignments of the paragraphs and
/// list items it touches, and the run looks of the text in it (as [`StyleSheet::look`]
/// resolves them). With no selection (`from == to`), or one without text, the looks at
/// `to`: its paragraph's, and `typing`'s run look when given, else what typing there takes.
///
/// [`StyleSheet::look`]: document_model::StyleSheet::look
pub fn looks(doc: &Document, from: Pos, to: Pos, typing: Option<&RunStyle>) -> Looks {
    let mut style = Seen::Nothing;
    let mut align = Seen::Nothing;
    let mut font = Seen::Nothing;
    let mut size = Seen::Nothing;
    let mut flags = [Seen::Nothing, Seen::Nothing, Seen::Nothing, Seen::Nothing];
    let mut color = Seen::Nothing;
    // As `StyleSheet::look` resolves them; the font's name copied once.
    let mut see = |base: &Look, run: &RunStyle| {
        let looks = [
            (run.bold, base.bold),
            (run.italic, base.italic),
            (run.underline, base.underline),
            (run.strike, base.strike),
        ];
        for (seen, (own, styled)) in flags.iter_mut().zip(looks) {
            seen.see(own.unwrap_or(styled));
        }
        size.see(run.size.unwrap_or(base.size));
        color.see(run.color.or(base.color));
        let name = run.font.as_deref().unwrap_or(&base.font);
        match &font {
            Seen::Nothing => font = Seen::One(name.to_owned()),
            Seen::One(f) if f == name => {}
            _ => font = Seen::Mixed,
        }
    };
    let mut any = false;
    if let Some((blocks, start, end)) = range(doc, from, to).filter(|(_, s, e)| s != e) {
        for (p, within) in paragraphs_in(blocks, start, end) {
            let paragraph_style = doc.styles.style_of(&p.style);
            style.see(&paragraph_style.id);
            align.see(p.align);
            for run in runs_within(p, within) {
                any = true;
                see(&paragraph_style.run, &run.style);
            }
        }
    }
    if !any {
        let p = blocks(doc, to.at)
            .and_then(|b| b.get(to.block))
            .and_then(paragraph);
        let Some(p) = p else {
            return Looks::default();
        };
        let paragraph_style = doc.styles.style_of(&p.style);
        if matches!(style, Seen::Nothing) {
            style.see(&paragraph_style.id);
            align.see(p.align);
        }
        match typing {
            Some(t) => see(&paragraph_style.run, t),
            None => see(&paragraph_style.run, &look(doc, to)),
        }
    }
    let [bold, italic, underline, strike] = flags.map(Seen::get);
    Looks {
        style: style.get().cloned(),
        font: font.get(),
        size: size.get(),
        bold,
        italic,
        underline,
        strike,
        color: color.get(),
        align: align.get(),
    }
}
