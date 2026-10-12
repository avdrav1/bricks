//! A Fenwick tree: prefix sums with point updates, and the element containing an offset,
//! each in O(log n).

/// Non-negative values with prefix sums.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fenwick {
    /// 1-based tree; `t[0]` is unused.
    t: Vec<i64>,
}

impl Fenwick {
    pub(crate) fn build(values: impl Iterator<Item = i64>) -> Self {
        let mut t = vec![0];
        t.extend(values);
        let n = t.len() - 1;
        for i in 1..=n {
            let j = i + (i & i.wrapping_neg());
            if j <= n {
                t[j] += t[i];
            }
        }
        Fenwick { t }
    }

    pub(crate) fn len(&self) -> usize {
        self.t.len().saturating_sub(1)
    }

    /// Add `delta` to element `i`.
    pub(crate) fn add(&mut self, i: usize, delta: i64) {
        let mut i = i + 1;
        while i < self.t.len() {
            self.t[i] += delta;
            i += i & i.wrapping_neg();
        }
    }

    /// The sum of the first `i` elements.
    pub(crate) fn prefix(&self, i: usize) -> i64 {
        let mut i = i.min(self.len());
        let mut sum = 0;
        while i > 0 {
            sum += self.t[i];
            i -= i & i.wrapping_neg();
        }
        sum
    }

    pub(crate) fn total(&self) -> i64 {
        self.prefix(self.len())
    }

    /// The element whose range holds `offset` (empty elements hold nothing), or the last
    /// element past the end; 0 when there are none.
    pub(crate) fn find(&self, offset: i64) -> usize {
        let n = self.len();
        let mut pos = 0;
        let mut rest = offset;
        let mut step = n.next_power_of_two();
        while step > 0 {
            if pos + step <= n && self.t[pos + step] <= rest {
                pos += step;
                rest -= self.t[pos];
            }
            step >>= 1;
        }
        pos.min(n.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_element_holding_an_offset_and_skips_empty_ones() {
        let values = [10, 0, 5, 0, 0, 7];
        let mut f = Fenwick::build(values.iter().copied());
        assert_eq!(f.total(), 22);
        assert_eq!(f.prefix(3), 15);
        for (offset, want) in [(0, 0), (9, 0), (10, 2), (14, 2), (15, 5), (21, 5), (99, 5)] {
            assert_eq!(f.find(offset), want, "offset {offset}");
        }
        f.add(1, 3);
        assert_eq!(f.find(10), 1);
        assert_eq!(f.find(13), 2);
        assert_eq!(f.total(), 25);
    }
}
