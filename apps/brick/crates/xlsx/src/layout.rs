//! The second pass over a worksheet part (STYLE-1): calamine reads values and formulas;
//! this reads what calamine skips — each cell's style index, row and column styles and
//! sizes, and merged ranges — without looking at values.

use crate::package::Zip;
use crate::xml::{attr, flag, num};
use crate::ImportError;
use data_model::{CellArea, ColSpec, FileLayout, FileStyles, FileStylesBuilder, RowSpec, StyleId};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, Ordering};

type Result<T, E = ImportError> = std::result::Result<T, E>;

/// Events between cancel checks.
const CHECK_EVENTS: u32 = 65_536;

/// Read sheet part `part`'s styles and layout.
pub(crate) fn read_layout(
    zip: &mut Zip,
    part: &str,
    cancel: &AtomicBool,
) -> Result<(FileStyles, FileLayout)> {
    let file = zip
        .by_name(part)
        .map_err(|e| ImportError::Xlsx(format!("{part}: {e}")))?;
    let mut reader = Reader::from_reader(BufReader::with_capacity(1 << 16, file));
    let mut buf = Vec::with_capacity(1024);
    let mut layout = FileLayout::default();
    let mut col_styles = Vec::new();
    let mut builder: Option<FileStylesBuilder> = None;
    // The next row and column when the file leaves out `r` (it may).
    let (mut row, mut next_col) = (0u64, 0u32);
    let mut events = 0u32;
    let mut seen_view = false;
    loop {
        events += 1;
        if events == CHECK_EVENTS {
            events = 0;
            if cancel.load(Ordering::Relaxed) {
                return Err(ImportError::Cancelled);
            }
        }
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| ImportError::Xlsx(format!("{part}: {e}")))?;
        let (e, empty) = match event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) if e.local_name().as_ref() == b"row" => {
                row += 1;
                buf.clear();
                continue;
            }
            Event::Eof => break,
            _ => {
                buf.clear();
                continue;
            }
        };
        {
            match e.local_name().as_ref() {
                b"c" => {
                    let (r, c) = attr(&e, b"r")
                        .and_then(|r| cell_ref(r.as_bytes()))
                        .unwrap_or((row, next_col));
                    next_col = c + 1;
                    let style = StyleId(num(&e, b"s").unwrap_or(0));
                    builder
                        .get_or_insert_with(|| {
                            FileStylesBuilder::new(std::mem::take(&mut col_styles))
                        })
                        .cell(r, c, style);
                }
                b"row" => {
                    row = num::<u64>(&e, b"r").map_or(row, |r| r.saturating_sub(1));
                    next_col = 0;
                    let b = builder.get_or_insert_with(|| {
                        FileStylesBuilder::new(std::mem::take(&mut col_styles))
                    });
                    if flag(&e, b"customFormat") == Some(true) {
                        b.row(row, StyleId(num(&e, b"s").unwrap_or(0)));
                    }
                    if let Some(spec) = row_spec(&e, layout.default_row_height) {
                        layout.push_row(row, spec);
                    }
                    if empty {
                        row += 1; // `<row/>` has no end tag to count
                    }
                }
                b"col" => {
                    let (Some(min), Some(max)) = (num::<u32>(&e, b"min"), num::<u32>(&e, b"max"))
                    else {
                        buf.clear();
                        continue;
                    };
                    let (first, last) = (min.saturating_sub(1), max.saturating_sub(1).min(16_383));
                    if let Some(s) = num::<u32>(&e, b"style") {
                        col_styles.push((first, last, StyleId(s)));
                    }
                    layout.cols.push(ColSpec {
                        first,
                        last,
                        width: num(&e, b"width"),
                        custom_width: flag(&e, b"customWidth").unwrap_or(false),
                        hidden: flag(&e, b"hidden").unwrap_or(false),
                        best_fit: flag(&e, b"bestFit").unwrap_or(false),
                        outline: num(&e, b"outlineLevel").unwrap_or(0),
                        collapsed: flag(&e, b"collapsed").unwrap_or(false),
                    });
                }
                b"sheetFormatPr" => {
                    layout.default_row_height = num(&e, b"defaultRowHeight");
                    layout.default_col_width = num(&e, b"defaultColWidth");
                    layout.base_col_width = num(&e, b"baseColWidth");
                }
                // The first view is the one Excel opens with.
                b"sheetView" if !seen_view => {
                    seen_view = true;
                    layout.hide_grid_lines = flag(&e, b"showGridLines") == Some(false);
                }
                b"mergeCell" => {
                    if let Some(area) = attr(&e, b"ref").and_then(|r| area(r.as_bytes())) {
                        layout.merges.push(area);
                    }
                }
                _ => {}
            }
        }
        buf.clear();
    }
    layout.cols.sort_by_key(|c| c.first);
    let styles = builder
        .unwrap_or_else(|| FileStylesBuilder::new(col_styles))
        .finish();
    Ok((styles, layout))
}

