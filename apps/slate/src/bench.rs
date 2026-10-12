//! The benchmarks, each in a [`WINDOW`]-sized window at Full; each prints one JSON line
//! and quits. A frame's CPU time runs from the frame clock's before-paint to its
//! after-paint: layout, snapshot, and rendering.
//!
//! - `--bench-flow` (SLATE-11), as ADR 0007's spike measured: once the document is laid
//!   out, the text width switches Narrow ↔ Full ten times. Each switch records how many
//!   frames pass until one shows the new column (with that frame's CPU time) and how long
//!   until every block is measured at the new width. Then the page scrolls 37 px a frame
//!   for 900 frames.
//! - `--bench-typing` (SLATE-6): the caret goes to the middle of the document and 100
//!   characters are typed 80 to 150 ms apart through the input method's path
//!   ([`Page::type_text`]). Each one's latency runs from the call to the after-paint of the
//!   frame that shows it; input delivery from the compositor isn't included.

use crate::settings::TextWidth;
use crate::view::{self, Page};
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The window's size.
pub const WINDOW: (i32, i32) = (1600, 1000);
const SWITCHES: usize = 10;
/// Frames between one switch's complete layout and the next switch.
const SETTLE: u32 = 30;
const SCROLL_FRAMES: u32 = 900;
const SCROLL_STEP: f64 = 37.0;
/// The longest a layout may take before the benchmark gives up on it.
const GIVE_UP: Duration = Duration::from_secs(20);
const KEYSTROKES: usize = 100;
/// What is typed, a character at a time, round and round.
const TYPED: &str = "the quick brown fox jumps over the lazy dog ";

/// Which benchmark to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Flow,
    Typing,
}

#[derive(Clone, Copy)]
enum Phase {
    /// Waiting for the opened document's layout.
    Open,
    /// `left` frames until switch `next` (or scrolling, after the last).
    Settle {
        left: u32,
        next: usize,
    },
    /// Switch `n` to a column `target` wide, made at `at`.
    Switch {
        n: usize,
        at: Instant,
        target: i32,
        frames: u32,
        shown: bool,
    },
    Scroll {
        left: u32,
    },
}

struct Rec {
    phase: Phase,
    started: Instant,
    frame_start: Option<Instant>,
    open_ms: f64,
    /// Per switch: frames until the new column showed, and that frame's CPU time.
    first: Vec<(u32, f64)>,
    /// Per switch: until every block was measured at the new width.
    whole_ms: Vec<f64>,
    unfinished: usize,
    scroll_cpu_ms: Vec<f64>,
    columns: (i32, i32),
}

/// Run `mode` on `page`, the document of `words` words, then quit.
pub fn run(app: &gtk::Application, page: &Page, words: usize, mode: Mode) {
    match mode {
        Mode::Flow => flow(app, page, words),
        Mode::Typing => typing(app, page, words),
    }
}

struct Typing {
    /// When the last keystroke was typed and the revision that shows it, until a frame does.
    pending: Option<(Instant, u64)>,
    frame_start: Option<Instant>,
    latency_ms: Vec<f64>,
    frame_cpu_ms: Vec<f64>,
    typed: usize,
    /// A fixed pseudo-random sequence for the gaps between keystrokes.
    seed: u64,
}

