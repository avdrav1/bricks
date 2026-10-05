// Shared by both DEC-5 spikes via `#[path]`. Throwaway.
//
// The UI thread is a glib main loop (GTK's loop, per ADR 0001) with a simulated 60 fps
// frame clock: every 16.667 ms it busy-works FRAME_WORK (about a GTK grid frame from the
// DEC-1 spike) and records when the frame started and finished. A background engine runs
// case-insensitive searches over the mmap'd corpus and streams per-chunk progress to the
// UI thread. Phases:
//   baseline  frames only
//   search    full searches back to back (first may be cold)
//   cancel    "typing": each search is cancelled part-way and a new one starts at once
// Output: one JSON object on stdout.

use memmap2::Mmap;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const FRAME: Duration = Duration::from_micros(16_667);
pub const FRAME_WORK: Duration = Duration::from_millis(4);
pub const CHUNK: usize = 4 << 20;

pub enum Msg {
    Progress { hits: u64, sent: Instant },
    Done { cancelled: bool, sent: Instant },
}

pub trait Inbox {
    fn recv(&mut self) -> impl Future<Output = Option<Msg>>;
}

pub trait Job {
    fn cancel(&self);
}

pub trait Engine {
    type J: Job + 'static;
    type R: Inbox + 'static;
    fn start(&self, data: Arc<Mmap>, needle: Arc<[u8]>) -> (Self::J, Self::R);
}

pub struct Args {
    pub file: String,
    pub threads: usize,
    pub runs: usize,
    pub cancel_secs: f64,
}

impl Args {
    pub fn parse() -> Self {
        let mut a = Args {
            file: "corpus/rows_1024mb.csv".into(),
            threads: std::thread::available_parallelism().map_or(4, |n| n.get()),
            runs: 10,
            cancel_secs: 5.0,
        };
        let mut it = std::env::args().skip(1);
        while let Some(k) = it.next() {
            let v = it.next().unwrap_or_else(|| panic!("missing value for {k}"));
            match k.as_str() {
                "--file" => a.file = v,
                "--threads" => a.threads = v.parse().expect("--threads"),
                "--runs" => a.runs = v.parse().expect("--runs"),
                "--cancel-secs" => a.cancel_secs = v.parse().expect("--cancel-secs"),
                _ => panic!("unknown arg {k}"),
            }
        }
        a
    }
}

pub fn chunk_count(len: usize) -> usize {
    len.div_ceil(CHUNK)
}

/// Case-insensitive (ASCII) count of `needle` (lowercase) matches that start in chunk `i`.
pub fn search_chunk(data: &[u8], i: usize, needle: &[u8], buf: &mut Vec<u8>) -> u64 {
    let start = i * CHUNK;
    let end = (start + CHUNK).min(data.len());
    let ext = (end + needle.len().saturating_sub(1)).min(data.len());
    buf.clear();
    buf.extend(data[start..ext].iter().map(u8::to_ascii_lowercase));
    memchr::memmem::find_iter(buf, needle)
        .filter(|&p| p < end - start)
        .count() as u64
}

fn spin(d: Duration) {
    let t = Instant::now();
    while t.elapsed() < d {
        std::hint::spin_loop();
    }
}

struct Frame {
    phase: &'static str,
    deadline: Instant,
    start: Instant,
    end: Instant,
    /// Deadlines skipped since the previous frame (the frame clock missed a vsync).
    skipped: u32,
}

#[derive(Default)]
struct Rec {
    phase: Cell<&'static str>,
    frames: RefCell<Vec<Frame>>,
    lags: RefCell<BTreeMap<&'static str, Vec<f64>>>,
}

impl Rec {
    fn lag(&self, sent: Instant) {
        let ms = sent.elapsed().as_secs_f64() * 1e3;
        self.lags
            .borrow_mut()
            .entry(self.phase.get())
            .or_default()
            .push(ms);
    }
}

