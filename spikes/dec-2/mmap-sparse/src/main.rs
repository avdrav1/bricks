// DEC-2 option A: mmap the file, keep the byte offset of every Kth row start.
// DEC2_STRIDE=1 gives the dense u64-per-row index from the execution plan's lean.
#[path = "../../common.rs"]
mod common;

use common::*;
use memmap2::{Advice, Mmap};
use std::fs::File;

struct MmapSparse {
    file: File,
    map: Mmap,
    stride: u64,
    checkpoints: Vec<u64>,
    rows: u64,
}

fn stride() -> u64 {
    std::env::var("DEC2_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64)
}

impl Store for MmapSparse {
    fn build(file: &File) -> Self {
        // SAFETY: read-only map; the spike never truncates the file underneath it.
        #[allow(unsafe_code)]
        let map = unsafe { Mmap::map(file) }.expect("mmap");
        let _ = map.advise(Advice::Sequential);
        let (stride, len) = (stride(), map.len());
        let mut checkpoints = vec![0u64];
        let mut ends = 0u64;
        for_each_row_end(&map, false, |end| {
            ends += 1;
            if ends % stride == 0 && end < len {
                checkpoints.push(end as u64);
            }
        });
        let rows = ends + u64::from(len > 0 && map[len - 1] != b'\n');
        let _ = map.advise(Advice::Normal);
        let file = file.try_clone().unwrap();
        Self {
            file,
            map,
            stride,
            checkpoints,
            rows,
        }
    }

    fn rows(&self) -> u64 {
        self.rows
    }

    fn index_bytes(&self) -> usize {
        self.checkpoints.capacity() * 8
    }

    fn fetch(&mut self, row: u64, out: &mut Row) {
        let mut pos = self.checkpoints[(row / self.stride) as usize] as usize;
        for _ in 0..row % self.stride {
            pos += row_end(&self.map[pos..]);
        }
        let end = pos + row_end(&self.map[pos..]);
        split_into(&self.map[pos..end], out);
    }

    fn drop_caches(&mut self) {
        // Pages mapped into this process survive fadvise(DONTNEED); unmap them first.
        #[allow(unsafe_code)]
        let map = unsafe { Mmap::map(&self.file) }.expect("mmap");
        self.map = map;
    }
}

fn main() {
    run::<MmapSparse>("mmap-sparse", &format!("stride={}", stride()));
}
