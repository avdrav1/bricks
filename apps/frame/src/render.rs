//! Drawing a slide into a GTK snapshot. One renderer serves the slide list, the canvas, and
//! the slideshow (ADR 0004): lengths in the model are EMU and draw at `scale` pixels per
//! EMU, with the slide's top-left corner at the snapshot's origin. A shape's text lays out
//! once for drawing and for editing ([`place_text`]), so the caret lands where the text is.

use crate::deck::Deck;
use deck_edit::TextPos;
use gtk::prelude::*;
use gtk::{gdk, graphene, gsk, pango};
use presentation_model::{
    Align, Anchor, Background, Caps, Color, Crop, Dash, Fill, Frame, Layer, Outline, Paragraph,
    Shape, ShapeKind, Slide, Spacing, Table, TextBody, Vertical, EMU_PER_POINT,
};

fn rgba(c: Color) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
        f32::from(c.alpha) / 255.0,
    )
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> graphene::Rect {
    graphene::Rect::new(x, y, w, h)
}

/// The colour labelled outlines of content Frame can't draw use.
const UNSUPPORTED: Color = Color {
    r: 0x77,
    g: 0x76,
    b: 0x7B,
    alpha: 255,
};

/// Draw `slide` with its top-left corner at the origin, `scale` pixels per EMU.
pub fn draw_slide(
    snap: &gtk::Snapshot,
    ctx: &pango::Context,
    deck: &Deck,
    slide: &Slide,
    scale: f32,
) {
    let size = deck.presentation().size;
    let bounds = rect(0.0, 0.0, size.cx as f32 * scale, size.cy as f32 * scale);
    snap.push_clip(&bounds);
    match &slide.background {
        Background::Solid(c) => snap.append_color(&rgba(*c), &bounds),
        Background::Other { approx, .. } => snap.append_color(&rgba(*approx), &bounds),
        Background::Picture(image) => {
            snap.append_color(&gdk::RGBA::WHITE, &bounds);
            if let Some(t) = deck.texture(image) {
                snap.append_scaled_texture(&t, gsk::ScalingFilter::Trilinear, &bounds);
            }
        }
    }
    for shape in &slide.shapes {
        draw_shape(snap, ctx, deck, shape, scale);
    }
    snap.pop();
}

fn draw_shape(snap: &gtk::Snapshot, ctx: &pango::Context, deck: &Deck, shape: &Shape, scale: f32) {
    let f = &shape.frame;
    let (w, h) = (f.cx as f32 * scale, f.cy as f32 * scale);
    let (cx, cy) = f.centre();
    snap.save();
    snap.translate(&graphene::Point::new(cx as f32 * scale, cy as f32 * scale));
    if f.rotation != 0.0 {
        snap.rotate(f.rotation as f32);
    }
    // The shape itself mirrors; its text and tables don't (see `in_frame`).
    snap.save();
    snap.scale(
        if f.flip_h { -1.0 } else { 1.0 },
        if f.flip_v { -1.0 } else { 1.0 },
    );
    snap.translate(&graphene::Point::new(-w / 2.0, -h / 2.0));
    match &shape.kind {
        ShapeKind::Geometry { preset, adjust } => {
            let path = geometry(preset, *adjust, w, h);
            if !is_line(preset) {
                fill_path(snap, deck, &path, &shape.fill, w, h);
            }
            if let Some(o) = &shape.outline {
                stroke(snap, &path, o, scale);
            }
        }
        ShapeKind::Picture(p) => {
            match deck.texture(&p.image) {
                Some(t) => picture(snap, &t, &p.crop, w, h),
                None => unsupported(
                    snap,
                    ctx,
                    &format!("Picture ({})", p.image.format.name()),
                    w,
                    h,
                ),
            }
            if let Some(o) = &shape.outline {
                stroke(snap, &geometry("rect", None, w, h), o, scale);
            }
        }
        // Drawn below, unmirrored like text.
        ShapeKind::Table(_) => {}
        ShapeKind::Unsupported { label, preview } => {
            match preview.as_ref().and_then(|i| deck.texture(i)) {
                Some(t) => picture(snap, &t, &Crop::default(), w, h),
                None => unsupported(snap, ctx, label, w, h),
            }
        }
    }
    snap.restore();
    snap.restore();
    if let ShapeKind::Table(t) = &shape.kind {
        in_frame(snap, f, scale, |_, _| draw_table(snap, ctx, t, scale));
    }
    if let Some(placed) = shape
        .text
        .as_ref()
        .filter(|t| !t.is_empty())
        .and_then(|_| place_text(ctx, shape, scale))
    {
        snap.save();
        snap.transform(Some(&placed.to_slide));
        draw_paras(snap, &placed.block, placed.x0, placed.y0);
        snap.restore();
    }
}

/// From `f`'s box to the slide's pixels: the origin at the box's top-left corner, turned
/// with it. Text and tables turn with their shape but don't mirror (a vertical flip turns
/// them upside down, as PowerPoint draws them). Also gives the box's size in pixels.
fn frame_transform(f: &Frame, scale: f32) -> (gsk::Transform, f32, f32) {
    let (w, h) = (f.cx as f32 * scale, f.cy as f32 * scale);
    let (cx, cy) = f.centre();
    let turn = f.rotation as f32 + if f.flip_v { 180.0 } else { 0.0 };
    let t = gsk::Transform::new()
        .translate(&graphene::Point::new(cx as f32 * scale, cy as f32 * scale))
        .rotate(turn)
        .translate(&graphene::Point::new(-w / 2.0, -h / 2.0));
    (t, w, h)
}

