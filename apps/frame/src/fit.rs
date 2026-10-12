//! Text that fits its box (FRAME-12). After an edit changes a shape's text, looks, or box,
//! or a slide's layout, text that shrinks to fit (`normAutofit`, or a placeholder that
//! says nothing) takes the largest of [`deck_edit::FITS`] its box holds; after an edit
//! changes the text or its looks, a text box that grows (`spAutoFit`) takes its text's
//! height. Both join the edit's undo step.
//! Text is measured as the canvas lays it out, at a fixed scale, so the fit doesn't
//! depend on the window's size. Undo and redo don't refit: the step holds the fit.

use crate::render;
use crate::window::DeckWindow;
use deck_edit::{Change, Edit, Fit, FitEdit, FrameEdit, ShapeRef, EMU_PER_PIXEL, FITS};
use presentation_model::{Autofit, Frame, Presentation, Vertical};

/// Pixels per EMU text is measured at: 96 pixels an inch.
const SCALE: f32 = 1.0 / EMU_PER_PIXEL as f32;

/// Refit the text `change` changed, as part of the step that changed it; `text` says the
/// edit changed the text or its looks (else only its box), which boxes that grow follow.
/// A slide as a whole refits all its shapes: give it only for a layout change.
pub fn refit(view: &DeckWindow, change: Change, text: bool) {
    let shapes: Vec<ShapeRef> = match change {
        Change::Text { shape, .. } | Change::Look { shape } | Change::Shape { shape } => {
            vec![shape]
        }
        Change::Slide { slide } => {
            let n = view
                .deck
                .presentation()
                .slides
                .get(slide)
                .map_or(0, |s| s.shapes.len());
            (0..n).map(|shape| ShapeRef { slide, shape }).collect()
        }
        Change::Slides { .. } => return,
    };
    let ctx = view.canvas.pango();
    for shape in shapes {
        let edit = fitted(&ctx, &view.deck.presentation(), shape, text);
        if let Some(edit) = edit {
            view.amend(edit);
        }
    }
}

/// The edit that fits `at`'s text to its box, when it doesn't now; a box that grows only
/// after a `text` change.
fn fitted(
    ctx: &gtk::pango::Context,
    deck: &Presentation,
    at: ShapeRef,
    text: bool,
) -> Option<Edit> {
    let shape = deck.slides.get(at.slide)?.shapes.get(at.shape)?;
    let body = shape.text.as_ref()?;
    if body.autofit == Autofit::Grow {
        if !text || body.vertical != Vertical::Horizontal || shape.text_frame.is_some() {
            return None;
        }
        let (height, _) = render::text_room(ctx, shape, SCALE)?;
        let cy = (f64::from(height) / f64::from(SCALE)).round() as i64
            + body.insets.top
            + body.insets.bottom;
        // Within a pixel is a fit: no step for rounding.
        if (cy - shape.frame.cy).abs() < EMU_PER_PIXEL {
            return None;
        }
        let frame = Frame { cy, ..shape.frame };
        return FrameEdit::new(deck, at, frame).map(Edit::from);
    }
    if !deck_edit::shrinks(shape) {
        return None;
    }
    let now = Fit {
        font_scale: body.font_scale,
        line_reduction: body.line_reduction,
    };
    let mut trial = shape.clone();
    let fit = FITS
        .iter()
        .copied()
        .find(|fit| {
            if let Some(t) = trial.text.as_mut() {
                (t.font_scale, t.line_reduction) = (fit.font_scale, fit.line_reduction);
            }
            render::text_room(ctx, &trial, SCALE).is_some_and(|(height, room)| height <= room)
        })
        .unwrap_or(FITS[FITS.len() - 1]);
    if fit == now {
        return None;
    }
    FitEdit::new(deck, at, fit).map(Edit::from)
}