/// A row's spec, if anything about it differs from the sheet's default row.
fn row_spec(e: &BytesStart<'_>, default_height: Option<f64>) -> Option<RowSpec> {
    let spec = RowSpec {
        height: num(e, b"ht"),
        custom_height: flag(e, b"customHeight").unwrap_or(false),
        hidden: flag(e, b"hidden").unwrap_or(false),
        outline: num(e, b"outlineLevel").unwrap_or(0),
        collapsed: flag(e, b"collapsed").unwrap_or(false),
    };
    let default_height = match (spec.height, default_height) {
        (None, _) => true,
        (Some(h), Some(d)) => !spec.custom_height && (h - d).abs() < 1e-6,
        (Some(_), None) => false,
    };
    let plain = default_height && !spec.hidden && spec.outline == 0 && !spec.collapsed;
    (!plain).then_some(spec)
}

/// `B12` as (row 11, column 1); `$` signs are allowed.
pub(crate) fn cell_ref(r: &[u8]) -> Option<(u64, u32)> {
    let mut i = 0;
    let mut col: u32 = 0;
    if r.first() == Some(&b'$') {
        i += 1;
    }
    let letters = i;
    while let Some(&b) = r.get(i).filter(|b| b.is_ascii_alphabetic()) {
        col = col
            .checked_mul(26)?
            .checked_add(u32::from(b.to_ascii_uppercase() - b'A') + 1)?;
        i += 1;
    }
    if i == letters || col == 0 {
        return None;
    }
    if r.get(i) == Some(&b'$') {
        i += 1;
    }
    let digits = std::str::from_utf8(&r[i..]).ok()?;
    let row: u64 = digits.parse().ok().filter(|&n| n > 0)?;
    Some((row - 1, col - 1))
}

/// `A1:C3` (or a single cell) as an area.
fn area(r: &[u8]) -> Option<CellArea> {
    let (a, b) = match r.iter().position(|&c| c == b':') {
        Some(i) => (&r[..i], &r[i + 1..]),
        None => (r, r),
    };
    let ((r1, c1), (r2, c2)) = (cell_ref(a)?, cell_ref(b)?);
    Some(CellArea {
        first_row: r1.min(r2),
        first_col: c1.min(c2),
        last_row: r1.max(r2),
        last_col: c1.max(c2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_read_as_zero_based_positions() {
        assert_eq!(cell_ref(b"A1"), Some((0, 0)));
        assert_eq!(cell_ref(b"$AB$12"), Some((11, 27)));
        assert_eq!(cell_ref(b"XFD1048576"), Some((1_048_575, 16_383)));
        assert_eq!(cell_ref(b"A0"), None);
        assert_eq!(cell_ref(b"12"), None);
        assert_eq!(
            area(b"E8:G9"),
            Some(CellArea {
                first_row: 7,
                first_col: 4,
                last_row: 8,
                last_col: 6
            })
        );
    }
}
