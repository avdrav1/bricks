//! Pasting text into the table (CLIP-2). Spreadsheets and text editors put a range on the
//! clipboard as TSV; it lands with its top-left cell at the cursor, raw (invariant 1),
//! adding rows at the end when it runs past the last one. Rows that are too short get
//! longer as cells are set past their end, as with any edit.

use crate::{CellRef, Col, CsvTable, Edit, Row, RowBlock, RowId, TableSource};

/// Most cells one paste may set. Each is an edit in the overlay (about 50 bytes plus its
/// text), so this bounds a paste to some 100 MB; the whole file is never pasted cell by
/// cell (invariant 2).
pub const PASTE_MAX_CELLS: u64 = 1 << 21;

/// Rows read at a time while comparing a paste with what is there.
const CHUNK: u64 = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteError {
    /// More cells than [`PASTE_MAX_CELLS`].
    TooLarge(u64),
    /// The paste needs rows added at the end, which waits for indexing to finish.
    NotIndexed,
}

/// TSV the way spreadsheets put it on the clipboard: rows end at `\n`, `\r\n`, or `\r`,
/// cells split at tabs. A cell that starts with a quote is quoted, with `""` for a quote
/// inside, and may hold tabs and line breaks; a quote that doesn't close properly is kept
/// as text. A final line break ends the last row instead of starting an empty one.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    if text.is_empty() {
        return Vec::new();
    }
    let b = text.as_bytes();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut i = 0;
    loop {
        let (cell, end) = quoted_cell(text, i).unwrap_or_else(|| {
            let end = b[i..]
                .iter()
                .position(|&c| matches!(c, b'\t' | b'\n' | b'\r'))
                .map_or(b.len(), |n| i + n);
            (text[i..end].to_owned(), end)
        });
        row.push(cell);
        match b.get(end) {
            Some(b'\t') => i = end + 1,
            Some(&eol) => {
                rows.push(std::mem::take(&mut row));
                i = end + 1 + usize::from(eol == b'\r' && b.get(end + 1) == Some(&b'\n'));
                if i == b.len() {
                    break;
                }
            }
            None => {
                rows.push(row);
                break;
            }
        }
    }
    rows
}

/// A quoted cell starting at byte `i`: its text and where it ends, or `None` when it isn't
/// quoted, or its closing quote isn't followed by a tab, a line break, or the end.
fn quoted_cell(text: &str, i: usize) -> Option<(String, usize)> {
    let b = text.as_bytes();
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let mut out = String::new();
    let mut j = i + 1;
    loop {
        let q = j + b[j..].iter().position(|&c| c == b'"')?;
        out.push_str(&text[j..q]);
        match b.get(q + 1) {
            Some(b'"') => {
                out.push('"');
                j = q + 2;
            }
            None | Some(b'\t' | b'\n' | b'\r') => return Some((out, q + 1)),
            Some(_) => return None,
        }
    }
}

impl CsvTable {
    /// The edits that paste `cells` with its first cell at table row `row`, column `col`:
    /// rows added at the end of the file if it runs past the last row, then every cell set
    /// to its raw text. While filtered, the cells go into the rows shown, and added rows
    /// stay shown (FILT-4). Cells that already show that text, and empty text where a row
    /// has no such cell, are left alone. Run them in order (one undo step:
    /// `commands::Batch`).
    pub fn paste(
        &mut self,
        row: Row,
        col: Col,
        cells: &[Vec<String>],
    ) -> Result<Vec<Edit>, PasteError> {
        let count: u64 = cells.iter().map(|r| r.len() as u64).sum();
        if count > PASTE_MAX_CELLS {
            return Err(PasteError::TooLarge(count));
        }
        let rows = self.row_count();
        let start = row.min(rows);
        let end = start + cells.len() as u64;
        let mut edits = Vec::new();
        let mut added = None;
        if end > rows {
            if !self.is_complete() {
                return Err(PasteError::NotIndexed);
            }
            let insert = self
                .insert_rows(rows, end - rows)
                .ok_or(PasteError::NotIndexed)?;
            if let Edit::InsertRows { rows: runs, .. } = &insert {
                added = runs.first().map(|r| r.first);
            }
            edits.push(insert);
        }
        let mut set = Vec::new();
        let mut block = RowBlock::full_text();
        for (chunk_at, chunk) in (start..end)
            .step_by(CHUNK as usize)
            .zip(cells.chunks(CHUNK as usize))
        {
            let chunk_end = (chunk_at + CHUNK).min(rows);
            if chunk_at < chunk_end {
                self.read_rows(chunk_at..chunk_end, &mut block);
            }
            for (r, values) in (chunk_at..).zip(chunk) {
                let id = match added {
                    Some(first) if r >= rows => RowId(first.0 + (r - rows)),
                    _ => self.row_id(r),
                };
                for (c, value) in (col..).zip(values) {
                    let shown = if r < rows { block.cell(r, c) } else { None };
                    if shown.unwrap_or("") == value && (shown.is_some() || value.is_empty()) {
                        continue;
                    }
                    let at = CellRef {
                        row: id,
                        col: self.col_id(c),
                    };
                    set.push((at, Some(value.as_str().into())));
                }
            }
        }
        if !set.is_empty() {
            edits.push(Edit::Cells(set));
        }
        Ok(edits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn parses_spreadsheet_tsv() {
        assert_eq!(parse_tsv("a\tb\nc\td\n"), strs(&[&["a", "b"], &["c", "d"]]));
        assert_eq!(parse_tsv("a\tb\r\nc\td"), strs(&[&["a", "b"], &["c", "d"]]));
        assert_eq!(parse_tsv("one"), strs(&[&["one"]]));
        assert_eq!(
            parse_tsv("a\t\t\n\tb\n"),
            strs(&[&["a", "", ""], &["", "b"]])
        );
        assert_eq!(
            parse_tsv("\"line one\nline two\"\t\"tab\there\"\t\"say \"\"hi\"\"\"\n"),
            strs(&[&["line one\nline two", "tab\there", "say \"hi\""]])
        );
        assert_eq!(
            parse_tsv("\"open\tx\n\"6\" pipe\tz\n"),
            strs(&[&["\"open", "x"], &["\"6\" pipe", "z"]]),
            "quotes that don't close a quoted cell are text"
        );
        assert_eq!(parse_tsv("a\n\nb\n"), strs(&[&["a"], &[""], &["b"]]));
        assert_eq!(parse_tsv(""), Vec::<Vec<String>>::new());
        assert_eq!(parse_tsv("\n"), strs(&[&[""]]));
    }
}