/// Run `draw` with the origin at `f`'s top-left corner, turned with it
/// ([`frame_transform`]). `draw` gets the box's size in pixels.
fn in_frame(snap: &gtk::Snapshot, f: &Frame, scale: f32, draw: impl FnOnce(f32, f32)) {
    let (t, w, h) = frame_transform(f, scale);
    snap.save();
    snap.transform(Some(&t));
    draw(w, h);
    snap.restore();
}

/// `t` and a `w` x `h` box turned for `body`'s text: vertical text lays out in the box
/// turned a quarter, with width and height swapped.
fn upright(t: gsk::Transform, body: &TextBody, w: f32, h: f32) -> (gsk::Transform, f32, f32) {
    if body.vertical == Vertical::Horizontal {
        return (t, w, h);
    }
    let turn = if body.vertical == Vertical::Down {
        90.0
    } else {
        270.0
    };
    let t = t
        .translate(&graphene::Point::new(w / 2.0, h / 2.0))
        .rotate(turn)
        .translate(&graphene::Point::new(-h / 2.0, -w / 2.0));
    (t, h, w)
}

/// Whether the slide point (`x`, `y`) in pixels at `scale` is inside `f`'s box.
fn in_box(f: &Frame, scale: f32, x: f32, y: f32) -> bool {
    let (t, w, h) = frame_transform(f, scale);
    t.invert().is_ok_and(|inv| {
        let p = inv.transform_point(&graphene::Point::new(x, y));
        (0.0..=w).contains(&p.x()) && (0.0..=h).contains(&p.y())
    })
}

fn is_line(preset: &str) -> bool {
    preset == "line" || preset.contains("Connector")
}

/// The outline of preset geometry in a `w` x `h` box: rectangles, rounded rectangles,
/// ellipses, lines, and triangles; anything else is its bounding rectangle (spec FR0).
fn geometry(preset: &str, adjust: Option<f64>, w: f32, h: f32) -> gsk::Path {
    let b = gsk::PathBuilder::new();
    match preset {
        "roundRect" => {
            let r = (adjust.unwrap_or(0.16667) as f32).clamp(0.0, 0.5) * w.min(h);
            b.add_rounded_rect(&gsk::RoundedRect::from_rect(rect(0.0, 0.0, w, h), r));
        }
        "ellipse" => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            let k = std::f32::consts::FRAC_1_SQRT_2;
            b.move_to(w, ry);
            b.conic_to(w, h, rx, h, k);
            b.conic_to(0.0, h, 0.0, ry, k);
            b.conic_to(0.0, 0.0, rx, 0.0, k);
            b.conic_to(w, 0.0, w, ry, k);
            b.close();
        }
        "triangle" => {
            b.move_to(adjust.unwrap_or(0.5) as f32 * w, 0.0);
            b.line_to(w, h);
            b.line_to(0.0, h);
            b.close();
        }
        "rtTriangle" => {
            b.move_to(0.0, 0.0);
            b.line_to(w, h);
            b.line_to(0.0, h);
            b.close();
        }
        p if is_line(p) => {
            b.move_to(0.0, 0.0);
            b.line_to(w, h);
        }
        _ => b.add_rect(&rect(0.0, 0.0, w, h)),
    }
    b.to_path()
}

fn fill_path(snap: &gtk::Snapshot, deck: &Deck, path: &gsk::Path, fill: &Fill, w: f32, h: f32) {
    match fill {
        Fill::None => {}
        Fill::Solid(c) => snap.append_fill(path, gsk::FillRule::Winding, &rgba(*c)),
        // A gradient or pattern shows as the colour standing for it (its middle stop, its
        // foreground), closer to what PowerPoint shows than an outline.
        Fill::Unsupported { approx, .. } => {
            snap.append_fill(path, gsk::FillRule::Winding, &rgba(*approx))
        }
        Fill::Picture(image) => {
            if let Some(t) = deck.texture(image) {
                snap.push_fill(path, gsk::FillRule::Winding);
                snap.append_scaled_texture(
                    &t,
                    gsk::ScalingFilter::Trilinear,
                    &rect(0.0, 0.0, w, h),
                );
                snap.pop();
            }
        }
    }
}

fn stroke(snap: &gtk::Snapshot, path: &gsk::Path, o: &Outline, scale: f32) {
    // Width 0 is the thinnest line the screen shows.
    let width = (o.width as f32 * scale).max(0.75);
    let s = gsk::Stroke::new(width);
    let dash: &[f32] = match o.dash {
        Dash::Solid => &[],
        Dash::Dot => &[1.0, 1.0],
        Dash::Dash => &[4.0, 3.0],
        Dash::DashDot => &[4.0, 3.0, 1.0, 3.0],
        Dash::LongDash => &[8.0, 3.0],
    };
    if !dash.is_empty() {
        let d: Vec<f32> = dash.iter().map(|d| d * width).collect();
        s.set_dash(&d);
    }
    snap.append_stroke(path, &s, &rgba(o.color));
}

