//! Toolkit-independent grid state. The UI layer draws only what
//! `Viewport::visible_rows` returns, plus a small buffer (spec section 7).

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
}
