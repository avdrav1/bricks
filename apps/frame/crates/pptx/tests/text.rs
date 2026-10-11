//! FRAME-2 acceptance: every paragraph's text, level, and alignment, and every run's size,
//! bold, italic, underline, strikethrough, colour, and font match LibreOffice's reading
//! (`scripts/lo_pptx.py dump`).
//!
//! Runs are compared character by character (`common::compare_paragraphs`). Sizes are the
//! written sizes, before autofit's font scale (LibreOffice keeps that apart too).

mod common;

use common::{dump, DECKS};

#[test]
fn text_matches_libreoffice() {
    let (mut paragraphs, mut runs) = (0, 0);
    for name in DECKS {
        let deck = pptx::read(&common::data(&format!("{name}.pptx"))).unwrap();
        let lo = dump(name);
        for (i, (slide, lo_slide)) in deck.slides.iter().zip(&lo.slides).enumerate() {
            let at = format!("{name} slide {}", i + 1);
            for (shape, lo_shape) in common::pairs(slide, lo_slide, &at) {
                let at = format!("{at} shape {:?}", shape.name);
                if lo_shape.paragraphs.is_empty() {
                    // LibreOffice dumps no text for empty shapes and empty placeholders.
                    assert!(shape.text.as_ref().is_none_or(|t| t.is_empty()), "{at}");
                    continue;
                }
                let text = shape
                    .text
                    .as_ref()
                    .unwrap_or_else(|| panic!("{at}: no text"));
                runs += common::compare_paragraphs(&at, &text.paragraphs, &lo_shape.paragraphs);
                paragraphs += text.paragraphs.len();
            }
        }
    }
    eprintln!("{paragraphs} paragraphs, {runs} runs match LibreOffice");
    assert!(paragraphs > 60, "only {paragraphs} paragraphs compared");
}
