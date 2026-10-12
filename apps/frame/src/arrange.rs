//! Moving and resizing shapes on the canvas (FRAME-10). Dragging a shape moves it;
//! dragging one of the selected shape's handles resizes it; a shape whose text is being
//! edited moves by its border. While a shape is dragged it snaps, within 6 screen pixels,
//! to the slide's edges and centre, the layout's placeholders, the other shapes' edges and
//! centres, and equal spacing ([`deck_edit::snap`]), with guides drawn; Alt drags freely.
//! The arrows nudge the selected shape a pixel, Shift+arrows ten. Each drag and nudge is a
//! step in the deck's undo history.

use crate::slide_view::Preview;
use crate::window::DeckWindow;
use deck_edit::{FrameEdit, Grab, ShapeRef};
use gtk::gdk;
use gtk::prelude::*;
use presentation_model::Frame;
use std::cell::RefCell;
use std::rc::Rc;

/// How near, in screen pixels, a box snaps to a line, and a handle or border is hit.
const SNAP_PIXELS: f64 = 6.0;
const GRAB_PIXELS: f32 = 5.0;
/// How far, in pixels, the pointer goes before a press becomes a drag.
const DRAG_PIXELS: f64 = 3.0;
/// The smallest box a resize leaves, in screen pixels.
const MIN_PIXELS: f64 = 4.0;

/// A drag under way: the shape, what of it is held, and its box when the drag started.
struct Drag {
    shape: usize,
    grab: Grab,
    start: Frame,
    /// The pointer has gone far enough to drag.
    moving: bool,
}

/// Make `view`'s canvas move and resize shapes by dragging.
pub fn attach(view: &Rc<DeckWindow>) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    let state: Rc<RefCell<Option<Drag>>> = Rc::default();
    drag.connect_drag_begin({
        let view = Rc::downgrade(view);
        let state = state.clone();
        move |_, x, y| {
            if let Some(view) = view.upgrade() {
                *state.borrow_mut() = grab_at(&view, x, y);
            }
        }
    });
    drag.connect_drag_update({
        let view = Rc::downgrade(view);
        let state = state.clone();
        move |gesture, dx, dy| {
            let mut state = state.borrow_mut();
            let (Some(view), Some(d)) = (view.upgrade(), state.as_mut()) else {
                return;
            };
            if !d.moving {
                if dx.hypot(dy) < DRAG_PIXELS {
                    return;
                }
                // A drag, not a click: the click doesn't place a caret.
                d.moving = true;
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
            let free = gesture
                .current_event_state()
                .contains(gdk::ModifierType::ALT_MASK);
            dragged(&view, d, dx, dy, free);
        }
    });
    drag.connect_drag_end({
        let view = Rc::downgrade(view);
        let state = state.clone();
        move |_, _, _| {
            let (Some(view), Some(d)) = (view.upgrade(), state.borrow_mut().take()) else {
                return;
            };
            let landed = view.canvas.take_preview();
            if let Some(p) = landed.filter(|_| d.moving) {
                let s = ShapeRef {
                    slide: view.canvas.index(),
                    shape: d.shape,
                };
                let edit = FrameEdit::new(&view.deck.presentation(), s, p.frame);
                if let Some(edit) = edit {
                    view.seal();
                    view.apply(edit);
                }
            }
        }
    });
    drag.connect_cancel({
        let view = Rc::downgrade(view);
        move |_, _| {
            if let Some(view) = view.upgrade() {
                state.borrow_mut().take();
                view.canvas.set_preview(None);
            }
        }
    });
    view.canvas.add_controller(drag);
}

