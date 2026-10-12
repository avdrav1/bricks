//! Moving and resizing shapes (FRAME-10): [`FrameEdit`], one undoable change of a shape's
//! box, and [`snap`], where a dragged box lands: aligned with the slide's edges and centre,
//! the layout's placeholders, and the other shapes' edges and centres, or spaced as two
//! other shapes are, when within a tolerance; with the guide lines to draw for it.
//!
//! Lengths are EMU, as in the model. Boxes are the shapes' unrotated frames.

use crate::{Change, ShapeRef};
use presentation_model::{Frame, Layer, Presentation, Shape, Slide};

/// Whether a shape can be moved and resized on its own: one of the slide's own shapes that
/// a save can write, not inside a group.
pub fn movable(shape: &Shape) -> bool {
    shape.layer == Layer::Slide && shape.id.is_some() && !shape.grouped
}

/// A shape's box moved or resized: it holds the box before and after.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameEdit {
    shape: ShapeRef,
    from: Frame,
    to: Frame,
}

impl FrameEdit {
    /// Give `shape` the position and size of `to` (its rotation and flips stay); `None`
    /// when the shape can't move or that's where it is.
    pub fn new(deck: &Presentation, shape: ShapeRef, to: Frame) -> Option<FrameEdit> {
        let s = deck.slides.get(shape.slide)?.shapes.get(shape.shape)?;
        let from = s.frame;
        let to = Frame {
            rotation: from.rotation,
            flip_h: from.flip_h,
            flip_v: from.flip_v,
            cx: to.cx.max(1),
            cy: to.cy.max(1),
            ..to
        };
        (movable(s) && to != from).then_some(FrameEdit { shape, from, to })
    }

    fn set(&self, deck: &mut Presentation, frame: Frame) -> Change {
        deck.slides[self.shape.slide].shapes[self.shape.shape].frame = frame;
        Change::Shape { shape: self.shape }
    }

    pub(crate) fn apply(&self, deck: &mut Presentation) -> Change {
        self.set(deck, self.to)
    }

    pub(crate) fn revert(&self, deck: &mut Presentation) -> Change {
        self.set(deck, self.from)
    }
}

/// What part of a box a drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    /// The whole box: it moves.
    Move,
    /// Edges of it, by a resize handle: those edges move.
    Resize {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

/// A guide line to draw, from one point to another, in EMU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guide {
    pub from: (i64, i64),
    pub to: (i64, i64),
}

/// Where a dragged box lands, and the guides that show why.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapped {
    pub frame: Frame,
    pub guides: Vec<Guide>,
}

/// A line a box edge or centre can align with, on one axis: where it is, and how far it
/// runs on the other axis (for drawing its guide).
#[derive(Debug, Clone, Copy)]
struct Line {
    at: i64,
    from: i64,
    to: i64,
}

/// One axis of a box: start, size; and the other axis's start and end.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: i64,
    size: i64,
}

impl Span {
    fn end(&self) -> i64 {
        self.start + self.size
    }

    fn centre(&self) -> i64 {
        self.start + self.size / 2
    }
}

/// The best way to move one axis: by `delta`, and the guides it draws.
struct Pick {
    delta: i64,
    spacing: Option<(Span, Span)>,
}

