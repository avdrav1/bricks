//! Drawing a slide into a GTK snapshot. One renderer serves the slide list, the canvas, and
//! the slideshow (ADR 0004): lengths in the model are EMU and draw at `scale` pixels per
//! EMU, with the slide's top-left corner at the snapshot's origin.

use crate::deck::Deck;
use gtk::prelude::*;
use gtk::{gdk, graphene, gsk, pango};
use presentation_model::{
    Align, Anchor, Background, Caps, Color, Crop, Dash, Fill, Frame, Outline, Paragraph, Shape,
    ShapeKind, Slide, Spacing, Table, TextBody, Vertical, EMU_PER_POINT,
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
    let size = deck.presentation.size;
    let bounds = rect(0.0, 0.0, size.cx as f32 * scale, size.cy as f32 * scale);
    snap.push_clip(&bounds);
    match &slide.background {
        Background::Solid(c) => snap.append_color(&rgba(*c), &bounds),
        Background::Other { approx, .. } => snap.append_color(&rgba(*approx), &bounds),
        Background::Picture(image) => {
            snap.append_color(&gdk::RGBA::WHITE, &bounds);
            if let Some(t) = deck.texture(image) {
                snap.append_scaled_texture(t, gsk::ScalingFilter::Trilinear, &bounds);
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
                Some(t) => picture(snap, t, &p.crop, w, h),
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
                Some(t) => picture(snap, t, &Crop::default(), w, h),
                None => unsupported(snap, ctx, label, w, h),
            }
        }
    }
    snap.restore();
    snap.restore();
    if let ShapeKind::Table(t) = &shape.kind {
        in_frame(snap, f, scale, |_, _| draw_table(snap, ctx, t, scale));
    }
    if let Some(text) = &shape.text {
        let frame = shape.text_frame.as_ref().unwrap_or(f);
        in_frame(snap, frame, scale, |w, h| {
            draw_text(snap, ctx, text, w, h, scale)
        });
    }
}

/// Run `draw` with the origin at `f`'s top-left corner, turned with it: text and tables
/// turn with their shape but don't mirror (a vertical flip turns them upside down, as
/// PowerPoint draws them). `draw` gets the box's size in pixels.
fn in_frame(snap: &gtk::Snapshot, f: &Frame, scale: f32, draw: impl FnOnce(f32, f32)) {
    let (w, h) = (f.cx as f32 * scale, f.cy as f32 * scale);
    let (cx, cy) = f.centre();
    snap.save();
    snap.translate(&graphene::Point::new(cx as f32 * scale, cy as f32 * scale));
    let turn = f.rotation as f32 + if f.flip_v { 180.0 } else { 0.0 };
    if turn != 0.0 {
        snap.rotate(turn);
    }
    snap.translate(&graphene::Point::new(-w / 2.0, -h / 2.0));
    draw(w, h);
    snap.restore();
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
                snap.append_scaled_texture(t, gsk::ScalingFilter::Trilinear, &rect(0.0, 0.0, w, h));
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
        let first = p.runs.iter().find(|r| !r.text.is_empty() && r.text != "\n");
        let size_px = first.map_or(p.end_size, |r| r.size) as f32 * fs * pt_px;
        if i > 0 {
            y += spacing_px(p.space_before, size_px, pt_px);
        }
        let layout = paragraph_layout(
            ctx,
            p,
            first.map_or("", |r| r.font.as_str()),
            size_px,
            fs,
            pt_px,
        );
        let has_text = first.is_some();
        let bullet = p.bullet.as_ref().filter(|_| has_text).map(|b| {
            let bl = pango::Layout::new(ctx);
            bl.set_font_description(Some(&font(&b.font, b.size as f32 * fs * pt_px)));
            bl.set_text(&b.text);
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
            Caps::All => text.push_str(&run.text.to_uppercase()),
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
    if body.vertical == Vertical::Horizontal {
        let ins = &body.insets;
        let block = lay_out(
            ctx,
            body,
            (w - (ins.left + ins.right) as f32 * scale).max(1.0),
            scale,
        );
        draw_block(snap, body, &block, w, h, scale);
        return;
    }
    // Turned a quarter: lay out in the box with width and height swapped.
    snap.save();
    snap.translate(&graphene::Point::new(w / 2.0, h / 2.0));
    snap.rotate(if body.vertical == Vertical::Down {
        90.0
    } else {
        270.0
    });
    snap.translate(&graphene::Point::new(-h / 2.0, -w / 2.0));
    let ins = &body.insets;
    let block = lay_out(
        ctx,
        body,
        (h - (ins.left + ins.right) as f32 * scale).max(1.0),
        scale,
    );
    draw_block(snap, body, &block, h, w, scale);
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
