//! Row heights and column widths (GRID-5). Most rows and columns keep the default size, so
//! only the resized ones are stored, together with the running sum of how much they differ
//! from the default: an offset is one map lookup, at any row count (invariant 2).

use std::collections::BTreeMap;
use std::ops::Range;

/// Smallest size a row or column can be given, so every index covers some pixels.
pub const MIN_SIZE: f64 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Sizes {
    default: f64,
    /// Resized index -> (its size, sum of `size - default` over resized indexes up to and
    /// including this one).
    custom: BTreeMap<u64, (f64, f64)>,
}

impl Sizes {
    pub fn new(default: f64) -> Self {
        assert!(
            default >= MIN_SIZE,
            "default size {default} below {MIN_SIZE}"
        );
        Self {
            default,
            custom: BTreeMap::new(),
        }
    }

    pub fn default_size(&self) -> f64 {
        self.default
    }

    pub fn size(&self, i: u64) -> f64 {
        self.custom.get(&i).map_or(self.default, |&(s, _)| s)
    }

    /// Give index `i` a size (at least [`MIN_SIZE`]); the default size forgets it again.
    pub fn set(&mut self, i: u64, size: f64) {
        let size = size.max(MIN_SIZE);
        if size == self.default {
            self.custom.remove(&i);
        } else {
            self.custom.insert(i, (size, 0.0));
        }
        let mut sum = self.delta_before(i);
        for (_, (s, through)) in self.custom.range_mut(i..) {
            sum += *s - self.default;
            *through = sum;
        }
    }

    /// Back to the default size everywhere.
    pub fn clear(&mut self) {
        self.custom.clear();
    }

    fn delta_before(&self, i: u64) -> f64 {
        self.custom
            .range(..i)
            .next_back()
            .map_or(0.0, |(_, &(_, through))| through)
    }

    /// Where index `i` starts, from the start of index 0. `start(n)` is the total of `n`.
    pub fn start(&self, i: u64) -> f64 {
        i as f64 * self.default + self.delta_before(i)
    }

    /// The index covering position `pos` (clamped at 0); indexes run on past any count.
    pub fn index_at(&self, pos: f64) -> u64 {
        let pos = pos.max(0.0);
        let (mut base, mut base_pos) = (0u64, 0.0);
        for (&k, &(size, _)) in &self.custom {
            let start = self.start(k);
            if start > pos {
                break;
            }
            if pos < start + size {
                return k;
            }
            (base, base_pos) = (k + 1, start + size);
        }
        base + ((pos - base_pos) / self.default).floor() as u64
    }
}

/// One axis of the grid body (rows top to bottom, or columns left to right), scrolled to
/// `scroll` and `extent` pixels long, over `count` rows or columns.
#[derive(Debug, Clone, Copy)]
pub struct Viewport<'a> {
    pub scroll: f64,
    pub extent: f64,
    pub sizes: &'a Sizes,
    pub count: u64,
}