/// Where a box dragged to `frame` on slide `slide`, as `grab` holds it, lands: each axis
/// snaps on its own to the nearest alignment within `tolerance` EMU (6 screen pixels at
/// the canvas's scale): the slide's edges and centre; the slide's layout's placeholders'
/// and the slide's other shapes' edges and centres; or, moving, the spacing of two other
/// shapes in a row with it (after the second, before the first, or between them). Shape
/// `moving` (the one dragged) isn't a target.
pub fn snap(
    deck: &Presentation,
    slide: usize,
    moving: usize,
    frame: Frame,
    grab: Grab,
    tolerance: i64,
) -> Snapped {
    let Some(s) = deck.slides.get(slide) else {
        return Snapped {
            frame,
            guides: Vec::new(),
        };
    };
    let size = deck.size;
    let boxes = targets(deck, s, moving);
    let x = Span {
        start: frame.x,
        size: frame.cx,
    };
    let y = Span {
        start: frame.y,
        size: frame.cy,
    };
    let (moves_start, moves_end, moves_whole) = match grab {
        Grab::Move => (true, true, true),
        Grab::Resize { left, right, .. } => (left, right, false),
    };
    let x_lines = lines(size.cx, size.cy, boxes.iter().map(|b| (b.0, b.1)));
    let x_pick = pick(
        x,
        y,
        &x_lines,
        &boxes_axis(&boxes, true),
        moves_start,
        moves_end,
        moves_whole,
        tolerance,
    );
    let (moves_start, moves_end) = match grab {
        Grab::Move => (true, true),
        Grab::Resize { top, bottom, .. } => (top, bottom),
    };
    let y_lines = lines(size.cy, size.cx, boxes.iter().map(|b| (b.1, b.0)));
    let y_pick = pick(
        y,
        x,
        &y_lines,
        &boxes_axis(&boxes, false),
        moves_start,
        moves_end,
        moves_whole,
        tolerance,
    );

    let apply = |span: Span, p: &Option<Pick>, starts: bool, ends: bool| -> Span {
        let Some(p) = p else { return span };
        match (starts, ends) {
            (true, true) => Span {
                start: span.start + p.delta,
                ..span
            },
            (true, false) => Span {
                start: span.start + p.delta,
                size: span.size - p.delta,
            },
            (false, true) => Span {
                size: span.size + p.delta,
                ..span
            },
            (false, false) => span,
        }
    };
    let (sx, ex) = match grab {
        Grab::Move => (true, true),
        Grab::Resize { left, right, .. } => (left, right),
    };
    let (sy, ey) = match grab {
        Grab::Move => (true, true),
        Grab::Resize { top, bottom, .. } => (top, bottom),
    };
    let nx = apply(x, &x_pick, sx, ex);
    let ny = apply(y, &y_pick, sy, ey);
    let snapped = Frame {
        x: nx.start,
        y: ny.start,
        cx: nx.size.max(1),
        cy: ny.size.max(1),
        ..frame
    };

    // Guides: every line the box's moving edges or centre now sit on, and the spacing
    // that was matched.
    let mut guides = Vec::new();
    let features = |span: Span, starts: bool, ends: bool| -> Vec<i64> {
        let mut f = Vec::new();
        if starts {
            f.push(span.start);
        }
        if starts && ends {
            f.push(span.centre());
        }
        if ends {
            f.push(span.end());
        }
        f
    };
    if x_pick.is_some() {
        for l in &x_lines {
            if features(nx, sx, ex).contains(&l.at) {
                guides.push(Guide {
                    from: (l.at, l.from.min(ny.start)),
                    to: (l.at, l.to.max(ny.end())),
                });
            }
        }
    }
    if y_pick.is_some() {
        for l in &y_lines {
            if features(ny, sy, ey).contains(&l.at) {
                guides.push(Guide {
                    from: (l.from.min(nx.start), l.at),
                    to: (l.to.max(nx.end()), l.at),
                });
            }
        }
    }
    let mid_y = ny.centre();
    if let Some((a, b)) = x_pick.as_ref().and_then(|p| p.spacing) {
        guides.push(Guide {
            from: (a.end(), mid_y),
            to: (b.start, mid_y),
        });
        guides.push(gap_guide(a, b, nx, mid_y, true));
    }
    let mid_x = nx.centre();
    if let Some((a, b)) = y_pick.as_ref().and_then(|p| p.spacing) {
        guides.push(Guide {
            from: (mid_x, a.end()),
            to: (mid_x, b.start),
        });
        guides.push(gap_guide(a, b, ny, mid_x, false));
    }
    Snapped {
        frame: snapped,
        guides,
    }
}

