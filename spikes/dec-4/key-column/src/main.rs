// DEC-4 option B: one parallel pass extracts a typed key column (u64, order-preserving)
// with the overlay applied; sorting produces a flat Vec<u32> permutation of RowIds.
// Filters: one parallel pass evaluates the predicate into a bitmap (plus rank directory)
// or a visible Vec<u32>; both are measured.
// Usage: key-column <file> <rows; 0 = all>
#[path = "../../common.rs"]
mod common;

use common::*;
use rayon::prelude::*;
use std::hint::black_box;
use std::time::Instant;

const BLOCK: u64 = 1 << 16;

/// Bitmap of visible rows (bit i = row i+1) with a rank directory every 8 words.
struct Bitmap {
    words: Vec<u64>,
    ranks: Vec<u32>,
}

impl Bitmap {
    fn build(words: Vec<u64>) -> Self {
        let mut ranks = Vec::with_capacity(words.len() / 8 + 1);
        let mut acc = 0u32;
        for chunk in words.chunks(8) {
            ranks.push(acc);
            acc += chunk.iter().map(|w| w.count_ones()).sum::<u32>();
        }
        Self { words, ranks }
    }

    fn count(&self) -> u64 {
        self.words.iter().map(|w| w.count_ones() as u64).sum()
    }

    /// RowId of the k-th visible row (0-based).
    fn select(&self, k: u32) -> u32 {
        let sb = self.ranks.partition_point(|&r| r <= k) - 1;
        let mut left = k - self.ranks[sb];
        let mut w = sb * 8;
        loop {
            let c = self.words[w].count_ones();
            if left < c {
                break;
            }
            left -= c;
            w += 1;
        }
        let mut word = self.words[w];
        for _ in 0..left {
            word &= word - 1;
        }
        (w as u32) * 64 + word.trailing_zeros() + 1
    }

    fn contains(&self, id: u32) -> bool {
        let i = id - 1;
        self.words[(i / 64) as usize] >> (i % 64) & 1 == 1
    }

    fn bytes(&self) -> usize {
        self.words.capacity() * 8 + self.ranks.capacity() * 4
    }
}

fn read_window(src: &Source, ids: impl Iterator<Item = u32>) {
    for id in ids {
        let row = src.row_bytes(id as u64);
        for c in 0..8 {
            black_box(nth_field(row, c));
        }
    }
}

