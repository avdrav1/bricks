// DEC-3 option B: a piece table over bytes. The document is a sequence of pieces, each a
// byte range of either the original file (mmap) or an append-only add buffer. Pieces are
// row-aligned (VS Code-style piece tree with row counts): a cell edit re-encodes its row
// into the add buffer and swaps that row's piece; an insert adds a piece. Pieces live in
// the same order-statistic treap as option A, keyed by row count.
#[path = "../../common.rs"]
mod common;

use common::*;
use std::io::Write;

#[derive(Clone)]
struct Piece {
    add: bool,
    start: u64,
    len: u64,
    /// Original-file pieces: source row number of the first row.
    row: u64,
    rows: u64,
}

impl Item for Piece {
    fn rows(&self) -> u64 {
        self.rows
    }
}

struct PieceTable {
    src: Source,
    tree: Treap<Piece>,
    add: Vec<u8>,
}

/// Byte offset just past the `k`th row of an add-buffer piece.
fn add_offset(add: &[u8], p: &Piece, k: u64) -> usize {
    let mut off = p.start as usize;
    for _ in 0..k {
        off += row_end(&add[off..(p.start + p.len) as usize]);
    }
    off
}

fn cut(src: &Source, add: &[u8], p: &Piece, k: u64) -> (Piece, Piece) {
    let off = if p.add {
        add_offset(add, p, k) as u64
    } else {
        src.row_start(p.row + k) as u64
    };
    (
        Piece {
            len: off - p.start,
            rows: k,
            ..p.clone()
        },
        Piece {
            add: p.add,
            start: off,
            len: p.start + p.len - off,
            row: p.row + k,
            rows: p.rows - k,
        },
    )
}

impl PieceTable {
    fn row_bytes(&self, row: u64) -> &[u8] {
        let (p, k) = self.tree.find(row);
        if p.add {
            let off = add_offset(&self.add, p, k);
            &self.add[off..off + row_end(&self.add[off..])]
        } else {
            self.src.row_bytes(p.row + k)
        }
    }

    fn append(&mut self, fields: &[&[u8]]) -> Piece {
        let start = self.add.len();
        encode_row(fields.iter().copied(), &mut self.add);
        Piece {
            add: true,
            start: start as u64,
            len: (self.add.len() - start) as u64,
            row: 0,
            rows: 1,
        }
    }
}

impl Model for PieceTable {
    fn open(src: Source) -> Self {
        let whole = Piece {
            add: false,
            start: 0,
            len: src.row_start(src.rows()) as u64,
            row: 0,
            rows: src.rows(),
        };
        Self {
            src,
            tree: Treap::new(whole),
            add: Vec::new(),
        }
    }

    fn rows(&self) -> u64 {
        self.tree.total()
    }

    fn edit(&mut self, row: u64, col: usize, value: &[u8]) {
        let mut cur = Row::default();
        split_into(self.row_bytes(row), &mut cur);
        let fields: Vec<&[u8]> = (0..cur.fields.len())
            .map(|c| if c == col { value } else { cur.field(c) })
            .collect();
        let piece = self.append(&fields);
        let (src, add) = (&self.src, &self.add);
        self.tree
            .replace(row, 1, piece, &mut |p, k| cut(src, add, p, k));
    }

    fn insert_row(&mut self, row: u64, fields: &[&[u8]]) {
        let piece = self.append(fields);
        let (src, add) = (&self.src, &self.add);
        self.tree
            .insert_at(row, piece, &mut |p, k| cut(src, add, p, k));
    }

    fn fetch(&self, row: u64, out: &mut Row) {
        split_into(self.row_bytes(row), out);
    }

    fn save(&self, w: &mut impl Write) {
        self.tree.for_each(|p| {
            let bytes = if p.add {
                &self.add[p.start as usize..(p.start + p.len) as usize]
            } else {
                self.src.bytes(p.start as usize, (p.start + p.len) as usize)
            };
            w.write_all(bytes).unwrap();
        });
    }

    fn mem_bytes(&self) -> usize {
        self.tree.arena_bytes() + self.add.capacity()
    }

    fn shape(&self) -> String {
        format!(
            "pieces={} add_bytes={}",
            self.tree.live_items(),
            self.add.len()
        )
    }
}

fn main() {
    run::<PieceTable>("piece-table");
}