/// A picture in a `w` x `h` box, cropped by `crop` (fractions of the image cut off at
/// each edge; negative ones pad).
fn picture(snap: &gtk::Snapshot, t: &gdk::Texture, crop: &Crop, w: f32, h: f32) {
    let (l, t_, r, b) = (
        crop.left as f32,
        crop.top as f32,
        crop.right as f32,
        crop.bottom as f32,
    );
    let (fw, fh) = (1.0 - l - r, 1.0 - t_ - b);
    if fw <= 0.0 || fh <= 0.0 {
        return;
    }
    let (iw, ih) = (w / fw, h / fh);
    let bounds = rect(0.0, 0.0, w, h);
    snap.push_clip(&bounds);
    snap.append_scaled_texture(
        t,
        gsk::ScalingFilter::Trilinear,
        &rect(-l * iw, -t_ * ih, iw, ih),
    );
    snap.pop();
}

/// A labelled outline for content Frame can't draw, so nothing silently disappears.
fn unsupported(snap: &gtk::Snapshot, ctx: &pango::Context, label: &str, w: f32, h: f32) {
    let path = geometry("rect", None, w, h);
    let mut tint = UNSUPPORTED;
    tint.alpha = 24;
    snap.append_fill(&path, gsk::FillRule::Winding, &rgba(tint));
    let s = gsk::Stroke::new(1.0);
    s.set_dash(&[4.0, 3.0]);
    snap.append_stroke(&path, &s, &rgba(UNSUPPORTED));
    let layout = pango::Layout::new(ctx);
    let mut desc = pango::FontDescription::from_string("Sans");
    let px = (w.min(h) * 0.12).clamp(3.0, 16.0);
    desc.set_absolute_size(f64::from(px) * f64::from(pango::SCALE));
    layout.set_font_description(Some(&desc));
    layout.set_text(label);
    layout.set_width((w * pango::SCALE as f32) as i32);
    layout.set_alignment(pango::Alignment::Center);
    layout.set_wrap(pango::WrapMode::WordChar);
    let (_, logical) = layout.extents();
    let th = logical.height() as f32 / pango::SCALE as f32;
    snap.save();
    snap.translate(&graphene::Point::new(0.0, (h - th) / 2.0));
    snap.append_layout(&layout, &rgba(UNSUPPORTED));
    snap.restore();
}

/// Text laid out for one box width: paragraphs placed top to bottom from 0.
struct TextBlock {
    paras: Vec<LaidPara>,
    height: f32,
    /// The widest line's right edge.
    width: f32,
}

struct LaidPara {
    layout: pango::Layout,
    x: f32,
    y: f32,
    /// The layout's logical height.
    height: f32,
    bullet: Option<(pango::Layout, f32, f32)>,
}

fn units(px: f32) -> i32 {
    (px * pango::SCALE as f32).round() as i32
}

fn px(units: i32) -> f32 {
    units as f32 / pango::SCALE as f32
}

fn font(family: &str, size_px: f32) -> pango::FontDescription {
    let mut desc = pango::FontDescription::new();
    desc.set_family(if family.is_empty() { "Sans" } else { family });
    desc.set_absolute_size(f64::from(size_px.max(0.1)) * f64::from(pango::SCALE));
    desc
}

fn spacing_px(s: Spacing, size_px: f32, pt_px: f32) -> f32 {
    match s {
        Spacing::Points(p) => p as f32 * pt_px,
        Spacing::Lines(l) => l as f32 * size_px * 1.2,
    }
}

/// Lay out `body`'s paragraphs for text `width` pixels wide.
fn lay_out(ctx: &pango::Context, body: &TextBody, width: f32, scale: f32) -> TextBlock {
    let pt_px = EMU_PER_POINT as f32 * scale;
    let fs = body.font_scale as f32;
    let (mut y, mut right) = (0.0f32, 0.0f32);
    let mut paras = Vec::with_capacity(body.paragraphs.len());
    for (i, p) in body.paragraphs.iter().enumerate() {
        let first = p.first_run();
        let size_px = first.size as f32 * fs * pt_px;
        if i > 0 {
            y += spacing_px(p.space_before, size_px, pt_px);
        }
        let layout = paragraph_layout(ctx, p, &first.font, size_px, fs, pt_px);
        let bullet = p.shown_bullet().map(|b| {
            let bl = pango::Layout::new(ctx);
            bl.set_font_description(Some(&font(b.font, b.size as f32 * fs * pt_px)));
            bl.set_text(b.text);
            let attrs = pango::AttrList::new();
            let c = b.color;
            attrs.insert(pango::AttrColor::new_foreground(
                u16::from(c.r) * 257,
                u16::from(c.g) * 257,
                u16::from(c.b) * 257,
            ));
            attrs.insert(pango::AttrInt::new_foreground_alpha(
                u16::from(c.alpha) * 257,
            ));
            bl.set_attributes(Some(&attrs));
            bl
        });
        let (mar, ind) = (p.margin_left as f32 * scale, p.indent as f32 * scale);
        // DrawingML: the first line starts at marL + indent, the others at marL; a
        // bullet sits where the first line starts, and a hanging one leaves the text at
        // marL.
        let (mut x, indent) = match &bullet {
            Some(_) if ind < 0.0 => (mar, 0.0),
            Some(bl) => {
                let bw = px(bl.extents().1.width()) + 0.3 * size_px;
                (mar, ind + bw)
            }
            None => (mar + ind.min(0.0), ind),
        };
        layout.set_indent(units(indent));
        if body.wrap {
            layout.set_width(units((width - x).max(1.0)));
            layout.set_wrap(pango::WrapMode::WordChar);
        }
        match p.align {
            Align::Left => {}
            Align::Centre => layout.set_alignment(pango::Alignment::Center),
            Align::Right => layout.set_alignment(pango::Alignment::Right),
            Align::Justify | Align::Distributed => layout.set_justify(true),
        }
        let reduce = 1.0 - body.line_reduction as f32;
        match p.line_spacing {
            Spacing::Lines(l) => {
                let f = l as f32 * reduce;
                if (f - 1.0).abs() > 0.01 {
                    layout.set_line_spacing(f);
                }
            }
            Spacing::Points(pts) => {
                let natural = px(layout.extents().1.height()) / layout.line_count().max(1) as f32;
                if natural > 0.0 {
                    layout.set_line_spacing(pts as f32 * pt_px * reduce / natural);
                }
            }
        }
        let (_, logical) = layout.extents();
        let (lw, lh) = (px(logical.width()), px(logical.height()));
        if !body.wrap {
            // Unwrapped lines align in the box, not just among themselves.
            x += match p.align {
                Align::Centre => ((width - x) - lw) / 2.0,
                Align::Right => (width - x) - lw,
                _ => 0.0,
            };
        }
        let bullet = bullet.map(|bl| {
            let by = px(layout.baseline()) - px(bl.baseline());
            (bl, mar + ind, by)
        });
        right = right.max(x + px(logical.x()) + lw);
        paras.push(LaidPara {
            layout,
            x,
            y,
            height: lh,
            bullet: bullet.map(|(bl, bx, by)| (bl, bx, y + by)),
        });
        y += lh + spacing_px(p.space_after, size_px, pt_px);
    }
    TextBlock {
        paras,
        height: y,
        width: right,
    }
}

