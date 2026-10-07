//! Column order (EDIT-4, ADR 0003): which column is shown at each position. Rows are ragged
//! and the widest one isn't known until the whole file has been read, so the map is an
//! explicit list of ids for the first positions, then the file's columns in order from
//! `tail` on, however many a row has.

use crate::{Col, ColId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColMap {
    /// Ids of positions `0..explicit.len()`.
    explicit: Vec<ColId>,
    /// The file column shown at position `explicit.len()`; later positions follow on.
    tail: Col,
}

impl Default for ColMap {
    /// File order.
    fn default() -> Self {
        Self {
            explicit: Vec::new(),
            tail: 0,
        }
    }
}

impl ColMap {
    pub fn get(&self, pos: Col) -> ColId {
        match self.explicit.get(pos as usize) {
            Some(&id) => id,
            None => ColId::source(self.tail + (pos - self.explicit.len() as Col)),
        }
    }

    /// Where column `id` is shown; `None` once it was deleted.
    pub fn position(&self, id: ColId) -> Option<Col> {
        if let Some(i) = self.explicit.iter().position(|&c| c == id) {
            return Some(i as Col);
        }
        (!id.is_inserted() && id.0 >= self.tail)
            .then(|| self.explicit.len() as Col + (id.0 - self.tail))
    }

    /// Put `ids` at position `at`; later columns move right.
    pub fn insert(&mut self, at: Col, ids: &[ColId]) {
        self.reach(at);
        let at = at as usize;
        self.explicit.splice(at..at, ids.iter().copied());
    }

    /// Take out `count` columns from position `at`; returns their ids in order.
    pub fn remove(&mut self, at: Col, count: Col) -> Vec<ColId> {
        self.reach(at + count);
        self.explicit
            .drain(at as usize..(at + count) as usize)
            .collect()
    }

    /// Make positions `0..pos` explicit.
    fn reach(&mut self, pos: Col) {
        while (self.explicit.len() as Col) < pos {
            self.explicit.push(ColId::source(self.tail));
            self.tail += 1;
        }
    }

    /// Positions a row spans: one past the last that holds anything, given its number of
    /// file fields and whether a column id has an edit in the row.
    pub fn width(&self, fields: Col, edited: impl Fn(ColId) -> bool) -> Col {
        let len = self.explicit.len() as Col;
        let mut w = if fields > self.tail {
            len + (fields - self.tail)
        } else {
            0
        };
        for (i, &id) in self.explicit.iter().enumerate().rev() {
            if (i as Col) < w {
                break;
            }
            if (!id.is_inserted() && id.0 < fields) || edited(id) {
                w = i as Col + 1;
            }
        }
        w
    }

    /// Width when edits can sit anywhere: also past the row's fields, in the tail.
    pub fn width_with(&self, fields: Col, edits: impl Iterator<Item = ColId> + Clone) -> Col {
        let edited = |id: ColId| edits.clone().any(|e| e == id);
        let mut w = self.width(fields, edited);
        for id in edits {
            if let Some(p) = self.position(id) {
                w = w.max(p + 1);
            }
        }
        w
    }

    /// Columns inserted, and file columns deleted, compared with file order.
    pub fn changes(&self) -> usize {
        let inserted = self.explicit.iter().filter(|c| c.is_inserted()).count();
        let kept = self.explicit.len() - inserted;
        inserted + (self.tail as usize - kept)
    }

    /// Still file order (everything inserted was deleted again, or undone).
    pub fn is_file_order(&self) -> bool {
        self.explicit
            .iter()
            .enumerate()
            .all(|(i, &c)| c == ColId::source(i as Col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same operations on an explicit list of 40 columns, as the reference.
    #[test]
    fn matches_a_flat_list_through_random_inserts_and_removes() {
        let mut map = ColMap::default();
        let mut flat: Vec<ColId> = (0..40).map(ColId::source).collect();
        let (mut x, mut next) = (0x2545_f491u64, 0u32);
        for _ in 0..500 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let at = (x % 20) as Col;
            if x % 3 == 0 {
                let count = ((x >> 12) % 3 + 1) as Col;
                let gone = map.remove(at, count);
                let want: Vec<ColId> = flat.drain(at as usize..(at + count) as usize).collect();
                assert_eq!(gone, want);
                flat.push(ColId::source(1_000)); // keep the list long enough; never read
                flat.pop();
            } else {
                let ids = [ColId::inserted(next), ColId::inserted(next + 1)];
                next += 2;
                map.insert(at, &ids);
                flat.splice(at as usize..at as usize, ids);
            }
            for (p, &id) in flat.iter().enumerate().take(20) {
                assert_eq!(map.get(p as Col), id);
                assert_eq!(map.position(id), Some(p as Col));
            }
        }
    }

    #[test]
    fn width_counts_file_fields_inserted_edits_and_the_tail() {
        let mut map = ColMap::default();
        map.insert(1, &[ColId::inserted(0)]); // a, NEW, b, c, ...
        assert_eq!(
            map.width(3, |_| false),
            4,
            "three fields span four positions"
        );
        assert_eq!(
            map.width(1, |_| false),
            1,
            "a short row ends before the new column"
        );
        assert_eq!(
            map.width(1, |c| c == ColId::inserted(0)),
            2,
            "unless it has an edit"
        );
        let gone = map.remove(0, 1); // delete a
        assert_eq!(gone, [ColId::source(0)]);
        assert_eq!(map.get(0), ColId::inserted(0));
        assert_eq!(map.get(1), ColId::source(1));
        assert_eq!(map.position(ColId::source(0)), None);
        assert_eq!(map.changes(), 2, "one in, one out");
        assert_eq!(
            map.width_with(0, [ColId::source(9)].into_iter()),
            10,
            "an edit past the row's end in the tail"
        );
        map.insert(0, &gone);
        map.remove(1, 1);
        assert!(map.is_file_order(), "undone");
        assert_eq!(map.changes(), 0);
    }
}
