// DEC-2 option C: parse the whole file into per-column storage (one byte buffer plus a u32
// end-offset per cell, Arrow-style). Raw field bytes are kept verbatim. Streams the file in
// 4 MiB reads so the only large allocation is the columns themselves.
#[path = "../../common.rs"]
mod common;

use common::*;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

struct Column {
    data: Vec<u8>,
    ends: Vec<u32>,
}

struct Columnar {
    cols: Vec<Column>,
    rows: u64,
}

impl Columnar {
    fn push_row(&mut self, row: &[u8], scratch: &mut Row) {
        split_into(row, scratch);
        if self.cols.is_empty() {
            self.cols = (0..scratch.fields.len())
                .map(|_| Column {
                    data: Vec::new(),
                    ends: Vec::new(),
                })
                .collect();
        }
        for (i, c) in self.cols.iter_mut().enumerate() {
            // Ragged rows: missing cells are empty; extra cells are dropped (spike only).
            if i < scratch.fields.len() {
                c.data.extend_from_slice(scratch.field(i));
            }
            c.ends.push(c.data.len() as u32);
        }
        self.rows += 1;
    }
}

impl Store for Columnar {
    fn build(file: &File) -> Self {
        // try_clone shares the file position with the caller's handle; start from 0.
        let mut file = file.try_clone().unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut s = Self {
            cols: Vec::new(),
            rows: 0,
        };
        let (mut buf, mut scratch) = (Vec::with_capacity(8 << 20), Row::default());
        let mut chunk = vec![0u8; 4 << 20];
        loop {
            let n = file.read(&mut chunk).expect("read");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let mut start = 0;
            let mut ends = Vec::new();
            for_each_row_end(&buf, false, |e| ends.push(e));
            for e in ends {
                s.push_row(&buf[start..e], &mut scratch);
                start = e;
            }
            buf.drain(..start);
        }
        if !buf.is_empty() {
            let rest = std::mem::take(&mut buf);
            s.push_row(&rest, &mut scratch);
        }
        s
    }

    fn rows(&self) -> u64 {
        self.rows
    }

    fn index_bytes(&self) -> usize {
        self.cols
            .iter()
            .map(|c| c.data.capacity() + c.ends.capacity() * 4)
            .sum()
    }

    fn fetch(&mut self, row: u64, out: &mut Row) {
        out.clear();
        let r = row as usize;
        for c in &self.cols {
            let a = if r == 0 { 0 } else { c.ends[r - 1] as usize };
            out.push(&c.data[a..c.ends[r] as usize]);
        }
    }
}

fn main() {
    run::<Columnar>("columnar", "u32 cell ends, raw bytes");
}