/// A paragraph's runs as one Pango layout, each run's look as attributes.
fn paragraph_layout(
    ctx: &pango::Context,
    p: &Paragraph,
    family: &str,
    size_px: f32,
    fs: f32,
    pt_px: f32,
) -> pango::Layout {
    let layout = pango::Layout::new(ctx);
    layout.set_font_description(Some(&font(family, size_px)));
    let mut text = String::new();
    let attrs = pango::AttrList::new();
    for run in &p.runs {
        let start = text.len() as u32;
        match run.caps {
            // Letter by letter, so the layout's text keeps the model's byte offsets (the
            // caret's positions): a letter whose capital is longer (ß) stays as it is.
            Caps::All => text.extend(run.text.chars().map(|c| {
                let mut upper = c.to_uppercase();
                match (upper.next(), upper.next()) {
                    (Some(u), None) if u.len_utf8() == c.len_utf8() => u,
                    _ => c,
                }
            })),
            _ => text.push_str(&run.text),
        }
        let end = text.len() as u32;
        if start == end {
            continue;
        }
        let add = |mut a: pango::Attribute| {
            a.set_start_index(start);
            a.set_end_index(end);
            attrs.insert(a);
        };
        let mut size = run.size as f32 * fs * pt_px;
        if run.baseline != 0.0 {
            add(pango::AttrInt::new_rise(units(run.baseline as f32 / 100.0 * size)).into());
            size *= 0.67;
        }
        add(pango::AttrString::new_family(if run.font.is_empty() {
            "Sans"
        } else {
            &run.font
        })
        .into());
        add(pango::AttrSize::new_size_absolute(units(size.max(0.1))).into());
        if run.bold {
            add(pango::AttrInt::new_weight(pango::Weight::Bold).into());
        }
        if run.italic {
            add(pango::AttrInt::new_style(pango::Style::Italic).into());
        }
        if run.underline {
            add(pango::AttrInt::new_underline(pango::Underline::Single).into());
        }
        if run.strike {
            add(pango::AttrInt::new_strikethrough(true).into());
        }
        if run.caps == Caps::Small {
            add(pango::AttrInt::new_variant(pango::Variant::SmallCaps).into());
        }
        let c = run.color;
        add(pango::AttrColor::new_foreground(
            u16::from(c.r) * 257,
            u16::from(c.g) * 257,
            u16::from(c.b) * 257,
        )
        .into());
        if c.alpha != 255 {
            add(pango::AttrInt::new_foreground_alpha(u16::from(c.alpha) * 257).into());
        }
    }
    // A word too long for its line breaks without a hyphen, as in PowerPoint.
    attrs.insert(pango::AttrInt::new_insert_hyphens(false));
    layout.set_text(&text);
    layout.set_attributes(Some(&attrs));
    layout
}

/// Lay out `body` in a `w` x `h` box: the block, and where it starts in the box (insets,
/// anchoring).
fn lay_out_in(
    ctx: &pango::Context,
    body: &TextBody,
    w: f32,
    h: f32,
    scale: f32,
) -> (TextBlock, f32, f32) {
    let ins = &body.insets;
    let block = lay_out(
        ctx,
        body,
        (w - (ins.left + ins.right) as f32 * scale).max(1.0),
        scale,
    );
    let (x0, y0) = origin(body, &block, w, h, scale);
    (block, x0, y0)
}

