// Shared by the DEC-3 spikes via `#[path]`. Throwaway.
//
// - `Source`: DEC-2's accepted storage (mmap + byte offset of every 64th row).
// - `Treap<T>`: implicit treap (order-statistic by row count) with split/merge, used by
//   both models so they differ only in what they store.
// - `run`: the measured op sequence, identical for every model (same seed):
//     5,000 random cell edits, an insert at row 500,000, 5,000 more edits,
//     1,000 single-row inserts at random rows, 1,000 random 100-row window fetches,
//     then a full save to /tmp/dec3-<name>.csv. Every edit and insert is read back.
// Output: one JSON object on stdout.

use memmap2::Mmap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

pub const NCOLS: usize = 8;

// ---------- source (ADR 0002) ----------

pub struct Source {
    map: Mmap,
    checkpoints: Vec<u64>,
    rows: u64,
}

const STRIDE: u64 = 64;

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
        assert_eq!(map.last(), Some(&b'\n'), "spike assumes a trailing newline");
        Self {
            map,
            checkpoints,
            rows: ends,
        }
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Byte offset of row `i`'s start; `rows()` maps to the file length.
    pub fn row_start(&self, i: u64) -> usize {
        if i == self.rows {
            return self.map.len();
        }
        let mut pos = self.checkpoints[(i / STRIDE) as usize] as usize;
        for _ in 0..i % STRIDE {
            pos += row_end(&self.map[pos..]);
        }
        pos
    }

    pub fn row_bytes(&self, i: u64) -> &[u8] {
        let s = self.row_start(i);
        &self.map[s..s + row_end(&self.map[s..])]
    }

    /// Raw bytes of rows `[a, b)`.
    pub fn rows_bytes(&self, a: u64, b: u64) -> &[u8] {
        &self.map[self.row_start(a)..self.row_start(b)]
    }

    pub fn bytes(&self, a: usize, b: usize) -> &[u8] {
        &self.map[a..b]
    }
}

// ---------- rows ----------

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
    pub fn push(&mut self, f: &[u8]) {
        let a = self.bytes.len() as u32;
        self.bytes.extend_from_slice(f);
        self.fields.push((a, self.bytes.len() as u32));
    }
    pub fn field(&self, i: usize) -> &[u8] {
        let (a, b) = self.fields[i];
        &self.bytes[a as usize..b as usize]
    }
}

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

/// CSV-encode raw field values into one `\n`-terminated row.
pub fn encode_row<'a>(fields: impl Iterator<Item = &'a [u8]>, out: &mut Vec<u8>) {
    for (i, f) in fields.enumerate() {
        if i > 0 {
            out.push(b',');
        }
        if f.iter().any(|&b| matches!(b, b',' | b'"' | b'\n' | b'\r')) {
            out.push(b'"');
            for &b in f {
                if b == b'"' {
                    out.push(b'"');
                }
                out.push(b);
            }
            out.push(b'"');
        } else {
            out.extend_from_slice(f);
        }
    }
    out.push(b'\n');
}

// ---------- implicit treap ----------

pub trait Item: Clone {
    fn rows(&self) -> u64;
}

pub const NIL: u32 = u32::MAX;

struct Node<T> {
    item: T,
    prio: u64,
    l: u32,
    r: u32,
    sum: u64,
}

pub struct Treap<T> {
    nodes: Vec<Node<T>>,
    pub root: u32,
    seed: u64,
}

impl<T: Item> Treap<T> {
    pub fn new(item: T) -> Self {
        let mut t = Self {
            nodes: Vec::new(),
            root: NIL,
            seed: 1,
        };
        t.root = t.node(item);
        t
    }

    fn node(&mut self, item: T) -> u32 {
        self.seed = splitmix(self.seed);
        let sum = item.rows();
        self.nodes.push(Node {
            item,
            prio: self.seed,
            l: NIL,
            r: NIL,
            sum,
        });
        (self.nodes.len() - 1) as u32
    }

    fn sum(&self, t: u32) -> u64 {
        if t == NIL {
            0
        } else {
            self.nodes[t as usize].sum
        }
    }

