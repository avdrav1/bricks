//! Changing a slide's layout (FRAME-11): its text moves into the new layout's matching
//! placeholders and takes their looks, keeping the looks set on it; the rest of the slide
//! stays. One undoable step, holding the slide before and after.

use crate::Change;
use presentation_model::{Layer, Paragraph, PlaceholderKind, Presentation, Run, Shape, Slide};

/// A slide moved to another layout.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutEdit {
    slide: usize,
    before: Slide,
    after: Slide,
}

/// Placeholders of a family stand in for each other when a layout changes: titles, and
/// the rest of the text placeholders; footers only for their own kind.
fn family(k: PlaceholderKind) -> u8 {
    match k {
        PlaceholderKind::Title | PlaceholderKind::CentredTitle => 0,
        PlaceholderKind::Body | PlaceholderKind::Subtitle | PlaceholderKind::Content => 1,
        PlaceholderKind::Date => 2,
        PlaceholderKind::Footer => 3,
        PlaceholderKind::SlideNumber => 4,
        PlaceholderKind::Header => 5,
    }
}

/// A run moved into a placeholder: `base`'s looks (the new placeholder's, at its level)
/// but those set on the run itself, its text, and its markup.
fn moved_run(r: &Run, base: &Run) -> Run {
    let mut n = base.clone();
    n.text = r.text.clone();
    n.markup = r.markup.clone();
    n.slide_number = r.slide_number;
    let (s, from) = (&mut n.sources, &r.sources);
    if from.font.is_own() {
        (n.font, s.font) = (r.font.clone(), from.font);
    }
    if from.size.is_own() {
        (n.size, s.size) = (r.size, from.size);
    }
    if from.bold.is_own() {
        (n.bold, s.bold) = (r.bold, from.bold);
    }
    if from.italic.is_own() {
        (n.italic, s.italic) = (r.italic, from.italic);
    }
    if from.underline.is_own() {
        (n.underline, s.underline) = (r.underline, from.underline);
    }
    if from.strike.is_own() {
        (n.strike, s.strike) = (r.strike, from.strike);
    }
    if from.color.is_own() {
        (n.color, s.color) = (r.color, from.color);
    }
    if from.baseline.is_own() {
        (n.baseline, s.baseline) = (r.baseline, from.baseline);
    }
    if from.caps.is_own() {
        (n.caps, s.caps) = (r.caps, from.caps);
    }
    n
}

/// A paragraph moved into a placeholder whose paragraphs look like `levels` (by level).
fn moved_paragraph(p: &Paragraph, levels: &[Paragraph]) -> Option<Paragraph> {
    let base = levels.get(p.level as usize).or(levels.last())?;
    let mut q = base.clone();
    q.level = p.level;
    if p.sources.align.is_own() {
        (q.align, q.sources.align) = (p.align, p.sources.align);
    }
    if p.sources.bullet.is_own() {
        (q.bullet, q.sources.bullet) = (p.bullet.clone(), p.sources.bullet);
        (q.margin_left, q.indent) = (p.margin_left, p.indent);
    }
    q.markup = p.markup.clone();
    q.runs = p.runs.iter().map(|r| moved_run(r, &base.end)).collect();
    q.end = moved_run(&p.end, &base.end);
    Some(q)
}

/// Placeholder `old`'s text and own looks in the new layout's placeholder `new`.
fn moved(old: &Shape, new: &Shape) -> Shape {
    let mut s = new.clone();
    s.id = old.id;
    s.name = old.name.clone();
    if old.sources.fill.is_own() {
        (s.fill, s.sources.fill) = (old.fill.clone(), old.sources.fill);
    }
    if old.sources.outline.is_own() {
        (s.outline, s.sources.outline) = (old.outline, old.sources.outline);
    }
    if let (Some(text), Some(old_text)) = (s.text.as_mut(), old.text.as_ref()) {
        let fallback;
        let levels: &[Paragraph] = if text.levels.is_empty() {
            fallback = text.paragraphs.clone();
            &fallback
        } else {
            &text.levels
        };
        let paragraphs: Option<Vec<Paragraph>> = old_text
            .paragraphs
            .iter()
            .map(|p| moved_paragraph(p, levels))
            .collect();
        if let Some(paragraphs) = paragraphs {
            text.paragraphs = paragraphs;
            text.number_bullets();
        }
    }
    s
}

