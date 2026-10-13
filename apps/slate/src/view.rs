//! The document's page (ADR 0005, ADR 0007): Slate's own widget, drawing the pageless flow
//! ([`flow::Flow`]) through `snapshot` with one `pango::Layout` per leaf.
//!
//! The column is as wide as the user's text width setting (narrower when the window is) and
//! centred. The flow lays out what the viewport shows synchronously in `size_allocate`, so a
//! width change shows re-wrapped text in its next frame; idle slices of at most 4 ms, below
//! redraw priority, measure the rest. The scroll position is anchored to a block, so replacing
//! estimated heights never moves what the viewport shows.
//!
//! Text is edited in place (SLATE-6): typing comes through an input method context, and
//! every key that changes the document makes a `document_edit` command, kept in the page's
//! history for undo and redo. The caret moves by character, word, line, and block. An edit
//! rebuilds only the blocks it changed ([`Flow::edit`]). The page's `Command`s are the
//! formatting, list, and undo commands its keys and the window's menu share.
//!
//! The text selects by dragging, and Ctrl+A and Ctrl+C copy it as plain text. Ctrl+click
//! opens a link. Screen readers read the text through `GtkAccessibleText`; page breaks are
//! child widgets named "Page break" that draw their own dashed line.

mod a11y;
mod caret;
mod shaper;

use crate::settings::TextWidth;
use document_edit::{Change, Container, Edit, Flag, Fragment, History, Pos};
use document_model::RunStyle;
use flow::{Flow, Located, Paint, Splice, LEVEL_INDENT};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use shaper::PangoShaper;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The smallest margin beside the column, when the window is narrower than the text width
/// (and on each side of a Full column).
const MIN_MARGIN: i32 = 24;
/// The narrowest the column gets, however narrow the window.
const MIN_COLUMN: i32 = 120;
/// The longest an idle layout slice runs.
const SLICE: Duration = Duration::from_millis(4);
/// How far the scroll wheel's and scrollbar's step moves.
const LINE_STEP: f64 = 40.0;
/// Room kept above or below the caret when the page scrolls to show it.
const REVEAL_MARGIN: f64 = 24.0;
/// How far past the page's edge a drag scrolls per frame, as a fraction of the overshoot.
const AUTOSCROLL: f64 = 0.4;
/// The document is paper: dark text on white whatever the theme, as it was written.
const PAPER: u32 = 0xFFFFFF;
const INK: u32 = 0x1E1E1E;
const CODE_BAND: u32 = 0xF4F4F4;
/// Table borders and rules.
const LINE: u32 = 0xC0BFBC;
/// The selection: GNOME's blue, translucent over the paper.
const SELECTION: (u32, f32) = (0x3584E4, 0.3);

/// Decoded pictures, by the address of their bytes in the document.
pub type Textures = HashMap<usize, gdk::Texture>;

/// The key of an embedded picture's bytes in [`Textures`].
pub fn texture_key(bytes: &Arc<[u8]>) -> usize {
    Arc::as_ptr(bytes) as *const u8 as usize
}

/// The text column's width for `setting` in a view `view_width` pixels wide: the setting's
/// width, or the view's less a margin each side when that is narrower (Full always), but
/// never under [`MIN_COLUMN`].
pub fn column_width(setting: TextWidth, view_width: i32) -> i32 {
    let room = view_width - 2 * MIN_MARGIN;
    setting
        .pixels()
        .map_or(room, |px| px.min(room))
        .max(MIN_COLUMN)
}

/// The column's left edge in a view `view_width` wide.
fn column_left(view_width: i32, column: i32) -> i32 {
    ((view_width - column) / 2).max(MIN_MARGIN)
}

fn rgba(hex: u32) -> gdk::RGBA {
    rgba_alpha(hex, 1.0)
}

fn rgba_alpha(hex: u32, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
        alpha,
    )
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> graphene::Rect {
    graphene::Rect::new(x, y, w, h)
}

/// What the page's menu and keys do besides typing and moving the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Undo,
    Redo,
    Bold,
    Italic,
    /// Normal text (0) or a heading level (1 to 6).
    Style(u8),
    Bullets,
    Numbers,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// The document, its layouts, and its editing state.
struct State {
    flow: Flow,
    shaper: PangoShaper,
    history: History,
    /// `None` when the document has no editable block.
    caret: Option<Pos>,
    /// Where Up and Down aim, in pixels from the column's left edge.
    goal: Option<f64>,
    /// The look the next typed text takes when Ctrl+B or Ctrl+I changed it at the caret.
    typing: Option<RunStyle>,
    /// Plain text: no formatting, lists, or headings.
    plain: bool,
    /// Counts the edits shown, for the typing benchmark.
    revision: u64,
    /// The input method's preedit at the caret: its text, looks, and cursor (bytes into it).
    preedit: Option<(String, pango::AttrList, usize)>,
    /// Where the selection was started, as made (the flow holds it with its ends that run
    /// into a table moved to the table's edges).
    anchor: Option<flow::Position>,
}