    fn update(&mut self, t: u32) {
        let n = &self.nodes[t as usize];
        let s = self.sum(n.l) + n.item.rows() + self.sum(n.r);
        self.nodes[t as usize].sum = s;
    }

    fn merge(&mut self, a: u32, b: u32) -> u32 {
        if a == NIL {
            return b;
        }
        if b == NIL {
            return a;
        }
        if self.nodes[a as usize].prio > self.nodes[b as usize].prio {
            let r = self.merge(self.nodes[a as usize].r, b);
            self.nodes[a as usize].r = r;
            self.update(a);
            a
        } else {
            let l = self.merge(a, self.nodes[b as usize].l);
            self.nodes[b as usize].l = l;
            self.update(b);
            b
        }
    }

    /// Split into (first `k` rows, rest). `cut(item, j)` splits an item after its `j`th row.
    fn split(&mut self, t: u32, k: u64, cut: &mut impl FnMut(&T, u64) -> (T, T)) -> (u32, u32) {
        if t == NIL {
            return (NIL, NIL);
        }
        let (l, r) = (self.nodes[t as usize].l, self.nodes[t as usize].r);
        let (ls, own) = (self.sum(l), self.nodes[t as usize].item.rows());
        if k <= ls {
            let (a, b) = self.split(l, k, cut);
            self.nodes[t as usize].l = b;
            self.update(t);
            (a, t)
        } else if k >= ls + own {
            let (a, b) = self.split(r, k - ls - own, cut);
            self.nodes[t as usize].r = a;
            self.update(t);
            (t, b)
        } else {
            let item = self.nodes[t as usize].item.clone();
            let (x, y) = cut(&item, k - ls);
            self.nodes[t as usize].item = x;
            self.nodes[t as usize].r = NIL;
            self.update(t);
            let ny = self.node(y);
            let rt = self.merge(ny, r);
            (t, rt)
        }
    }

    pub fn total(&self) -> u64 {
        self.sum(self.root)
    }

    /// Item holding logical row `k`, and `k`'s offset inside it.
    pub fn find(&self, mut k: u64) -> (&T, u64) {
        let mut t = self.root;
        loop {
            let n = &self.nodes[t as usize];
            let ls = self.sum(n.l);
            if k < ls {
                t = n.l;
            } else if k < ls + n.item.rows() {
                return (&n.item, k - ls);
            } else {
                k -= ls + n.item.rows();
                t = n.r;
            }
        }
    }

    pub fn insert_at(&mut self, k: u64, item: T, cut: &mut impl FnMut(&T, u64) -> (T, T)) {
        let (a, b) = self.split(self.root, k, cut);
        let n = self.node(item);
        let a = self.merge(a, n);
        self.root = self.merge(a, b);
    }

    /// Replace rows `[k, k + count)` with `item`.
    pub fn replace(
        &mut self,
        k: u64,
        count: u64,
        item: T,
        cut: &mut impl FnMut(&T, u64) -> (T, T),
    ) {
        let (a, b) = self.split(self.root, k, cut);
        let (_old, c) = self.split(b, count, cut);
        let n = self.node(item);
        let a = self.merge(a, n);
        self.root = self.merge(a, c);
    }

    pub fn for_each(&self, mut f: impl FnMut(&T)) {
        let (mut stack, mut t) = (Vec::new(), self.root);
        loop {
            while t != NIL {
                stack.push(t);
                t = self.nodes[t as usize].l;
            }
            let Some(n) = stack.pop() else { break };
            f(&self.nodes[n as usize].item);
            t = self.nodes[n as usize].r;
        }
    }

    /// Items reachable from the root (old nodes stay in the arena; a real impl would free them).
    pub fn live_items(&self) -> usize {
        let mut n = 0;
        self.for_each(|_| n += 1);
        n
    }

    pub fn arena_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node<T>>()
    }
}

// ---------- harness ----------

