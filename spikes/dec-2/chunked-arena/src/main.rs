// DEC-2 option B: index the file as ~4 MiB row-aligned chunks (offset, length, first row);
// load chunks on demand with pread into a heap arena bounded by an LRU byte budget, and build
// a per-chunk u32 row-start table on load. No mmap, so a file changed underneath us cannot
// SIGBUS the process. DEC2_BUDGET_MB sets the arena budget (default 64).
#[path = "../../common.rs"]
mod common;

use common::*;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::os::unix::fs::FileExt;

const CHUNK: usize = 4 << 20;

struct Meta {
    off: u64,
    len: u32,
    first_row: u64,
}

struct Loaded {
    buf: Vec<u8>,
    starts: Vec<u32>,
}

struct Arena {
    file: File,
    metas: Vec<Meta>,
    rows: u64,
    cache: HashMap<usize, Loaded>,
    lru: VecDeque<usize>,
    cached: usize,
    budget: usize,
}

fn budget_mb() -> usize {
    std::env::var("DEC2_BUDGET_MB")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64)
}

fn read_full(file: &File, buf: &mut [u8], off: u64) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match file.read_at(&mut buf[n..], off + n as u64).expect("pread") {
            0 => break,
            k => n += k,
        }
    }
    n
}

impl Arena {
    fn load(&mut self, ci: usize) {
        if self.cache.contains_key(&ci) {
            if let Some(p) = self.lru.iter().position(|&c| c == ci) {
                self.lru.remove(p);
            }
            self.lru.push_back(ci);
            return;
        }
        let m = &self.metas[ci];
        let mut buf = vec![0u8; m.len as usize];
        read_full(&self.file, &mut buf, m.off);
        let mut starts = vec![0u32];
        let len = buf.len();
        for_each_row_end(&buf, false, |e| {
            if e < len {
                starts.push(e as u32);
            }
        });
        self.cached += buf.capacity() + starts.capacity() * 4;
        self.cache.insert(ci, Loaded { buf, starts });
        self.lru.push_back(ci);
        while self.cached > self.budget && self.lru.len() > 1 {
            let old = self.lru.pop_front().unwrap();
            let l = self.cache.remove(&old).unwrap();
            self.cached -= l.buf.capacity() + l.starts.capacity() * 4;
        }
    }
}

impl Store for Arena {
    fn build(file: &File) -> Self {
        let file = file.try_clone().unwrap();
        let mut buf = vec![0u8; CHUNK];
        let (mut metas, mut off, mut rows) = (Vec::new(), 0u64, 0u64);
        loop {
            let n = read_full(&file, &mut buf, off);
            if n == 0 {
                break;
            }
            let (mut last_end, mut count) = (0usize, 0u64);
            for_each_row_end(&buf[..n], false, |e| {
                last_end = e;
                count += 1;
            });
            if n < CHUNK && last_end < n {
                // Final row without a trailing newline.
                last_end = n;
                count += 1;
            }
            assert!(
                last_end > 0,
                "row longer than {CHUNK} bytes at offset {off}"
            );
            metas.push(Meta {
                off,
                len: last_end as u32,
                first_row: rows,
            });
            rows += count;
            off += last_end as u64;
        }
        Self {
            file,
            metas,
            rows,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            cached: 0,
            budget: budget_mb() << 20,
        }
    }

    fn rows(&self) -> u64 {
        self.rows
    }

    fn index_bytes(&self) -> usize {
        self.metas.capacity() * std::mem::size_of::<Meta>()
    }

    fn fetch(&mut self, row: u64, out: &mut Row) {
        let ci = self.metas.partition_point(|m| m.first_row <= row) - 1;
        self.load(ci);
        let l = &self.cache[&ci];
        let i = (row - self.metas[ci].first_row) as usize;
        let a = l.starts[i] as usize;
        let b = l.starts.get(i + 1).map_or(l.buf.len(), |&s| s as usize);
        split_into(&l.buf[a..b], out);
    }

    fn drop_caches(&mut self) {
        self.cache.clear();
        self.lru.clear();
        self.cached = 0;
    }
}

fn main() {
    run::<Arena>(
        "chunked-arena",
        &format!("chunk=4MiB budget={}MiB", budget_mb()),
    );
}
