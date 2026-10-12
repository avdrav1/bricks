//! Row order (EDIT-3, ADR 0003): which row is shown at each position, as an
//! order-statistic treap of runs of consecutive [`RowId`]s. Source order plus local
//! inserts and deletes stays a handful of runs, so finding, inserting, and removing are
//! O(log runs) at any row count. A sort (SORT-1, ADR 0004) makes every row its own run,
//! so a sorted order is a flat permutation instead: 4 bytes a row ([`RowOrder::Sorted`]).

use crate::RowId;
use std::sync::Arc;

/// Consecutive row ids `first, first + 1, ...` shown one after another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub first: RowId,
    pub len: u64,
}

impl Run {
    fn split(self, at: u64) -> (Run, Run) {
        (
            Run {
                first: self.first,
                len: at,
            },
            Run {
                first: RowId(self.first.0 + at),
                len: self.len - at,
            },
        )
    }
}

const NIL: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    run: Run,
    prio: u64,
    l: u32,
    r: u32,
    /// Rows in this subtree.
    sum: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowMap {
    nodes: Vec<Node>,
    /// Slots of removed nodes, reused before the arena grows.
    free: Vec<u32>,
    root: u32,
    seed: u64,
}

impl RowMap {
    /// Rows `first..first + len` in order.
    pub fn new(run: Run) -> Self {
        let mut m = Self {
            nodes: Vec::new(),
            free: Vec::new(),
            root: NIL,
            seed: 0x853c_49e6_748f_ea9b,
        };
        if run.len > 0 {
            m.root = m.node(run);
        }
        m
    }

    /// Rows shown.
    pub fn len(&self) -> u64 {
        self.sum(self.root)
    }

    /// Id of the row at position `k` (< `len()`).
    pub fn get(&self, mut k: u64) -> RowId {
        let mut t = self.root;
        loop {
            let n = &self.nodes[t as usize];
            let ls = self.sum(n.l);
            if k < ls {
                t = n.l;
            } else if k < ls + n.run.len {
                return RowId(n.run.first.0 + (k - ls));
            } else {
                k -= ls + n.run.len;
                t = n.r;
            }
        }
    }

    /// Position of row `id`, if it is shown. Walks the runs: O(runs).
    pub fn position(&self, id: RowId) -> Option<u64> {
        let mut at = 0;
        let mut found = None;
        self.for_each(|run| {
            if found.is_none() {
                if run.first.0 <= id.0 && id.0 < run.first.0 + run.len {
                    found = Some(at + (id.0 - run.first.0));
                }
                at += run.len;
            }
        });
        found
    }

    /// Put `runs` (in order) at position `k`, shifting later rows down.
    pub fn insert(&mut self, k: u64, runs: &[Run]) {
        let (a, b) = self.split(self.root, k);
        let mut mid = NIL;
        for &run in runs.iter().filter(|r| r.len > 0) {
            let n = self.node(run);
            mid = self.merge(mid, n);
        }
        let a = self.merge(a, mid);
        self.root = self.merge(a, b);
    }

    /// Take out `count` rows at position `k`; returns them as runs, in order.
    pub fn remove(&mut self, k: u64, count: u64) -> Vec<Run> {
        let (a, b) = self.split(self.root, k);
        let (gone, c) = self.split(b, count);
        let mut runs = Vec::new();
        self.walk(gone, &mut |run| runs.push(*run));
        self.free_subtree(gone);
        self.root = self.merge(a, c);
        runs
    }

    /// The runs covering positions `range`, cut to it, in order.
    pub fn runs_in(&self, range: std::ops::Range<u64>) -> Vec<Run> {
        let mut out = Vec::new();
        let mut at = 0;
        self.for_each(|run| {
            let (start, end) = (at, at + run.len);
            at = end;
            let (lo, hi) = (start.max(range.start), end.min(range.end));
            if lo < hi {
                out.push(Run {
                    first: RowId(run.first.0 + (lo - start)),
                    len: hi - lo,
                });
            }
        });
        out
    }