/// Where `block` starts in a `w` x `h` box: after the insets, anchored.
fn origin(body: &TextBody, block: &TextBlock, w: f32, h: f32, scale: f32) -> (f32, f32) {
    let ins = &body.insets;
    let (l, t, r, b) = (
        ins.left as f32 * scale,
        ins.top as f32 * scale,
        ins.right as f32 * scale,
        ins.bottom as f32 * scale,
    );
    let (aw, ah) = (w - l - r, h - t - b);
    let y0 = t + match body.anchor {
        Anchor::Top => 0.0,
        Anchor::Middle => (ah - block.height) / 2.0,
        Anchor::Bottom => ah - block.height,
    };
    let x0 = l + if body.anchor_centre {
        (aw - block.width) / 2.0
    } else {
        0.0
    };
    (x0, y0)
}

/// Draw `body` in a `w` x `h` box at the origin: insets, anchoring, autofit's font scale,
/// vertical text.
fn draw_text(
    snap: &gtk::Snapshot,
    ctx: &pango::Context,
    body: &TextBody,
    w: f32,
    h: f32,
    scale: f32,
) {
    if body.is_empty() {
        return;
    }
    let (t, w, h) = upright(gsk::Transform::new(), body, w, h);
    let (block, x0, y0) = lay_out_in(ctx, body, w, h, scale);
    snap.save();
    snap.transform(Some(&t));
    draw_paras(snap, &block, x0, y0);
    snap.restore();
}

fn draw_block(
    snap: &gtk::Snapshot,
    body: &TextBody,
    block: &TextBlock,
    w: f32,
    h: f32,
    scale: f32,
) {
    let (x0, y0) = origin(body, block, w, h, scale);
    draw_paras(snap, block, x0, y0);
}

/// Draw `block`'s paragraphs and bullets starting at (`x0`, `y0`).
fn draw_paras(snap: &gtk::Snapshot, block: &TextBlock, x0: f32, y0: f32) {
    let black = gdk::RGBA::BLACK;
    for p in &block.paras {
        if let Some((bl, bx, by)) = &p.bullet {
            snap.save();
            snap.translate(&graphene::Point::new(x0 + bx, y0 + by));
            snap.append_layout(bl, &black);
            snap.restore();
        }
        snap.save();
        snap.translate(&graphene::Point::new(x0 + p.x, y0 + p.y));
        snap.append_layout(&p.layout, &black);
        snap.restore();
    }
}

/// A shape's text as it lays out on the slide: for drawing it, and for the caret.
pub struct PlacedText {
    block: TextBlock,
    /// Where the block starts in the text's box.
    x0: f32,
    y0: f32,
    /// From the text's box (upright, its origin at the top-left corner) to the slide's
    /// pixels.
    to_slide: gsk::Transform,
}

/// Lay out `shape`'s text, empty or not, on the slide at `scale`; `None` without text.
pub fn place_text(ctx: &pango::Context, shape: &Shape, scale: f32) -> Option<PlacedText> {
    let body = shape.text.as_ref()?;
    let frame = shape.text_frame.as_ref().unwrap_or(&shape.frame);
    let (t, w, h) = frame_transform(frame, scale);
    let (to_slide, w, h) = upright(t, body, w, h);
    let (block, x0, y0) = lay_out_in(ctx, body, w, h, scale);
    Some(PlacedText {
        block,
        x0,
        y0,
        to_slide,
    })
}

/// The height of `shape`'s text laid out at `scale`, and the height its box leaves for
/// text (less its insets), in pixels; `None` without text.
pub fn text_room(ctx: &pango::Context, shape: &Shape, scale: f32) -> Option<(f32, f32)> {
    let body = shape.text.as_ref()?;
    let frame = shape.text_frame.as_ref().unwrap_or(&shape.frame);
    let (t, w, h) = frame_transform(frame, scale);
    let (_, w, h) = upright(t, body, w, h);
    let (block, _, _) = lay_out_in(ctx, body, w, h, scale);
    let ins = &body.insets;
    Some((block.height, h - (ins.top + ins.bottom) as f32 * scale))
}

/// The byte offset `trailing` characters after byte `index` of `layout`'s text.
fn offset(layout: &pango::Layout, index: i32, trailing: i32) -> usize {
    let text = layout.text();
    let start = usize::try_from(index).unwrap_or(0).min(text.len());
    let after: usize = text[start..]
        .chars()
        .take(usize::try_from(trailing).unwrap_or(0))
        .map(char::len_utf8)
        .sum();
    start + after
}

/// The top and bottom of `layout`'s line `line`, in Pango units.
fn line_yrange(layout: &pango::Layout, line: i32) -> (i32, i32) {
    let mut it = layout.iter();
    for _ in 0..line {
        if !it.next_line() {
            break;
        }
    }
    it.line_yrange()
}

impl PlacedText {
    /// The transform from the text's box to the slide's pixels.
    pub fn to_slide(&self) -> &gsk::Transform {
        &self.to_slide
    }

    /// The position nearest the slide point (`x`, `y`), in pixels.
    pub fn hit(&self, x: f32, y: f32) -> TextPos {
        let point = graphene::Point::new(x, y);
        let p = match self.to_slide.clone().invert() {
            Ok(inv) => inv.transform_point(&point),
            Err(_) => point,
        };
        self.hit_box(p.x(), p.y())
    }