impl Viewport<'_> {
    /// Indexes at least partly on screen, plus `buffer` more on each side.
    pub fn visible(&self, buffer: u64) -> Range<u64> {
        if self.count == 0 {
            return 0..0;
        }
        let first = self.sizes.index_at(self.scroll);
        let last = self.sizes.index_at(self.scroll + self.extent.max(0.0));
        let end = last
            .saturating_add(1)
            .saturating_add(buffer)
            .min(self.count);
        first.saturating_sub(buffer).min(end)..end
    }

    /// Where index `i` starts, relative to the start of the body (negative when scrolled
    /// past). Exact at any count: f64 holds offsets far beyond 2^53 px / size.
    pub fn pos(&self, i: u64) -> f64 {
        self.sizes.start(i) - self.scroll
    }

    pub fn size(&self, i: u64) -> f64 {
        self.sizes.size(i)
    }

    /// The index under body-relative `p`, if there is one.
    pub fn at(&self, p: f64) -> Option<u64> {
        if p < 0.0 {
            return None;
        }
        let i = self.sizes.index_at(self.scroll + p);
        (i < self.count).then_some(i)
    }

    /// The index under body-relative `p`, clamped into the body and the count (dragging
    /// past an edge). `None` only when there is nothing.
    pub fn nearest(&self, p: f64) -> Option<u64> {
        let p = p.clamp(0.0, (self.extent - 1.0).max(0.0));
        let i = self.sizes.index_at(self.scroll + p);
        (self.count > 0).then(|| i.min(self.count - 1))
    }

    /// The index whose far edge (right or bottom) is within `slop` pixels of `p`: where a
    /// press starts resizing it.
    pub fn edge_at(&self, p: f64, slop: f64) -> Option<u64> {
        let i = self.sizes.index_at(self.scroll + p.max(0.0));
        [i.checked_sub(1), Some(i)]
            .into_iter()
            .flatten()
            .filter(|&j| j < self.count)
            .find(|&j| (self.pos(j) + self.size(j) - p).abs() <= slop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resized_entries_shift_everything_after_them() {
        let mut s = Sizes::new(22.0);
        s.set(10, 50.0); // +28
        s.set(3, 2.0); // clamped to 4: -18
        assert_eq!(s.size(3), MIN_SIZE);
        assert_eq!(s.start(3), 66.0);
        assert_eq!(s.start(4), 70.0);
        assert_eq!(s.start(11), 11.0 * 22.0 - 18.0 + 28.0);
        // Far beyond any real file, still exact.
        let n = 10_000_000_000u64;
        assert_eq!(s.start(n), n as f64 * 22.0 + 10.0);
        assert_eq!(s.index_at(s.start(n) + 21.9), n);
        s.set(10, 22.0); // the default size forgets it
        assert_eq!(s.start(11), 11.0 * 22.0 - 18.0);
        assert_eq!(s, {
            let mut t = Sizes::new(22.0);
            t.set(3, 4.0);
            t
        });
    }

    #[test]
    fn index_at_inverts_start() {
        let mut s = Sizes::new(22.0);
        for (i, size) in [(0, 30.0), (1, 5.0), (7, 100.0), (8, 4.0), (40, 60.0)] {
            s.set(i, size);
        }
        for i in 0..60 {
            let (a, b) = (s.start(i), s.start(i + 1));
            assert_eq!(b - a, s.size(i));
            assert_eq!(s.index_at(a), i, "start of {i}");
            assert_eq!(s.index_at(b - 0.01), i, "end of {i}");
        }
        assert_eq!(s.index_at(-5.0), 0);
    }

    #[test]
    fn visible_range_follows_resized_rows() {
        let mut s = Sizes::new(20.0);
        fn v(s: &Sizes, scroll: f64) -> Viewport<'_> {
            Viewport {
                scroll,
                extent: 100.0,
                sizes: s,
                count: 1_000,
            }
        }
        assert_eq!(v(&s, 0.0).visible(0), 0..6);
        s.set(2, 200.0); // row 2 alone fills the rest of the screen
        assert_eq!(v(&s, 0.0).visible(0), 0..3);
        assert_eq!(v(&s, 0.0).visible(2), 0..5);
        assert_eq!(v(&s, 40.0).pos(3), 200.0);
        assert!(
            v(&s, 1_000_000.0).visible(0).is_empty(),
            "scrolled past the end"
        );
        let tail = Viewport {
            count: 4,
            ..v(&s, 0.0)
        };
        assert_eq!(tail.at(250.0), Some(3));
        assert_eq!(tail.at(300.0), None, "below the last row");
        assert_eq!(
            tail.nearest(5_000.0),
            Some(2),
            "clamped to the body, then the rows"
        );
    }

    #[test]
    fn edges_grab_within_slop() {
        let mut s = Sizes::new(100.0);
        s.set(1, 50.0);
        let v = Viewport {
            scroll: 30.0,
            extent: 400.0,
            sizes: &s,
            count: 3,
        };
        // Column 0 ends at 70 on screen, column 1 at 120, column 2 at 220.
        assert_eq!(v.edge_at(68.0, 3.0), Some(0));
        assert_eq!(v.edge_at(72.0, 3.0), Some(0));
        assert_eq!(v.edge_at(95.0, 3.0), None);
        assert_eq!(v.edge_at(121.0, 3.0), Some(1));
        assert_eq!(v.edge_at(220.0, 3.0), Some(2));
        assert_eq!(v.edge_at(320.0, 3.0), None, "no column 3");
    }
}