fn main() {
    let threads = init_pool();
    let (file, req) = args();
    let t = Instant::now();
    let src = Source::open(&file);
    let index_ms = ms(t);
    let n = if req == 0 {
        src.rows() - 1
    } else {
        req.min(src.rows() - 1)
    };
    let overlay = make_overlay(n);

    // ---- sort ----
    let t = Instant::now();
    let mut keys = vec![0u64; n as usize];
    keys.par_chunks_mut(BLOCK as usize)
        .enumerate()
        .for_each(|(b, out)| {
            let a = 1 + b as u64 * BLOCK;
            src.scan(a, a + out.len() as u64, |r, bytes| {
                out[(r - a) as usize] = num_key(cell(&overlay, r, bytes, SCORE))
            });
        });
    let extract_ms = ms(t);
    let key = |id: u32| keys[id as usize - 1];

    let t = Instant::now();
    let mut pairs: Vec<(u64, u32)> = keys
        .par_iter()
        .enumerate()
        .map(|(i, &k)| (k, i as u32 + 1))
        .collect();
    pairs.par_sort_by_key(|p| p.0); // stable merge sort
    let perm: Vec<u32> = pairs.par_iter().map(|p| p.1).collect();
    let sort_stable_pairs_ms = ms(t);

    let t = Instant::now();
    let mut pairs: Vec<(u64, u32)> = keys
        .par_iter()
        .enumerate()
        .map(|(i, &k)| (k, i as u32 + 1))
        .collect();
    pairs.par_sort_unstable(); // (key, RowId) is a total order, so the result is stable anyway
    let perm_u: Vec<u32> = pairs.par_iter().map(|p| p.1).collect();
    let sort_unstable_pairs_ms = ms(t);
    drop(pairs);
    assert_eq!(perm_u, perm);

    let t = Instant::now();
    let mut idx: Vec<u32> = (1..=n as u32).collect();
    idx.par_sort_by_key(|&i| key(i)); // indirect: compares through the key column
    let sort_argsort_ms = ms(t);
    assert_eq!(idx, perm);
    drop((perm_u, idx));
    check_sorted(&perm, key, n);

    let mut windows = Vec::new();
    for k in 0..1_000u64 {
        let s = (splitmix(k) % (n - 100)) as usize;
        let t = Instant::now();
        read_window(&src, perm[s..s + 100].iter().copied());
        windows.push(ms(t));
    }

    // ---- two-column filter: city == "Portland" AND revenue > 50000 ----
    let limit = num_key(b"50000");
    let pred = |r: u64, bytes: &[u8]| {
        cell(&overlay, r, bytes, CITY) == b"Portland"
            && num_key(cell(&overlay, r, bytes, REVENUE)) > limit
    };

    let t = Instant::now();
    let mut words = vec![0u64; n.div_ceil(64) as usize];
    words
        .par_chunks_mut((BLOCK / 64) as usize)
        .enumerate()
        .for_each(|(b, out)| {
            let a = 1 + b as u64 * BLOCK;
            let end = (a + BLOCK).min(n + 1);
            src.scan(a, end, |r, bytes| {
                if pred(r, bytes) {
                    let i = r - a;
                    out[(i / 64) as usize] |= 1 << (i % 64);
                }
            });
        });
    let bitmap = Bitmap::build(words);
    let filter_bitmap_ms = ms(t);

    let t = Instant::now();
    let parts: Vec<Vec<u32>> = (0..n.div_ceil(BLOCK))
        .into_par_iter()
        .map(|b| {
            let a = 1 + b * BLOCK;
            let mut v = Vec::new();
            src.scan(a, (a + BLOCK).min(n + 1), |r, bytes| {
                if pred(r, bytes) {
                    v.push(r as u32);
                }
            });
            v
        })
        .collect();
    let list: Vec<u32> = parts.concat();
    let filter_list_ms = ms(t);
    let visible = bitmap.count();
    assert_eq!(visible, list.len() as u64);

    let (mut win_list, mut win_bitmap) = (Vec::new(), Vec::new());
    for k in 0..1_000u64 {
        let s = (splitmix(k ^ 0xF1) % (visible - 100)) as usize;
        let t = Instant::now();
        read_window(&src, list[s..s + 100].iter().copied());
        win_list.push(ms(t));
        let t = Instant::now();
        read_window(&src, (s as u32..s as u32 + 100).map(|v| bitmap.select(v)));
        win_bitmap.push(ms(t));
        assert_eq!(bitmap.select(s as u32), list[s]);
    }

    // Filter applied to the sorted view: keep sort order, drop hidden rows.
    let t = Instant::now();
    let sorted_visible: Vec<u32> = perm
        .par_iter()
        .copied()
        .filter(|&id| bitmap.contains(id))
        .collect();
    let filter_over_sort_ms = ms(t);
    assert_eq!(sorted_visible.len() as u64, visible);

    println!(
        r#"{{"option":"key-column","threads":{threads},"rows":{n},"index_ms":{index_ms:.0},"extract_ms":{extract_ms:.1},"sort_stable_pairs_ms":{sort_stable_pairs_ms:.1},"sort_unstable_pairs_ms":{sort_unstable_pairs_ms:.1},"sort_argsort_ms":{sort_argsort_ms:.1},"perm_digest":"{:016x}","sorted_window_ms":{},"key_column_bytes":{},"perm_bytes":{},"filter_bitmap_ms":{filter_bitmap_ms:.1},"filter_list_ms":{filter_list_ms:.1},"visible":{visible},"bitmap_bytes":{},"list_bytes":{},"filtered_window_list_ms":{},"filtered_window_bitmap_ms":{},"filter_over_sort_ms":{filter_over_sort_ms:.1}}}"#,
        digest(&perm),
        summary(windows),
        keys.capacity() * 8,
        perm.capacity() * 4,
        bitmap.bytes(),
        list.capacity() * 4,
        summary(win_list),
        summary(win_bitmap),
    );
}