impl State {
    /// Where the caret shows: inside the preedit while there is one.
    fn shown_caret(&self) -> Option<Pos> {
        let c = self.caret?;
        Some(match &self.preedit {
            Some((_, _, cursor)) => Pos {
                offset: c.offset + cursor,
                ..c
            },
            None => c,
        })
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Page {
        pub(super) state: RefCell<Option<State>>,
        pub(super) setting: Cell<TextWidth>,
        hadj: RefCell<Option<gtk::Adjustment>>,
        vadj: RefCell<Option<gtk::Adjustment>>,
        /// The scroll policies as set; `None` is `Minimum`.
        hpolicy: Cell<Option<gtk::ScrollablePolicy>>,
        vpolicy: Cell<Option<gtk::ScrollablePolicy>>,
        /// Set while the page configures its own adjustments.
        guard: Cell<bool>,
        idle: RefCell<Option<glib::SourceId>>,
        /// One per page break, in document order.
        pub(super) breaks: RefCell<Vec<gtk::Separator>>,
        /// The pointer while a drag selects, in widget coordinates.
        dragging: Cell<Option<(f64, f64)>>,
        autoscroll: RefCell<Option<gtk::TickCallbackId>>,
        /// When every block was last measured at the column width; `None` while not.
        pub(super) completed: Cell<Option<Instant>>,
        im: OnceCell<gtk::IMMulticontext>,
        /// Scroll the caret into view at the next allocation.
        pub(super) reveal: Cell<bool>,
        /// The last press: when, where, and how many presses in a row it made.
        presses: Cell<Option<(Instant, f64, f64, u32)>>,
        /// A removal being announced to screen readers: where it was and what it removed.
        removal: RefCell<Option<(usize, String)>>,
        /// Called after every edit, undo, and redo has been shown.
        pub(super) edited: RefCell<Option<Box<dyn Fn()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Page {
        const NAME: &'static str = "SlatePage";
        type Type = super::Page;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Scrollable, gtk::AccessibleText);

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::TextBox);
            klass.set_css_name("slatepage");
        }
    }

    impl ObjectImpl for Page {
        fn properties() -> &'static [glib::ParamSpec] {
            static PROPERTIES: std::sync::LazyLock<Vec<glib::ParamSpec>> =
                std::sync::LazyLock::new(|| {
                    [
                        "hadjustment",
                        "vadjustment",
                        "hscroll-policy",
                        "vscroll-policy",
                    ]
                    .into_iter()
                    .map(glib::ParamSpecOverride::for_interface::<gtk::Scrollable>)
                    .collect()
                });
            &PROPERTIES
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "hadjustment" => {
                    self.hadj.replace(value.get().expect("an adjustment"));
                }
                "vadjustment" => {
                    let adj: Option<gtk::Adjustment> = value.get().expect("an adjustment");
                    if let Some(adj) = &adj {
                        let page = self.obj().downgrade();
                        adj.connect_value_changed(move |adj| {
                            if let Some(page) = page.upgrade() {
                                page.imp().scrolled(adj.value());
                            }
                        });
                    }
                    self.vadj.replace(adj);
                }
                "hscroll-policy" => self.hpolicy.set(value.get().ok()),
                "vscroll-policy" => self.vpolicy.set(value.get().ok()),
                name => unreachable!("no property {name}"),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "hadjustment" => self.hadj.borrow().to_value(),
                "vadjustment" => self.vadj.borrow().to_value(),
                "hscroll-policy" => self
                    .hpolicy
                    .get()
                    .unwrap_or(gtk::ScrollablePolicy::Minimum)
                    .to_value(),
                "vscroll-policy" => self
                    .vpolicy
                    .get()
                    .unwrap_or(gtk::ScrollablePolicy::Minimum)
                    .to_value(),
                name => unreachable!("no property {name}"),
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.set_cursor_from_name(Some("text"));
            obj.update_property(&[
                gtk::accessible::Property::MultiLine(true),
                gtk::accessible::Property::ReadOnly(false),
            ]);
            let im = gtk::IMMulticontext::new();
            im.set_client_widget(Some(&*obj));
            im.connect_commit({
                let page = obj.downgrade();
                move |_, text| {
                    if let Some(page) = page.upgrade() {
                        page.imp().set_preedit(None);
                        page.imp().type_text(text);
                    }
                }
            });
            im.connect_preedit_changed({
                let page = obj.downgrade();
                move |im| {
                    if let Some(page) = page.upgrade() {
                        let (text, attrs, cursor) = im.preedit_string();
                        let cursor = text
                            .char_indices()
                            .nth(cursor.max(0) as usize)
                            .map_or(text.len(), |(i, _)| i);
                        page.imp().set_preedit(
                            (!text.is_empty()).then(|| (text.to_string(), attrs, cursor)),
                        );
                    }
                }
            });
            im.connect_retrieve_surrounding({
                let page = obj.downgrade();
                move |im| {
                    let Some((text, cursor, anchor)) =
                        page.upgrade().and_then(|p| p.imp().surrounding())
                    else {
                        return false;
                    };
                    im.set_surrounding_with_selection(&text, cursor as i32, anchor as i32);
                    true
                }
            });
            im.connect_delete_surrounding({
                let page = obj.downgrade();
                move |_, offset, chars| {
                    page.upgrade()
                        .is_some_and(|p| p.imp().delete_surrounding(offset, chars))
                }
            });
            let _ = self.im.set(im);
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(|focus| {
                if let Some(page) = focus.widget().and_downcast::<super::Page>() {
                    if let Some(im) = page.imp().im.get() {
                        im.focus_in();
                    }
                    page.queue_draw();
                }
            });
            focus.connect_leave(|focus| {
                if let Some(page) = focus.widget().and_downcast::<super::Page>() {
                    if let Some(im) = page.imp().im.get() {
                        im.focus_out();
                    }
                    page.queue_draw();
                }
            });
            obj.add_controller(focus);
            self.select_by_dragging();
            self.point_at_links();
            self.keys();
        }

        fn dispose(&self) {
            if let Some(id) = self.idle.take() {
                id.remove();
            }
            if let Some(id) = self.autoscroll.take() {
                id.remove();
            }
            for line in self.breaks.take() {
                line.unparent();
            }
            if let Some(im) = self.im.get() {
                im.set_client_widget(None::<&gtk::Widget>);
            }
        }
    }

    impl WidgetImpl for Page {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => {
                    let min = MIN_COLUMN + 2 * MIN_MARGIN;
                    let natural = self
                        .setting
                        .get()
                        .pixels()
                        .map_or(min, |px| px + 2 * MIN_MARGIN);
                    (min, natural.max(min), -1, -1)
                }
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let column = column_width(self.setting.get(), width);
            let mut lines = Vec::new();
            let mut relayout = false;
            let mut caret_box = None;
            {
                let mut state = self.state.borrow_mut();
                let Some(st) = state.as_mut() else {
                    return;
                };
                if st.flow.width() != column {
                    st.flow.set_width(column, &mut st.shaper);
                    self.completed.set(None);
                    relayout = true;
                }
                let page = f64::from(height);
                st.flow.layout_viewport(&mut st.shaper, page);
                if self.reveal.take() {
                    if let Some((_, y, h)) = st.shown_caret().and_then(|c| caret::rect(st, c)) {
                        let top = st.flow.scroll();
                        if y < top {
                            st.flow.scroll_to(y - REVEAL_MARGIN);
                        } else if y + h > top + page {
                            st.flow.scroll_to(y + h - page + REVEAL_MARGIN);
                        }
                        st.flow.layout_viewport(&mut st.shaper, page);
                    }
                }
                if st.flow.is_complete() && self.completed.get().is_none() {
                    self.completed.set(Some(Instant::now()));
                }
                let (left, scroll) = (column_left(width, column), st.flow.scroll());
                if let Some((x, y, h)) = st.shown_caret().and_then(|c| caret::rect(st, c)) {
                    caret_box = Some(gdk::Rectangle::new(
                        left + x as i32,
                        (y - scroll) as i32,
                        1,
                        h.ceil() as i32,
                    ));
                }
                st.flow.page_breaks(&mut |p| {
                    if let Paint::PageBreak {
                        index,
                        x,
                        y,
                        width,
                        height,
                    } = p
                    {
                        lines.push((index, left + x, (y - scroll) as f32, width, height));
                    }
                });
            }
            if let (Some(caret), Some(im)) = (caret_box, self.im.get()) {
                im.set_cursor_location(&caret);
            }
            if relayout {
                self.start_layout();
            }
            self.sync_adjustments(width, height);
            for (index, x, y, w, h) in lines {
                if let Some(line) = self.breaks.borrow().get(index) {
                    line.measure(gtk::Orientation::Vertical, w);
                    let at = gsk::Transform::new().translate(&graphene::Point::new(x as f32, y));
                    line.allocate(w, h, -1, Some(at));
                }
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (w, h) = (obj.width(), obj.height());
            snapshot.append_color(&rgba(PAPER), &rect(0.0, 0.0, w as f32, h as f32));
            if let Some(st) = self.state.borrow_mut().as_mut() {
                let left = column_left(w, st.flow.width()) as f32;
                let top = st.flow.scroll();
                let State { flow, shaper, .. } = st;
                flow.paint(top, top + f64::from(h), &mut |p| {
                    draw(snapshot, shaper, p, left, top);
                });
                if obj.has_focus() {
                    if let Some((x, y, ch)) = st.shown_caret().and_then(|c| caret::rect(st, c)) {
                        snapshot.append_color(
                            &rgba(INK),
                            &rect(left + x as f32, (y - top) as f32, 1.0, ch as f32),
                        );
                    }
                }
            }
            for line in self.breaks.borrow().iter() {
                obj.snapshot_child(line, snapshot);
            }
        }
    }

    impl ScrollableImpl for Page {}

    impl AccessibleTextImpl for Page {
        fn contents(&self, start: u32, end: u32) -> Option<glib::Bytes> {
            // While a removal is announced, its range reads as the text it removed.
            if let Some((at, text)) = self.removal.borrow().as_ref() {
                if *at == start as usize && *at + text.chars().count() == end as usize {
                    return Some(glib::Bytes::from(text.as_bytes()));
                }
            }
            let state = self.state.borrow();
            let text = state.as_ref()?.flow.text(start as usize, end as usize);
            Some(glib::Bytes::from_owned(text.into_bytes()))
        }

        fn contents_at(
            &self,
            offset: u32,
            granularity: gtk::AccessibleTextGranularity,
        ) -> Option<(u32, u32, glib::Bytes)> {
            let mut state = self.state.borrow_mut();
            let State { flow, shaper, .. } = state.as_mut()?;
            let (start, end) = a11y::boundary(flow, shaper, offset as usize, granularity)?;
            let text = flow.text(start, end);
            Some((
                start as u32,
                end as u32,
                glib::Bytes::from_owned(text.into_bytes()),
            ))
        }

        fn caret_position(&self) -> u32 {
            let state = self.state.borrow();
            let Some(st) = state.as_ref() else {
                return 0;
            };
            st.caret
                .and_then(|c| caret::to_flow(st, c))
                .map_or(0, |p| st.flow.char_offset(p) as u32)
        }

        fn selection(&self) -> Vec<gtk::AccessibleTextRange> {
            let state = self.state.borrow();
            let Some((flow, (start, end))) = state
                .as_ref()
                .and_then(|s| Some((&s.flow, s.flow.selection()?)))
            else {
                return Vec::new();
            };
            let (s, e) = (flow.char_offset(start), flow.char_offset(end));
            vec![gtk::AccessibleTextRange::new(s, e - s)]
        }

        fn default_attributes(&self) -> Vec<(glib::GString, glib::GString)> {
            let state = self.state.borrow();
            let Some(st) = state.as_ref() else {
                return Vec::new();
            };
            a11y::default_attributes(&st.shaper)
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect()
        }

        fn attributes(
            &self,
            offset: u32,
        ) -> Vec<(gtk::AccessibleTextRange, glib::GString, glib::GString)> {
            let mut state = self.state.borrow_mut();
            let Some(State { flow, shaper, .. }) = state.as_mut() else {
                return Vec::new();
            };
            let Some((start, end, looks)) = a11y::attributes(flow, shaper, offset as usize) else {
                return Vec::new();
            };
            let range = gtk::AccessibleTextRange::new(start, end - start);
            looks
                .into_iter()
                .map(|(name, value)| (range, name.into(), value.into()))
                .collect()
        }
    }

    impl Page {
        /// The vertical adjustment moved (scrollbar, wheel, keys): show from `value` on.
        fn scrolled(&self, value: f64) {
            if self.guard.get() {
                return;
            }
            if let Some(state) = self.state.borrow_mut().as_mut() {
                state.flow.scroll_to(value);
            }
            let obj = self.obj();
            obj.queue_allocate();
            obj.queue_draw();
        }

        /// Tell the adjustments where the flow is and how tall the document is now.
        fn sync_adjustments(&self, width: i32, height: i32) {
            let (value, upper) = match self.state.borrow().as_ref() {
                Some(s) => (s.flow.scroll(), s.flow.content_height()),
                None => (0.0, 0.0),
            };
            let page = f64::from(height);
            self.guard.set(true);
            if let Some(adj) = self.vadj.borrow().as_ref() {
                adj.configure(value, 0.0, upper.max(page), LINE_STEP, page * 0.9, page);
            }
            if let Some(adj) = self.hadj.borrow().as_ref() {
                let w = f64::from(width);
                adj.configure(0.0, 0.0, w, 1.0, w, w);
            }
            self.guard.set(false);
        }

        /// Measure the blocks not yet measured at the width in idle slices.
        fn start_layout(&self) {
            if self.idle.borrow().is_some() {
                return;
            }
            let page = self.obj().downgrade();
            let id = glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
                let Some(page) = page.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let flow = page.imp().layout_slice();
                if flow.is_break() {
                    page.imp().idle.take();
                }
                flow
            });
            self.idle.replace(Some(id));
        }

        fn layout_slice(&self) -> glib::ControlFlow {
            let started = Instant::now();
            let progress = {
                let mut state = self.state.borrow_mut();
                let Some(State { flow, shaper, .. }) = state.as_mut() else {
                    return glib::ControlFlow::Break;
                };
                flow.layout_slice(shaper, &mut || started.elapsed() < SLICE)
            };
            let obj = self.obj();
            if progress.measured > 0 {
                self.sync_adjustments(obj.width(), obj.height());
                if !self.breaks.borrow().is_empty() {
                    obj.queue_allocate();
                }
            }
            if progress.complete {
                if self.completed.get().is_none() {
                    self.completed.set(Some(Instant::now()));
                }
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }

        /// What the widget point (`x`, `y`) hits.
        fn locate(&self, x: f64, y: f64) -> Option<Located> {
            let width = self.obj().width();
            let mut state = self.state.borrow_mut();
            let State { flow, shaper, .. } = state.as_mut()?;
            let left = f64::from(column_left(width, flow.width()));
            flow.hit(shaper, x - left, flow.scroll() + y)
        }

        fn link_at(&self, x: f64, y: f64) -> Option<String> {
            let over = self.locate(x, y)?.over?;
            let state = self.state.borrow();
            state.as_ref()?.flow.link_at(over).map(str::to_owned)
        }

        fn selection_changed(&self) {
            let obj = self.obj();
            obj.queue_draw();
            obj.update_selection_bound();
            obj.update_caret_position();
        }

        /// Extend the selection being dragged to the widget point (`x`, `y`); the caret
        /// follows it.
        fn drag_to(&self, x: f64, y: f64) {
            self.dragging.set(Some((x, y)));
            let Some(focus) = self.locate(x, y) else {
                return;
            };
            if let Some(st) = self.state.borrow_mut().as_mut() {
                let anchor = st.anchor.unwrap_or(focus.position);
                caret::select(st, anchor, focus.position);
                if let Some(c) = caret::to_model(st, focus.position, true) {
                    st.caret = Some(c);
                }
            }
            self.selection_changed();
        }

        fn select_by_dragging(&self) {
            let obj = self.obj();
            let drag = gtk::GestureDrag::new();
            drag.set_button(gdk::BUTTON_PRIMARY);
            drag.connect_drag_begin(|gesture, x, y| {
                let Some(page) = gesture.widget().and_downcast::<super::Page>() else {
                    return;
                };
                page.grab_focus();
                let imp = page.imp();
                imp.reset_im();
                let shift = gesture
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK);
                let presses = imp.press_count(x, y);
                if let Some(at) = imp.locate(x, y) {
                    if let Some(st) = imp.state.borrow_mut().as_mut() {
                        let p = at.position;
                        let (anchor, focus) = match presses {
                            2 => caret::word_at(st, p),
                            3 => caret::paragraph_at(st, p),
                            _ if shift => (caret::anchor(st).unwrap_or(p), p),
                            _ => (p, p),
                        };
                        caret::select(st, anchor, focus);
                        if let Some(c) = caret::to_model(st, focus, true) {
                            st.caret = Some(c);
                        }
                        st.goal = None;
                        st.typing = None;
                        st.history.seal();
                    }
                }
                imp.dragging.set(Some((x, y)));
                imp.selection_changed();
            });
            drag.connect_drag_update(|gesture, dx, dy| {
                let (Some(page), Some((x, y))) = (
                    gesture.widget().and_downcast::<super::Page>(),
                    gesture.start_point(),
                ) else {
                    return;
                };
                let imp = page.imp();
                imp.drag_to(x + dx, y + dy);
                if (y + dy < 0.0 || y + dy > f64::from(page.height()))
                    && imp.autoscroll.borrow().is_none()
                {
                    imp.start_autoscroll();
                }
            });
            drag.connect_drag_end(|gesture, dx, dy| {
                let Some(page) = gesture.widget().and_downcast::<super::Page>() else {
                    return;
                };
                let imp = page.imp();
                imp.dragging.set(None);
                let ctrl = gesture
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK);
                if ctrl && dx.abs() < 3.0 && dy.abs() < 3.0 {
                    if let Some((x, y)) = gesture.start_point() {
                        if let Some(url) = imp.link_at(x, y) {
                            open_link(&page, url);
                        }
                    }
                }
            });
            obj.add_controller(drag);
        }

        /// While a drag is above or below the page, scroll towards it and extend the
        /// selection.
        fn start_autoscroll(&self) {
            let id = self.obj().add_tick_callback(|page, _| {
                let imp = page.imp();
                let (Some((x, y)), Some(adj)) = (imp.dragging.get(), imp.vadj.borrow().clone())
                else {
                    imp.autoscroll.take();
                    return glib::ControlFlow::Break;
                };
                let h = f64::from(page.height());
                let over = if y < 0.0 { y } else { (y - h).max(0.0) };
                if over == 0.0 {
                    imp.autoscroll.take();
                    return glib::ControlFlow::Break;
                }
                adj.set_value(adj.value() + over * AUTOSCROLL);
                imp.drag_to(x, y);
                glib::ControlFlow::Continue
            });
            self.autoscroll.replace(Some(id));
        }

        /// The pointer is a hand over a link (Ctrl+click opens it), a text cursor elsewhere.
        fn point_at_links(&self) {
            let motion = gtk::EventControllerMotion::new();
            motion.connect_motion(|motion, x, y| {
                let Some(page) = motion.widget().and_downcast::<super::Page>() else {
                    return;
                };
                if page.imp().dragging.get().is_some() {
                    return;
                }
                let cursor = if page.imp().link_at(x, y).is_some() {
                    "pointer"
                } else {
                    "text"
                };
                page.set_cursor_from_name(Some(cursor));
            });
            self.obj().add_controller(motion);
        }

        fn keys(&self) {
            let keys = gtk::EventControllerKey::new();
            if let Some(im) = self.im.get() {
                keys.set_im_context(Some(im));
            }
            keys.connect_key_pressed(|keys, key, _, mods| {
                let Some(page) = keys.widget().and_downcast::<super::Page>() else {
                    return glib::Propagation::Proceed;
                };
                page.imp().key(key, mods)
            });
            self.obj().add_controller(keys);
        }

        fn key(&self, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
            use gdk::Key as K;
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            if ctrl {
                let digit = key.to_unicode().and_then(|c| c.to_digit(10));
                if matches!(key, K::s | K::S) {
                    // The window's File → Save and Save As (`saving`).
                    let action = if shift { "win.save-as" } else { "win.save" };
                    let _ = self.obj().activate_action(action, None);
                    return glib::Propagation::Stop;
                }
                let command = match key {
                    K::z | K::Z => Some(if shift { Command::Redo } else { Command::Undo }),
                    K::y | K::Y => Some(Command::Redo),
                    K::b | K::B => Some(Command::Bold),
                    K::i | K::I => Some(Command::Italic),
                    K::asterisk | K::_8 if shift => Some(Command::Bullets),
                    K::ampersand | K::_7 if shift => Some(Command::Numbers),
                    K::a | K::A => Some(Command::SelectAll),
                    K::c | K::C | K::Insert => Some(Command::Copy),
                    K::x | K::X => Some(Command::Cut),
                    K::v | K::V => Some(Command::Paste),
                    _ => match digit {
                        Some(n @ 0..=6) if !shift => Some(Command::Style(n as u8)),
                        _ => None,
                    },
                };
                if let Some(command) = command {
                    self.command(command);
                    return glib::Propagation::Stop;
                }
            }
            match key {
                K::Insert | K::KP_Insert if shift => self.command(Command::Paste),
                K::Delete | K::KP_Delete if shift => self.command(Command::Cut),
                K::BackSpace => self.backspace(),
                K::Delete | K::KP_Delete => self.delete(),
                K::Return | K::KP_Enter | K::ISO_Enter if shift => self.type_text("\n"),
                K::Return | K::KP_Enter | K::ISO_Enter => self.enter(),
                K::Tab | K::KP_Tab | K::ISO_Left_Tab => self.tab(shift || key == K::ISO_Left_Tab),
                K::Left | K::KP_Left => {
                    self.move_caret(false, shift, |st, c| match caret::selection(st) {
                        Some((s, _)) if !ctrl && !shift => s,
                        _ if ctrl => caret::word_left(st, c),
                        _ => caret::left(st, c),
                    })
                }
                K::Right | K::KP_Right => {
                    self.move_caret(false, shift, |st, c| match caret::selection(st) {
                        Some((_, e)) if !ctrl && !shift => e,
                        _ if ctrl => caret::word_right(st, c),
                        _ => caret::right(st, c),
                    })
                }
                K::Up | K::KP_Up | K::Down | K::KP_Down => {
                    let down = matches!(key, K::Down | K::KP_Down);
                    self.move_caret(true, shift, |st, c| {
                        let (to, goal) = caret::vertical(st, c, st.goal, down);
                        st.goal = goal;
                        to
                    });
                }
                K::Home | K::KP_Home if ctrl => {
                    self.move_caret(false, shift, |st, c| caret::first(st).unwrap_or(c))
                }
                K::End | K::KP_End if ctrl => {
                    self.move_caret(false, shift, |st, c| caret::last(st).unwrap_or(c))
                }
                K::Home | K::KP_Home => {
                    self.move_caret(false, shift, |st, c| caret::line_edge(st, c, false))
                }
                K::End | K::KP_End => {
                    self.move_caret(false, shift, |st, c| caret::line_edge(st, c, true))
                }
                K::Page_Up | K::KP_Page_Up | K::Page_Down | K::KP_Page_Down => {
                    let Some(adj) = self.vadj.borrow().clone() else {
                        return glib::Propagation::Proceed;
                    };
                    let page = adj.page_size();
                    let by = if matches!(key, K::Page_Up | K::KP_Page_Up) {
                        -0.9
                    } else {
                        0.9
                    };
                    let value = adj.value() + page * by;
                    adj.set_value(value.clamp(adj.lower(), (adj.upper() - page).max(adj.lower())));
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }

        // ---- editing ----

        /// How many presses in a row the press at (`x`, `y`) makes: 1, 2 (a double click), or
        /// 3 (a triple click), by GTK's double-click time and distance; a fourth starts over.
        fn press_count(&self, x: f64, y: f64) -> u32 {
            let (time, distance) = gtk::Settings::default().map_or((400, 5), |s| {
                (s.gtk_double_click_time(), s.gtk_double_click_distance())
            });
            let now = Instant::now();
            let near = |(at, px, py, _): (Instant, f64, f64, u32)| {
                now.duration_since(at).as_millis() <= time.max(0) as u128
                    && (x - px).abs() <= f64::from(distance)
                    && (y - py).abs() <= f64::from(distance)
            };
            let count = match self.presses.get() {
                Some(last) if near(last) => last.3 % 3 + 1,
                _ => 1,
            };
            self.presses.set(Some((now, x, y, count)));
            count
        }

        /// Drop the input method's composition (the caret moved under it).
        fn reset_im(&self) {
            if let Some(im) = self.im.get() {
                im.reset();
            }
        }

        /// Show the input method's preedit at the caret, or none.
        pub(super) fn set_preedit(&self, preedit: Option<(String, pango::AttrList, usize)>) {
            let complete = {
                let mut state = self.state.borrow_mut();
                let Some(st) = state.as_mut() else {
                    return;
                };
                if st.preedit.is_none() && preedit.is_none() {
                    return;
                }
                st.preedit = preedit;
                show_preedit(st);
                st.flow.is_complete()
            };
            if !complete {
                self.completed.set(None);
                self.start_layout();
            }
            self.reveal_caret();
        }

        /// What the input method reads around the caret: the caret's paragraph, the caret's
        /// byte offset in it, and the selection's other end when it is in that paragraph.
        fn surrounding(&self) -> Option<(String, usize, usize)> {
            let state = self.state.borrow();
            let st = state.as_ref()?;
            let c = st.caret?;
            let text = document_edit::text(st.flow.document(), c.at, c.block)?.into_owned();
            let anchor = match caret::selection(st) {
                Some((s, e)) if s.at == c.at && s.block == c.block && e.block == c.block => {
                    if s == c {
                        e.offset
                    } else {
                        s.offset
                    }
                }
                _ => c.offset,
            };
            Some((text, c.offset, anchor))
        }

        /// Delete `chars` characters from `offset` characters after the caret (before it
        /// when negative), within the caret's paragraph, as the input method asks.
        fn delete_surrounding(&self, offset: i32, chars: i32) -> bool {
            self.edit_with(false, |st| {
                let c = st.caret?;
                let doc = st.flow.document();
                let text = document_edit::text(doc, c.at, c.block)?;
                let starts: Vec<usize> = text
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(text.len()))
                    .collect();
                let here = starts.iter().position(|&b| b == c.offset)? as i64;
                let last = starts.len() as i64 - 1;
                let from = (here + i64::from(offset)).clamp(0, last) as usize;
                let to = (from as i64 + i64::from(chars)).clamp(0, last) as usize;
                (from < to).then_some(())?;
                document_edit::replace(
                    doc,
                    Pos {
                        offset: starts[from],
                        ..c
                    },
                    Pos {
                        offset: starts[to],
                        ..c
                    },
                    "",
                    &RunStyle::default(),
                )
            });
            true
        }

        /// Move the caret where `to` says and show it: with `extend` (Shift), the selection
        /// runs from its anchor (else the caret's old place) to it; otherwise the selection
        /// goes. Up and Down (`keep_goal`) keep the column they aim for; other moves forget it.
        fn move_caret(
            &self,
            keep_goal: bool,
            extend: bool,
            to: impl FnOnce(&mut State, Pos) -> Pos,
        ) {
            self.reset_im();
            {
                let mut state = self.state.borrow_mut();
                let Some(st) = state.as_mut() else {
                    return;
                };
                let Some(c) = st.caret else {
                    return;
                };
                if !keep_goal {
                    st.goal = None;
                }
                let anchor = caret::anchor(st);
                let moved = to(st, c);
                st.caret = Some(moved);
                st.typing = None;
                st.history.seal();
                match (extend, anchor, caret::to_flow(st, moved)) {
                    (true, Some(anchor), Some(focus)) => caret::select(st, anchor, focus),
                    _ => caret::clear_selection(st),
                }
            }
            self.reveal_caret();
        }

        fn reveal_caret(&self) {
            self.reveal.set(true);
            let obj = self.obj();
            obj.queue_allocate();
            obj.queue_draw();
            obj.update_caret_position();
            obj.update_selection_bound();
        }

        /// Make the edit `make` builds from the state, apply it, and show the result.
        /// `keep_caret`: a paragraph command given at the caret, not a selection, leaves
        /// the caret where it was rather than selecting what it changed.
        fn edit_with(&self, keep_caret: bool, make: impl FnOnce(&mut State) -> Option<Edit>) {
            let (change, text) = {
                let mut state = self.state.borrow_mut();
                let Some(st) = state.as_mut() else {
                    return;
                };
                let kept = if keep_caret && caret::selection(st).is_none() {
                    st.caret
                } else {
                    None
                };
                let Some(edit) = make(st) else {
                    return;
                };
                let State {
                    flow,
                    shaper,
                    history,
                    ..
                } = st;
                let before = a11y::Before::take(flow, edit.span(flow.document()));
                let change = flow.edit(shaper, |doc| {
                    let change = history.apply(doc, edit);
                    (change, splice(change))
                });
                let change = match kept {
                    Some(caret) => Change {
                        caret,
                        anchor: None,
                        ..change
                    },
                    None => change,
                };
                (change, before.after(flow))
            };
            self.announce(text);
            self.changed(change);
        }

        /// Undo, or redo, a step.
        fn step(&self, redo: bool) {
            let result = {
                let mut state = self.state.borrow_mut();
                let Some(State {
                    flow,
                    shaper,
                    history,
                    ..
                }) = state.as_mut()
                else {
                    return;
                };
                let Some(span) = history.next_span(redo) else {
                    return;
                };
                let before = a11y::Before::take(flow, span);
                flow.edit(shaper, |doc| {
                    let change = if redo {
                        history.redo(doc)
                    } else {
                        history.undo(doc)
                    };
                    let s = change.map_or(
                        Splice {
                            start: 0,
                            removed: 0,
                            inserted: 0,
                        },
                        splice,
                    );
                    (change, s)
                })
                .map(|change| (change, before.after(flow)))
            };
            if let Some((change, text)) = result {
                self.announce(text);
                self.changed(change);
            }
        }

        /// Tell screen readers what an edit removed and inserted, as GTK's text-changed
        /// events: a removal is announced with the text it removed.
        fn announce(&self, text: a11y::TextChange) {
            let obj = self.obj();
            if let Some((at, removed)) = text.removed {
                let end = at + removed.chars().count();
                self.removal.replace(Some((at, removed)));
                obj.update_contents(
                    gtk::AccessibleTextContentChange::Remove,
                    at as u32,
                    end as u32,
                );
                self.removal.take();
            }
            if let Some(inserted) = text.inserted {
                obj.update_contents(
                    gtk::AccessibleTextContentChange::Insert,
                    inserted.start as u32,
                    inserted.end as u32,
                );
            }
        }

        /// Show what an edit, undo, or redo changed: the caret (and a selection) where it
        /// says, the page breaks it left, the layout measured again.
        fn changed(&self, change: Change) {
            let (complete, breaks) = {
                let mut state = self.state.borrow_mut();
                let Some(st) = state.as_mut() else {
                    return;
                };
                st.caret = Some(change.caret);
                st.goal = None;
                st.typing = None;
                st.revision += 1;
                if st.preedit.is_some() {
                    // The edit rebuilt the caret's leaf: show the preedit in it again.
                    show_preedit(st);
                }
                match change.anchor {
                    Some(anchor) => {
                        if let (Some(a), Some(c)) =
                            (caret::to_flow(st, anchor), caret::to_flow(st, change.caret))
                        {
                            caret::select(st, a, c);
                        }
                    }
                    // The flow dropped the selection with the edit.
                    None => st.anchor = None,
                }
                (st.flow.is_complete(), st.flow.page_break_count())
            };
            self.sync_breaks(breaks);
            if !complete {
                self.completed.set(None);
                self.start_layout();
            }
            self.reveal_caret();
            if let Some(edited) = self.edited.borrow().as_ref() {
                edited();
            }
        }

        /// Keep one page-break child per page break.
        fn sync_breaks(&self, count: usize) {
            let mut breaks = self.breaks.borrow_mut();
            while breaks.len() > count {
                if let Some(line) = breaks.pop() {
                    line.unparent();
                }
            }
            while breaks.len() < count {
                breaks.push(page_break(&self.obj()));
            }
        }

        /// The selection, else the caret, as the range an edit replaces.
        fn target(st: &State) -> Option<(Pos, Pos)> {
            caret::selection(st).or_else(|| st.caret.map(|c| (c, c)))
        }

        pub(super) fn type_text(&self, text: &str) {
            self.edit_with(false, |st| {
                let (from, to) = Self::target(st)?;
                let style = if st.plain {
                    RunStyle::default()
                } else {
                    st.typing
                        .clone()
                        .unwrap_or_else(|| document_edit::look(st.flow.document(), from))
                };
                let doc = st.flow.document();
                // Typing over a selection that runs into a table replaces it all, the
                // table with it, as a paste of the typed text does.
                document_edit::replace(doc, from, to, text, &style).or_else(|| {
                    document_edit::paste(doc, from, to, &Fragment::from_plain(text, &style))
                })
            });
        }

        fn enter(&self) {
            self.edit_with(false, |st| {
                let (from, to) = Self::target(st)?;
                let doc = st.flow.document();
                // A selection split can't make (it runs into a table) is deleted instead.
                document_edit::split(doc, from, to).or_else(|| document_edit::delete(doc, from, to))
            });
        }

        fn backspace(&self) {
            self.edit_with(false, |st| {
                let doc = st.flow.document();
                if let Some((s, e)) = caret::selection(st) {
                    return document_edit::delete(doc, s, e);
                }
                let c = st.caret?;
                match caret::char_before(st, c) {
                    Some(before) => {
                        document_edit::replace(doc, before, c, "", &RunStyle::default())
                    }
                    None => document_edit::join_backward(doc, c),
                }
            });
        }

        fn delete(&self) {
            self.edit_with(false, |st| {
                let doc = st.flow.document();
                if let Some((s, e)) = caret::selection(st) {
                    return document_edit::delete(doc, s, e);
                }
                let c = st.caret?;
                match caret::char_after(st, c) {
                    Some(after) => document_edit::replace(doc, c, after, "", &RunStyle::default()),
                    None => document_edit::join_forward(doc, c),
                }
            });
        }

        /// The selection as a fragment, for the clipboard.
        fn selected_fragment(&self) -> Option<Fragment> {
            let state = self.state.borrow();
            let st = state.as_ref()?;
            let (s, e) = caret::selection(st)?;
            document_edit::copy(st.flow.document(), s, e)
        }

        /// Put the selection on the clipboard: Slate's fragment for Slate windows, plain text
        /// for other apps. False when nothing is selected.
        fn copy(&self) -> bool {
            let Some(fragment) = self.selected_fragment() else {
                return false;
            };
            let rich = glib::Bytes::from_owned(fragment.to_json().into_bytes());
            let provider = gdk::ContentProvider::new_union(&[
                gdk::ContentProvider::for_bytes(Fragment::MIME, &rich),
                gdk::ContentProvider::for_value(&fragment.plain_text().to_value()),
            ]);
            if let Err(e) = self.obj().clipboard().set_content(Some(&provider)) {
                tracing::warn!("cannot copy: {e}");
                return false;
            }
            true
        }

        fn cut(&self) {
            if self.copy() {
                self.edit_with(false, |st| {
                    let (s, e) = caret::selection(st)?;
                    document_edit::delete(st.flow.document(), s, e)
                });
            }
        }

        /// Paste the clipboard over the selection or at the caret: a Slate fragment with its
        /// looks, else plain text in the caret's look. The clipboard is read asynchronously;
        /// the paste is one undoable step.
        fn paste(&self) {
            let page = self.obj().downgrade();
            let clipboard = self.obj().clipboard();
            glib::spawn_future_local(async move {
                let clip = read_clipboard(&clipboard).await;
                if let (Some(page), Some(clip)) = (page.upgrade(), clip) {
                    page.imp().paste_clip(clip);
                }
            });
        }

        fn paste_clip(&self, clip: Clip) {
            self.edit_with(false, |st| {
                let (from, to) = Self::target(st)?;
                let doc = st.flow.document();
                let fragment = match clip {
                    Clip::Rich(fragment) => fragment,
                    Clip::Plain(text) => {
                        let look = if st.plain {
                            RunStyle::default()
                        } else {
                            document_edit::look(doc, from)
                        };
                        Fragment::from_plain(&text, &look)
                    }
                };
                (!fragment.blocks.is_empty()).then_some(())?;
                document_edit::paste(doc, from, to, &fragment)
            });
        }

        fn select_all(&self) {
            if let Some(st) = self.state.borrow_mut().as_mut() {
                st.flow.select_all();
                st.anchor = st.flow.selection_anchor();
                st.caret = caret::last(st);
            }
            self.selection_changed();
        }

        /// Tab in a table cell goes to the next cell (Shift+Tab the one before), and out of
        /// the table past its last; in a list item it nests the item a level deeper
        /// (Shift+Tab shallower); elsewhere it types a tab.
        fn tab(&self, back: bool) {
            let (in_cell, in_list) = self.state.borrow().as_ref().map_or((false, false), |st| {
                let c = st.caret;
                (
                    c.is_some_and(|c| matches!(c.at, Container::Cell { .. })),
                    !st.plain && c.is_some_and(|c| caret::is_list_item(st, c)),
                )
            });
            if in_cell {
                self.move_caret(false, false, |st, c| {
                    caret::next_cell(st, c, !back).unwrap_or(c)
                });
            } else if in_list {
                self.edit_with(true, |st| {
                    let (at, blocks) = caret::blocks(st)?;
                    document_edit::indent(st.flow.document(), at, blocks, if back { -1 } else { 1 })
                });
            } else if !back {
                self.type_text("\t");
            }
        }

        pub(super) fn command(&self, command: Command) {
            match command {
                Command::Undo => self.step(false),
                Command::Redo => self.step(true),
                Command::Bold | Command::Italic => {
                    let flag = if command == Command::Bold {
                        Flag::Bold
                    } else {
                        Flag::Italic
                    };
                    self.edit_with(false, |st| {
                        if st.plain {
                            return None;
                        }
                        if let Some((s, e)) = caret::selection(st) {
                            return document_edit::toggle(st.flow.document(), s, e, flag);
                        }
                        // No selection: what is typed next takes the look.
                        let c = st.caret?;
                        let doc = st.flow.document();
                        let look = st
                            .typing
                            .clone()
                            .unwrap_or_else(|| document_edit::look(doc, c));
                        st.typing = Some(document_edit::toggled(doc, c, &look, flag));
                        None
                    });
                }
                Command::Style(level) => self.edit_with(true, |st| {
                    let (at, blocks) = caret::blocks(st).filter(|_| !st.plain)?;
                    let doc = st.flow.document();
                    let styles = &doc.styles;
                    let style = match level {
                        0 => styles.default_id().to_owned(),
                        n => {
                            let n = n.min(6);
                            styles
                                .heading(n)
                                .map_or_else(|| format!("Heading{n}"), |s| s.id.clone())
                        }
                    };
                    document_edit::set_style(doc, at, blocks, &style)
                }),
                Command::Bullets | Command::Numbers => self.edit_with(true, |st| {
                    let (at, blocks) = caret::blocks(st).filter(|_| !st.plain)?;
                    document_edit::toggle_list(
                        st.flow.document(),
                        at,
                        blocks,
                        command == Command::Numbers,
                    )
                }),
                Command::Cut => self.cut(),
                Command::Copy => {
                    self.copy();
                }
                Command::Paste => self.paste(),
                Command::SelectAll => self.select_all(),
            }
        }
    }
}