/// Type into the middle of `page` once it is laid out, a keystroke every 80 to 150 ms.
fn typing(app: &gtk::Application, page: &Page, words: usize) {
    let Some(clock) = page.frame_clock() else {
        let app = app.clone();
        page.connect_realize(move |page| typing(&app, page, words));
        return;
    };
    let started = Instant::now();
    let rec = Rc::new(RefCell::new(Typing {
        pending: None,
        frame_start: None,
        latency_ms: Vec::new(),
        frame_cpu_ms: Vec::new(),
        typed: 0,
        seed: 0x5EED,
    }));
    clock.connect_before_paint({
        let rec = rec.clone();
        move |_| rec.borrow_mut().frame_start = Some(Instant::now())
    });
    clock.connect_after_paint({
        let (rec, page) = (rec.clone(), page.downgrade());
        move |_| {
            let Some(page) = page.upgrade() else { return };
            let mut r = rec.borrow_mut();
            let frame = r.frame_start.take();
            if let Some((at, revision)) = r.pending {
                if page.revision() >= revision {
                    r.pending = None;
                    r.latency_ms.push(ms(at.elapsed()));
                    if let Some(f) = frame {
                        r.frame_cpu_ms.push(ms(f.elapsed()));
                    }
                }
            }
        }
    });
    // Wait for the layout, then put the caret mid-document and start typing.
    let app = app.clone();
    page.add_tick_callback(move |page, _| {
        if page.completed_at().is_none() || page.column() == 0 {
            if started.elapsed() > GIVE_UP {
                println!(r#"{{"error":"the document never finished its layout"}}"#);
                std::process::exit(1);
            }
            return glib::ControlFlow::Continue;
        }
        page.put_caret(page.block_count() / 2);
        keystroke(&app, page, &rec, words);
        glib::ControlFlow::Break
    });
}

/// Type the next character after a pause, or report once all are typed and shown.
fn keystroke(app: &gtk::Application, page: &Page, rec: &Rc<RefCell<Typing>>, words: usize) {
    let gap = {
        let mut r = rec.borrow_mut();
        r.seed = r
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        80 + (r.seed >> 33) % 71
    };
    let (app, page, rec) = (app.clone(), page.downgrade(), rec.clone());
    glib::timeout_add_local_once(Duration::from_millis(gap), move || {
        let Some(page) = page.upgrade() else { return };
        let typed = rec.borrow().typed;
        if typed == KEYSTROKES {
            if rec.borrow().pending.is_none() {
                let r = rec.borrow();
                println!(
                    r#"{{"words":{},"keystrokes":{},"latency_ms":{},"frame_cpu_ms":{}}}"#,
                    words,
                    r.latency_ms.len(),
                    summary(&r.latency_ms),
                    summary(&r.frame_cpu_ms),
                );
                app.quit();
            } else {
                keystroke(&app, &page, &rec, words);
            }
            return;
        }
        let c = TYPED
            .chars()
            .nth(typed % TYPED.chars().count())
            .unwrap_or('a');
        let at = Instant::now();
        page.type_text(&c.to_string());
        {
            let mut r = rec.borrow_mut();
            r.pending = Some((at, page.revision()));
            r.typed += 1;
        }
        keystroke(&app, &page, &rec, words);
    });
}

/// Measure re-wrapping and scrolling on `page`, the document of `words` words.
fn flow(app: &gtk::Application, page: &Page, words: usize) {
    let Some(clock) = page.frame_clock() else {
        // Not realized yet: start once it is.
        let app = app.clone();
        page.connect_realize(move |page| flow(&app, page, words));
        return;
    };
    let rec = Rc::new(RefCell::new(Rec {
        phase: Phase::Open,
        started: Instant::now(),
        frame_start: None,
        open_ms: 0.0,
        first: Vec::new(),
        whole_ms: Vec::new(),
        unfinished: 0,
        scroll_cpu_ms: Vec::new(),
        columns: (0, 0),
    }));
    clock.connect_before_paint({
        let rec = rec.clone();
        move |_| rec.borrow_mut().frame_start = Some(Instant::now())
    });
    clock.connect_after_paint({
        let (rec, page) = (rec.clone(), page.downgrade());
        move |_| {
            let Some(page) = page.upgrade() else { return };
            let mut r = rec.borrow_mut();
            let Some(cpu) = r
                .frame_start
                .take()
                .map(|t| t.elapsed().as_secs_f64() * 1e3)
            else {
                return;
            };
            match &mut r.phase {
                Phase::Switch {
                    target,
                    frames,
                    shown,
                    ..
                } => {
                    *frames += 1;
                    if !*shown && page.column() == *target {
                        *shown = true;
                        let frames = *frames;
                        r.first.push((frames, cpu));
                    }
                }
                Phase::Scroll { .. } => r.scroll_cpu_ms.push(cpu),
                _ => {}
            }
        }
    });
    let app = app.clone();
    page.add_tick_callback(move |page, _| {
        let mut r = rec.borrow_mut();
        let phase = r.phase;
        r.phase = match phase {
            Phase::Open => match page.completed_at() {
                Some(done) if page.column() > 0 => {
                    r.open_ms = ms(done - r.started);
                    Phase::Settle {
                        left: SETTLE,
                        next: 0,
                    }
                }
                _ if r.started.elapsed() > GIVE_UP => {
                    println!(r#"{{"error":"the document never finished its layout"}}"#);
                    std::process::exit(1);
                }
                _ => Phase::Open,
            },
            Phase::Settle { left: 0, next } if next == SWITCHES => Phase::Scroll {
                left: SCROLL_FRAMES,
            },
            Phase::Settle { left: 0, next } => {
                let setting = if next % 2 == 0 {
                    TextWidth::Narrow
                } else {
                    TextWidth::Full
                };
                let target = view::column_width(setting, page.width());
                if setting == TextWidth::Full {
                    r.columns.1 = target;
                } else {
                    r.columns.0 = target;
                }
                page.set_text_width(setting);
                Phase::Switch {
                    n: next,
                    at: Instant::now(),
                    target,
                    frames: 0,
                    shown: false,
                }
            }
            Phase::Settle { left, next } => Phase::Settle {
                left: left - 1,
                next,
            },
            Phase::Switch { n, at, shown, .. } => match page.completed_at() {
                Some(done) if shown && done > at => {
                    r.whole_ms.push(ms(done - at));
                    Phase::Settle {
                        left: SETTLE,
                        next: n + 1,
                    }
                }
                _ if at.elapsed() > GIVE_UP => {
                    r.unfinished += 1;
                    Phase::Settle {
                        left: SETTLE,
                        next: n + 1,
                    }
                }
                _ => phase,
            },
            Phase::Scroll { left: 0 } => {
                report(&r, page, words);
                app.quit();
                return glib::ControlFlow::Break;
            }
            Phase::Scroll { left } => {
                if let Some(adj) = page.vadjustment() {
                    let bottom = adj.upper() - adj.page_size();
                    let next = adj.value() + SCROLL_STEP;
                    adj.set_value(if next > bottom { 0.0 } else { next });
                }
                Phase::Scroll { left: left - 1 }
            }
        };
        glib::ControlFlow::Continue
    });
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn report(r: &Rec, page: &Page, words: usize) {
    let window = page.root().map_or((0, 0), |w| (w.width(), w.height()));
    let first_frames = r.first.iter().map(|f| f.0).max().unwrap_or(0);
    let first_cpu: Vec<f64> = r.first.iter().map(|f| f.1).collect();
    println!(
        concat!(
            r#"{{"words":{},"window":[{},{}],"page":[{},{}],"columns":[{},{}],"open_ms":{:.1},"#,
            r#""switches":{},"unfinished":{},"first_frame":{{"frames_max":{},"cpu_ms":{}}},"#,
            r#""whole_ms":{},"scroll":{{"frames":{},"frame_cpu_ms":{}}}}}"#
        ),
        words,
        window.0,
        window.1,
        page.width(),
        page.height(),
        r.columns.0,
        r.columns.1,
        r.open_ms,
        r.first.len(),
        r.unfinished,
        first_frames,
        summary(&first_cpu),
        summary(&r.whole_ms),
        r.scroll_cpu_ms.len(),
        summary(&r.scroll_cpu_ms),
    );
}

/// `{"p50":..,"p99":..,"max":..}` of `xs`.
fn summary(xs: &[f64]) -> String {
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |q: f64| {
        if v.is_empty() {
            0.0
        } else {
            v[((v.len() - 1) as f64 * q).round() as usize]
        }
    };
    format!(
        r#"{{"p50":{:.3},"p99":{:.3},"max":{:.3}}}"#,
        at(0.5),
        at(0.99),
        at(1.0)
    )
}
