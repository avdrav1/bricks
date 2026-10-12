//! Copying a selection for the clipboard (CLIP-1), in the two forms other programs paste:
//!
//! - **TSV**, for text editors and anything else that takes plain text: cells split by
//!   tabs, rows by newlines; a cell holding a tab, line break, or leading quote is quoted
//!   with `""` escapes, the way LibreOffice and Excel read it back.
//! - **An HTML table**, which LibreOffice and Excel paste straight into cells (for plain
//!   text LibreOffice asks how to split it first). Values a spreadsheet would change on
//!   the way in are marked as text, in LibreOffice's (`sdnum`) and Excel's
//!   (`mso-number-format`) terms: leading zeros (`00123`), digit strings past 15 digits
//!   (IDs a float can't hold), and a leading `=` (it would become a formula). Spaces that
//!   HTML would collapse are `&nbsp;`, which LibreOffice reads back as plain spaces.
//!
//! Both are built from a snapshot of the table on a worker, with progress and cancel.

use crate::{
    CellRef, Col, CsvTable, Edit, Fill, Pattern, Row, RowBlock, RowId, StyleEdit, StyleId,
    StylePool, StyleRange, TableSource, Underline,
};
use std::fmt::Write as _;
use std::ops::{Bound, Range, RangeBounds};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Cells the HTML table is made for, at most. Spreadsheets take about a million rows; a
/// bigger copy is plain text only, which is also a third of the memory.
pub const HTML_MAX_CELLS: u64 = 1 << 20;

/// Rows read at a time; progress and cancel are checked between them.
const CHUNK: u64 = 4_096;

/// A selection's clipboard text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub tsv: String,
    /// `None` past [`HTML_MAX_CELLS`].
    pub html: Option<String>,
    pub cells: u64,
}

const HTML_START: &str = "<html><head><meta charset=\"utf-8\"></head><body><table>\n";
const HTML_END: &str = "</table></body></html>\n";

/// Cells' styles in a copied block (STYLE-7), row by row, `width` to a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleBlock {
    pub width: u32,
    pub styles: Vec<StyleId>,
}

impl StyleBlock {
    pub fn rows(&self) -> u64 {
        if self.width == 0 {
            0
        } else {
            (self.styles.len() / self.width as usize) as u64
        }
    }
}