    /// The position nearest the point (`x`, `y`) of the text's box.
    fn hit_box(&self, x: f32, y: f32) -> TextPos {
        let (x, y) = (x - self.x0, y - self.y0);
        let paras = &self.block.paras;
        let Some(last) = paras.len().checked_sub(1) else {
            return TextPos::default();
        };
        let i = paras
            .iter()
            .position(|p| y < p.y + p.height)
            .unwrap_or(last);
        let p = &paras[i];
        let (_, index, trailing) = p.layout.xy_to_index(units(x - p.x), units(y - p.y));
        TextPos::new(i, offset(&p.layout, index, trailing))
    }

    /// The caret at `pos`: a rectangle of no width as tall as its line, in the text's box.
    pub fn caret(&self, pos: TextPos) -> graphene::Rect {
        let Some(p) = self.block.paras.get(pos.paragraph) else {
            return rect(self.x0, self.y0, 0.0, 0.0);
        };
        let (strong, _) = p.layout.cursor_pos(pos.offset as i32);
        rect(
            self.x0 + p.x + px(strong.x()),
            self.y0 + p.y + px(strong.y()),
            0.0,
            px(strong.height()),
        )
    }

    /// The rectangles that highlight `from..to`, in the text's box.
    pub fn selection(&self, from: TextPos, to: TextPos) -> Vec<graphene::Rect> {
        let mut out = Vec::new();
        let paras = self.block.paras.iter().enumerate();
        for (i, p) in paras.take(to.paragraph + 1).skip(from.paragraph) {
            let a = if i == from.paragraph { from.offset } else { 0 };
            let b = if i == to.paragraph {
                to.offset
            } else {
                p.layout.text().len()
            };
            let mut it = p.layout.iter();
            loop {
                let (top, bottom) = it.line_yrange();
                if let Some(line) = it.line_readonly() {
                    let start = line.start_index().max(0) as usize;
                    let end = start + line.length().max(0) as usize;
                    let (la, lb) = (a.max(start), b.min(end));
                    if la < lb {
                        let ranges = line.x_ranges(la as i32, lb as i32);
                        for [x1, x2] in ranges.as_chunks::<2>().0 {
                            out.push(rect(
                                self.x0 + p.x + px(*x1),
                                self.y0 + p.y + px(top),
                                px(x2 - x1),
                                px(bottom - top),
                            ));
                        }
                    }
                }
                if !it.next_line() {
                    break;
                }
            }
        }
        out
    }

    /// The position a character left (`forward` false) or right of `pos` as the text
    /// shows, into the paragraph before or after at a paragraph's ends.
    pub fn step(&self, pos: TextPos, forward: bool) -> Option<TextPos> {
        let paras = &self.block.paras;
        let p = paras.get(pos.paragraph)?;
        let dir = if forward { 1 } else { -1 };
        let (index, trailing) = p
            .layout
            .move_cursor_visually(true, pos.offset as i32, 0, dir);
        if index < 0 {
            let i = pos.paragraph.checked_sub(1)?;
            return Some(TextPos::new(i, paras[i].layout.text().len()));
        }
        if index == i32::MAX {
            return (pos.paragraph + 1 < paras.len()).then(|| TextPos::new(pos.paragraph + 1, 0));
        }
        Some(TextPos::new(
            pos.paragraph,
            offset(&p.layout, index, trailing),
        ))
    }

    /// The position on the line above or below `pos` nearest `x` of the text's box.
    pub fn vertical(&self, pos: TextPos, up: bool, x: f32) -> Option<TextPos> {
        let paras = &self.block.paras;
        let p = paras.get(pos.paragraph)?;
        let (line, _) = p.layout.index_to_line_x(pos.offset as i32, false);
        let (i, line) = if up && line > 0 {
            (pos.paragraph, line - 1)
        } else if up {
            let i = pos.paragraph.checked_sub(1)?;
            (i, paras[i].layout.line_count() - 1)
        } else if line + 1 < p.layout.line_count() {
            (pos.paragraph, line + 1)
        } else if pos.paragraph + 1 < paras.len() {
            (pos.paragraph + 1, 0)
        } else {
            return None;
        };
        let q = &paras[i];
        let (top, bottom) = line_yrange(&q.layout, line);
        let (_, index, trailing) = q
            .layout
            .xy_to_index(units(x - self.x0 - q.x), (top + bottom) / 2);
        Some(TextPos::new(i, offset(&q.layout, index, trailing)))
    }

    /// The start and end of the line `pos` is on (Home and End): the end before the
    /// line's break or the space it wraps at.
    pub fn line_ends(&self, pos: TextPos) -> (TextPos, TextPos) {
        let Some(p) = self.block.paras.get(pos.paragraph) else {
            return (pos, pos);
        };
        let (n, _) = p.layout.index_to_line_x(pos.offset as i32, false);
        let Some(line) = p.layout.line_readonly(n) else {
            return (pos, pos);
        };
        let text = p.layout.text();
        let start = (line.start_index().max(0) as usize).min(text.len());
        let mut end = (start + line.length().max(0) as usize).min(text.len());
        if let Some(c) = text[start..end].chars().next_back() {
            if c == '\n' || (end < text.len() && c.is_whitespace()) {
                end -= c.len_utf8();
            }
        }
        (
            TextPos::new(pos.paragraph, start),
            TextPos::new(pos.paragraph, end),
        )
    }
}

