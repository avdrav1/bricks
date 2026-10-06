// DEC-4 option A: no key cache. Sort RowIds with a comparator that fetches both rows
// through the sparse index and parses the cell on every comparison; filter by fetching
// each row. Same parallel pool and overlay as option B.
// Usage: fetch-compare <file> <rows; 0 = all>
#[path = "../../common.rs"]
mod common;

use common::*;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn main() {
    let threads = init_pool();
    let (file, req) = args();
    let src = Source::open(&file);
    let n = if req == 0 {
        src.rows() - 1
    } else {
        req.min(src.rows() - 1)
    };
    let overlay = make_overlay(n);
    let compares = AtomicU64::new(0);
    let key = |id: u32| num_key(cell(&overlay, id as u64, src.row_bytes(id as u64), SCORE));

    let t = Instant::now();
    let mut ids: Vec<u32> = (1..=n as u32).collect();
    ids.par_sort_by(|&a, &b| {
        compares.fetch_add(1, Ordering::Relaxed);
        key(a).cmp(&key(b))
    });
    let sort_ms = ms(t);
    check_sorted(&ids, key, n);

    let limit = num_key(b"50000");
    let t = Instant::now();
    let visible = (1..=n)
        .into_par_iter()
        .filter(|&r| {
            let bytes = src.row_bytes(r);
            cell(&overlay, r, bytes, CITY) == b"Portland"
                && num_key(cell(&overlay, r, bytes, REVENUE)) > limit
        })
        .count();
    let filter_ms = ms(t);

    println!(
        r#"{{"option":"fetch-compare","threads":{threads},"rows":{n},"sort_ms":{sort_ms:.1},"compares":{},"perm_digest":"{:016x}","filter_ms":{filter_ms:.1},"visible":{visible}}}"#,
        compares.load(Ordering::Relaxed),
        digest(&ids),
    );
}
