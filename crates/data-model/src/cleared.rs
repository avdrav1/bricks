//! Cleared cells (EDIT-5). Delete empties a whole selection at once, and a selection can
//! be every row of a 1 GB file, so a clear is kept as one block, file rows × columns by
//! identity, never as an edit per cell. Blocks are flattened into sorted, non-overlapping
//! row segments, each with the columns cleared there, so a lookup is one binary search
//! however many blocks there are. Cell edits sit above blocks: a clear removes the edits
//! it covers (its inverse keeps them), and typing into a cleared cell edits it again.

use crate::{Col, ColId};
use std::ops::Range;
use std::sync::Arc;

/// Columns by identity: the listed ids, and with `rest` every file column from `rest` on
/// (whole rows reach past the widest row seen so far).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColSet {
    /// Sorted, no duplicates.
    ids: Vec<ColId>,
    rest: Option<Col>,
}

impl ColSet {
    pub fn new(mut ids: Vec<ColId>, rest: Option<Col>) -> Self {
        ids.sort_unstable();
        ids.dedup();
        Self { ids, rest }
    }

    pub fn contains(&self, id: ColId) -> bool {
        self.rest.is_some_and(|r| !id.is_inserted() && id.0 >= r)
            || self.ids.binary_search(&id).is_ok()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty() && self.rest.is_none()
    }

    /// Only the file's columns: inserted ones have nothing in the file to clear.
    pub(crate) fn file_columns(&self) -> Self {
        Self {
            ids: self
                .ids
                .iter()
                .copied()
                .filter(|c| !c.is_inserted())
                .collect(),
            rest: self.rest,
        }
    }

    fn add(&mut self, other: &Self) {
        self.ids.extend_from_slice(&other.ids);
        self.ids.sort_unstable();
        self.ids.dedup();
        self.rest = match (self.rest, other.rest) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }

    pub fn heap_bytes(&self) -> usize {
        self.ids.capacity() * std::mem::size_of::<ColId>()
    }
}

/// File rows `rows` with the columns cleared in all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub rows: Range<u64>,
    pub cols: ColSet,
}

/// The columns cleared in file row `row`, from segments sorted by row.
pub(crate) fn cleared_at(segments: &[Segment], row: u64) -> Option<&ColSet> {
    let i = segments.partition_point(|s| s.rows.end <= row);
    segments
        .get(i)
        .filter(|s| s.rows.start <= row)
        .map(|s| &s.cols)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    /// File rows, sorted, merged.
    rows: Vec<Range<u64>>,
    cols: ColSet,
}

/// Every clear still in effect, oldest first, and their flattened segments.
#[derive(Debug, Default, Clone)]
pub(crate) struct Cleared {
    blocks: Vec<Block>,
    segments: Arc<Vec<Segment>>,
}

impl Cleared {
    /// Clear file rows `rows` (sorted, merged) in columns `cols`. Nothing happens when
    /// either is empty.
    pub fn push(&mut self, rows: Vec<Range<u64>>, cols: ColSet) {
        if rows.is_empty() || cols.is_empty() {
            return;
        }
        self.blocks.push(Block { rows, cols });
        self.flatten();
    }

    /// Take back the latest clear of exactly these rows and columns.
    pub fn remove(&mut self, rows: Vec<Range<u64>>, cols: ColSet) {
        let block = Block { rows, cols };
        if let Some(i) = self.blocks.iter().rposition(|b| *b == block) {
            self.blocks.remove(i);
            self.flatten();
        }
    }

    /// Clears in effect.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn segments(&self) -> &Arc<Vec<Segment>> {
        &self.segments
    }

    pub fn at(&self, row: u64) -> Option<&ColSet> {
        cleared_at(&self.segments, row)
    }

    /// Rebuild the segments: sweep the blocks' row ranges in order, cutting wherever one
    /// starts or ends; neighbors with the same columns join.
    fn flatten(&mut self) {
        let mut events: Vec<(u64, usize, i32)> = Vec::new();
        for (i, b) in self.blocks.iter().enumerate() {
            for r in &b.rows {
                events.push((r.start, i, 1));
                events.push((r.end, i, -1));
            }
        }
        events.sort_unstable_by_key(|e| e.0);
        let mut active = vec![0i32; self.blocks.len()];
        let mut out: Vec<Segment> = Vec::new();
        let mut e = 0;
        while e < events.len() {
            let at = events[e].0;
            while e < events.len() && events[e].0 == at {
                active[events[e].1] += events[e].2;
                e += 1;
            }
            let Some(&(end, _, _)) = events.get(e) else {
                break;
            };
            let mut cols = ColSet::default();
            for (b, _) in self.blocks.iter().zip(&active).filter(|(_, &n)| n > 0) {
                cols.add(&b.cols);
            }
            if cols.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.rows.end == at && last.cols == cols => last.rows.end = end,
                _ => out.push(Segment {
                    rows: at..end,
                    cols,
                }),
            }
        }
        self.segments = Arc::new(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(ids: &[Col]) -> ColSet {
        ColSet::new(ids.iter().copied().map(ColId::source).collect(), None)
    }

    fn first_ten() -> Vec<Range<u64>> {
        let rows = 0..10;
        vec![rows]
    }

    /// Overlapping clears give the union where they overlap; taking one back leaves the
    /// other exactly as it was, even when both clear the same cells.
    #[test]
    fn overlapping_blocks_union_and_come_apart() {
        let mut c = Cleared::default();
        c.push(first_ten(), cols(&[1]));
        c.push(vec![5..20, 30..31], cols(&[2]));
        c.push(first_ten(), cols(&[1]));
        assert_eq!(c.at(4), Some(&cols(&[1])));
        assert_eq!(c.at(5), Some(&cols(&[1, 2])));
        assert_eq!(c.at(10), Some(&cols(&[2])));
        assert_eq!(c.at(20), None);
        assert_eq!(c.at(30), Some(&cols(&[2])));
        c.remove(first_ten(), cols(&[1]));
        assert_eq!(c.at(4), Some(&cols(&[1])), "the other clear of it stays");
        c.remove(vec![5..20, 30..31], cols(&[2]));
        assert_eq!(c.at(5), Some(&cols(&[1])));
        assert_eq!(c.segments().len(), 1);
        c.remove(first_ten(), cols(&[1]));
        assert!(c.segments().is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn rest_covers_every_later_file_column() {
        let whole_rows = ColSet::new(vec![ColId::inserted(0)], Some(3));
        assert!(whole_rows.contains(ColId::source(3)));
        assert!(whole_rows.contains(ColId::source(9_999)));
        assert!(!whole_rows.contains(ColId::source(2)));
        assert!(whole_rows.contains(ColId::inserted(0)));
        assert!(!whole_rows.contains(ColId::inserted(1)));
    }
}
