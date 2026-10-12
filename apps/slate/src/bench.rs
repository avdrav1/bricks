//! `slate FILE --bench-flow`: SLATE-11's measurements of the flow, as ADR 0007's spike took
//! them, in a [`WINDOW`]-sized window at Full.
//!
//! Once the document is laid out, the text width switches Narrow ↔ Full ten times. Each
//! switch records how many frames pass until one shows the new column (with that frame's
//! CPU time) and how long until every block is measured at the new width. Then the page
//! scrolls 37 px a frame for 900 frames. A frame's CPU time runs from the frame clock's
//! before-paint to its after-paint: layout, snapshot, and rendering. Prints one JSON line
//! and quits.

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

/// Measure `page`, the document of `words` words, then quit.
pub fn run(app: &gtk::Application, page: &Page, words: usize) {
    let Some(clock) = page.frame_clock() else {
        // Not realized yet: start once it is.
        let app = app.clone();
        page.connect_realize(move |page| run(&app, page, words));
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