fn schedule_frame(rec: Rc<Rec>, deadline: Instant, skipped: u32, stop: Rc<Cell<bool>>) {
    // glib timeouts have ms resolution; round up so we never fire early.
    let wait = deadline.saturating_duration_since(Instant::now());
    let wait = Duration::from_millis(wait.as_micros().div_ceil(1000) as u64);
    glib::timeout_add_local_once(wait, move || {
        if stop.get() {
            return;
        }
        let start = Instant::now();
        spin(FRAME_WORK);
        let end = Instant::now();
        rec.frames.borrow_mut().push(Frame {
            phase: rec.phase.get(),
            deadline,
            start,
            end,
            skipped,
        });
        // Like vsync: the next frame is the first deadline after this one finished.
        let mut next = deadline + FRAME;
        let mut skipped = 0;
        while next < end {
            next += FRAME;
            skipped += 1;
        }
        schedule_frame(rec, next, skipped, stop);
    });
}

fn pct(xs: &mut [f64], p: f64) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    xs.sort_by(f64::total_cmp);
    xs[((p / 100.0) * (xs.len() - 1) as f64).round() as usize]
}

fn summary(mut xs: Vec<f64>) -> String {
    let n = xs.len();
    let max = xs.iter().copied().fold(f64::NAN, f64::max);
    format!(
        r#"{{"n":{n},"p50":{:.2},"p99":{:.2},"max":{:.2}}}"#,
        pct(&mut xs, 50.0),
        pct(&mut xs, 99.0),
        max
    )
}

fn peak_rss_mib() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .map(str::to_owned)
        })
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(f64::NAN, |kb| kb / 1024.0)
}

/// Deterministic jitter in [0, 1).
fn unit(seed: u64) -> f64 {
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x ^ (x >> 31)) >> 11) as f64 / (1u64 << 53) as f64
}