/// The topmost of the slide's own shapes at the slide point (`x`, `y`), in pixels at
/// `scale`, when it has text a save can write (it has an id; a SmartArt drawing's shapes
/// don't): the shape to edit there.
pub fn text_shape_at(slide: &Slide, scale: f32, x: f32, y: f32) -> Option<usize> {
    let (i, shape) = slide
        .shapes
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, s)| s.layer == Layer::Slide)
        .find(|(_, s)| {
            in_box(&s.frame, scale, x, y) || s.text_frame.is_some_and(|f| in_box(&f, scale, x, y))
        })?;
    (shape.text.is_some() && shape.id.is_some()).then_some(i)
}

fn dashed_box(snap: &gtk::Snapshot, f: &Frame, scale: f32, color: &gdk::RGBA) {
    in_frame(snap, f, scale, |w, h| {
        let s = gsk::Stroke::new(1.0);
        s.set_dash(&[4.0, 3.0]);
        snap.append_stroke(&geometry("rect", None, w, h), &s, color);
    });
}

/// Dashed outlines of the slide's empty placeholders, so there's somewhere to click to
/// type into them (a placeholder a picture fills isn't empty). The canvas shows them;
/// thumbnails and the slideshow don't.
pub fn draw_empty_placeholders(snap: &gtk::Snapshot, slide: &Slide, scale: f32, color: &gdk::RGBA) {
    let empty = slide.shapes.iter().filter(|s| {
        s.layer == Layer::Slide
            && s.placeholder.is_some()
            && matches!(s.kind, ShapeKind::Geometry { .. })
            && s.text.as_ref().is_none_or(TextBody::is_empty)
    });
    for shape in empty {
        dashed_box(snap, &shape.frame, scale, color);
    }
}

/// The text being edited: its box, the selection from `anchor` to `caret`, and the caret
/// in the colour of the text it types.
pub fn draw_editing(
    snap: &gtk::Snapshot,
    ctx: &pango::Context,
    shape: &Shape,
    scale: f32,
    anchor: TextPos,
    caret: TextPos,
    accent: &gdk::RGBA,
) {
    let (Some(body), Some(placed)) = (shape.text.as_ref(), place_text(ctx, shape, scale)) else {
        return;
    };
    dashed_box(
        snap,
        shape.text_frame.as_ref().unwrap_or(&shape.frame),
        scale,
        accent,
    );
    snap.save();
    snap.transform(Some(&placed.to_slide));
    if anchor != caret {
        let highlight = gdk::RGBA::new(accent.red(), accent.green(), accent.blue(), 0.3);
        for r in placed.selection(anchor.min(caret), anchor.max(caret)) {
            snap.append_color(&highlight, &r);
        }
    }
    let mut color = deck_edit::look(body, caret, caret).map_or(Color::BLACK, |r| r.color);
    if color.alpha == 0 {
        color = Color::BLACK;
    }
    color.alpha = 255;
    let c = placed.caret(caret);
    snap.append_color(&rgba(color), &rect(c.x() - 0.75, c.y(), 1.5, c.height()));
    snap.restore();
}

/// The topmost of the slide's own shapes whose box is at the slide point (`x`, `y`), in
/// pixels at `scale`, give or take `slop` pixels (so a line can be hit).
pub fn shape_at(slide: &Slide, scale: f32, x: f32, y: f32, slop: f32) -> Option<usize> {
    slide
        .shapes
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, s)| s.layer == Layer::Slide)
        .find(|(_, s)| local_point(&s.frame, scale, x, y).is_some_and(|p| p.within(slop)))
        .map(|(i, _)| i)
}

/// A slide point in a box's own frame (origin at its top-left corner, turned with it),
/// and the box's size, all in pixels.
#[derive(Debug, Clone, Copy)]
pub struct LocalPoint {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl LocalPoint {
    /// Inside the box grown by `slop` on every side.
    pub fn within(&self, slop: f32) -> bool {
        (-slop..=self.w + slop).contains(&self.x) && (-slop..=self.h + slop).contains(&self.y)
    }