/// What a press at the canvas point (`x`, `y`) would drag: a handle of the selected shape
/// (unless it's turned) resizes it; the border of the shape whose text is edited moves
/// it (inside, the press is the text's); anywhere on another shape that can move moves
/// that.
fn grab_at(view: &DeckWindow, x: f64, y: f64) -> Option<Drag> {
    let canvas = &view.canvas;
    let deck = view.deck.presentation();
    let slide = deck.slides.get(canvas.index())?;
    let drag = |shape: usize, grab: Grab| {
        let s = slide.shapes.get(shape)?;
        deck_edit::movable(s).then_some(Drag {
            shape,
            grab,
            start: s.frame,
            moving: false,
        })
    };
    if let Some(sel) = canvas.selected() {
        let unturned = slide
            .shapes
            .get(sel)
            .is_some_and(|s| s.frame.rotation == 0.0);
        let held = canvas
            .local_point(sel, x, y)
            .and_then(|p| p.handle(GRAB_PIXELS));
        if let (true, Some((left, right, top, bottom))) = (unturned, held) {
            return drag(
                sel,
                Grab::Resize {
                    left,
                    right,
                    top,
                    bottom,
                },
            );
        }
    }
    if let Some(e) = canvas.editing() {
        if let Some(p) = canvas.local_point(e.shape, x, y) {
            if p.within(GRAB_PIXELS) {
                return if p.within(-GRAB_PIXELS) {
                    None
                } else {
                    drag(e.shape, Grab::Move)
                };
            }
        }
    }
    drag(canvas.shape_at(x, y, GRAB_PIXELS)?, Grab::Move)
}

/// The drag `d` with the pointer moved (`dx`, `dy`) pixels: where the shape would land,
/// snapped unless `free`.
fn dragged(view: &DeckWindow, d: &Drag, dx: f64, dy: f64, free: bool) {
    let Some(epp) = view.canvas.emu_per_pixel() else {
        return;
    };
    let (dx, dy) = ((dx * epp).round() as i64, (dy * epp).round() as i64);
    let min = (MIN_PIXELS * epp).round() as i64;
    let s = d.start;
    let mut f = s;
    match d.grab {
        Grab::Move => {
            f.x += dx;
            f.y += dy;
        }
        Grab::Resize {
            left,
            right,
            top,
            bottom,
        } => {
            let (x0, x1) = resize(s.x, s.x + s.cx, dx, left, right, min);
            let (y0, y1) = resize(s.y, s.y + s.cy, dy, top, bottom, min);
            (f.x, f.cx, f.y, f.cy) = (x0, x1 - x0, y0, y1 - y0);
        }
    }
    let preview = if free {
        Preview {
            shape: d.shape,
            frame: f,
            guides: Vec::new(),
        }
    } else {
        let tolerance = (SNAP_PIXELS * epp).round() as i64;
        let deck = view.deck.presentation();
        let snapped = deck_edit::snap(&deck, view.canvas.index(), d.shape, f, d.grab, tolerance);
        Preview {
            shape: d.shape,
            frame: snapped.frame,
            guides: snapped.guides,
        }
    };
    view.canvas.set_preview(Some(preview));
}

/// The span `start`..`end` with its held ends moved by `by`, at least `min` long.
fn resize(start: i64, end: i64, by: i64, first: bool, last: bool, min: i64) -> (i64, i64) {
    match (first, last) {
        (true, _) => ((start + by).min(end - min), end),
        (_, true) => (start, (end + by).max(start + min)),
        _ => (start, end),
    }
}

/// Move the selected shape (`dx`, `dy`) screen pixels.
pub fn nudge(view: &DeckWindow, dx: f64, dy: f64) {
    let (Some(shape), Some(epp)) = (view.canvas.selected(), view.canvas.emu_per_pixel()) else {
        return;
    };
    let s = ShapeRef {
        slide: view.canvas.index(),
        shape,
    };
    let edit = {
        let deck = view.deck.presentation();
        let Some(f) = deck.slides.get(s.slide).and_then(|sl| sl.shapes.get(shape)) else {
            return;
        };
        let to = Frame {
            x: f.frame.x + (dx * epp).round() as i64,
            y: f.frame.y + (dy * epp).round() as i64,
            ..f.frame
        };
        FrameEdit::new(&deck, s, to)
    };
    if let Some(edit) = edit {
        view.seal();
        view.apply(edit);
    }
}