glib::wrapper! {
    /// A document's page: the pageless column, scrollable.
    pub struct Page(ObjectSubclass<imp::Page>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Page {
    /// The page showing `doc`, whose runs [`document_edit::normalize`] has merged so undo
    /// gives each paragraph back exactly.
    pub fn new(doc: document_model::Document, textures: Textures, setting: TextWidth) -> Self {
        let page: Self = glib::Object::new();
        let imp = page.imp();
        imp.setting.set(setting);
        let shaper = PangoShaper::new(page.pango_context(), &doc.format, textures);
        let plain = matches!(doc.format, document_model::Format::Text(_));
        let flow = Flow::new(doc);
        let lines = (0..flow.page_break_count())
            .map(|_| page_break(&page))
            .collect();
        imp.breaks.replace(lines);
        let mut state = State {
            flow,
            shaper,
            history: History::new(),
            caret: None,
            goal: None,
            typing: None,
            plain,
            revision: 0,
            preedit: None,
            anchor: None,
        };
        state.caret = caret::first(&state);
        imp.state.replace(Some(state));
        page
    }

    /// Run a menu or key command.
    pub fn command(&self, command: Command) {
        self.imp().command(command);
    }

    /// Type `text` at the caret, as the input method's commit does.
    pub fn type_text(&self, text: &str) {
        self.imp().type_text(text);
    }

    /// Put the caret in the middle of the first editable block from top-level block
    /// `block` on, and show it.
    pub fn put_caret(&self, block: usize) {
        let imp = self.imp();
        if let Some(st) = imp.state.borrow_mut().as_mut() {
            st.caret = caret::middle_from(st, block).or(st.caret);
            caret::clear_selection(st);
        }
        self.queue_allocate();
        imp.reveal.set(true);
    }

    /// How many edits the page has shown.
    pub fn revision(&self) -> u64 {
        self.imp().state.borrow().as_ref().map_or(0, |s| s.revision)
    }

    /// Which document the undo history is at ([`document_edit::History::position`]): the
    /// same number means the same document.
    pub fn position(&self) -> u64 {
        self.imp()
            .state
            .borrow()
            .as_ref()
            .map_or(0, |s| s.history.position())
    }

    /// The document as it is now.
    pub fn document(&self) -> document_model::Document {
        let state = self.imp().state.borrow();
        let st = state.as_ref().expect("a page has its document");
        st.flow.document().clone()
    }

    /// Call `f` after every edit, undo, and redo is shown.
    pub fn connect_edited(&self, f: impl Fn() + 'static) {
        self.imp().edited.replace(Some(Box::new(f)));
    }

    /// How many top-level blocks the document has.
    pub fn block_count(&self) -> usize {
        self.imp()
            .state
            .borrow()
            .as_ref()
            .map_or(0, |s| s.flow.block_count())
    }

    /// Follow a new text width setting.
    pub fn set_text_width(&self, setting: TextWidth) {
        if self.imp().setting.replace(setting) != setting {
            self.queue_resize();
        }
    }

    /// The column's width, 0 before the page is first allocated.
    pub fn column(&self) -> i32 {
        self.imp()
            .state
            .borrow()
            .as_ref()
            .map_or(0, |s| s.flow.width())
    }

    /// When every block was last measured at the column's width; `None` while some are
    /// still estimated.
    pub fn completed_at(&self) -> Option<Instant> {
        self.imp().completed.get()
    }
}

/// Give the shaper the preedit at the caret's leaf (or none), and have the flow measure
/// that leaf again, its text now laid out with or without it.
fn show_preedit(st: &mut State) {
    let leaf = st.caret.and_then(|c| caret::to_flow(st, c)).map(|p| p.leaf);
    match (leaf, &st.preedit) {
        (Some(leaf), Some((text, attrs, _))) => {
            let key = st.flow.layout_key(leaf);
            let at = st.caret.map_or(0, |c| c.offset);
            st.shaper.set_preedit(Some((key, at, text, attrs)));
        }
        _ => st.shaper.set_preedit(None),
    }
    if let Some(leaf) = leaf {
        st.flow.remeasure(leaf);
    }
}

/// What a paste reads from the clipboard.
enum Clip {
    /// Copied in Slate, with its looks.
    Rich(Fragment),
    /// From another app.
    Plain(String),
}

/// The clipboard's content: Slate's fragment when it offers one, else its text.
async fn read_clipboard(clipboard: &gdk::Clipboard) -> Option<Clip> {
    if clipboard.formats().contain_mime_type(Fragment::MIME) {
        if let Ok((stream, _)) = clipboard
            .read_future(&[Fragment::MIME], glib::Priority::DEFAULT)
            .await
        {
            let mut bytes = Vec::new();
            loop {
                match stream
                    .read_bytes_future(1 << 16, glib::Priority::DEFAULT)
                    .await
                {
                    Ok(chunk) if !chunk.is_empty() => bytes.extend_from_slice(&chunk),
                    Ok(_) => break,
                    Err(e) => {
                        tracing::warn!("cannot read the clipboard: {e}");
                        break;
                    }
                }
            }
            if let Some(fragment) = std::str::from_utf8(&bytes)
                .ok()
                .and_then(Fragment::from_json)
            {
                return Some(Clip::Rich(fragment));
            }
        }
    }
    let text = clipboard.read_text_future().await.ok()??;
    Some(Clip::Plain(text.to_string()))
}

/// The top-level blocks an edit's change replaced, for the flow: an edit in a cell
/// replaces its table.
fn splice(change: Change) -> Splice {
    match change.at {
        Container::Top => Splice {
            start: change.start,
            removed: change.removed,
            inserted: change.inserted,
        },
        Container::Cell { table, .. } => Splice {
            start: table,
            removed: 1,
            inserted: 1,
        },
    }
}

/// A page break's line: a child of `page` named "Page break" for screen readers.
fn page_break(page: &Page) -> gtk::Separator {
    let line = gtk::Separator::new(gtk::Orientation::Horizontal);
    line.add_css_class("page-break");
    line.update_property(&[gtk::accessible::Property::Label("Page break")]);
    line.set_parent(page);
    line
}

/// Draw `p`, with the column's left edge at `left` and the viewport's top at document y
/// `top`.
fn draw(snapshot: &gtk::Snapshot, shaper: &mut PangoShaper, p: Paint<'_>, left: f32, top: f64) {
    let at = |x: i32, y: f64| (left + x as f32, (y - top) as f32);
    match p {
        Paint::Text {
            key,
            content,
            x,
            y,
            width,
            marker,
            selected,
            ..
        } => {
            let (x, y) = at(x, y);
            if marker {
                if let flow::Content::Paragraph {
                    list: Some(item), ..
                } = content
                {
                    snapshot.save();
                    snapshot.translate(&graphene::Point::new(x - LEVEL_INDENT as f32, y));
                    snapshot.append_layout(shaper.marker(item), &rgba(INK));
                    snapshot.restore();
                }
            }
            let layout = shaper.layout(key, content, width);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(x, y));
            if let Some(range) = selected {
                highlight(snapshot, layout, range);
            }
            snapshot.append_layout(layout, &rgba(INK));
            snapshot.restore();
        }
        Paint::Band {
            x,
            y,
            width,
            height,
        } => {
            let (x, y) = at(x, y);
            snapshot.append_color(&rgba(CODE_BAND), &rect(x, y, width as f32, height as f32));
        }
        Paint::Picture {
            image,
            x,
            y,
            width,
            height,
            selected,
            ..
        } => {
            let (x, y) = at(x, y);
            let bounds = rect(x, y, width as f32, height as f32);
            if let Some(texture) = shaper.texture(image) {
                snapshot.append_scaled_texture(texture, gsk::ScalingFilter::Trilinear, &bounds);
            }
            if selected {
                snapshot.append_color(&rgba_alpha(SELECTION.0, SELECTION.1), &bounds);
            }
        }
        // Each page break is a child widget that draws itself.
        Paint::PageBreak { .. } => {}
        Paint::Rule { x, y, width } => {
            let (x, y) = at(x, y);
            snapshot.append_color(&rgba(LINE), &rect(x, y, width as f32, 1.0));
        }
        Paint::Border {
            x,
            y,
            width,
            height,
        } => {
            let (x, y) = at(x, y);
            let outline = gsk::RoundedRect::from_rect(rect(x, y, width as f32, height as f32), 0.0);
            snapshot.append_border(&outline, &[1.0; 4], &[rgba(LINE); 4]);
        }
    }
}

/// The selection's background behind bytes `range` of `layout`.
fn highlight(snapshot: &gtk::Snapshot, layout: &pango::Layout, range: Range<usize>) {
    let colour = rgba_alpha(SELECTION.0, SELECTION.1);
    let scale = pango::SCALE as f32;
    let mut lines = layout.iter();
    loop {
        if let Some(line) = lines.line_readonly() {
            let start = line.start_index().max(0) as usize;
            let end = start + line.length().max(0) as usize;
            let (s, e) = (range.start.max(start), range.end.min(end));
            if s < e {
                let (y0, y1) = lines.line_yrange();
                let xs = line.x_ranges(s as i32, e as i32);
                for &[x0, x1] in xs.as_chunks::<2>().0 {
                    snapshot.append_color(
                        &colour,
                        &rect(
                            x0 as f32 / scale,
                            y0 as f32 / scale,
                            (x1 - x0) as f32 / scale,
                            (y1 - y0) as f32 / scale,
                        ),
                    );
                }
            }
        }
        if !lines.next_line() {
            break;
        }
    }
}

fn open_link(page: &Page, url: String) {
    let window = page.root().and_downcast::<gtk::Window>();
    gtk::UriLauncher::new(&url).launch(
        window.as_ref(),
        gtk::gio::Cancellable::NONE,
        move |result| {
            if let Err(e) = result {
                tracing::warn!("cannot open link {url}: {e}");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_window_gets_the_settings_width() {
        for (setting, px) in [
            (TextWidth::Narrow, 480),
            (TextWidth::Medium, 624),
            (TextWidth::Wide, 816),
        ] {
            assert_eq!(column_width(setting, 1280), px);
            // Exactly room for it and a margin each side.
            assert_eq!(column_width(setting, px + 2 * MIN_MARGIN), px);
        }
    }

    #[test]
    fn full_is_the_window_less_a_margin_each_side() {
        assert_eq!(column_width(TextWidth::Full, 1280), 1232);
        assert_eq!(column_width(TextWidth::Full, 900), 852);
        assert_eq!(column_width(TextWidth::Full, 3840), 3792);
    }

    #[test]
    fn a_narrower_window_narrows_the_column() {
        assert_eq!(
            column_width(TextWidth::Medium, 624 + 2 * MIN_MARGIN - 1),
            623
        );
        assert_eq!(column_width(TextWidth::Wide, 640), 592);
        assert_eq!(column_width(TextWidth::Narrow, 500), 452);
    }

    #[test]
    fn the_column_never_gets_narrower_than_the_minimum() {
        for setting in TextWidth::ALL {
            assert_eq!(
                column_width(setting, MIN_COLUMN + 2 * MIN_MARGIN),
                MIN_COLUMN
            );
            assert_eq!(column_width(setting, 100), MIN_COLUMN);
            assert_eq!(column_width(setting, 0), MIN_COLUMN);
        }
    }
}