    /// The selection handle within `slop` of the point: which edges it holds (left,
    /// right, top, bottom).
    pub fn handle(&self, slop: f32) -> Option<(bool, bool, bool, bool)> {
        HANDLES.iter().find_map(|&(fx, fy)| {
            let near = (self.x - fx * self.w).abs() <= slop && (self.y - fy * self.h).abs() <= slop;
            near.then_some((fx == 0.0, fx == 1.0, fy == 0.0, fy == 1.0))
        })
    }
}

/// The slide point (`x`, `y`) in pixels at `scale` in `f`'s own frame.
pub fn local_point(f: &Frame, scale: f32, x: f32, y: f32) -> Option<LocalPoint> {
    let (t, w, h) = frame_transform(f, scale);
    let p = t
        .invert()
        .ok()?
        .transform_point(&graphene::Point::new(x, y));
    Some(LocalPoint {
        x: p.x(),
        y: p.y(),
        w,
        h,
    })
}

/// The selection's handles, as fractions of its box: corners and the middles of edges.
const HANDLES: [(f32, f32); 8] = [
    (0.0, 0.0),
    (0.5, 0.0),
    (1.0, 0.0),
    (0.0, 0.5),
    (1.0, 0.5),
    (0.0, 1.0),
    (0.5, 1.0),
    (1.0, 1.0),
];

/// A selected shape: its box outlined, with handles to resize it by. `k` is the extra
/// scale the slide is drawn with (the outline stays one screen pixel wide).
pub fn draw_selection(snap: &gtk::Snapshot, f: &Frame, scale: f32, k: f32, color: &gdk::RGBA) {
    let px = 1.0 / k;
    in_frame(snap, f, scale, |w, h| {
        snap.append_stroke(&geometry("rect", None, w, h), &gsk::Stroke::new(px), color);
        let size = 7.0 * px;
        let white = gdk::RGBA::WHITE;
        for (fx, fy) in HANDLES {
            let r = rect(fx * w - size / 2.0, fy * h - size / 2.0, size, size);
            snap.append_color(color, &r);
            let inner = rect(r.x() + px, r.y() + px, size - 2.0 * px, size - 2.0 * px);
            snap.append_color(&white, &inner);
        }
    });
}

/// Smart guides: lines one screen pixel wide, on whole pixels so they draw solid (`k` as
/// in [`draw_selection`]; the slide's origin is on a whole pixel).
pub fn draw_guides(
    snap: &gtk::Snapshot,
    guides: &[deck_edit::Guide],
    scale: f32,
    k: f32,
    color: &gdk::RGBA,
) {
    let px = 1.0 / k;
    let pixel = |emu: i64| (emu as f32 * scale * k).floor() / k;
    for g in guides {
        let (x0, y0) = (pixel(g.from.0), pixel(g.from.1));
        let (x1, y1) = (pixel(g.to.0), pixel(g.to.1));
        let r = if g.from.0 == g.to.0 {
            rect(x0, y0.min(y1), px, (y1 - y0).abs())
        } else {
            rect(x0.min(x1), y0, (x1 - x0).abs(), px)
        };
        snap.append_color(color, &r);
    }
}

/// A table at the origin: rows grow to fit their text, as PowerPoint draws them; fills,
/// then text, then borders over both.
fn draw_table(snap: &gtk::Snapshot, ctx: &pango::Context, table: &Table, scale: f32) {
    let mut xs = vec![0.0f32];
    for c in &table.columns {
        xs.push(xs.last().copied().unwrap_or(0.0) + *c as f32 * scale);
    }
    let ncols = table.columns.len();
    let col_x = |c: usize| xs[c.min(ncols)];
    let mut blocks: Vec<Vec<Option<TextBlock>>> = Vec::with_capacity(table.rows.len());
    let mut ys = vec![0.0f32];
    for row in &table.rows {
        let mut height = row.height as f32 * scale;
        let mut laid = Vec::with_capacity(row.cells.len());
        for (c, cell) in row.cells.iter().enumerate() {
            if cell.merged || cell.text.is_empty() || cell.text.vertical != Vertical::Horizontal {
                laid.push(None);
                continue;
            }
            let ins = &cell.text.insets;
            let w = col_x(c + cell.col_span.max(1) as usize) - col_x(c);
            let block = lay_out(
                ctx,
                &cell.text,
                (w - (ins.left + ins.right) as f32 * scale).max(1.0),
                scale,
            );
            if cell.row_span <= 1 {
                height = height.max(block.height + (ins.top + ins.bottom) as f32 * scale);
            }
            laid.push(Some(block));
        }
        blocks.push(laid);
        ys.push(ys.last().copied().unwrap_or(0.0) + height);
    }
    let nrows = table.rows.len();
    let row_y = |r: usize| ys[r.min(nrows)];
    let cell_rect = |r: usize, c: usize, cell: &presentation_model::TableCell| {
        let (x0, y0) = (col_x(c), row_y(r));
        let x1 = col_x(c + cell.col_span.max(1) as usize);
        let y1 = row_y(r + cell.row_span.max(1) as usize);
        (x0, y0, x1 - x0, y1 - y0)
    };
    let cells = || {
        table.rows.iter().enumerate().flat_map(|(r, row)| {
            row.cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| !cell.merged)
                .map(move |(c, cell)| (r, c, cell))
        })
    };
    for (r, c, cell) in cells() {
        if let Some(fill) = cell.fill {
            let (x, y, w, h) = cell_rect(r, c, cell);
            snap.append_color(&rgba(fill), &rect(x, y, w, h));
        }
    }
    for (r, c, cell) in cells() {
        let (x, y, w, h) = cell_rect(r, c, cell);
        snap.save();
        snap.translate(&graphene::Point::new(x, y));
        match &blocks[r][c] {
            Some(block) => draw_block(snap, &cell.text, block, w, h, scale),
            None => draw_text(snap, ctx, &cell.text, w, h, scale),
        }
        snap.restore();
    }
    for (r, c, cell) in cells() {
        let (x, y, w, h) = cell_rect(r, c, cell);
        let b = &cell.borders;
        let sides = [
            (b.top, (x, y), (x + w, y)),
            (b.bottom, (x, y + h), (x + w, y + h)),
            (b.left, (x, y), (x, y + h)),
            (b.right, (x + w, y), (x + w, y + h)),
        ];
        for (side, (x1, y1), (x2, y2)) in sides {
            if let Some(o) = side {
                let pb = gsk::PathBuilder::new();
                pb.move_to(x1, y1);
                pb.line_to(x2, y2);
                stroke(snap, &pb.to_path(), &o, scale);
            }
        }
    }
}
