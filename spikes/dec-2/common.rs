// Shared by the DEC-2 spikes via `#[path]`. Throwaway.
//
// Each option implements `Store`. `run` measures, single-threaded:
//   index_cold_ms   build after evicting the file from the page cache (posix_fadvise DONTNEED)
//   index_warm_ms   rebuild with the file cached
//   index_bytes     heap bytes the index (or parsed data) holds
//   rss             RssAnon / RssFile after the build
//   fetch_*         random single rows and 100-row windows (the grid's visible page), warm,
//                   then cold (page cache evicted and app caches dropped before each window)
// Every fetch copies the row's raw field bytes out, so all options pay the same copy.
// Output: one JSON object on stdout.

use std::fmt::Write as _;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::time::Instant;

/// Raw fields of one row: `bytes[a..b]` for each `(a, b)` in `fields`.
#[derive(Default)]
pub struct Row {
    pub bytes: Vec<u8>,
    pub fields: Vec<(u32, u32)>,
}

impl Row {
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.fields.clear();
    }

    pub fn push(&mut self, field: &[u8]) {
        let a = self.bytes.len() as u32;
        self.bytes.extend_from_slice(field);
        self.fields.push((a, self.bytes.len() as u32));
    }

    pub fn field(&self, i: usize) -> &[u8] {
        let (a, b) = self.fields[i];
        &self.bytes[a as usize..b as usize]
    }
}

pub trait Store: Sized {
    fn build(file: &File) -> Self;
    /// Data rows plus header row.
    fn rows(&self) -> u64;
    fn index_bytes(&self) -> usize;
    fn fetch(&mut self, row: u64, out: &mut Row);
    /// Forget app-level caches (for cold measurements).
    fn drop_caches(&mut self) {}
}

/// Quote-aware row splitter: calls `f(offset_after_newline)` for every row terminator in
/// `buf`. `in_quotes` is the state at `buf[0]`; returns the state at the end.
pub fn for_each_row_end(buf: &[u8], mut in_quotes: bool, mut f: impl FnMut(usize)) -> bool {
    for i in memchr::memchr2_iter(b'"', b'\n', buf) {
        if buf[i] == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            f(i + 1);
        }
    }
    in_quotes
}

/// Offset just past the end of the row starting at `buf[0]` (newline included), or `buf.len()`.
#[allow(dead_code)] // only the mmap option scans row by row at fetch time
pub fn row_end(buf: &[u8]) -> usize {
    let mut in_quotes = false;
    for i in memchr::memchr2_iter(b'"', b'\n', buf) {
        if buf[i] == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            return i + 1;
        }
    }
    buf.len()
}

/// Split one row (newline included or not) into raw fields, quote-aware. Bytes are copied
/// verbatim: quotes stay, which is what "raw values are authoritative" requires.
pub fn split_into(row: &[u8], out: &mut Row) {
    out.clear();
    let mut row = row;
    if let Some(r) = row.strip_suffix(b"\n") {
        row = r.strip_suffix(b"\r").unwrap_or(r);
    }
    let (mut start, mut in_quotes) = (0, false);
    for i in memchr::memchr2_iter(b'"', b',', row) {
        if row[i] == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            out.push(&row[start..i]);
            start = i + 1;
        }
    }
    out.push(&row[start..]);
}

pub fn evict(file: &File) {
    // SAFETY: valid fd; advisory call with no memory effects.
    #[allow(unsafe_code)]
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

fn status_kib(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(key))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn summary(mut xs: Vec<f64>) -> String {
    xs.sort_by(f64::total_cmp);
    let p = |q: f64| xs[((q / 100.0) * (xs.len() - 1) as f64).round() as usize];
    format!(
        r#"{{"n":{},"p50":{:.4},"p99":{:.4},"max":{:.4}}}"#,
        xs.len(),
        p(50.0),
        p(99.0),
        xs[xs.len() - 1]
    )
}

/// Corpus rows look like `<id>,...` where data row r (1-based, after the header) has id r-1.
fn check(row: u64, r: &Row) {
    if row == 0 {
        assert_eq!(r.field(0), b"id");
    } else {
        assert_eq!(r.field(0), (row - 1).to_string().as_bytes(), "row {row}");
    }
    assert_eq!(r.fields.len(), 8, "row {row}");
}

pub fn run<S: Store>(name: &str, params: &str) {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "corpus/rows_1024mb.csv".into());
    let file = File::open(&path).expect("open corpus file");
    let len = file.metadata().unwrap().len();

    evict(&file);
    let t = Instant::now();
    let store = S::build(&file);
    let index_cold_ms = t.elapsed().as_secs_f64() * 1e3;
    let (rows, index_bytes) = (store.rows(), store.index_bytes());
    let (rss_anon, rss_file, hwm) = (
        status_kib("RssAnon:"),
        status_kib("RssFile:"),
        status_kib("VmHWM:"),
    );
    drop(store);

    let t = Instant::now();
    let mut store = S::build(&file);
    let index_warm_ms = t.elapsed().as_secs_f64() * 1e3;

    let mut row = Row::default();
    let (mut single, mut window, mut cold) = (Vec::new(), Vec::new(), Vec::new());
    for k in 0..10_000u64 {
        let r = splitmix(k) % rows;
        let t = Instant::now();
        store.fetch(r, &mut row);
        single.push(t.elapsed().as_secs_f64() * 1e3);
        check(r, &row);
    }
    for k in 0..1_000u64 {
        let start = splitmix(k ^ 0xABCD) % (rows - 100);
        let t = Instant::now();
        for r in start..start + 100 {
            store.fetch(r, &mut row);
        }
        window.push(t.elapsed().as_secs_f64() * 1e3);
        check(start + 99, &row);
    }
    for k in 0..100u64 {
        let start = splitmix(k ^ 0x5EED) % (rows - 100);
        store.drop_caches();
        evict(&file);
        let t = Instant::now();
        for r in start..start + 100 {
            store.fetch(r, &mut row);
        }
        cold.push(t.elapsed().as_secs_f64() * 1e3);
        check(start + 99, &row);
    }

    let mut out = String::new();
    let _ = write!(
        out,
        r#"{{"store":"{name}","params":"{params}","file_bytes":{len},"rows":{rows},"index_cold_ms":{index_cold_ms:.0},"index_warm_ms":{index_warm_ms:.0},"index_bytes":{index_bytes},"rss_anon_mib":{:.1},"rss_file_mib":{:.1},"vm_hwm_mib":{:.1},"fetch_row_ms":{},"fetch_100_rows_ms":{},"fetch_100_rows_cold_ms":{}}}"#,
        rss_anon as f64 / 1024.0,
        rss_file as f64 / 1024.0,
        hwm as f64 / 1024.0,
        summary(single),
        summary(window),
        summary(cold),
    );
    println!("{out}");
}