pub trait Model {
    fn open(src: Source) -> Self;
    fn rows(&self) -> u64;
    fn edit(&mut self, row: u64, col: usize, value: &[u8]);
    fn insert_row(&mut self, row: u64, fields: &[&[u8]]);
    fn fetch(&self, row: u64, out: &mut Row);
    fn save(&self, w: &mut impl Write);
    /// Heap bytes held by the edit model (excluding the source index).
    fn mem_bytes(&self) -> usize;
    fn shape(&self) -> String;
}

pub fn splitmix(mut x: u64) -> u64 {
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

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

pub fn run<M: Model>(name: &str) {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "corpus/rows_1024mb.csv".into());
    let t = Instant::now();
    let src = Source::open(&path);
    let index_ms = ms(t);
    let src_rows = src.rows();
    let t = Instant::now();
    let mut m = M::open(src);
    let open_ms = ms(t);

    let mut row = Row::default();
    let (mut edits, mut inserts, mut seq) = (Vec::new(), Vec::new(), 0u64);
    fn edit_batch<M: Model>(
        m: &mut M,
        row: &mut Row,
        edits: &mut Vec<f64>,
        seq: &mut u64,
        n: usize,
        base: usize,
    ) {
        for k in 0..n {
            *seq += 1;
            let r = 1 + splitmix(*seq) % (m.rows() - 1);
            *seq += 1;
            let c = (splitmix(*seq) % NCOLS as u64) as usize;
            let v = format!("e{}", base + k);
            let t = Instant::now();
            m.edit(r, c, v.as_bytes());
            edits.push(ms(t));
            m.fetch(r, row);
            assert_eq!(
                row.field(c),
                v.as_bytes(),
                "edit read-back at row {r} col {c}"
            );
        }
    }
    edit_batch(&mut m, &mut row, &mut edits, &mut seq, 5_000, 0);

    let fields: Vec<&[u8]> = vec![b"ins500k", b"a", b"b", b"c", b"d", b"e", b"f", b"g"];
    let before = {
        m.fetch(500_000, &mut row);
        row.bytes.clone()
    };
    let t = Instant::now();
    m.insert_row(500_000, &fields);
    let insert_500k_ms = ms(t);
    m.fetch(500_000, &mut row);
    assert_eq!(row.field(0), b"ins500k");
    m.fetch(500_001, &mut row);
    assert_eq!(
        row.bytes, before,
        "row after the insert shifted down intact"
    );

    edit_batch(&mut m, &mut row, &mut edits, &mut seq, 5_000, 5_000);

    let mut rng = 0xDEC3u64;
    for k in 0..1_000u64 {
        rng = splitmix(rng);
        let r = 1 + rng % (m.rows() - 1);
        let id = format!("ins{k}");
        let f: Vec<&[u8]> = vec![id.as_bytes(), b"", b"", b"", b"", b"", b"", b""];
        let t = Instant::now();
        m.insert_row(r, &f);
        inserts.push(ms(t));
        m.fetch(r, &mut row);
        assert_eq!(row.field(0), id.as_bytes());
    }

    let mut windows = Vec::new();
    for k in 0..1_000u64 {
        let start = splitmix(k ^ 0xF00D) % (m.rows() - 100);
        let t = Instant::now();
        for r in start..start + 100 {
            m.fetch(r, &mut row);
        }
        windows.push(ms(t));
    }

    let out = format!("/tmp/dec3-{name}.csv");
    let t = Instant::now();
    {
        let mut w = BufWriter::with_capacity(1 << 20, File::create(&out).unwrap());
        m.save(&mut w);
        w.flush().unwrap();
    }
    let save_ms = ms(t);
    let saved = std::fs::metadata(&out).unwrap().len();

    let mut s = String::new();
    let _ = write!(
        s,
        r#"{{"model":"{name}","source_rows":{src_rows},"rows":{},"index_ms":{index_ms:.0},"open_ms":{open_ms:.1},"edit_ms":{},"insert_500k_ms":{insert_500k_ms:.4},"insert_ms":{},"fetch_100_rows_ms":{},"save_ms":{save_ms:.0},"saved_bytes":{saved},"model_bytes":{},"shape":"{}"}}"#,
        m.rows(),
        summary(edits),
        summary(inserts),
        summary(windows),
        m.mem_bytes(),
        m.shape(),
    );
    println!("{s}");
}
