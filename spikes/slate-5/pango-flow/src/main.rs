// SLATE-5 spike: a custom Pango flow editor widget (one pango::Layout per paragraph,
// drawn in `snapshot`). Throwaway; see common.rs for the content, looks, and protocol.
//
// Layout strategy: every paragraph has a height in a Fenwick tree (y <-> paragraph in
// O(log n)). Heights start as estimates (chars x approximate char width / column width,
// rounded up to lines); the viewport lays out its paragraphs synchronously in
// size_allocate (the frame's layout phase), anchored to the paragraph at the top so text
// never jumps when estimates above it are replaced; an idle source on the main thread
// (below redraw priority) lays out the rest in <= 4 ms chunks and emits L when every
// paragraph is exact at the current width. Layouts are cached per paragraph together with
// the width they were laid out at; a width change re-wraps each cached layout in place.
#[path = "../../common.rs"]
mod common;

use common::*;
use gtk::{gdk, glib, graphene, pango, prelude::*, subclass::prelude::*};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

const CHUNK_NS: u64 = 4_000_000;
const SCALE: i32 = pango::SCALE;

/// Pushes the elapsed µs into the vector on drop (stderr diagnostics only).
struct Timer<'a>(&'a RefCell<Vec<u32>>, u64);

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().push(((mono_ns() - self.1) / 1000) as u32);
    }
}

fn peak_rss_kib() -> i64 {
    // SAFETY: getrusage fills the zeroed struct.
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        ru.ru_maxrss
    }
}
// PangoLogAttr bitfield positions (pango-break.h).
const LA_WORD_START: u32 = 1 << 5;
const LA_SENTENCE_START: u32 = 1 << 8;

/// Prefix sums with point updates; `find(y)` is the element containing offset y.
#[derive(Default)]
struct Fenwick {
    t: Vec<i64>,
}

impl Fenwick {
    fn build(v: impl Iterator<Item = i64>) -> Self {
        let mut t = vec![0];
        t.extend(v);
        let n = t.len() - 1;
        for i in 1..=n {
            let j = i + (i & i.wrapping_neg());
            if j <= n {
                t[j] += t[i];
            }
        }
        Fenwick { t }
    }

    fn add(&mut self, i: usize, d: i64) {
        let mut i = i + 1;
        while i < self.t.len() {
            self.t[i] += d;
            i += i & i.wrapping_neg();
        }
    }

    /// Sum of the first `i` elements.
    fn prefix(&self, mut i: usize) -> i64 {
        let mut s = 0;
        while i > 0 {
            s += self.t[i];
            i -= i & i.wrapping_neg();
        }
        s
    }

    fn total(&self) -> i64 {
        self.prefix(self.t.len() - 1)
    }

    /// Index of the element containing offset `y` (all elements > 0), clamped to the last.
    fn find(&self, y: i64) -> usize {
        let n = self.t.len() - 1;
        let mut pos = 0;
        let mut rem = y;
        let mut step = n.next_power_of_two();
        while step > 0 {
            if pos + step <= n && self.t[pos + step] <= rem {
                pos += step;
                rem -= self.t[pos];
            }
            step >>= 1;
        }
        pos.min(n.saturating_sub(1))
    }
}

struct Para {
    p: Paragraph,
    chars: usize,
    layout: Option<pango::Layout>,
    /// Width the cached layout was wrapped at (-1: never).
    laid_w: i32,
    laid_h: i32,
    /// Height in the index (exact iff laid_w == col_w), including PARAGRAPH_GAP.
    h: i32,
    /// Text changed since the last measure.
    dirty: bool,
}

fn kind_ix(k: Kind) -> usize {
    match k {
        Kind::Heading(l) => l as usize,
        _ => 0,
    }
}

fn indent(k: Kind) -> i32 {
    match k {
        Kind::Bullet(l) => LEVEL_INDENT * (l as i32 + 1),
        _ => 0,
    }
}

fn attrs(p: &Paragraph) -> pango::AttrList {
    let list = pango::AttrList::new();
    for &(s, e, look) in &p.spans {
        let mut a: pango::Attribute = match look {
            Look::Bold => pango::AttrInt::new_weight(pango::Weight::Bold).into(),
            Look::Italic => pango::AttrInt::new_style(pango::Style::Italic).into(),
        };
        a.set_start_index(s as u32);
        a.set_end_index(e as u32);
        list.insert(a);
    }
    list
}

fn fonts() -> Vec<pango::FontDescription> {
    let mut v = Vec::new();
    let mut body = pango::FontDescription::from_string("Sans");
    body.set_size((BODY_PT * SCALE as f64).round() as i32);
    v.push(body);
    for l in 1..=3 {
        let mut h = pango::FontDescription::from_string("Sans Bold");
        h.set_size((BODY_PT * heading_scale(l) * SCALE as f64).round() as i32);
        v.push(h);
    }
    v
}

fn make_layout(w: &gtk::Widget, p: &Paragraph, fonts: &[pango::FontDescription]) -> pango::Layout {
    let l = w.create_pango_layout(Some(&p.text));
    l.set_font_description(Some(&fonts[kind_ix(p.kind)]));
    l.set_wrap(pango::WrapMode::WordChar);
    l.set_attributes(Some(&attrs(p)));
    l
}

fn log_attrs(l: &pango::Layout) -> Vec<u32> {
    let mut n = 0;
    // SAFETY: PangoLogAttr is a 32-bit bitfield struct; the array lives as long as the layout.
    unsafe {
        let p = pango::ffi::pango_layout_get_log_attrs_readonly(l.to_glib_none().0, &mut n);
        std::slice::from_raw_parts(p as *const u32, n as usize).to_vec()
    }
}

