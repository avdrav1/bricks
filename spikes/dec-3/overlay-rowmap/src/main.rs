// DEC-3 option A: source rows stay in the file; cell edits live in an overlay keyed by a
// stable RowId; row order is a row map from logical row to RowId.
//   DEC3_ROWMAP=treap (default): treap of runs of consecutive RowIds, O(log runs) per op.
//   DEC3_ROWMAP=vec: one u64 per row, the O(n)-insert baseline.
#[path = "../../common.rs"]
mod common;

use common::*;
use std::collections::{BTreeSet, HashMap};
use std::io::Write;

/// RowIds with this bit set are inserted rows; others are source row numbers.
const NEW: u64 = 1 << 63;

#[derive(Clone)]
struct Run {
    first: u64,
    len: u64,
}

impl Item for Run {
    fn rows(&self) -> u64 {
        self.len
    }
}

fn cut(r: &Run, k: u64) -> (Run, Run) {
    (
        Run {
            first: r.first,
            len: k,
        },
        Run {
            first: r.first + k,
            len: r.len - k,
        },
    )
}

enum RowMap {
    Treap(Treap<Run>),
    Vec(Vec<u64>),
}

struct Overlay {
    src: Source,
    map: RowMap,
    edits: HashMap<(u64, u32), Box<[u8]>>,
    edited_src: BTreeSet<u64>,
    next_new: u64,
}

impl Overlay {
    fn id(&self, row: u64) -> u64 {
        match &self.map {
            RowMap::Treap(t) => {
                let (run, k) = t.find(row);
                run.first + k
            }
            RowMap::Vec(v) => v[row as usize],
        }
    }

    fn fetch_id(&self, id: u64, out: &mut Row) {
        if id & NEW == 0 {
            split_into(self.src.row_bytes(id), out);
            if !self.edited_src.contains(&id) {
                return;
            }
        } else {
            out.clear();
            for _ in 0..NCOLS {
                out.push(b"");
            }
        }
        let mut merged = Row::default();
        for c in 0..out.fields.len() {
            match self.edits.get(&(id, c as u32)) {
                Some(v) => merged.push(v),
                None => merged.push(out.field(c)),
            }
        }
        *out = merged;
    }

    fn write_edited(&self, id: u64, w: &mut impl Write, row: &mut Row, buf: &mut Vec<u8>) {
        self.fetch_id(id, row);
        buf.clear();
        encode_row((0..row.fields.len()).map(|c| row.field(c)), buf);
        w.write_all(buf).unwrap();
    }
}

impl Model for Overlay {
    fn open(src: Source) -> Self {
        let rows = src.rows();
        let map = match std::env::var("DEC3_ROWMAP").as_deref() {
            Ok("vec") => RowMap::Vec((0..rows).collect()),
            _ => RowMap::Treap(Treap::new(Run {
                first: 0,
                len: rows,
            })),
        };
        Self {
            src,
            map,
            edits: HashMap::new(),
            edited_src: BTreeSet::new(),
            next_new: 0,
        }
    }

    fn rows(&self) -> u64 {
        match &self.map {
            RowMap::Treap(t) => t.total(),
            RowMap::Vec(v) => v.len() as u64,
        }
    }

    fn edit(&mut self, row: u64, col: usize, value: &[u8]) {
        let id = self.id(row);
        self.edits.insert((id, col as u32), value.into());
        if id & NEW == 0 {
            self.edited_src.insert(id);
        }
    }

    fn insert_row(&mut self, row: u64, fields: &[&[u8]]) {
        let id = NEW | self.next_new;
        self.next_new += 1;
        match &mut self.map {
            RowMap::Treap(t) => t.insert_at(row, Run { first: id, len: 1 }, &mut cut),
            RowMap::Vec(v) => v.insert(row as usize, id),
        }
        for (c, f) in fields.iter().enumerate() {
            if !f.is_empty() {
                self.edits.insert((id, c as u32), (*f).into());
            }
        }
    }

    fn fetch(&self, row: u64, out: &mut Row) {
        self.fetch_id(self.id(row), out);
    }

    fn save(&self, w: &mut impl Write) {
        let (mut row, mut buf) = (Row::default(), Vec::new());
        match &self.map {
            RowMap::Treap(t) => t.for_each(|run| {
                if run.first & NEW != 0 {
                    for id in run.first..run.first + run.len {
                        self.write_edited(id, w, &mut row, &mut buf);
                    }
                    return;
                }
                // Untouched source rows go out as one byte-range copy between edited rows.
                let (a, b) = (run.first, run.first + run.len);
                let mut pos = a;
                for &e in self.edited_src.range(a..b) {
                    w.write_all(self.src.rows_bytes(pos, e)).unwrap();
                    self.write_edited(e, w, &mut row, &mut buf);
                    pos = e + 1;
                }
                w.write_all(self.src.rows_bytes(pos, b)).unwrap();
            }),
            RowMap::Vec(v) => {
                for &id in v {
                    if id & NEW == 0 && !self.edited_src.contains(&id) {
                        w.write_all(self.src.row_bytes(id)).unwrap();
                    } else {
                        self.write_edited(id, w, &mut row, &mut buf);
                    }
                }
            }
        }
    }

    fn mem_bytes(&self) -> usize {
        let map = match &self.map {
            RowMap::Treap(t) => t.arena_bytes(),
            RowMap::Vec(v) => v.capacity() * 8,
        };
        // Estimate: hashbrown slot + control byte per capacity slot, value bytes, and
        // ~16 bytes per BTreeSet entry.
        let slot = std::mem::size_of::<((u64, u32), Box<[u8]>)>() + 1;
        let values: usize = self.edits.values().map(|v| v.len()).sum();
        map + self.edits.capacity() * slot + values + self.edited_src.len() * 16
    }

    fn shape(&self) -> String {
        match &self.map {
            RowMap::Treap(t) => format!(
                "rowmap=treap runs={} edits={}",
                t.live_items(),
                self.edits.len()
            ),
            RowMap::Vec(_) => format!("rowmap=vec edits={}", self.edits.len()),
        }
    }
}

fn main() {
    let name = match std::env::var("DEC3_ROWMAP").as_deref() {
        Ok("vec") => "overlay-vec",
        _ => "overlay-treap",
    };
    run::<Overlay>(name);
}