    /// Visit every run in order.
    pub fn for_each(&self, mut f: impl FnMut(&Run)) {
        self.walk(self.root, &mut f);
    }

    fn walk(&self, t: u32, f: &mut impl FnMut(&Run)) {
        let (mut stack, mut t) = (Vec::new(), t);
        loop {
            while t != NIL {
                stack.push(t);
                t = self.nodes[t as usize].l;
            }
            let Some(n) = stack.pop() else { break };
            f(&self.nodes[n as usize].run);
            t = self.nodes[n as usize].r;
        }
    }

    fn free_subtree(&mut self, t: u32) {
        let mut stack = vec![t];
        while let Some(t) = stack.pop() {
            if t != NIL {
                let n = &self.nodes[t as usize];
                stack.extend([n.l, n.r]);
                self.free.push(t);
            }
        }
    }

    fn node(&mut self, run: Run) -> u32 {
        self.node_below(run, u64::MAX)
    }

    /// A node with a random priority no higher than `cap`.
    fn node_below(&mut self, run: Run, cap: u64) -> u32 {
        // splitmix64: priorities only need to look random.
        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let node = Node {
            run,
            prio: if cap == u64::MAX { z } else { z % (cap + 1) },
            l: NIL,
            r: NIL,
            sum: run.len,
        };
        match self.free.pop() {
            Some(i) => {
                self.nodes[i as usize] = node;
                i
            }
            None => {
                self.nodes.push(node);
                (self.nodes.len() - 1) as u32
            }
        }
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
        let s = self.sum(n.l) + n.run.len + self.sum(n.r);
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

    /// Split into (first `k` rows, rest), cutting a run in two if `k` falls inside it.
    fn split(&mut self, t: u32, k: u64) -> (u32, u32) {
        if t == NIL {
            return (NIL, NIL);
        }
        let (l, r) = (self.nodes[t as usize].l, self.nodes[t as usize].r);
        let (ls, own) = (self.sum(l), self.nodes[t as usize].run.len);
        if k <= ls {
            let (a, b) = self.split(l, k);
            self.nodes[t as usize].l = b;
            self.update(t);
            (a, t)
        } else if k >= ls + own {
            let (a, b) = self.split(r, k - ls - own);
            self.nodes[t as usize].r = a;
            self.update(t);
            (t, b)
        } else {
            let (x, y) = self.nodes[t as usize].run.split(k - ls);
            self.nodes[t as usize].run = x;
            self.nodes[t as usize].r = NIL;
            self.update(t);
            // The new half goes where `t` was, under `t`'s ancestors: a priority above
            // `t`'s would break the heap order there and, split after split, let the tree
            // grow into a chain (FILT-4 found it: depth n/11 after many deletes).
            let cap = self.nodes[t as usize].prio;
            let ny = self.node_below(y, cap);
            let rt = self.merge(ny, r);
            (t, rt)
        }
    }
}

/// High bit of a packed id: an inserted row, numbered by the low 31 bits.
pub(crate) const PACKED_INSERTED: u32 = 1 << 31;

/// A row id in 4 bytes, for sorted orders: source rows and inserted rows below 2^31.
pub(crate) fn pack(id: RowId) -> Option<u32> {
    let n = id.0 & !RowId::INSERTED;
    (n < u64::from(PACKED_INSERTED))
        .then(|| n as u32 | if id.is_inserted() { PACKED_INSERTED } else { 0 })
}

pub(crate) fn unpack(v: u32) -> RowId {
    if v & PACKED_INSERTED != 0 {
        RowId::inserted(u64::from(v & !PACKED_INSERTED))
    } else {
        RowId::source(u64::from(v))
    }
}

/// Consecutive packed ids as runs, in order.
pub(crate) fn coalesce(ids: &[u32]) -> impl Iterator<Item = Run> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        let first = unpack(*ids.get(i)?);
        let mut len = 1;
        while ids
            .get(i + len as usize)
            .is_some_and(|&v| unpack(v).0 == first.0 + len)
        {
            len += 1;
        }
        i += len as usize;
        Some(Run { first, len })
    })
}