/// The guide for the moving box's own gap, beside the gap between `a` and `b` it matches.
fn gap_guide(a: Span, b: Span, moving: Span, across: i64, horizontal: bool) -> Guide {
    let (s, e) = if moving.start >= b.end() {
        (b.end(), moving.start)
    } else if moving.end() <= a.start {
        (moving.end(), a.start)
    } else {
        (a.end(), moving.start)
    };
    if horizontal {
        Guide {
            from: (s, across),
            to: (e, across),
        }
    } else {
        Guide {
            from: (across, s),
            to: (across, e),
        }
    }
}

/// The boxes to align with: the layout's placeholders and the slide's own other shapes,
/// each as its (x, y) spans.
fn targets(deck: &Presentation, slide: &Slide, moving: usize) -> Vec<(Span, Span)> {
    let layout = slide
        .layout
        .and_then(|l| deck.layouts.get(l))
        .into_iter()
        .flat_map(|l| l.slide.shapes.iter())
        .filter(|s| s.layer == Layer::Slide && s.placeholder.is_some());
    let others = slide
        .shapes
        .iter()
        .enumerate()
        .filter(|(i, s)| *i != moving && s.layer == Layer::Slide)
        .map(|(_, s)| s);
    layout
        .chain(others)
        .map(|s| {
            let f = &s.frame;
            (
                Span {
                    start: f.x,
                    size: f.cx,
                },
                Span {
                    start: f.y,
                    size: f.cy,
                },
            )
        })
        .collect()
}

/// The other-shape boxes on one axis, with their spans on the other.
fn boxes_axis(boxes: &[(Span, Span)], x: bool) -> Vec<(Span, Span)> {
    boxes
        .iter()
        .map(|&(a, b)| if x { (a, b) } else { (b, a) })
        .collect()
}

/// The lines on one axis: the slide's edges and centre (`len` long, running `across`), and
/// each box's edges and centre.
fn lines(len: i64, across: i64, boxes: impl Iterator<Item = (Span, Span)>) -> Vec<Line> {
    let mut out: Vec<Line> = [0, len / 2, len]
        .into_iter()
        .map(|at| Line {
            at,
            from: 0,
            to: across,
        })
        .collect();
    for (along, other) in boxes {
        for at in [along.start, along.centre(), along.end()] {
            out.push(Line {
                at,
                from: other.start,
                to: other.end(),
            });
        }
    }
    out
}

/// The nearest snap on one axis within `tolerance`: a moving edge or centre onto a line,
/// or (moving the whole box) its start onto a spacing two boxes in a row with it set.
#[allow(clippy::too_many_arguments)]
fn pick(
    span: Span,
    across: Span,
    lines: &[Line],
    boxes: &[(Span, Span)],
    starts: bool,
    ends: bool,
    whole: bool,
    tolerance: i64,
) -> Option<Pick> {
    let mut best: Option<Pick> = None;
    let mut consider = |delta: i64, spacing: Option<(Span, Span)>| {
        if delta.abs() <= tolerance && best.as_ref().is_none_or(|b| delta.abs() < b.delta.abs()) {
            best = Some(Pick { delta, spacing });
        }
    };
    let mut features = Vec::new();
    if starts {
        features.push(span.start);
    }
    if starts && ends {
        features.push(span.centre());
    }
    if ends {
        features.push(span.end());
    }
    for l in lines {
        for &f in &features {
            consider(l.at - f, None);
        }
    }
    if whole {
        // Boxes in a row with the moving one: overlapping it on the other axis.
        let mut row: Vec<Span> = boxes
            .iter()
            .filter(|(_, o)| o.start < across.end() && across.start < o.end())
            .map(|(a, _)| *a)
            .collect();
        row.sort_by_key(|s| s.start);
        for (i, a) in row.iter().enumerate() {
            for b in &row[i + 1..] {
                let gap = b.start - a.end();
                if gap < 0 {
                    continue;
                }
                // After the second, before the first, or centred between them.
                consider(b.end() + gap - span.start, Some((*a, *b)));
                consider(a.start - gap - span.size - span.start, Some((*a, *b)));
                if gap > span.size {
                    let between = a.end() + (gap - span.size) / 2;
                    consider(between - span.start, Some((*a, *b)));
                }
            }
        }
    }
    best
}
