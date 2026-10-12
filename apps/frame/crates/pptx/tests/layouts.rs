//! FRAME-7: the deck's slide layouts, which layout each slide is on, and the new slide each
//! layout makes, as PowerPoint makes it (empty placeholders that take everything from
//! the layout and master).

mod common;

use presentation_model::{Layer, PlaceholderKind, Shape, Source};

#[test]
fn layouts_are_listed_master_by_master_with_their_names_and_types() {
    let deck = pptx::read(&common::data("layouts.pptx")).unwrap();
    let layouts: Vec<(&str, &str)> = deck
        .layouts
        .iter()
        .map(|l| (l.name.as_str(), l.kind.as_str()))
        .collect();
    assert_eq!(
        layouts,
        [
            ("Title Slide", "title"),
            ("Title and Content", "obj"),
            ("Section Header", "secHead"),
            ("Two Content", "twoObj"),
            ("Comparison", "twoTxTwoObj"),
            ("Title Only", "titleOnly"),
            ("Blank", "blank"),
            ("Content with Caption", "objTx"),
            ("Picture with Caption", "picTx"),
            ("Title and Vertical Text", "vertTx"),
            ("Vertical Title and Text", "vertTitleAndTx"),
        ]
    );
    assert_eq!(deck.layouts[1].part, "ppt/slideLayouts/slideLayout2.xml");
    // Slides 1 to 9 are on layouts 1 to 9, slide 10 on Title and Content.
    let on: Vec<Option<usize>> = deck.slides.iter().map(|s| s.layout).collect();
    let want: Vec<Option<usize>> = (0..9).chain([1]).map(Some).collect();
    assert_eq!(on, want);
    assert_eq!(
        deck.slides[2].part.as_deref(),
        Some("ppt/slides/slide3.xml")
    );

    // Seven masters, each with its layouts, in presentation.xml's order.
    let themes = pptx::read(&common::data("themes.pptx")).unwrap();
    assert_eq!(themes.layouts.len(), 77);
    let masters: Vec<usize> = themes.layouts.iter().map(|l| l.master).collect();
    assert!(masters.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(masters.last(), Some(&6));
    for slide in &themes.slides {
        let l = slide.layout.expect("every slide's layout is listed");
        assert_eq!(themes.layouts[l].slide.layout, Some(l));
    }
}

fn own(shapes: &[Shape]) -> Vec<&Shape> {
    shapes.iter().filter(|s| s.layer == Layer::Slide).collect()
}

#[test]
fn a_new_slide_has_its_layouts_placeholders_empty() {
    let deck = pptx::read(&common::data("layouts.pptx")).unwrap();
    // Title and Content: a title and a content placeholder; no date, footer, or number.
    let blank = &deck.layouts[1].slide;
    let placeholders: Vec<Option<PlaceholderKind>> =
        own(&blank.shapes).iter().map(|s| s.placeholder).collect();
    assert_eq!(
        placeholders,
        [Some(PlaceholderKind::Title), Some(PlaceholderKind::Body)]
    );
    assert!(blank.part.is_none() && blank.text().is_empty());
    // Slide 2 is on the same layout, and its placeholders have no frames or looks of
    // their own: the new slide's sit where they do and type the same text.
    let slide2 = own(&deck.slides[1].shapes);
    for (new, old) in own(&blank.shapes).iter().zip(&slide2) {
        assert_eq!(new.frame, old.frame, "{}", old.name);
        assert_eq!(new.name, old.name);
        let mark = &new.text.as_ref().unwrap().paragraphs[0].end;
        let run = &old.text.as_ref().unwrap().paragraphs[0].runs[0];
        assert!(mark.same_look(run), "{}: {mark:?} vs {run:?}", old.name);
        assert_ne!(mark.sources.size, Source::Own);
    }
    // The body's empty paragraph keeps its bullet, shown once it has text.
    let body = own(&blank.shapes)[1].text.as_ref().unwrap();
    assert!(body.paragraphs[0].bullet.is_some());
    assert!(body.paragraphs[0].shown_bullet().is_none());
    // What the layout and master show comes along, as on slide 2.
    let behind = |shapes: &[Shape]| -> Vec<String> {
        shapes
            .iter()
            .filter(|s| s.layer != Layer::Slide)
            .map(|s| s.name.clone())
            .collect()
    };
    assert_eq!(behind(&blank.shapes), behind(&deck.slides[1].shapes));
    assert_eq!(blank.background, deck.slides[1].background);
}
