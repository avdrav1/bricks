//! Toolkit-independent grid state. The UI layer draws only the cells [`GridCache`] holds:
//! the visible rows plus a buffer above and below (spec section 7). Memory depends on the
//! viewport, never on the row count.

use data_model::{RowBlock, TableSource};
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub scroll_y: f64,
    pub height: f64,
    pub row_height: f64,
    pub total_rows: u64,
}

impl Viewport {
    /// Rows to render, including `buffer` extra rows above and below.
    pub fn visible_rows(&self, buffer: u64) -> Range<u64> {
        if self.total_rows == 0 || self.row_height <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_y / self.row_height).floor().max(0.0) as u64;
        let count = (self.height / self.row_height).ceil() as u64 + 1;
        let start = first.saturating_sub(buffer);
        let end = (first + count + buffer).min(self.total_rows);
        start.min(end)..end
    }

    /// Y of `row`'s top edge relative to the top of the body (negative when scrolled past).
    /// Exact at any row count: f64 holds row offsets far beyond 2^53 px / row height.
    pub fn row_y(&self, row: u64) -> f64 {
        row as f64 * self.row_height - self.scroll_y
    }
}

/// Horizontal counterpart of [`Viewport`], for fixed-width columns.
#[derive(Debug, Clone, Copy)]
pub struct ColumnViewport {
    pub scroll_x: f64,
    pub width: f64,
    pub col_width: f64,
    pub total_cols: u32,
}

impl ColumnViewport {
    pub fn visible_cols(&self) -> Range<u32> {
        if self.total_cols == 0 || self.col_width <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_x / self.col_width).floor().max(0.0) as u32;
        let count = (self.width / self.col_width).ceil() as u32 + 1;
        first.min(self.total_cols)..first.saturating_add(count).min(self.total_cols)
    }

    pub fn col_x(&self, col: u32) -> f64 {
        col as f64 * self.col_width - self.scroll_x
    }
}

/// The rows the grid currently holds as display text: the visible window plus a buffer.
#[derive(Default)]
pub struct GridCache {
    block: RowBlock,
    cols: u32,
}

impl GridCache {
    /// Make sure every available row of `visible` is held. When one is missing, reload
    /// `visible` widened by `buffer` rows on each side. Returns true if it reloaded.
    pub fn ensure(
        &mut self,
        visible: Range<u64>,
        buffer: u64,
        source: &mut dyn TableSource,
    ) -> bool {
        let total = source.row_count();
        let need = visible.start.min(total)..visible.end.min(total);
        let held = self.block.rows();
        if need.is_empty() || (held.start <= need.start && need.end <= held.end) {
            return false;
        }
        let want = need.start.saturating_sub(buffer)..need.end.saturating_add(buffer).min(total);
        source.read_rows(want.clone(), &mut self.block);
        for r in self.block.rows() {
            self.cols = self.cols.max(self.block.cells_in_row(r));
        }
        true
    }

    pub fn cell(&self, row: u64, col: u32) -> Option<&str> {
        self.block.cell(row, col)
    }

    /// Widest row seen so far; the grid shows this many columns.
    pub fn col_count(&self) -> u32 {
        self.cols
    }

    pub fn heap_bytes(&self) -> usize {
        self.block.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_window_not_the_whole_file() {
        let v = Viewport {
            scroll_y: 2_400_000.0,
            height: 800.0,
            row_height: 24.0,
            total_rows: 2_103_814,
        };
        let r = v.visible_rows(10);
        assert_eq!(r.start, 99_990);
        assert!(r.end - r.start < 100);
    }

    #[test]
    fn visible_columns_clip_to_the_table() {
        let c = ColumnViewport {
            scroll_x: 250.0,
            width: 400.0,
            col_width: 100.0,
            total_cols: 5,
        };
        assert_eq!(c.visible_cols(), 2..5);
        assert_eq!(c.col_x(2), -50.0);
    }

    /// Fake table of any size: every row has 8 cells of fixed-width text.
    struct Synthetic {
        rows: u64,
        reads: u32,
    }

    impl TableSource for Synthetic {
        fn row_count(&self) -> u64 {
            self.rows
        }
        fn is_complete(&self) -> bool {
            true
        }
        fn read_rows(&mut self, rows: Range<u64>, out: &mut RowBlock) {
            self.reads += 1;
            out.reset(rows.start);
            for r in rows.start..rows.end.min(self.rows) {
                for c in 0..8u64 {
                    out.push_cell(format!("{:012}", r * 8 + c).as_bytes());
                }
                out.end_row();
            }
        }
    }

    /// Scroll through the table the way a user flicks and drags, returning peak cache memory.
    fn scroll_through(total_rows: u64) -> usize {
        let mut src = Synthetic {
            rows: total_rows,
            reads: 0,
        };
        let mut cache = GridCache::default();
        let mut peak = 0;
        let mut v = Viewport {
            scroll_y: 0.0,
            height: 1000.0,
            row_height: 22.0,
            total_rows,
        };
        for step in 0..2_000u64 {
            v.scroll_y = if step % 50 == 0 {
                (step * 7_919 % total_rows) as f64 * v.row_height // a scrollbar jump
            } else {
                v.scroll_y + 37.0
            };
            let visible = v.visible_rows(0);
            cache.ensure(visible.clone(), 50, &mut src);
            for r in visible {
                assert_eq!(cache.cell(r, 3).unwrap(), format!("{:012}", r * 8 + 3));
            }
            peak = peak.max(cache.heap_bytes());
        }
        peak
    }

    #[test]
    fn memory_is_the_same_at_any_row_count() {
        let small = scroll_through(10_000);
        let huge = scroll_through(10_000_000_000);
        assert_eq!(small, huge);
        assert!(
            huge < 64 * 1024,
            "cache holds about one screen plus buffer: {huge} bytes"
        );
    }

    #[test]
    fn small_scrolls_reuse_the_buffer() {
        let mut src = Synthetic {
            rows: 1_000_000,
            reads: 0,
        };
        let mut cache = GridCache::default();
        let mut v = Viewport {
            scroll_y: 22_000.0,
            height: 1000.0,
            row_height: 22.0,
            total_rows: 1_000_000,
        };
        for _ in 0..60 {
            cache.ensure(v.visible_rows(0), 50, &mut src);
            v.scroll_y += 11.0; // 30 rows in total
        }
        assert_eq!(src.reads, 1);
        assert_eq!(cache.col_count(), 8);
    }

    #[test]
    fn picks_up_rows_that_arrive_while_indexing() {
        let mut src = Synthetic { rows: 10, reads: 0 };
        let mut cache = GridCache::default();
        assert!(cache.ensure(0..40, 5, &mut src));
        assert_eq!(cache.cell(9, 0).map(str::len), Some(12));
        assert_eq!(cache.cell(10, 0), None);
        assert!(!cache.ensure(0..40, 5, &mut src), "nothing new to load");
        src.rows = 100;
        assert!(cache.ensure(0..40, 5, &mut src));
        assert!(cache.cell(39, 0).is_some());
    }
}