use gtk::glib::translate::ToGlibPtr;

pub struct State {
    paras: Vec<Para>,
    /// Paragraph heights in px.
    hs: Fenwick,
    /// Characters per paragraph + 1 for the "\n" separator (accessible offsets).
    cs: Fenwick,
    fonts: Vec<pango::FontDescription>,
    /// Per kind: approximate char width and line height in px.
    est: [(f64, f64); 4],
    width: TextWidth,
    col_w: i32,
    exact: usize,
    bg_next: usize,
    /// Paragraph at the top of the viewport and the px offset into it.
    anchor: (usize, f64),
    /// Paragraph and byte index.
    cursor: (usize, usize),
    want_x: Option<i32>,
    revision: u64,
    l_pending: bool,
    dirty: Vec<usize>,
    reveal: bool,
    layouts_built: u64,
    /// Background pass since the last width change: ns spent, chunks, paragraphs laid out.
    bg_stats: (u64, u32, u32),
}

impl Default for State {
    fn default() -> Self {
        State {
            paras: Vec::new(),
            hs: Fenwick::default(),
            cs: Fenwick::default(),
            fonts: fonts(),
            est: [(7.0, 17.0); 4],
            width: TextWidth::Full,
            col_w: 0,
            exact: 0,
            bg_next: 0,
            anchor: (0, 0.0),
            cursor: (0, 0),
            want_x: None,
            revision: 0,
            l_pending: true,
            dirty: Vec::new(),
            reveal: false,
            layouts_built: 0,
            bg_stats: (0, 0, 0),
        }
    }
}

impl State {
    fn n(&self) -> usize {
        self.paras.len()
    }

    fn estimate(&self, i: usize) -> i32 {
        let p = &self.paras[i];
        let (cw, lh) = self.est[kind_ix(p.p.kind)];
        let tw = (self.col_w - indent(p.p.kind)).max(1) as f64;
        let lines = (p.chars as f64 * cw / tw).ceil().max(1.0);
        (lines * lh).round() as i32 + PARAGRAPH_GAP
    }

    fn rebuild_heights(&mut self) {
        self.hs = Fenwick::build(self.paras.iter().map(|p| p.h as i64));
    }

    fn rebuild_chars(&mut self) {
        self.cs = Fenwick::build(self.paras.iter().map(|p| p.chars as i64 + 1));
    }

    fn value(&self) -> f64 {
        self.hs.prefix(self.anchor.0) as f64 + self.anchor.1
    }

    fn anchor_at(&self, v: f64) -> (usize, f64) {
        let v = v.max(0.0);
        let i = self.hs.find(v.floor() as i64);
        (i, v - self.hs.prefix(i) as f64)
    }

    /// New column width: cached layouts at this width keep their exact height, the rest
    /// fall back to estimates until laid out again. Does no layout itself.
    fn set_col(&mut self, col: i32) {
        let old_h = self.paras.get(self.anchor.0).map_or(1, |p| p.h.max(1));
        self.col_w = col;
        self.exact = 0;
        for i in 0..self.n() {
            let h = if self.paras[i].laid_w == col {
                self.exact += 1;
                self.paras[i].laid_h
            } else {
                self.estimate(i)
            };
            self.paras[i].h = h;
        }
        self.rebuild_heights();
        eprintln!(
            "col {col}: {} exact, estimated content_h {}, anchor paragraph {}",
            self.exact,
            self.hs.total(),
            self.anchor.0
        );
        if let Some(p) = self.paras.get(self.anchor.0) {
            self.anchor.1 = self.anchor.1 * p.h as f64 / old_h as f64;
        }
        self.l_pending = true;
        self.bg_stats = (0, 0, 0);
        self.bg_next = 0;
    }

    /// Lays out paragraph `i` at the current width if needed; true if it did.
    fn ensure(&mut self, i: usize, w: &gtk::Widget) -> bool {
        let col = self.col_w;
        let fonts = &self.fonts;
        let p = &mut self.paras[i];
        if p.laid_w == col && !p.dirty {
            return false;
        }
        let was_exact = p.laid_w == col;
        let layout = p.layout.get_or_insert_with(|| {
            self.layouts_built += 1;
            make_layout(w, &p.p, fonts)
        });
        layout.set_width((col - indent(p.p.kind)) * SCALE);
        let h = layout.pixel_size().1 + PARAGRAPH_GAP;
        p.laid_w = col;
        p.laid_h = h;
        p.dirty = false;
        let d = h - p.h;
        p.h = h;
        if d != 0 {
            self.hs.add(i, d as i64);
        }
        if !was_exact {
            self.exact += 1;
        }
        true
    }

    /// A layout for paragraph i at the current width without touching the index.
    fn layout_for(&self, i: usize, w: &gtk::Widget) -> pango::Layout {
        let p = &self.paras[i];
        match &p.layout {
            Some(l) if p.laid_w == self.col_w && !p.dirty => l.clone(),
            _ => {
                let l = make_layout(w, &p.p, &self.fonts);
                l.set_width((self.col_w - indent(p.p.kind)) * SCALE);
                l
            }
        }
    }

    /// The paragraph's text or attributes changed: refresh its cached layout lazily.
    fn touch(&mut self, i: usize) {
        let p = &mut self.paras[i];
        p.chars = p.p.text.chars().count();
        if let Some(l) = &p.layout {
            l.set_text(&p.p.text);
            l.set_attributes(Some(&attrs(&p.p)));
        }
        p.dirty = true;
        if !self.dirty.contains(&i) {
            self.dirty.push(i);
        }
    }

    fn flush_dirty(&mut self, w: &gtk::Widget) {
        for i in std::mem::take(&mut self.dirty) {
            self.ensure(i, w);
        }
    }