pub fn run<E: Engine + 'static>(name: &'static str, engine: E, args: Args) {
    let file = std::fs::File::open(&args.file).expect("open corpus file");
    // SAFETY: read-only map of a file nobody writes during the spike.
    #[allow(unsafe_code)]
    let data = Arc::new(unsafe { Mmap::map(&file) }.expect("mmap"));
    let main_loop = glib::MainLoop::new(None, false);
    let rec = Rc::new(Rec::default());
    rec.phase.set("baseline");
    let stop = Rc::new(Cell::new(false));
    schedule_frame(rec.clone(), Instant::now() + FRAME, 0, stop.clone());

    let ml = main_loop.clone();
    glib::MainContext::default().spawn_local(async move {
        let sleep = |d: Duration| glib::timeout_future(d);
        sleep(Duration::from_secs(2)).await;

        // Full searches.
        let needle: Arc<[u8]> = Arc::from(&b"portland"[..]);
        let (mut times, mut first_hits, mut total_hits) = (Vec::new(), Vec::new(), 0u64);
        for _ in 0..args.runs {
            rec.phase.set("search");
            let t0 = Instant::now();
            let (_job, mut rx) = engine.start(data.clone(), needle.clone());
            let (mut hits, mut first) = (0u64, None);
            while let Some(m) = rx.recv().await {
                match m {
                    Msg::Progress { hits: h, sent } => {
                        rec.lag(sent);
                        hits += h;
                        if h > 0 && first.is_none() {
                            first = Some(t0.elapsed());
                        }
                    }
                    Msg::Done { .. } => break,
                }
            }
            times.push(t0.elapsed().as_secs_f64() * 1e3);
            first_hits.push(first.map_or(f64::NAN, |d| d.as_secs_f64() * 1e3));
            total_hits = hits;
            rec.phase.set("idle");
            sleep(Duration::from_millis(200)).await;
        }
        let mut warm = times[1..].to_vec();
        let median_search = Duration::from_secs_f64(pct(&mut warm, 50.0) / 1e3);

        // Typing: cancel part-way, restart immediately with the next prefix.
        rec.phase.set("cancel");
        let words: [&[u8]; 8] = [b"p", b"po", b"por", b"port", b"portl", b"portla", b"portlan", b"portland"];
        let end = Instant::now() + Duration::from_secs_f64(args.cancel_secs);
        let (mut ui_cancel, mut worker_stop, mut completed, mut raced, mut k) =
            (Vec::new(), Vec::new(), 0u32, 0u32, 0u64);
        while Instant::now() < end {
            let needle: Arc<[u8]> = Arc::from(words[(k % 8) as usize]);
            let (job, mut rx) = engine.start(data.clone(), needle);
            let job = Rc::new(job);
            let cancelled_at = Rc::new(Cell::new(None::<Instant>));
            let after = median_search.mul_f64(0.1 + 0.7 * unit(k));
            k += 1;
            let src = glib::timeout_add_local_once(after, {
                let (job, cancelled_at) = (job.clone(), cancelled_at.clone());
                move || {
                    cancelled_at.set(Some(Instant::now()));
                    job.cancel();
                }
            });
            while let Some(m) = rx.recv().await {
                match m {
                    Msg::Progress { sent, .. } => rec.lag(sent),
                    Msg::Done { cancelled, sent } => {
                        match cancelled_at.get() {
                            Some(c) => {
                                // Cancel requested but every chunk was already claimed.
                                raced += u32::from(!cancelled);
                                ui_cancel.push(c.elapsed().as_secs_f64() * 1e3);
                                worker_stop.push(sent.saturating_duration_since(c).as_secs_f64() * 1e3);
                            }
                            None => {
                                src.remove();
                                completed += 1;
                            }
                        }
                        break;
                    }
                }
            }
        }
        rec.phase.set("idle");
        sleep(Duration::from_millis(300)).await;
        stop.set(true);

        // Report.
        let mut out = String::new();
        let _ = write!(
            out,
            r#"{{"engine":"{name}","threads":{},"file_mb":{},"hits":{total_hits},"search_ms":{:?},"first_hit_ms":{},"#,
            args.threads,
            data.len() >> 20,
            times.iter().map(|t| (t * 10.0).round() / 10.0).collect::<Vec<_>>(),
            summary(first_hits[1..].to_vec()),
        );
        let _ = write!(
            out,
            r#""cancel":{{"searches":{},"completed_before_cancel":{completed},"finished_after_cancel":{raced},"ui_observed_ms":{},"worker_stop_ms":{}}},"#,
            k,
            summary(ui_cancel),
            summary(worker_stop)
        );
        let frames = rec.frames.borrow();
        let mut phases = String::new();
        for phase in ["baseline", "search", "cancel"] {
            let fs: Vec<&Frame> = frames.iter().filter(|f| f.phase == phase).collect();
            let late = fs.iter().map(|f| (f.start - f.deadline).as_secs_f64() * 1e3).collect();
            let done = fs.iter().map(|f| (f.end - f.deadline).as_secs_f64() * 1e3).collect();
            let over = fs.iter().filter(|f| f.end - f.deadline > FRAME).count();
            let skipped: u32 = fs.iter().map(|f| f.skipped).sum();
            let lag = rec.lags.borrow().get(phase).cloned().unwrap_or_default();
            let _ = write!(
                phases,
                r#"{}"{phase}":{{"frames":{},"start_late_ms":{},"done_after_deadline_ms":{},"over_budget":{over},"skipped_vsyncs":{skipped},"progress_lag_ms":{}}}"#,
                if phases.is_empty() { "" } else { "," },
                fs.len(),
                summary(late),
                summary(done),
                summary(lag)
            );
        }
        let _ = write!(out, r#""phases":{{{phases}}},"peak_rss_mib":{:.1}}}"#, peak_rss_mib());
        println!("{out}");
        ml.quit();
    });
    main_loop.run();
}
