//! Find and replace on the page (SLATE-16): the matches of the find bar's text, found off
//! the UI thread in a copy of the document ([`document_edit::find`]), highlighted in the
//! flow, the current one stronger; stepping through them selects each in turn.
//!
//! Showing a search costs the UI thread nothing per match: the matches stay in document
//! order, and drawing a block looks its own up by binary search.

use document_edit::{Container, Match, Pos};

/// The most matches a search keeps: a one-letter search in a long document stops there
/// (the bar says "100,000+").
pub(super) const MAX_MATCHES: usize = 100_000;

/// The matches shown, in document order.
pub(super) struct Found {
    pub matches: Vec<Match>,
    /// The search stopped at [`MAX_MATCHES`].
    pub capped: bool,
    /// The match the find bar counts from ("3 of 12"), and Replace replaces.
    pub current: Option<usize>,
}

impl Found {
    pub fn new(mut matches: Vec<Match>) -> Found {
        let capped = matches.len() > MAX_MATCHES;
        matches.truncate(MAX_MATCHES);
        Found {
            matches,
            capped,
            current: None,
        }
    }

    /// The matches in the block at flow path `path` (a top-level block, or a block of a
    /// top-level table's cell), with their indices.
    pub fn in_block(&self, path: &[u32]) -> impl Iterator<Item = (usize, &Match)> {
        let (at, block) = match *path {
            [block] => (Container::Top, block as usize),
            [table, row, cell, block] => (
                Container::Cell {
                    table: table as usize,
                    row: row as usize,
                    cell: cell as usize,
                },
                block as usize,
            ),
            _ => (Container::Top, usize::MAX),
        };
        let start = Pos {
            at,
            block,
            offset: 0,
        };
        let end = Pos {
            at,
            block,
            offset: usize::MAX,
        };
        let from = self.matches.partition_point(|m| m.from < start);
        let to = self.matches.partition_point(|m| m.from < end);
        (from..to).map(move |i| (i, &self.matches[i]))
    }

    /// The first match starting at or after `pos`, else the first (wrapping round).
    pub fn at_or_after(&self, pos: Pos) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let i = self.matches.partition_point(|m| m.from < pos);
        Some(if i == self.matches.len() { 0 } else { i })
    }

    /// The match after the current one (before it, when not `forward`) when it's
    /// `selected`, else the first starting at or after `caret` (the last before it);
    /// wrapping round.
    pub fn step(&self, forward: bool, selected: bool, caret: Option<Pos>) -> Option<usize> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        Some(match (self.current.filter(|_| selected), forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => caret.and_then(|c| self.at_or_after(c)).unwrap_or(0),
            (None, false) => {
                let c = caret.unwrap_or(self.matches[0].from);
                let i = self.matches.partition_point(|m| m.from < c);
                (i + n - 1) % n
            }
        })
    }
}