impl LayoutEdit {
    /// Move slide `slide` to layout `layout`. Each of the layout's placeholders takes the
    /// text of the slide's placeholder in the same slot, else of the same kind, else of
    /// the same family (a title for a title, body text for body text); the slide's empty
    /// placeholders that none took go, and the layout's that took nothing come new. Other
    /// shapes, and placeholders with text that none took, stay. `None` when the slide is
    /// on that layout.
    pub fn new(deck: &Presentation, slide: usize, layout: usize) -> Option<LayoutEdit> {
        let old = deck.slides.get(slide)?;
        let template = &deck.layouts.get(layout)?.slide;
        if old.layout == Some(layout) {
            return None;
        }
        let own: Vec<&Shape> = old
            .shapes
            .iter()
            .filter(|s| s.layer == Layer::Slide)
            .collect();
        let slots: Vec<&Shape> = template
            .shapes
            .iter()
            .filter(|s| s.layer == Layer::Slide && s.placeholder.is_some())
            .collect();
        // Which of the slide's shapes each of the layout's placeholders takes.
        let mut taken: Vec<Option<usize>> = vec![None; slots.len()];
        let mut used = vec![false; own.len()];
        let candidate = |s: &Shape| s.placeholder.is_some() && s.text.is_some() && s.id.is_some();
        type Same<'a> = &'a dyn Fn(&Shape, &Shape) -> bool;
        let passes: [Same; 3] = [
            &|a, b| a.slot == b.slot,
            &|a, b| a.slot.as_ref().map(|s| &s.kind) == b.slot.as_ref().map(|s| &s.kind),
            &|a, b| a.placeholder.map(family) == b.placeholder.map(family),
        ];
        for same in passes {
            for (t, slot) in slots.iter().enumerate() {
                if taken[t].is_some() {
                    continue;
                }
                let found = own
                    .iter()
                    .enumerate()
                    .find(|(i, s)| !used[*i] && candidate(s) && same(slot, s));
                if let Some((i, _)) = found {
                    taken[t] = Some(i);
                    used[i] = true;
                }
            }
        }
        let mut next_id = own.iter().filter_map(|s| s.id).max().unwrap_or(1) + 1;
        let mut shapes: Vec<Shape> = template
            .shapes
            .iter()
            .filter(|s| s.layer != Layer::Slide)
            .cloned()
            .collect();
        for (t, slot) in slots.iter().enumerate() {
            if taken[t].is_none() {
                let mut s = (*slot).clone();
                s.id = Some(next_id);
                next_id += 1;
                shapes.push(s);
            }
        }
        for (i, s) in own.iter().enumerate() {
            match taken.iter().position(|t| *t == Some(i)) {
                Some(t) => shapes.push(moved(s, slots[t])),
                None => {
                    let empty = s.text.as_ref().is_some_and(|t| t.is_empty());
                    if !(s.placeholder.is_some() && empty) {
                        shapes.push((*s).clone());
                    }
                }
            }
        }
        let (background, background_source) = if old.background_source.is_own() {
            (old.background.clone(), old.background_source)
        } else {
            (template.background.clone(), template.background_source)
        };
        let after = Slide {
            background,
            background_source,
            shapes,
            hidden: old.hidden,
            notes: old.notes.clone(),
            layout: Some(layout),
            part: old.part.clone(),
        };
        Some(LayoutEdit {
            slide,
            before: old.clone(),
            after,
        })
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        deck.slides[self.slide] = self.after.clone();
        deck.number_slides();
        Change::Slide { slide: self.slide }
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        deck.slides[self.slide] = self.before.clone();
        deck.number_slides();
        Change::Slide { slide: self.slide }
    }
}
