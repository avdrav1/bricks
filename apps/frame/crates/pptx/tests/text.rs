//! FRAME-2 acceptance: every paragraph's text, level, and alignment, and every run's size,
//! bold, italic, underline, strikethrough, colour, and font match LibreOffice's reading
//! (`scripts/lo_pptx.py dump`).
//!
//! Runs are compared character by character: LibreOffice merges neighbouring runs that
//! look the same and splits where a field starts, so run boundaries differ while every
//! character's look is the same. Sizes are the written sizes, before autofit's font scale
//! (LibreOffice keeps that apart too).

mod common;

use common::{dump, DPara, DRun, DECKS};
use presentation_model::{Align, Paragraph, Run};

fn align(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Centre => "center",
        Align::Right => "right",
        Align::Justify | Align::Distributed => "justify",
    }
}

/// Each character with the look of its run.
fn chars_of_model(p: &Paragraph) -> Vec<(char, &Run)> {
    p.runs
        .iter()
        .flat_map(|r| r.text.chars().map(move |c| (c, r)))
        .collect()
}

fn chars_of_dump(p: &DPara) -> Vec<(char, &DRun)> {
    p.runs
        .iter()
        .flat_map(|r| r.text.chars().map(move |c| (c, r)))
        .collect()
}

fn compare(at: &str, ours: &[Paragraph], theirs: &[DPara]) -> usize {
    assert_eq!(
        ours.len(),
        theirs.len(),
        "{at}: paragraphs {:?} vs LibreOffice {:?}",
        ours.iter().map(Paragraph::text).collect::<Vec<_>>(),
        theirs.iter().map(|p| &p.text).collect::<Vec<_>>()
    );
    let mut runs = 0;
    for (i, (p, lo)) in ours.iter().zip(theirs).enumerate() {
        let at = format!("{at} paragraph {i} {:?}", lo.text);
        assert_eq!(p.text(), lo.text, "{at}: text");
        if lo.text.is_empty() {
            continue;
        }
        assert_eq!(u32::from(p.level), lo.level, "{at}: level");
        assert_eq!(align(p.align), lo.align, "{at}: alignment");
        let (a, b) = (chars_of_model(p), chars_of_dump(lo));
        assert_eq!(a.len(), b.len(), "{at}: characters in runs");
        for ((c, run), (_, lo_run)) in a.iter().zip(&b) {
            let at = format!("{at} at {c:?}");
            assert!(
                (run.size - lo_run.size).abs() < 0.05,
                "{at}: size {} vs {}",
                run.size,
                lo_run.size
            );
            assert_eq!(run.bold, lo_run.bold, "{at}: bold");
            assert_eq!(run.italic, lo_run.italic, "{at}: italic");
            assert_eq!(run.underline, lo_run.underline, "{at}: underline");
            assert_eq!(run.strike, lo_run.strike, "{at}: strikethrough");
            assert_eq!(run.color.hex(), lo_run.color, "{at}: colour");
            assert_eq!(run.font, lo_run.font, "{at}: font");
        }
        runs += p.runs.len();
    }
    runs
}

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
                runs += compare(&at, &text.paragraphs, &lo_shape.paragraphs);
                paragraphs += text.paragraphs.len();
            }
        }
    }
    eprintln!("{paragraphs} paragraphs, {runs} runs match LibreOffice");
    assert!(paragraphs > 60, "only {paragraphs} paragraphs compared");
}
