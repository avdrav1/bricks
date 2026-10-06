// Shared by the DEC-4 spikes via `#[path]`. Throwaway.
//
// Source: ADR 0002 (mmap + offset of every 64th row). Row identity: ADR 0003 RowIds; the
// spike's rows are source rows 1..=N (row 0 is the header), so RowId == source row.
// An overlay of 10,000 edited "score" cells is applied wherever cells are read.
// Pool: Rayon with available_parallelism() - 1 threads (ADR 0005); DEC4_THREADS overrides.

use memmap2::Mmap;
use std::collections::HashMap;
use std::fs::File;
use std::time::Instant;

pub const SCORE: usize = 4; // decimal, e.g. 523.41
pub const CITY: usize = 2;
pub const REVENUE: usize = 3; // integer 0..99999
const STRIDE: u64 = 64;

pub struct Source {
    map: Mmap,
    checkpoints: Vec<u64>,
    rows: u64,
}

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

impl Source {
    pub fn open(path: &str) -> Self {
        let file = File::open(path).expect("open corpus file");
        // SAFETY: read-only map; nothing truncates the corpus during the spike.
        #[allow(unsafe_code)]
        let map = unsafe { Mmap::map(&file) }.expect("mmap");
        let (mut checkpoints, mut ends, mut in_quotes) = (vec![0u64], 0u64, false);
        for i in memchr::memchr2_iter(b'"', b'\n', &map) {
            if map[i] == b'"' {
                in_quotes = !in_quotes;
            } else if !in_quotes {
                ends += 1;
                if ends % STRIDE == 0 && i + 1 < map.len() {
                    checkpoints.push(i as u64 + 1);
                }
            }
        }
        Self {
            map,
            checkpoints,
            rows: ends,
        }
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn row_bytes(&self, i: u64) -> &[u8] {
        let mut pos = self.checkpoints[(i / STRIDE) as usize] as usize;
        for _ in 0..i % STRIDE {
            pos += row_end(&self.map[pos..]);
        }
        &self.map[pos..pos + row_end(&self.map[pos..])]
    }

    /// Call `f(row, bytes)` for rows `[a, b)` in order, scanning forward from one checkpoint.
    pub fn scan(&self, a: u64, b: u64, mut f: impl FnMut(u64, &[u8])) {
        if a >= b {
            return;
        }
        let first = self.row_bytes(a);
        let mut pos = first.as_ptr() as usize - self.map.as_ptr() as usize;
        for r in a..b {
            let e = pos + row_end(&self.map[pos..]);
            f(r, &self.map[pos..e]);
            pos = e;
        }
    }
}

/// Field `c` of a row, quote-aware, without the line ending. Raw bytes.
pub fn nth_field(row: &[u8], c: usize) -> &[u8] {
    let mut row = row;
    if let Some(r) = row.strip_suffix(b"\n") {
        row = r.strip_suffix(b"\r").unwrap_or(r);
    }
    let (mut start, mut idx, mut in_quotes) = (0, 0, false);
    for i in memchr::memchr2_iter(b'"', b',', row) {
        if row[i] == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if idx == c {
                return &row[start..i];
            }
            idx += 1;
            start = i + 1;
        }
    }
    if idx == c {
        &row[start..]
    } else {
        b""
    }
}

/// Order-preserving u64 for a numeric cell; non-numeric and empty cells sort after all
/// numbers (`u64::MAX`). The raw bytes are never changed, only read.
pub fn num_key(field: &[u8]) -> u64 {
    let f = field
        .strip_prefix(b"\"")
        .and_then(|f| f.strip_suffix(b"\""))
        .unwrap_or(field);
    match std::str::from_utf8(f)
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
    {
        Some(v) if !v.is_nan() => {
            let bits = v.to_bits();
            if bits >> 63 == 1 {
                !bits
            } else {
                bits | (1 << 63)
            }
        }
        _ => u64::MAX,
    }
}

pub type Overlay = HashMap<(u64, usize), Box<[u8]>>;

/// 10,000 deterministic edits to the score column of rows 1..=n.
pub fn make_overlay(n: u64) -> Overlay {
    let mut o = Overlay::new();
    for k in 0..10_000u64 {
        let r = 1 + splitmix(k ^ 0x5C0E) % n;
        o.insert((r, SCORE), format!("{}.5", k % 997).into_bytes().into());
    }
    o
}

pub fn cell<'a>(o: &'a Overlay, row: u64, bytes: &'a [u8], c: usize) -> &'a [u8] {
    match o.get(&(row, c)) {
        Some(v) => v,
        None => nth_field(bytes, c),
    }
}

pub fn init_pool() -> usize {
    let n = std::env::var("DEC4_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1))
        });
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global()
        .unwrap();
    n
}

pub fn args() -> (String, u64) {
    let mut a = std::env::args().skip(1);
    let file = a.next().unwrap_or_else(|| "corpus/rows_1024mb.csv".into());
    let rows = a.next().and_then(|s| s.parse().ok()).unwrap_or(2_000_000);
    (file, rows)
}

pub fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

pub fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// Order-sensitive digest of a permutation, to compare results across binaries.
pub fn digest(ids: &[u32]) -> u64 {
    ids.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &i| {
        (h ^ i as u64).wrapping_mul(0x1000_0000_01B3)
    })
}

/// Panics unless `perm` sorts rows 1..=n by `key` ascending, ties in RowId order (stable).
pub fn check_sorted(perm: &[u32], key: impl Fn(u32) -> u64, n: u64) {
    assert_eq!(perm.len() as u64, n);
    let mut seen = vec![false; n as usize + 1];
    for w in perm.windows(2) {
        let (ka, kb) = (key(w[0]), key(w[1]));
        assert!(
            ka < kb || (ka == kb && w[0] < w[1]),
            "order/stability broken at {:?}",
            w
        );
    }
    for &i in perm {
        assert!(
            !std::mem::replace(&mut seen[i as usize], true),
            "duplicate id {i}"
        );
    }
}

pub fn summary(mut xs: Vec<f64>) -> String {
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
