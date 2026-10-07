//! Row order (EDIT-3, ADR 0003): which row is shown at each position, as an
//! order-statistic treap of runs of consecutive [`RowId`]s. Source order plus local
//! inserts and deletes stays a handful of runs, so finding, inserting, and removing are
//! O(log runs) at any row count.

use crate::RowId;

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

#[derive(Debug, Clone)]
struct Node {
    run: Run,
    prio: u64,
    l: u32,
    r: u32,
    /// Rows in this subtree.
    sum: u64,
}

#[derive(Debug, Clone)]
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
        // splitmix64: priorities only need to look random.
        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        let node = Node {
            run,
            prio: z ^ (z >> 31),
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
            let ny = self.node(y);
            let rt = self.merge(ny, r);
            (t, rt)
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
