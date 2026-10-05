// Shared by both DEC-1 spikes via `#[path]`. Throwaway code.
//
// Protocol on stdout (read by measure.py):
//   READY <w> <h> <visible_rows> <visible_cols>
//   K <mono_ns> <key>                 key event delivered to the app
//   F <mono_ns> <top_row> <cpu_us> [<snapshot_us>]
//        frame finished; cpu_us = toolkit-reported frame CPU; snapshot_us (GTK only) =
//        time spent in Widget::snapshot building the render tree
//   DONE

use std::fmt::Write as _;
use std::io::Write as _;

pub const ROW_H: f64 = 22.0;
pub const COL_W: f64 = 120.0;
pub const ROW_HEADER_W: f64 = 80.0;
pub const HEADER_H: f64 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Interactive,
    Scroll,
    Jump,
}

#[derive(Clone, Copy, Debug)]
pub struct Args {
    pub rows: u64,
    pub cols: u32,
    pub mode: Mode,
    pub frames: u32,
}

impl Args {
    pub fn parse() -> Self {
        let mut a = Args {
            rows: 1_000_000,
            cols: 30,
            mode: Mode::Interactive,
            frames: 600,
        };
        let mut it = std::env::args().skip(1);
        while let Some(k) = it.next() {
            let v = it.next().unwrap_or_else(|| panic!("missing value for {k}"));
            match k.as_str() {
                "--rows" => a.rows = v.parse().expect("--rows"),
                "--cols" => a.cols = v.parse().expect("--cols"),
                "--frames" => a.frames = v.parse().expect("--frames"),
                "--mode" => {
                    a.mode = match v.as_str() {
                        "interactive" => Mode::Interactive,
                        "scroll" => Mode::Scroll,
                        "jump" => Mode::Jump,
                        _ => panic!("--mode interactive|scroll|jump"),
                    }
                }
                _ => panic!("unknown arg {k}"),
            }
        }
        a
    }

    pub fn content_height(&self) -> f64 {
        self.rows as f64 * ROW_H
    }
}

/// CLOCK_MONOTONIC in ns; same clock as Python's time.monotonic_ns().
pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid pointer to a timespec on the stack.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Viewport over a virtual row space. `scroll_px` is f64 so 10M+ rows keep pixel precision.
pub struct Viewport {
    pub first_row: u64,
    /// y of `first_row`'s top edge relative to the body origin (<= 0).
    pub y_offset: f64,
    pub row_count: u64,
}

pub fn viewport(scroll_px: f64, body_h: f64, rows: u64) -> Viewport {
    let first_row = ((scroll_px / ROW_H).floor() as u64).min(rows.saturating_sub(1));
    let y_offset = first_row as f64 * ROW_H - scroll_px;
    let row_count = (((body_h - y_offset) / ROW_H).ceil() as u64).min(rows - first_row);
    Viewport {
        first_row,
        y_offset,
        row_count,
    }
}

pub fn clamp_scroll(scroll_px: f64, body_h: f64, rows: u64) -> f64 {
    let max = (rows as f64 * ROW_H - body_h).max(0.0);
    scroll_px.clamp(0.0, max)
}

/// Autopilot step for the frame-time runs. Returns the new scroll position.
pub fn autopilot(mode: Mode, frame: u32, scroll_px: f64, body_h: f64, rows: u64) -> f64 {
    match mode {
        Mode::Interactive => scroll_px,
        // ~1.7 rows per frame: every frame shows new rows, like a fast flick.
        Mode::Scroll => clamp_scroll(scroll_px + 37.0, body_h, rows),
        // Scrollbar-drag stress: every frame lands on an unrelated region.
        Mode::Jump => {
            let r = splitmix(frame as u64) % rows;
            clamp_scroll(r as f64 * ROW_H, body_h, rows)
        }
    }
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

const NAMES: [&str; 8] = [
    "Alvarez",
    "Brennan",
    "Chowdhury",
    "Dubois",
    "Eriksson",
    "Fujimoto",
    "Gallo",
    "Haddad",
];
const CITIES: [&str; 6] = [
    "Lisbon",
    "Osaka",
    "Winnipeg",
    "Nairobi",
    "Tallinn",
    "Valparaíso",
];

/// Deterministic fake cell contents with mixed widths and types, written into `out`.
pub fn cell_text(row: u64, col: u32, out: &mut String) {
    out.clear();
    let h = splitmix(row.wrapping_mul(1_000_003) ^ col as u64);
    let _ = match col % 6 {
        0 => write!(out, "{:07}", row + 1),
        1 => write!(out, "{} {}", NAMES[(h % 8) as usize], (h >> 8) % 100),
        2 => write!(out, "{}.{:02}", (h >> 3) % 100_000, h % 100),
        3 => write!(
            out,
            "20{:02}-{:02}-{:02}",
            (h >> 5) % 30,
            1 + (h >> 9) % 12,
            1 + (h >> 13) % 28
        ),
        4 if *UNICODE_SAMPLES => write!(out, "{}", SCRIPTS[(h % 6) as usize]),
        4 => write!(out, "{}", CITIES[(h % 6) as usize]),
        _ => write!(out, "{}", h as i64 % 1_000_000),
    };
}

/// Text-correctness probe (not used in timed runs): DEC1_UNICODE=1 fills column E with
/// scripts that need shaping, bidi, or font fallback.
const SCRIPTS: [&str; 6] = [
    "東京都",
    "القاهرة",
    "תל אביב",
    "नई दिल्ली",
    "Ελλάδα",
    "Zürich 🚆",
];

static UNICODE_SAMPLES: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("DEC1_UNICODE").is_some());

pub fn col_name(mut col: u32, out: &mut String) {
    out.clear();
    let mut buf = [0u8; 8];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'A' + (col % 26) as u8;
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}

pub fn emit(line: std::fmt::Arguments) {
    let mut so = std::io::stdout().lock();
    let _ = so.write_fmt(line);
    let _ = so.write_all(b"\n");
    let _ = so.flush();
}