impl CsvTable {
    /// The text of table rows `rows` in columns `cols`. A bounded column range gives every
    /// row the same number of cells (short rows padded), so the copy is a rectangle; an
    /// open end (`2..`, whole rows) takes each row's own cells. `done` counts rows copied;
    /// `None` once `cancel` is set.
    pub fn copy_range(
        &mut self,
        rows: Range<Row>,
        cols: impl RangeBounds<Col>,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Option<Copied> {
        self.copy_cells(rows, cols, None, cancel, done)
    }

    /// [`Self::copy_range`], with each cell's look written into the HTML for other
    /// applications (STYLE-7): bold, italic, underline, strikethrough, colours.
    pub fn copy_range_styled(
        &mut self,
        rows: Range<Row>,
        cols: impl RangeBounds<Col>,
        styles: &StylePool,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Option<Copied> {
        self.copy_cells(rows, cols, Some(styles), cancel, done)
    }

    /// The styles of table rows `rows` in columns `cols`, for a paste back into Brick
    /// (STYLE-7).
    pub fn copy_styles(&self, rows: Range<Row>, cols: Range<Col>) -> StyleBlock {
        let rows = rows.start..rows.end.min(self.row_count());
        let mut styles = Vec::with_capacity((rows.end - rows.start) as usize * cols.len());
        let col_ids: Vec<_> = cols.clone().map(|c| self.col_id(c)).collect();
        for r in rows {
            let row = self.row_id(r);
            styles.extend(
                col_ids
                    .iter()
                    .map(|&col| self.style_of(CellRef { row, col })),
            );
        }
        StyleBlock {
            width: cols.len() as u32,
            styles,
        }
    }

    /// The edit that gives the cells a paste wrote (`pasted`, from [`Self::paste`] at table
    /// row `row`, column `col`, past the last row too) the styles of `block`; run it after
    /// them, in the same undo step. `None` if there is nothing to style.
    pub fn paste_styles(
        &self,
        pasted: &[Edit],
        row: Row,
        col: Col,
        block: &StyleBlock,
    ) -> Option<Edit> {
        let (rows, start) = (self.row_count(), row);
        let added = pasted.iter().find_map(|e| match e {
            Edit::InsertRows { rows, .. } => rows.first().map(|r| r.first),
            _ => None,
        });
        let mut edit = StyleEdit::default();
        for c in 0..block.width {
            let id = self.col_id(col + c);
            let mut ranges: Vec<StyleRange> = Vec::new();
            for r in 0..block.rows() {
                let at = start + r;
                let row_id = match added {
                    Some(first) if at >= rows => RowId(first.0 + (at - rows)),
                    _ if at >= rows => continue,
                    _ => self.row_id(at),
                };
                let style = block.styles[(r * u64::from(block.width) + u64::from(c)) as usize];
                match ranges.last_mut() {
                    Some(last) if last.1 == row_id.0 && last.2 == Some(style) => last.1 += 1,
                    _ => ranges.push((row_id.0, row_id.0 + 1, Some(style))),
                }
            }
            if !ranges.is_empty() {
                edit.cells.push((id, ranges));
            }
        }
        (!edit.is_empty()).then_some(Edit::Styles(edit))
    }

    fn copy_cells(
        &mut self,
        rows: Range<Row>,
        cols: impl RangeBounds<Col>,
        styles: Option<&StylePool>,
        cancel: &AtomicBool,
        done: &AtomicU64,
    ) -> Option<Copied> {
        let start = match cols.start_bound() {
            Bound::Included(&c) => c,
            Bound::Excluded(&c) => c.saturating_add(1),
            Bound::Unbounded => 0,
        };
        let end = match cols.end_bound() {
            Bound::Included(&c) => Some(c.saturating_add(1)),
            Bound::Excluded(&c) => Some(c),
            Bound::Unbounded => None,
        };
        let rows = rows.start..rows.end.min(self.row_count());
        let mut out = Copied {
            tsv: String::new(),
            html: Some(HTML_START.to_owned()),
            cells: 0,
        };
        let mut block = RowBlock::full_text();
        let mut at = rows.start;
        while at < rows.end {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let chunk = at..rows.end.min(at + CHUNK);
            self.read_rows(chunk.clone(), &mut block);
            for r in chunk.clone() {
                if r > rows.start {
                    out.tsv.push('\n');
                }
                let stop = end.unwrap_or_else(|| block.cells_in_row(r)).max(start);
                out.cells += u64::from(stop - start);
                if out.cells > HTML_MAX_CELLS {
                    out.html = None;
                }
                if let Some(html) = &mut out.html {
                    html.push_str("<tr>");
                }
                let row = self.row_id(r);
                for c in start..stop {
                    let value = block.cell(r, c).unwrap_or("");
                    if c > start {
                        out.tsv.push('\t');
                    }
                    push_tsv(value, &mut out.tsv);
                    if let Some(html) = &mut out.html {
                        let css = styles.map(|pool| {
                            let at = CellRef {
                                row,
                                col: self.col_id(c),
                            };
                            css_of(pool, self.style_of(at))
                        });
                        push_html_cell(value, css.as_deref().unwrap_or(""), html);
                    }
                }
                if let Some(html) = &mut out.html {
                    html.push_str("</tr>\n");
                }
            }
            at = chunk.end;
            done.store(at - rows.start, Ordering::Relaxed);
        }
        if let Some(html) = &mut out.html {
            html.push_str(HTML_END);
        }
        Some(out)
    }
}

/// One TSV field: as it is, or quoted when it holds what would split it or start a
/// quoted field.
fn push_tsv(value: &str, out: &mut String) {
    if value.contains(['\t', '\n', '\r']) || value.starts_with('"') {
        out.push('"');
        out.push_str(&value.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(value);
    }
}

/// Whether a spreadsheet would change `value` when it reads it as a number or formula.
fn needs_text_format(value: &str) -> bool {
    let digits = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    value.starts_with('=')
        || (digits && (value.len() > 15 || (value.len() > 1 && value.starts_with('0'))))
}

/// A style as CSS for a `<td>`: what other applications read of it (STYLE-7).
fn css_of(pool: &StylePool, id: StyleId) -> String {
    let xf = pool.xf(id);
    let font = pool.font(xf);
    let mut css = String::new();
    if font.bold {
        css.push_str("font-weight:bold;");
    }
    if font.italic {
        css.push_str("font-style:italic;");
    }
    match (font.underline != Underline::None, font.strike) {
        (true, true) => css.push_str("text-decoration:underline line-through;"),
        (true, false) => css.push_str("text-decoration:underline;"),
        (false, true) => css.push_str("text-decoration:line-through;"),
        (false, false) => {}
    }
    if let Some(rgb) = pool
        .rgb(font.color)
        .filter(|_| font.color != pool.default_font().color)
    {
        let _ = write!(css, "color:#{rgb:06x};");
    }
    if let Some(Fill::Pattern {
        pattern: Pattern::Solid,
        fg,
        ..
    }) = pool.fill(xf)
    {
        if let Some(rgb) = pool.rgb(*fg) {
            let _ = write!(css, "background-color:#{rgb:06x};");
        }
    }
    css
}

/// One `<td>`: text escaped, line breaks as `<br>`, tabs kept, spaces kept where HTML
/// would collapse them; `css` its look.
fn push_html_cell(value: &str, css: &str, out: &mut String) {
    if needs_text_format(value) {
        out.push_str("<td sdnum=\"1033;0;@\" style=\"mso-number-format:'\\@'");
        if !css.is_empty() {
            out.push(';');
            out.push_str(css);
        }
        out.push_str("\">");
    } else if css.is_empty() {
        out.push_str("<td>");
    } else {
        out.push_str("<td style=\"");
        out.push_str(css);
        out.push_str("\">");
    }
    let keep_spaces = value.starts_with(' ')
        || value.ends_with(' ')
        || value.contains("  ")
        || value.contains('\n');
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' | '\n' => out.push_str("<br>"),
            ' ' if keep_spaces => out.push_str("&nbsp;"),
            _ => out.push(ch),
        }
    }
    out.push_str("</td>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_quotes_only_what_would_split_a_field() {
        let mut s = String::new();
        for v in [
            "plain",
            "a\tb",
            "two\nlines",
            "\"quoted\" start",
            "mid \"q\"",
            "",
        ] {
            push_tsv(v, &mut s);
            s.push('|');
        }
        assert_eq!(
            s,
            "plain|\"a\tb\"|\"two\nlines\"|\"\"\"quoted\"\" start\"|mid \"q\"||"
        );
    }

    #[test]
    fn html_marks_values_a_spreadsheet_would_change() {
        let cell = |v: &str| {
            let mut s = String::new();
            push_html_cell(v, "", &mut s);
            s
        };
        let text = "<td sdnum=\"1033;0;@\" style=\"mso-number-format:'\\@'\">";
        assert_eq!(cell("00123"), format!("{text}00123</td>"));
        assert_eq!(
            cell("1234567890123456"),
            format!("{text}1234567890123456</td>")
        );
        assert_eq!(cell("=1+1"), format!("{text}=1+1</td>"));
        assert_eq!(cell("0"), "<td>0</td>");
        assert_eq!(cell("123"), "<td>123</td>");
        assert_eq!(cell("2.50"), "<td>2.50</td>");
        assert_eq!(cell("a & <b>"), "<td>a &amp; &lt;b&gt;</td>");
        assert_eq!(cell("l1\r\nl2"), "<td>l1<br>l2</td>");
        assert_eq!(cell(" x  y"), "<td>&nbsp;x&nbsp;&nbsp;y</td>");
        assert_eq!(cell("a b"), "<td>a b</td>");
        assert_eq!(cell("1\t2"), "<td>1&#9;2</td>");
    }

    fn table(name: &str, content: &str) -> (std::path::PathBuf, CsvTable) {
        let path = std::env::temp_dir().join(format!("copy-{name}-{}.csv", std::process::id()));
        std::fs::write(&path, content).unwrap();
        let t = CsvTable::open(&path, crate::DelimiterChoice::Auto).unwrap();
        t.index().build(&AtomicBool::new(false)).unwrap();
        (path, t)
    }

    fn copy(t: &mut CsvTable, rows: Range<Row>, cols: impl RangeBounds<Col>) -> Copied {
        let done = AtomicU64::new(0);
        let out = t.copy_range(rows.clone(), cols, &AtomicBool::new(false), &done);
        assert_eq!(done.load(Ordering::Relaxed), rows.end - rows.start);
        out.unwrap()
    }

    /// A range copies what the grid shows, with whole values: edits, cleared cells, short
    /// rows padded to the rectangle, cells longer than the display limit kept whole.
    /// Whole rows take each row's own cells. A snapshot copies the table as it was.
    #[test]
    fn copies_what_the_table_shows_as_a_rectangle() {
        let long = "x".repeat(1000);
        let (path, mut t) = table(
            "range",
            &format!("id,name,code\n1,\"Ann, A\",00123\n2,Bo\n3,{long},z,extra\n"),
        );
        let at = |t: &CsvTable, r, c| crate::CellRef {
            row: t.row_id(r),
            col: t.col_id(c),
        };
        t.apply(crate::Edit::set(at(&t, 0, 0), "one"));
        let clear = t.clear_cells(1..2, 0..1).unwrap();
        t.apply(clear);

        let rect = copy(&mut t, 0..3, 0..3);
        assert_eq!(
            rect.tsv,
            format!("one\tAnn, A\t00123\n\tBo\t\n3\t{long}\tz")
        );
        assert_eq!(rect.cells, 9);
        let html = rect.html.unwrap();
        assert!(html.starts_with("<html><head><meta charset=\"utf-8\">"));
        assert!(html.contains("<tr><td>one</td><td>Ann, A</td><td sdnum="));
        assert!(html.contains("<tr><td></td><td>Bo</td><td></td></tr>"));

        let rows = copy(&mut t, 1..3, 1..);
        assert_eq!(rows.tsv, format!("Bo\n{long}\tz\textra"));
        assert_eq!(rows.cells, 4);

        let mut before = t.snapshot();
        t.apply(crate::Edit::set(at(&t, 0, 1), "changed later"));
        assert_eq!(copy(&mut before, 0..1, 1..2).tsv, "Ann, A");
        assert_eq!(copy(&mut t, 0..1, 1..2).tsv, "changed later");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn cancel_stops_the_copy() {
        let (path, mut t) = table("cancel", "a\n1\n2\n");
        let out = t.copy_range(0..2, 0..1, &AtomicBool::new(true), &AtomicU64::new(0));
        assert_eq!(out, None);
        std::fs::remove_file(path).unwrap();
    }
}