    // ---- accessible text: paragraphs joined by "\n", offsets in characters ----

    fn total_chars(&self) -> usize {
        (self.cs.total() as usize).saturating_sub(1)
    }

    fn char_of(&self, i: usize, byte: usize) -> usize {
        self.cs.prefix(i) as usize + self.paras[i].p.text[..byte].chars().count()
    }

    fn caret_offset(&self) -> usize {
        self.char_of(self.cursor.0, self.cursor.1)
    }

    fn text_range(&self, s: usize, e: usize) -> String {
        let e = e.min(self.total_chars());
        let mut out = String::new();
        if s >= e {
            return out;
        }
        let mut i = self.cs.find(s as i64);
        let mut k = s - self.cs.prefix(i) as usize;
        let mut left = e - s;
        while left > 0 && i < self.n() {
            for c in self.paras[i].p.text.chars().skip(k).take(left) {
                out.push(c);
                left -= 1;
            }
            if left > 0 && i + 1 < self.n() {
                out.push('\n');
                left -= 1;
            }
            i += 1;
            k = 0;
        }
        out
    }

    fn boundary(
        &self,
        offset: usize,
        gran: gtk::AccessibleTextGranularity,
        w: &gtk::Widget,
    ) -> (usize, usize) {
        use gtk::AccessibleTextGranularity as G;
        let total = self.total_chars();
        let o = offset.min(total);
        let i = self.cs.find(o as i64);
        let p = &self.paras[i];
        let pstart = self.cs.prefix(i) as usize;
        let pend = pstart + p.chars + usize::from(i + 1 < self.n());
        let k = o - pstart;
        match gran {
            G::Character => (o, (o + 1).min(total)),
            G::Paragraph => (pstart, pend),
            G::Line => {
                let l = self.layout_for(i, w);
                let byte = p.p.text.char_indices().nth(k).map_or(p.p.text.len(), |(b, _)| b);
                let lines = l.lines_readonly();
                let mut s = 0;
                let mut e = pend;
                for (n, line) in lines.iter().enumerate() {
                    let ls = line.start_index() as usize;
                    let le = ls + line.length() as usize;
                    if byte < le || n + 1 == lines.len() {
                        s = pstart + p.p.text[..ls].chars().count();
                        if n + 1 < lines.len() {
                            e = pstart + p.p.text[..le].chars().count();
                        }
                        break;
                    }
                }
                (s, e)
            }
            _ => {
                let bit = if gran == G::Word {
                    LA_WORD_START
                } else {
                    LA_SENTENCE_START
                };
                let la = log_attrs(&self.layout_for(i, w));
                let s = (0..=k.min(p.chars))
                    .rev()
                    .find(|&j| la.get(j).is_some_and(|a| a & bit != 0))
                    .unwrap_or(0);
                let e = (k + 1..p.chars)
                    .find(|&j| la.get(j).is_some_and(|a| a & bit != 0))
                    .map_or(pend, |j| pstart + j);
                (pstart + s, e)
            }
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Flow {
        pub st: RefCell<State>,
        pub hadj: RefCell<Option<gtk::Adjustment>>,
        pub vadj: RefCell<Option<gtk::Adjustment>>,
        pub hpol: Cell<Option<gtk::ScrollablePolicy>>,
        pub vpol: Cell<Option<gtk::ScrollablePolicy>>,
        /// Set while we change the adjustment ourselves.
        pub guard: Cell<bool>,
        pub allocated: Cell<bool>,
        pub idle: RefCell<Option<glib::SourceId>>,
        pub im: OnceCell<gtk::IMMulticontext>,
        pub marker: RefCell<Option<pango::Layout>>,
        pub misses: Cell<u64>,
        pub snap_us: RefCell<Vec<u32>>,
        pub alloc_us: RefCell<Vec<u32>>,
        /// Revision and column width as of the last snapshot (what the frame shows).
        pub shown: Cell<(u64, i32)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Flow {
        const NAME: &'static str = "Slate5PangoFlow";
        type Type = super::Flow;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Scrollable, gtk::AccessibleText);

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::TextBox);
        }
    }

    impl ObjectImpl for Flow {
        fn properties() -> &'static [glib::ParamSpec] {
            static P: std::sync::LazyLock<Vec<glib::ParamSpec>> = std::sync::LazyLock::new(|| {
                vec![
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("hadjustment"),
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("vadjustment"),
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("hscroll-policy"),
                    glib::ParamSpecOverride::for_interface::<gtk::Scrollable>("vscroll-policy"),
                ]
            });
            &P
        }

        fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
            match pspec.name() {
                "hadjustment" => {
                    self.hadj.replace(value.get().unwrap());
                }
                "vadjustment" => {
                    let adj: Option<gtk::Adjustment> = value.get().unwrap();
                    if let Some(adj) = &adj {
                        let obj = self.obj().downgrade();
                        adj.connect_value_changed(move |adj| {
                            let Some(obj) = obj.upgrade() else { return };
                            let imp = obj.imp();
                            if imp.guard.get() || !imp.allocated.get() {
                                return;
                            }
                            {
                                let mut st = imp.st.borrow_mut();
                                st.anchor = st.anchor_at(adj.value());
                            }
                            obj.queue_allocate();
                        });
                    }
                    self.vadj.replace(adj);
                }
                "hscroll-policy" => self.hpol.set(Some(value.get().unwrap())),
                "vscroll-policy" => self.vpol.set(Some(value.get().unwrap())),
                _ => unreachable!(),
            }
        }

        fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
            match pspec.name() {
                "hadjustment" => self.hadj.borrow().to_value(),
                "vadjustment" => self.vadj.borrow().to_value(),
                "hscroll-policy" => self
                    .hpol
                    .get()
                    .unwrap_or(gtk::ScrollablePolicy::Minimum)
                    .to_value(),
                "vscroll-policy" => self
                    .vpol
                    .get()
                    .unwrap_or(gtk::ScrollablePolicy::Minimum)
                    .to_value(),
                _ => unreachable!(),
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.update_property(&[
                gtk::accessible::Property::MultiLine(true),
                gtk::accessible::Property::ReadOnly(false),
                gtk::accessible::Property::Label("Document"),
            ]);
            let im = gtk::IMMulticontext::new();
            im.set_client_widget(Some(&*obj));
            im.connect_commit({
                let obj = obj.downgrade();
                move |_, text| {
                    if let Some(obj) = obj.upgrade() {
                        obj.imp().insert(text);
                    }
                }
            });
            let _ = self.im.set(im);
        }

        fn dispose(&self) {
            if let Some(im) = self.im.get() {
                im.set_client_widget(None::<&gtk::Widget>);
            }
        }
    }

    impl WidgetImpl for Flow {
        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let _t = Timer(&self.alloc_us, mono_ns());
            let obj = self.obj();
            if !self.allocated.get() {
                let ctx = obj.pango_context();
                let mut st = self.st.borrow_mut();
                let est: Vec<(f64, f64)> = st
                    .fonts
                    .iter()
                    .map(|f| {
                        let m = ctx.metrics(Some(f), None);
                        (
                            m.approximate_char_width() as f64 / SCALE as f64,
                            m.height() as f64 / SCALE as f64,
                        )
                    })
                    .collect();
                st.est.copy_from_slice(&est);
            }
            self.apply_col(width);
            self.allocated.set(true);
            self.layout_viewport(height as f64);
            if let Some(h) = self.hadj.borrow().as_ref() {
                self.guard.set(true);
                h.configure(0.0, 0.0, width as f64, 1.0, width as f64, width as f64);
                self.guard.set(false);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let _t = Timer(&self.snap_us, mono_ns());
            let obj = self.obj();
            let (w, h) = (obj.width() as f32, obj.height() as f32);
            snapshot.append_color(&gdk::RGBA::WHITE, &graphene::Rect::new(0.0, 0.0, w, h));
            // Black on white regardless of the (dark) desktop theme.
            let fg = gdk::RGBA::BLACK;
            let st = self.st.borrow();
            self.shown.set((st.revision, st.col_w));
            if st.n() == 0 {
                return;
            }
            let marker = self
                .marker
                .borrow_mut()
                .get_or_insert_with(|| {
                    let l = obj.create_pango_layout(Some("•"));
                    l.set_font_description(Some(&st.fonts[0]));
                    l
                })
                .clone();
            let x0 = ((obj.width() - st.col_w) / 2) as f32;
            let value = st.value();
            let mut i = st.hs.find(value.floor() as i64);
            let mut y = st.hs.prefix(i) as f64 - value;
            while i < st.n() && y < h as f64 {
                let p = &st.paras[i];
                match &p.layout {
                    Some(l) if p.laid_w == st.col_w => {
                        snapshot.save();
                        snapshot.translate(&graphene::Point::new(
                            x0 + indent(p.p.kind) as f32,
                            y as f32,
                        ));
                        snapshot.append_layout(l, &fg);
                        if i == st.cursor.0 {
                            let (c, _) = l.cursor_pos(st.cursor.1 as i32);
                            snapshot.append_color(
                                &fg,
                                &graphene::Rect::new(
                                    c.x() as f32 / SCALE as f32,
                                    c.y() as f32 / SCALE as f32,
                                    1.0,
                                    c.height() as f32 / SCALE as f32,
                                ),
                            );
                        }
                        snapshot.restore();
                        if let Kind::Bullet(level) = p.p.kind {
                            snapshot.save();
                            snapshot.translate(&graphene::Point::new(
                                x0 + (LEVEL_INDENT * level as i32) as f32,
                                y as f32,
                            ));
                            snapshot.append_layout(&marker, &fg);
                            snapshot.restore();
                        }
                    }
                    _ => self.misses.set(self.misses.get() + 1),
                }
                y += p.h as f64;
                i += 1;
            }
        }
    }

    impl ScrollableImpl for Flow {}

    impl AccessibleTextImpl for Flow {
        fn contents(&self, start: u32, end: u32) -> Option<glib::Bytes> {
            let st = self.st.borrow();
            let s = st.text_range(start as usize, end as usize);
            Some(glib::Bytes::from_owned(s.into_bytes()))
        }

        fn contents_at(
            &self,
            offset: u32,
            granularity: gtk::AccessibleTextGranularity,
        ) -> Option<(u32, u32, glib::Bytes)> {
            let st = self.st.borrow();
            if st.n() == 0 {
                return None;
            }
            let (s, e) = st.boundary(offset as usize, granularity, self.obj().upcast_ref());
            let text = st.text_range(s, e);
            Some((s as u32, e as u32, glib::Bytes::from_owned(text.into_bytes())))
        }

        fn caret_position(&self) -> u32 {
            self.st.borrow().caret_offset() as u32
        }

        fn selection(&self) -> Vec<gtk::AccessibleTextRange> {
            Vec::new()
        }

        fn default_attributes(&self) -> Vec<(glib::GString, glib::GString)> {
            Vec::new()
        }

        fn attributes(
            &self,
            _offset: u32,
        ) -> Vec<(gtk::AccessibleTextRange, glib::GString, glib::GString)> {
            Vec::new()
        }
    }

    impl Flow {
        /// Picks up a new column width (width mode or widget width). No layout here.
        pub fn apply_col(&self, view_w: i32) {
            let mut st = self.st.borrow_mut();
            let col = st.width.column(view_w);
            if col == st.col_w {
                return;
            }
            // W for every change after the first column (width keys and window resizes).
            if st.col_w != 0 {
                emit(format_args!("W {} {}", mono_ns(), col));
            }
            st.set_col(col);
            drop(st);
            self.start_bg();
        }

        /// The frame's layout phase: lay out what is visible, anchored at the top paragraph.
        fn layout_viewport(&self, page: f64) {
            let obj = self.obj();
            let w: &gtk::Widget = obj.upcast_ref();
            let mut st = self.st.borrow_mut();
            if st.n() == 0 {
                return;
            }
            st.flush_dirty(w);
            if st.reveal {
                st.reveal = false;
                let (ci, cb) = st.cursor;
                st.ensure(ci, w);
                let c = st.paras[ci].layout.as_ref().unwrap().cursor_pos(cb as i32).0;
                let top = st.hs.prefix(ci) as f64 + (c.y() / SCALE) as f64;
                let bottom = top + (c.height() / SCALE) as f64;
                let v = st.value();
                if top < v {
                    st.anchor = st.anchor_at(top);
                } else if bottom > v + page {
                    st.anchor = st.anchor_at(bottom - page);
                }
            }
            for _ in 0..8 {
                let maxv = (st.hs.total() as f64 - page).max(0.0);
                let mut value = st.value();
                if value > maxv {
                    st.anchor = st.anchor_at(maxv);
                    value = maxv;
                }
                let mut changed = false;
                let mut i = st.anchor.0;
                while i < st.n() && (st.hs.prefix(i) as f64) < value + page {
                    changed |= st.ensure(i, w);
                    i += 1;
                }
                let ah = st.paras[st.anchor.0].h as f64;
                if st.anchor.1 >= ah {
                    st.anchor.1 = (ah - 1.0).max(0.0);
                }
                if !changed {
                    break;
                }
            }
            // IM cursor location in widget coordinates.
            let (ci, cb) = st.cursor;
            let caret = match &st.paras[ci].layout {
                Some(l) if st.paras[ci].laid_w == st.col_w => {
                    let c = l.cursor_pos(cb as i32).0;
                    let x = (obj.width() - st.col_w) / 2 + indent(st.paras[ci].p.kind);
                    let y = st.hs.prefix(ci) as f64 - st.value();
                    Some(gdk::Rectangle::new(
                        x + c.x() / SCALE,
                        y as i32 + c.y() / SCALE,
                        1,
                        c.height() / SCALE,
                    ))
                }
                _ => None,
            };
            drop(st);
            self.sync_adj();
            if let (Some(r), Some(im)) = (caret, self.im.get()) {
                im.set_cursor_location(&r);
            }
            self.check_l();
        }

        /// Pushes the index into the adjustment, keeping the anchor; true if it had to clamp.
        fn sync_adj(&self) -> bool {
            let obj = self.obj();
            let page = obj.height() as f64;
            let (value, upper) = {
                let st = self.st.borrow();
                (st.value(), st.hs.total() as f64)
            };
            let v = value.min(upper - page).max(0.0);
            if let Some(adj) = self.vadj.borrow().as_ref() {
                self.guard.set(true);
                adj.configure(v, 0.0, upper.max(page), 40.0, page * 0.9, page);
                self.guard.set(false);
            }
            if v != value {
                let mut st = self.st.borrow_mut();
                st.anchor = st.anchor_at(v);
                return true;
            }
            false
        }

        fn check_l(&self) {
            let mut st = self.st.borrow_mut();
            if st.l_pending && st.exact == st.n() && st.dirty.is_empty() {
                st.l_pending = false;
                eprintln!(
                    "L at col {}: content_h {}, anchor paragraph {} (+{:.0} px); background pass {:.1} ms CPU in {} chunks for {} paragraphs",
                    st.col_w,
                    st.hs.total(),
                    st.anchor.0,
                    st.anchor.1,
                    st.bg_stats.0 as f64 / 1e6,
                    st.bg_stats.1,
                    st.bg_stats.2
                );
                emit(format_args!(
                    "L {} {} {}",
                    mono_ns(),
                    st.col_w,
                    st.hs.total()
                ));
            }
        }

        pub fn start_bg(&self) {
            if self.idle.borrow().is_some() {
                return;
            }
            let obj = self.obj().downgrade();
            let id = glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, move || {
                let Some(obj) = obj.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let flow = obj.imp().bg_chunk();
                if flow == glib::ControlFlow::Break {
                    obj.imp().idle.borrow_mut().take();
                }
                flow
            });
            self.idle.replace(Some(id));
        }

        /// One <= 4 ms slice of the background layout pass.
        fn bg_chunk(&self) -> glib::ControlFlow {
            if !self.allocated.get() {
                return glib::ControlFlow::Continue;
            }
            let t0 = mono_ns();
            let obj = self.obj();
            let w: &gtk::Widget = obj.upcast_ref();
            let mut st = self.st.borrow_mut();
            let n = st.n();
            while st.exact < n && mono_ns() - t0 < CHUNK_NS {
                if st.bg_next >= n {
                    st.bg_next = 0;
                }
                let i = st.bg_next;
                st.bg_next += 1;
                if st.ensure(i, w) {
                    st.bg_stats.2 += 1;
                }
            }
            st.bg_stats.0 += mono_ns() - t0;
            st.bg_stats.1 += 1;
            let done = st.exact == n;
            drop(st);
            if self.sync_adj() {
                obj.queue_allocate();
            }
            self.check_l();
            if done {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }

        pub fn set_width(&self, width: TextWidth) {
            let obj = self.obj();
            self.st.borrow_mut().width = width;
            self.apply_col(obj.width());
            obj.queue_allocate();
        }

        fn edited(&self, change: Option<(gtk::AccessibleTextContentChange, usize, usize)>) {
            let obj = self.obj();
            let rev = {
                let mut st = self.st.borrow_mut();
                st.revision += 1;
                st.want_x = None;
                st.reveal = true;
                st.revision
            };
            emit(format_args!("E {} {}", mono_ns(), rev));
            if let Some((c, s, e)) = change {
                obj.update_contents(c, s as u32, e as u32);
            }
            obj.update_caret_position();
            obj.queue_allocate();
            obj.queue_draw();
        }

        pub fn insert(&self, text: &str) {
            if text.is_empty() {
                return;
            }
            let (s, e) = {
                let mut st = self.st.borrow_mut();
                let (pi, bi) = st.cursor;
                let p = &mut st.paras[pi].p;
                p.text.insert_str(bi, text);
                let len = text.len();
                for sp in &mut p.spans {
                    if sp.0 >= bi {
                        sp.0 += len;
                        sp.1 += len;
                    } else if sp.1 >= bi {
                        sp.1 += len;
                    }
                }
                st.touch(pi);
                let added = text.chars().count();
                st.cs.add(pi, added as i64);
                st.cursor.1 += len;
                let s = st.char_of(pi, bi);
                (s, s + added)
            };
            self.edited(Some((gtk::AccessibleTextContentChange::Insert, s, e)));
        }

        fn backspace(&self) {
            let obj = self.obj();
            let w: &gtk::Widget = obj.upcast_ref();
            let (pi, bi) = self.st.borrow().cursor;
            if bi > 0 {
                let (prev, s) = {
                    let st = self.st.borrow();
                    let text = &st.paras[pi].p.text;
                    let prev = text[..bi].char_indices().last().map_or(0, |(b, _)| b);
                    (prev, st.char_of(pi, prev))
                };
                // Before removing: AT-SPI reads the removed text.
                obj.update_contents(gtk::AccessibleTextContentChange::Remove, s as u32, s as u32 + 1);
                let mut st = self.st.borrow_mut();
                let p = &mut st.paras[pi].p;
                p.text.replace_range(prev..bi, "");
                let len = bi - prev;
                let adj = |x: usize| {
                    if x <= prev {
                        x
                    } else if x >= bi {
                        x - len
                    } else {
                        prev
                    }
                };
                for sp in &mut p.spans {
                    *sp = (adj(sp.0), adj(sp.1), sp.2);
                }
                p.spans.retain(|sp| sp.1 > sp.0);
                st.touch(pi);
                st.cs.add(pi, -1);
                st.cursor.1 = prev;
                drop(st);
                self.edited(None);
            } else if pi > 0 {
                // Join with the previous paragraph: a structural edit, rebuild the indexes.
                let s = self.st.borrow().char_of(pi, 0) - 1;
                obj.update_contents(gtk::AccessibleTextContentChange::Remove, s as u32, s as u32 + 1);
                let mut st = self.st.borrow_mut();
                st.flush_dirty(w);
                let gone = st.paras.remove(pi);
                let prev = &mut st.paras[pi - 1];
                let base = prev.p.text.len();
                prev.p.text.push_str(&gone.p.text);
                prev.p
                    .spans
                    .extend(gone.p.spans.iter().map(|&(a, b, l)| (a + base, b + base, l)));
                if gone.laid_w == st.col_w {
                    st.exact -= 1;
                }
                st.touch(pi - 1);
                st.rebuild_heights();
                st.rebuild_chars();
                st.cursor = (pi - 1, base);
                if st.anchor.0 >= pi {
                    st.anchor = if st.anchor.0 == pi { (pi - 1, 0.0) } else { (st.anchor.0 - 1, st.anchor.1) };
                }
                drop(st);
                self.edited(None);
            }
        }

        fn split(&self) {
            let obj = self.obj();
            let w: &gtk::Widget = obj.upcast_ref();
            let s = {
                let mut st = self.st.borrow_mut();
                st.flush_dirty(w);
                let (pi, bi) = st.cursor;
                let p = &mut st.paras[pi].p;
                let tail = p.text.split_off(bi);
                let mut spans = Vec::new();
                for sp in &mut p.spans {
                    if sp.1 > bi {
                        spans.push((sp.0.max(bi) - bi, sp.1 - bi, sp.2));
                        sp.1 = bi;
                    }
                }
                p.spans.retain(|sp| sp.1 > sp.0);
                let kind = match p.kind {
                    Kind::Heading(_) => Kind::Body,
                    k => k,
                };
                let np = Para {
                    chars: tail.chars().count(),
                    p: Paragraph {
                        kind,
                        text: tail,
                        spans,
                    },
                    layout: None,
                    laid_w: -1,
                    laid_h: 0,
                    h: 0,
                    dirty: false,
                };
                st.paras.insert(pi + 1, np);
                st.paras[pi + 1].h = st.estimate(pi + 1);
                st.touch(pi);
                st.touch(pi + 1);
                st.rebuild_heights();
                st.rebuild_chars();
                if st.anchor.0 > pi {
                    st.anchor.0 += 1;
                }
                st.cursor = (pi + 1, 0);
                st.char_of(pi + 1, 0) - 1
            };
            self.edited(Some((gtk::AccessibleTextContentChange::Insert, s, s + 1)));
        }

        fn moved(&self) {
            let obj = self.obj();
            self.st.borrow_mut().reveal = true;
            obj.update_caret_position();
            obj.queue_allocate();
            obj.queue_draw();
        }

        fn move_h(&self, dir: i32) {
            let obj = self.obj();
            let mut st = self.st.borrow_mut();
            let (pi, bi) = st.cursor;
            st.ensure(pi, obj.upcast_ref());
            let l = st.paras[pi].layout.clone().unwrap();
            let (ni, nt) = l.move_cursor_visually(true, bi as i32, 0, dir);
            if ni < 0 {
                if pi > 0 {
                    st.cursor = (pi - 1, st.paras[pi - 1].p.text.len());
                }
            } else if ni == i32::MAX {
                if pi + 1 < st.n() {
                    st.cursor = (pi + 1, 0);
                }
            } else {
                let text = &st.paras[pi].p.text;
                let mut idx = ni as usize;
                for _ in 0..nt {
                    idx += text[idx..].chars().next().map_or(0, char::len_utf8);
                }
                st.cursor.1 = idx;
            }
            st.want_x = None;
            drop(st);
            self.moved();
        }

        fn move_v(&self, dir: i32) {
            let obj = self.obj();
            let w: &gtk::Widget = obj.upcast_ref();
            let mut st = self.st.borrow_mut();
            let (pi, bi) = st.cursor;
            st.ensure(pi, w);
            let l = st.paras[pi].layout.clone().unwrap();
            let pos = l.index_to_pos(bi as i32);
            let x = st
                .want_x
                .unwrap_or(pos.x() + indent(st.paras[pi].p.kind) * SCALE);
            st.want_x = Some(x);
            let ny = if dir > 0 {
                pos.y() + pos.height() + SCALE / 2
            } else {
                pos.y() - SCALE / 2
            };
            let (ti, ty) = if ny < 0 {
                if pi == 0 {
                    return;
                }
                st.ensure(pi - 1, w);
                (pi - 1, st.paras[pi - 1].layout.as_ref().unwrap().size().1 - SCALE / 2)
            } else if ny >= l.size().1 {
                if pi + 1 >= st.n() {
                    return;
                }
                st.ensure(pi + 1, w);
                (pi + 1, SCALE / 2)
            } else {
                (pi, ny)
            };
            let tl = st.paras[ti].layout.clone().unwrap();
            let (_, idx, trailing) = tl.xy_to_index(x - indent(st.paras[ti].p.kind) * SCALE, ty);
            let text = &st.paras[ti].p.text;
            let mut idx = idx as usize;
            for _ in 0..trailing {
                idx += text[idx..].chars().next().map_or(0, char::len_utf8);
            }
            st.cursor = (ti, idx);
            drop(st);
            self.moved();
        }

        pub fn click(&self, x: f64, y: f64) {
            let obj = self.obj();
            let w: &gtk::Widget = obj.upcast_ref();
            let mut st = self.st.borrow_mut();
            let doc_y = st.value() + y;
            let i = st.hs.find(doc_y.floor() as i64);
            st.ensure(i, w);
            let l = st.paras[i].layout.clone().unwrap();
            let lx = x - ((obj.width() - st.col_w) / 2 + indent(st.paras[i].p.kind)) as f64;
            let ly = (doc_y - st.hs.prefix(i) as f64).min((l.size().1 / SCALE) as f64 - 1.0);
            let (_, idx, trailing) = l.xy_to_index((lx * SCALE as f64) as i32, (ly * SCALE as f64) as i32);
            let text = &st.paras[i].p.text;
            let mut idx = idx as usize;
            for _ in 0..trailing {
                idx += text[idx..].chars().next().map_or(0, char::len_utf8);
            }
            st.cursor = (i, idx);
            st.want_x = None;
            drop(st);
            self.moved();
        }

        pub fn key(&self, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
            if mods.contains(gdk::ModifierType::CONTROL_MASK) {
                if let Some(w) = key
                    .to_unicode()
                    .and_then(|c| c.to_digit(10))
                    .and_then(TextWidth::from_key)
                {
                    self.set_width(w);
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::BackSpace => self.backspace(),
                gdk::Key::Return | gdk::Key::KP_Enter => self.split(),
                gdk::Key::Left => self.move_h(-1),
                gdk::Key::Right => self.move_h(1),
                gdk::Key::Up => self.move_v(-1),
                gdk::Key::Down => self.move_v(1),
                gdk::Key::Page_Down | gdk::Key::Page_Up => {
                    if let Some(adj) = self.vadj.borrow().clone() {
                        let d = adj.page_size() * 0.9;
                        let d = if key == gdk::Key::Page_Down { d } else { -d };
                        adj.set_value(adj.value() + d);
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }
    }
}

glib::wrapper! {
    pub struct Flow(ObjectSubclass<imp::Flow>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl Flow {
    fn new(args: Args) -> Self {
        let obj: Self = glib::Object::new();
        {
            let mut st = obj.imp().st.borrow_mut();
            st.width = args.width;
            st.paras = document(args.words)
                .into_iter()
                .map(|p| Para {
                    chars: p.text.chars().count(),
                    p,
                    layout: None,
                    laid_w: -1,
                    laid_h: 0,
                    h: 1,
                    dirty: false,
                })
                .collect();
            st.rebuild_heights();
            st.rebuild_chars();
            let n = st.n();
            let at = ((args.at * (n.max(1) - 1) as f64).round() as usize).min(n - 1);
            st.anchor = (at, 0.0);
            st.cursor = (at, 0);
        }
        obj
    }
}

fn build(app: &gtk::Application, args: Args) {
    let t_start = mono_ns();
    let flow = Flow::new(args);
    flow.set_hexpand(true);
    flow.set_vexpand(true);

    let sw = gtk::ScrolledWindow::new();
    sw.set_hscrollbar_policy(gtk::PolicyType::Never);
    sw.set_child(Some(&flow));

    // measure.py sets SLATE5_WINDOW=WxH so the first frame is already at the measured size.
    let (win_w, win_h) = std::env::var("SLATE5_WINDOW")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1600, 1000));
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("pango-flow")
        .default_width(win_w)
        .default_height(win_h)
        .child(&sw)
        .build();

    // K: every key the window receives, before anything handles it.
    let cap = gtk::EventControllerKey::new();
    cap.set_propagation_phase(gtk::PropagationPhase::Capture);
    cap.connect_key_pressed(|_, key, _, _| {
        emit(format_args!(
            "K {} {}",
            mono_ns(),
            key.name().unwrap_or_default()
        ));
        glib::Propagation::Proceed
    });
    window.add_controller(cap);

    let im = flow.imp().im.get().unwrap().clone();
    let keys = gtk::EventControllerKey::new();
    keys.set_im_context(Some(&im));
    keys.connect_key_pressed({
        let flow = flow.downgrade();
        move |_, key, _, mods| match flow.upgrade() {
            Some(f) => f.imp().key(key, mods),
            None => glib::Propagation::Proceed,
        }
    });
    flow.add_controller(keys);

    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter({
        let im = im.clone();
        move |_| im.focus_in()
    });
    focus.connect_leave(move |_| im.focus_out());
    flow.add_controller(focus);

    let click = gtk::GestureClick::new();
    click.connect_pressed({
        let flow = flow.downgrade();
        move |_, _, x, y| {
            if let Some(f) = flow.upgrade() {
                f.grab_focus();
                f.imp().click(x, y);
            }
        }
    });
    flow.add_controller(click);

    window.present();
    flow.grab_focus();

    let fc = window
        .frame_clock()
        .expect("realized window has a frame clock");
    let start = Rc::new(Cell::new(0u64));
    let painted = Rc::new(Cell::new(0u32));
    fc.connect_before_paint({
        let start = start.clone();
        move |_| start.set(mono_ns())
    });
    fc.connect_after_paint({
        let flow = flow.clone();
        let window = window.clone();
        move |_| {
            let now = mono_ns();
            let n = painted.get() + 1;
            painted.set(n);
            let imp = flow.imp();
            let st = imp.st.borrow();
            if n == 1 {
                let chars: usize = st.paras.iter().map(|p| p.chars).sum();
                emit(format_args!(
                    "READY {} {} {} {} {}",
                    window.width(),
                    window.height(),
                    st.col_w,
                    st.n(),
                    chars
                ));
                eprintln!("startup {} ms", (now - t_start) / 1_000_000);
                // Self-test hook (no pointer injection on this box): PF_TEST_CLICK=x,y
                // clicks there 1 s after READY, through the same handler as GestureClick.
                if let Ok(xy) = std::env::var("PF_TEST_CLICK") {
                    let (x, y) = xy.split_once(',').expect("x,y");
                    let (x, y): (f64, f64) = (x.parse().unwrap(), y.parse().unwrap());
                    let flow = flow.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
                        flow.imp().click(x, y);
                        let st = flow.imp().st.borrow();
                        let (pi, b) = st.cursor;
                        eprintln!(
                            "click {x},{y} -> paragraph {pi} byte {b} ({:?}), caret offset {}",
                            st.paras[pi].p.text[b..].chars().take(12).collect::<String>(),
                            st.caret_offset()
                        );
                    });
                }
            }
            // Revision and column as drawn: a frame may finish without redrawing the widget.
            let (rev, col) = imp.shown.get();
            emit(format_args!(
                "F {} {} {} {} {}",
                now,
                rev,
                col,
                (now - start.get()) / 1000,
                st.value().round() as i64
            ));
        }
    });

    if args.mode != Mode::Interactive {
        let frame = Cell::new(0u32);
        let app = app.clone();
        flow.add_tick_callback(move |flow, _| {
            let n = frame.get() + 1;
            frame.set(n);
            if n > args.frames {
                let imp = flow.imp();
                let st = imp.st.borrow();
                let pct = |v: &RefCell<Vec<u32>>| {
                    let mut v = v.borrow().clone();
                    v.sort_unstable();
                    let q = |f: f64| v.get((f * v.len() as f64) as usize).copied().unwrap_or(0);
                    format!("n {} p50 {} p95 {} max {}", v.len(), q(0.5), q(0.95), v.last().copied().unwrap_or(0))
                };
                eprintln!(
                    "layouts built {}, snapshot misses {}; snapshot us {}; allocate us {}; peak RSS {} MiB",
                    st.layouts_built,
                    imp.misses.get(),
                    pct(&imp.snap_us),
                    pct(&imp.alloc_us),
                    peak_rss_kib() / 1024
                );
                emit(format_args!("DONE"));
                app.quit();
                return glib::ControlFlow::Break;
            }
            match args.mode {
                Mode::Scroll => {
                    if let Some(adj) = flow.imp().vadj.borrow().clone() {
                        let mut v = adj.value() + 37.0;
                        if v > adj.upper() - adj.page_size() {
                            v = 0.0;
                        }
                        adj.set_value(v);
                    }
                }
                Mode::Rewrap => {
                    if n % 90 == 0 {
                        let cur = flow.imp().st.borrow().width;
                        let next = if cur == TextWidth::Narrow {
                            TextWidth::Full
                        } else {
                            TextWidth::Narrow
                        };
                        flow.imp().set_width(next);
                    }
                }
                Mode::Interactive => {}
            }
            glib::ControlFlow::Continue
        });
    }
}

fn main() -> glib::ExitCode {
    let args = Args::parse();
    let app = gtk::Application::builder()
        .application_id("dev.cronkite.Slate5PangoFlow")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| build(app, args));
    app.run_with_args::<&str>(&[])
}