/// Which row is shown at each position once it is no longer file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOrder {
    /// File order with local inserts and deletes: a tree of runs.
    Runs(RowMap),
    /// A sorted order (SORT-1): one packed id per row. Shared, so snapshots and undo
    /// hold it without copying; an insert or delete copies it first if it is shared.
    Sorted(Arc<Vec<u32>>),
}

impl RowOrder {
    /// A run order of these runs, in order (the recovery journal, APP-7).
    pub(crate) fn from_runs(runs: &[Run]) -> Self {
        let mut m = RowMap::new(Run {
            first: RowId(0),
            len: 0,
        });
        m.insert(0, runs);
        Self::Runs(m)
    }

    pub fn len(&self) -> u64 {
        match self {
            Self::Runs(m) => m.len(),
            Self::Sorted(ids) => ids.len() as u64,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Id of the row at position `k` (< `len()`).
    pub fn get(&self, k: u64) -> RowId {
        match self {
            Self::Runs(m) => m.get(k),
            Self::Sorted(ids) => unpack(ids[k as usize]),
        }
    }

    /// Position of row `id`, if it is shown: O(runs), or O(rows) once sorted.
    pub fn position(&self, id: RowId) -> Option<u64> {
        match self {
            Self::Runs(m) => m.position(id),
            Self::Sorted(ids) => {
                let v = pack(id)?;
                ids.iter().position(|&x| x == v).map(|k| k as u64)
            }
        }
    }

    /// Put `runs` (in order) at position `k`, shifting later rows down.
    pub fn insert(&mut self, k: u64, runs: &[Run]) {
        match self {
            Self::Runs(m) => m.insert(k, runs),
            Self::Sorted(ids) => {
                let new = runs.iter().flat_map(|r| {
                    (0..r.len).map(move |i| pack(RowId(r.first.0 + i)).expect("a sorted row id"))
                });
                let k = k as usize;
                Arc::make_mut(ids).splice(k..k, new);
            }
        }
    }

    /// Take out `count` rows at position `k`; returns them as runs, in order.
    pub fn remove(&mut self, k: u64, count: u64) -> Vec<Run> {
        match self {
            Self::Runs(m) => m.remove(k, count),
            Self::Sorted(ids) => {
                let ids = Arc::make_mut(ids);
                let gone: Vec<u32> = ids.drain(k as usize..(k + count) as usize).collect();
                coalesce(&gone).collect()
            }
        }
    }

    /// The runs covering positions `range`, cut to it, in order.
    pub fn runs_in(&self, range: std::ops::Range<u64>) -> Vec<Run> {
        match self {
            Self::Runs(m) => m.runs_in(range),
            Self::Sorted(ids) => {
                let end = range.end.min(ids.len() as u64);
                let start = range.start.min(end);
                coalesce(&ids[start as usize..end as usize]).collect()
            }
        }
    }

    /// Visit every run in order.
    pub fn for_each(&self, mut f: impl FnMut(&Run)) {
        match self {
            Self::Runs(m) => m.for_each(f),
            Self::Sorted(ids) => coalesce(ids).for_each(|r| f(&r)),
        }
    }

    /// Packed ids of every row in order, for a sort to permute.
    pub(crate) fn packed(&self) -> Option<Vec<u32>> {
        match self {
            Self::Sorted(ids) => Some(ids.to_vec()),
            Self::Runs(m) => {
                let mut out = Vec::with_capacity(m.len() as usize);
                let mut ok = true;
                m.for_each(|r| {
                    for i in 0..r.len {
                        match pack(RowId(r.first.0 + i)) {
                            Some(v) => out.push(v),
                            None => ok = false,
                        }
                    }
                });
                ok.then_some(out)
            }
        }
    }

    /// Take out the rows at positions `ranges` (ascending, apart), as one step: each
    /// range's rows as runs, with the position it started at. A sorted order is rebuilt
    /// in one pass, however many ranges (FILT-4: the rows shown under a filter).
    pub fn remove_ranges(&mut self, ranges: &[std::ops::Range<u64>]) -> Vec<(u64, Vec<Run>)> {
        match self {
            Self::Runs(m) => {
                let mut out: Vec<(u64, Vec<Run>)> = ranges
                    .iter()
                    .rev()
                    .map(|r| (r.start, m.remove(r.start, r.end - r.start)))
                    .collect();
                out.reverse();
                out
            }
            Self::Sorted(ids) => {
                let ids = Arc::make_mut(ids);
                let mut out = Vec::with_capacity(ranges.len());
                let mut kept = 0;
                let mut next = 0;
                for r in ranges {
                    let (start, end) = (r.start as usize, r.end as usize);
                    ids.copy_within(next..start, kept);
                    kept += start - next;
                    out.push((r.start, coalesce(&ids[start..end]).collect()));
                    next = end;
                }
                let len = ids.len();
                ids.copy_within(next..len, kept);
                ids.truncate(kept + (len - next));
                out
            }
        }
    }

    /// Put back what [`Self::remove_ranges`] took: runs at the positions they had,
    /// ascending, as one step.
    pub fn insert_ranges(&mut self, parts: &[(u64, Vec<Run>)]) {
        match self {
            Self::Runs(m) => {
                for (at, runs) in parts {
                    m.insert(*at, runs);
                }
            }
            Self::Sorted(ids) => {
                let added: u64 = parts.iter().flat_map(|(_, r)| r).map(|r| r.len).sum();
                let mut out = Vec::with_capacity(ids.len() + added as usize);
                let mut src = ids.iter().copied();
                for (at, runs) in parts {
                    out.extend(src.by_ref().take(*at as usize - out.len()));
                    for r in runs {
                        out.extend(
                            (0..r.len)
                                .map(|i| pack(RowId(r.first.0 + i)).expect("a sorted row id")),
                        );
                    }
                }
                out.extend(src);
                *ids = Arc::new(out);
            }
        }
    }

    /// Heap the order holds.
    pub fn heap_bytes(&self) -> usize {
        match self {
            Self::Runs(m) => {
                (m.nodes.capacity() * std::mem::size_of::<Node>()) + m.free.capacity() * 4
            }
            Self::Sorted(ids) => ids.capacity() * 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(first: u64, len: u64) -> Run {
        Run {
            first: RowId(first),
            len,
        }
    }

    /// The same operations on a plain vector of ids, as the reference.
    #[test]
    fn matches_a_flat_vector_through_random_inserts_and_removes() {
        let mut map = RowMap::new(run(0, 1_000));
        let mut flat: Vec<u64> = (0..1_000).collect();
        let (mut x, mut next) = (0x9e37_79b9u64, 1u64 << 63);
        for _ in 0..2_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let k = x % (flat.len() as u64 + 1);
            if x % 3 == 0 && !flat.is_empty() {
                let k = k.min(flat.len() as u64 - 1);
                let count = ((x >> 20) % 5 + 1).min(flat.len() as u64 - k);
                let gone = map.remove(k, count);
                let want: Vec<u64> = flat.drain(k as usize..(k + count) as usize).collect();
                let got: Vec<u64> = gone
                    .iter()
                    .flat_map(|r| r.first.0..r.first.0 + r.len)
                    .collect();
                assert_eq!(got, want);
            } else {
                let n = (x >> 24) % 3 + 1;
                map.insert(k, &[run(next, n)]);
                for i in 0..n {
                    flat.insert((k + i) as usize, next + i);
                }
                next += n;
            }
            assert_eq!(map.len(), flat.len() as u64);
        }
        for (k, &id) in flat.iter().enumerate() {
            assert_eq!(map.get(k as u64), RowId(id));
        }
        let mid = flat.len() as u64 / 2;
        assert_eq!(map.position(RowId(flat[mid as usize])), Some(mid));
        let window: Vec<u64> = map
            .runs_in(mid..mid + 10)
            .iter()
            .flat_map(|r| r.first.0..r.first.0 + r.len)
            .collect();
        assert_eq!(window, flat[mid as usize..mid as usize + 10]);
    }

    /// FILT-4: many ranges out and back in one step, as runs and as a sorted order,
    /// against a flat vector.
    #[test]
    fn ranges_come_out_and_go_back_in_both_kinds_of_order() {
        let ranges = [1..3, 5..6, 7..10, 12..13];
        let flat: Vec<u64> = (0..15).collect();
        let left: Vec<u64> = flat
            .iter()
            .copied()
            .filter(|k| !ranges.iter().any(|r| r.contains(k)))
            .collect();
        let ids = |o: &RowOrder| -> Vec<u64> { (0..o.len()).map(|k| o.get(k).0).collect() };
        let sorted = RowOrder::Sorted(Arc::new((0..15).map(|k| pack(RowId(k)).unwrap()).collect()));
        for mut order in [RowOrder::Runs(RowMap::new(run(0, 15))), sorted] {
            let gone = order.remove_ranges(&ranges);
            assert_eq!(ids(&order), left);
            let took: Vec<(u64, Vec<u64>)> = gone
                .iter()
                .map(|(at, runs)| {
                    (
                        *at,
                        runs.iter()
                            .flat_map(|r| r.first.0..r.first.0 + r.len)
                            .collect(),
                    )
                })
                .collect();
            assert_eq!(
                took,
                [
                    (1, vec![1, 2]),
                    (5, vec![5]),
                    (7, vec![7, 8, 9]),
                    (12, vec![12])
                ]
            );
            order.insert_ranges(&gone);
            assert_eq!(ids(&order), flat);
        }
    }

    /// The tree stays shallow however many runs splits make: cutting a run must not give
    /// the new half a priority above its new ancestors (it once did, and 40,000
    /// single-row deletes made the tree 3,669 deep: slow, then a stack overflow).
    #[test]
    fn the_tree_stays_shallow_through_many_splits() {
        fn depth(m: &RowMap, t: u32) -> usize {
            let mut deepest = 0;
            let mut stack = vec![(t, 1)];
            while let Some((t, d)) = stack.pop() {
                if t != NIL {
                    deepest = deepest.max(d);
                    let n = &m.nodes[t as usize];
                    stack.extend([(n.l, d + 1), (n.r, d + 1)]);
                }
            }
            deepest
        }
        let n = 200_000u64;
        let mut order = RowOrder::Runs(RowMap::new(run(0, n)));
        let every_other: Vec<_> = (0..n / 2).map(|i| 2 * i..2 * i + 1).collect();
        let gone = order.remove_ranges(&every_other);
        let RowOrder::Runs(m) = &order else {
            unreachable!()
        };
        assert!(
            depth(m, m.root) < 120,
            "{} runs, depth {}",
            n / 2,
            depth(m, m.root)
        );
        order.insert_ranges(&gone);
        let RowOrder::Runs(m) = &order else {
            unreachable!()
        };
        assert!(depth(m, m.root) < 120, "depth {}", depth(m, m.root));
        assert_eq!(order.len(), n);
    }

    #[test]
    fn removed_rows_put_back_restore_the_order_and_reuse_nodes() {
        let mut map = RowMap::new(run(0, 19_000_000));
        map.insert(1_000_000, &[run(1 << 63, 1)]);
        assert_eq!(map.get(1_000_000), RowId(1 << 63));
        assert_eq!(map.get(1_000_001), RowId(1_000_000));
        let gone = map.remove(999_999, 3);
        assert_eq!(gone, [run(999_999, 1), run(1 << 63, 1), run(1_000_000, 1)]);
        let nodes = map.nodes.len();
        map.insert(999_999, &gone);
        assert_eq!(map.get(1_000_000), RowId(1 << 63));
        assert_eq!(map.len(), 19_000_001);
        assert_eq!(map.nodes.len(), nodes, "freed nodes are reused");
        assert_eq!(map.position(RowId(5)), Some(5));
        assert_eq!(map.position(RowId(42 << 40)), None);
    }
}
