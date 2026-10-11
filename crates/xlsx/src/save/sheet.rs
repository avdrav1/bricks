//! A worksheet part on save (ADR 0010). With the file's part: everything but the cell
//! data, dimension, columns, and merged ranges is copied as written; while rows and
//! columns keep their order, so are unchanged cells, and only changed and formula cells
//! are written. After inserts, deletes, or a sort every cell is written from the model,
//! and range attributes (`sqref`, `ref`) follow their rows and columns. Without a part,
//! the sheet is written whole.

use super::cells::{cell_name, escape, write_cell, CellStyles};
use super::SaveSheet;
use crate::layout::cell_ref;
use crate::xml::{attr, num};
use data_model::{CellArea, CellRef, ColId, ColSpec, RowBlock, RowId, SaveProgress, StyleId};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

type Result<T, E = io::Error> = std::result::Result<T, E>;

/// Rows read at a time.
const CHUNK: u64 = 65_536;

/// The order of a worksheet's children (CT_Worksheet).
const ORDER: [&[u8]; 39] = [
    b"sheetPr",
    b"dimension",
    b"sheetViews",
    b"sheetFormatPr",
    b"cols",
    b"sheetData",
    b"sheetCalcPr",
    b"sheetProtection",
    b"protectedRanges",
    b"scenarios",
    b"autoFilter",
    b"sortState",
    b"dataConsolidate",
    b"customSheetViews",
    b"mergeCells",
    b"phoneticPr",
    b"conditionalFormatting",
    b"dataValidations",
    b"hyperlinks",
    b"printOptions",
    b"pageMargins",
    b"pageSetup",
    b"headerFooter",
    b"rowBreaks",
    b"colBreaks",
    b"customProperties",
    b"cellWatches",
    b"ignoredErrors",
    b"smartTags",
    b"drawing",
    b"legacyDrawing",
    b"legacyDrawingHF",
    b"drawingHF",
    b"picture",
    b"oleObjects",
    b"controls",
    b"webPublishItems",
    b"tableParts",
    b"extLst",
];

fn rank(name: &[u8]) -> usize {
    ORDER.iter().position(|n| *n == name).unwrap_or(ORDER.len())
}

fn xml_error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("a worksheet doesn't read as XML: {e}"))
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "save cancelled")
}

/// Write `sheet` as a worksheet part to `out`; `src` is the file's part, if it had one.
pub(crate) fn write_sheet(
    out: &mut dyn Write,
    src: Option<&[u8]>,
    sheet: &mut SaveSheet,
    styles: &mut CellStyles<'_>,
    progress: &SaveProgress,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut w = Writer {
        sheet,
        styles,
        progress,
        cancel,
        buf: String::with_capacity(1 << 16),
    };
    match src {
        Some(src) => w.patched(out, src),
        None => w.whole(out),
    }
}

struct Writer<'s, 'p, 'q> {
    sheet: &'s mut SaveSheet,
    styles: &'s mut CellStyles<'p>,
    progress: &'q SaveProgress,
    cancel: &'q AtomicBool,
    /// Output waiting to go out; flushed past 64 KiB.
    buf: String,
}

impl Writer<'_, '_, '_> {
    fn flush(&mut self, out: &mut dyn Write, force: bool) -> Result<()> {
        if force || self.buf.len() > 1 << 16 {
            out.write_all(self.buf.as_bytes())?;
            self.buf.clear();
        }
        Ok(())
    }

    fn push_bytes(&mut self, raw: &[u8]) -> Result<()> {
        self.buf
            .push_str(std::str::from_utf8(raw).map_err(|_| xml_error("not UTF-8"))?);
        Ok(())
    }

    /// The sheet's used range for `<dimension>`: every row, and as many columns as its
    /// widest row (or the file's dimension) has.
    fn dimension(&mut self, file_cols: Option<u32>) -> String {
        let table = &mut self.sheet.table;
        let rows = table.total_rows();
        let cols = match file_cols {
            Some(c) => {
                // The file's width, moved by inserted and deleted columns, and edits past it.
                let edited = table
                    .file_edits()
                    .values()
                    .filter_map(|cols| cols.last())
                    .filter_map(|&c| table.col_of(ColId::source(c)))
                    .max()
                    .map_or(0, |c| c + 1);
                table.view_cols(c).max(edited)
            }
            None => {
                let mut block = RowBlock::full_text();
                let mut widest = 0;
                let mut start = 0;
                while start < rows {
                    let end = (start + CHUNK).min(rows);
                    table.read_all(start..end, &mut block);
                    for r in start..end {
                        widest = widest.max(block.cells_in_row(r));
                    }
                    start = end;
                }
                widest
            }
        };
        if rows == 0 || cols == 0 {
            return "A1".into();
        }
        let mut s = String::from("A1:");
        cell_name(rows - 1, cols - 1, &mut s);
        s
    }

    /// A worksheet from nothing (a CSV sheet, a new one).
    fn whole(&mut self, out: &mut dyn Write) -> Result<()> {
        let dimension = self.dimension(None);
        let selected = if self.sheet.active {
            " tabSelected=\"1\""
        } else {
            ""
        };
        let _ = write!(
            self.buf,
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><dimension ref=\"{dimension}\"/><sheetViews><sheetView{selected} workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>"
        );
        self.cols("");
        self.buf.push_str("<sheetData>");
        self.fresh_rows(out)?;
        self.buf.push_str("</sheetData>");
        self.merges("");
        self.buf.push_str("</worksheet>");
        self.flush(out, true)
    }

    /// The file's worksheet, patched.
    fn patched(&mut self, out: &mut dyn Write, src: &[u8]) -> Result<()> {
        let in_order = self.sheet.table.in_file_order();
        let file_cols = file_dimension_cols(src);
        let mut reader = Reader::from_reader(src);
        let mut depth = 0usize;
        let (mut wrote_dim, mut wrote_cols, mut wrote_merges) = (false, false, false);
        // An element being left out, and the depth it started at.
        let mut skip: Option<(Vec<u8>, usize)> = None;
        let mut p = String::new();
        loop {
            let start = reader.buffer_position() as usize;
            let event = reader.read_event().map_err(xml_error)?;
            let end = reader.buffer_position() as usize;
            let raw = &src[start..end];
            if let Some((name, at)) = &skip {
                match &event {
                    Event::Start(_) => depth += 1,
                    Event::End(e) => {
                        depth -= 1;
                        if depth == *at && e.local_name().as_ref() == name.as_slice() {
                            skip = None;
                        }
                    }
                    Event::Eof => return Err(xml_error("the part ends early")),
                    _ => {}
                }
                continue;
            }
            let (e, empty) = match &event {
                Event::Eof => break,
                Event::Start(e) => (e, false),
                Event::Empty(e) => (e, true),
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        // The worksheet's end: what it didn't have goes before it.
                        if !wrote_dim {
                            let d = self.dimension(file_cols);
                            let _ = write!(self.buf, "<{p}dimension ref=\"{d}\"/>");
                        }
                        if !wrote_cols {
                            self.cols(&p);
                        }
                        if !wrote_merges {
                            self.merges(&p);
                        }
                    }
                    self.push_bytes(raw)?;
                    self.flush(out, false)?;
                    continue;
                }
                _ => {
                    self.push_bytes(raw)?;
                    continue;
                }
            };
            let name = e.local_name().as_ref().to_vec();
            if depth == 0 {
                p = prefix_of(e);
            }
            if depth == 1 {
                // What the file lacks goes where it belongs among the children.
                let r = rank(&name);
                if r != ORDER.len() {
                    if !wrote_dim && r > rank(b"dimension") {
                        let d = self.dimension(file_cols);
                        let _ = write!(self.buf, "<{p}dimension ref=\"{d}\"/>");
                        wrote_dim = true;
                    }
                    if !wrote_cols && r > rank(b"cols") {
                        self.cols(&p);
                        wrote_cols = true;
                    }
                    if !wrote_merges && r > rank(b"mergeCells") {
                        self.merges(&p);
                        wrote_merges = true;
                    }
                }
                match name.as_slice() {
                    b"dimension" | b"cols" | b"mergeCells" => {
                        match name.as_slice() {
                            b"dimension" => {
                                let d = self.dimension(file_cols);
                                let _ = write!(self.buf, "<{p}dimension ref=\"{d}\"/>");
                                wrote_dim = true;
                            }
                            b"cols" => {
                                self.cols(&p);
                                wrote_cols = true;
                            }
                            _ => {
                                self.merges(&p);
                                wrote_merges = true;
                            }
                        }
                        if !empty {
                            skip = Some((name, depth));
                            depth += 1;
                        }
                        continue;
                    }
                    b"sheetData" => {
                        let _ = write!(self.buf, "<{p}sheetData>");
                        if empty {
                            self.fresh_rows(out)?;
                        } else if in_order {
                            self.rows_in_order(out, &mut reader, src, &p)?;
                        } else {
                            skip_to_end(&mut reader, b"sheetData")?;
                            self.fresh_rows(out)?;
                        }
                        let _ = write!(self.buf, "</{p}sheetData>");
                        self.flush(out, false)?;
                        continue;
                    }
                    _ => {}
                }
            }
            if !in_order && has_range_attr(e) {
                match self.mapped_tag(e, empty) {
                    Some(t) => self.buf.push_str(&t),
                    None => {
                        // Every range it had was deleted: so is the element.
                        if !empty {
                            skip = Some((name, depth));
                            depth += 1;
                        }
                        continue;
                    }
                }
            } else {
                self.push_bytes(raw)?;
            }
            if !empty {
                depth += 1;
            }
            self.flush(out, false)?;
        }
        self.flush(out, true)
    }

    /// A start tag with its range attributes (`sqref`, `ref`, `activeCell`) moved to where
    /// their rows and columns are now; `None` if all its ranges are gone.
    fn mapped_tag(&self, e: &BytesStart<'_>, empty: bool) -> Option<String> {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let mut out = format!("<{name}");
        for a in e.attributes().flatten() {
            let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
            let value = crate::xml::value(a.clone())
                .map(|v| v.into_owned())
                .unwrap_or_default();
            let value = match a.key.local_name().as_ref() {
                b"sqref" | b"ref" | b"activeCell" if !key.contains(':') => {
                    let mapped = self.map_ranges(&value);
                    if mapped.is_empty() {
                        return None;
                    }
                    mapped
                }
                _ => value,
            };
            out.push(' ');
            out.push_str(&key);
            out.push_str("=\"");
            escape(&value, &mut out);
            out.push('"');
        }
        out.push_str(if empty { "/>" } else { ">" });
        Some(out)
    }

    /// A space-separated list of ranges (`A1:B3 D4`) where their rows and columns are
    /// now; ranges that are gone are left out.
    fn map_ranges(&self, list: &str) -> String {
        let table = &self.sheet.table;
        let mut out = String::new();
        for part in list.split_whitespace() {
            let Some(area) = parse_area(part) else {
                continue;
            };
            let (Some((r1, r2)), Some((c1, c2))) = (
                table.file_rows_now(area.first_row, area.last_row),
                table.file_cols_now(area.first_col, area.last_col),
            ) else {
                continue;
            };
            if !out.is_empty() {
                out.push(' ');
            }
            cell_name(r1, c1, &mut out);
            if (r1, c1) != (r2, c2) {
                out.push(':');
                cell_name(r2, c2, &mut out);
            }
        }
        out
    }

    /// The `<cols>` element: the file's column specs and styles where their columns are
    /// now, and the widths the user set.
    fn cols(&mut self, p: &str) {
        let table = &self.sheet.table;
        let layout = table.layout();
        let styles = table.file_styles();
        let widths: HashMap<ColId, f64> = self.sheet.widths.iter().copied().collect();
        // Every file column a spec or a style covers (a style may run to column XFD),
        // where it is now, and the columns the user sized.
        let file_width = layout
            .cols
            .iter()
            .map(|c| c.last + 1)
            .chain(styles.column_ranges().iter().map(|c| c.1 + 1))
            .max()
            .unwrap_or(0);
        let sized = widths
            .keys()
            .filter_map(|&id| table.col_of(id))
            .max()
            .map_or(0, |c| c + 1);
        let shown = table.view_cols(file_width).max(sized).min(16_384);
        // Neighbours that read the same join into one range.
        let mut ranges: Vec<(u32, u32, ColAttrs)> = Vec::new();
        for c in 0..shown {
            let id = table.col_id(c);
            let style = if id.is_inserted() {
                StyleId(0)
            } else {
                styles.column(id.0)
            };
            let a = ColAttrs::of(table.col_spec(id), style, widths.get(&id).copied());
            match ranges.last_mut() {
                Some(prev) if prev.1 + 1 == c && prev.2 == a => prev.1 = c,
                _ => ranges.push((c, c, a)),
            }
        }
        ranges.retain(|r| r.2 != ColAttrs::default());
        if ranges.is_empty() {
            return;
        }
        let _ = write!(self.buf, "<{p}cols>");
        for (first, last, a) in ranges {
            let _ = write!(
                self.buf,
                "<{p}col min=\"{}\" max=\"{}\"",
                first + 1,
                last + 1
            );
            if let Some(w) = a.width {
                let _ = write!(self.buf, " width=\"{}\"", f64::from_bits(w));
            }
            if a.style != 0 {
                let _ = write!(self.buf, " style=\"{}\"", a.style);
            }
            for (flag, name) in [
                (a.hidden, "hidden"),
                (a.custom_width, "customWidth"),
                (a.best_fit, "bestFit"),
                (a.collapsed, "collapsed"),
            ] {
                if flag {
                    let _ = write!(self.buf, " {name}=\"1\"");
                }
            }
            if a.outline != 0 {
                let _ = write!(self.buf, " outlineLevel=\"{}\"", a.outline);
            }
            self.buf.push_str("/>");
        }
        let _ = write!(self.buf, "</{p}cols>");
    }

    /// `<mergeCells>` from the sheet's merged ranges.
    fn merges(&mut self, p: &str) {
        let merges = sheet_merges(&self.sheet.table);
        if merges.is_empty() {
            return;
        }
        let _ = write!(self.buf, "<{p}mergeCells count=\"{}\">", merges.len());
        for m in merges {
            let _ = write!(self.buf, "<{p}mergeCell ref=\"");
            cell_name(m.first_row, m.first_col, &mut self.buf);
            self.buf.push(':');
            cell_name(m.last_row, m.last_col, &mut self.buf);
            self.buf.push_str("\"/>");
        }
        let _ = write!(self.buf, "</{p}mergeCells>");
    }

    /// The row heights the user set, by row id, in points.
    fn heights(&self) -> HashMap<RowId, f64> {
        self.sheet.heights.iter().copied().collect()
    }

    /// Every row written from the model, in its current order.
    fn fresh_rows(&mut self, out: &mut dyn Write) -> Result<()> {
        let heights = self.heights();
        let total = self.sheet.table.total_rows();
        let style_cols = {
            let t = &self.sheet.table;
            (0..t.file_styles().width())
                .filter_map(|c| t.col_of(ColId::source(c)))
                .max()
                .map_or(0, |c| c + 1)
        };
        let mut block = RowBlock::full_text();
        let mut start = 0;
        while start < total {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let end = (start + CHUNK).min(total);
            self.sheet.table.read_all(start..end, &mut block);
            for r in start..end {
                self.fresh_row(r, &block, style_cols, &heights);
                self.flush(out, false)?;
            }
            self.progress.add_written(end - start);
            start = end;
        }
        Ok(())
    }

    /// Row at position `r` (its text in `block`), cells and styled empty cells.
    fn fresh_row(
        &mut self,
        r: u64,
        block: &RowBlock,
        style_cols: u32,
        heights: &HashMap<RowId, f64>,
    ) {
        let table = &self.sheet.table;
        let id = table.id_at(r);
        let spec = table.row_spec(id).cloned().unwrap_or_default();
        let row_style = (!id.is_inserted())
            .then(|| table.file_styles().row(id.0))
            .flatten();
        let height = heights.get(&id).copied();
        let mut row_attrs = String::new();
        if let Some(s) = row_style {
            let _ = write!(row_attrs, " s=\"{}\" customFormat=\"1\"", s.0);
        }
        match height {
            Some(h) => {
                let _ = write!(row_attrs, " ht=\"{h}\" customHeight=\"1\"");
            }
            None => {
                if let Some(h) = spec.height {
                    let _ = write!(row_attrs, " ht=\"{h}\"");
                }
                if spec.custom_height {
                    row_attrs.push_str(" customHeight=\"1\"");
                }
            }
        }
        if spec.hidden {
            row_attrs.push_str(" hidden=\"1\"");
        }
        if spec.outline != 0 {
            let _ = write!(row_attrs, " outlineLevel=\"{}\"", spec.outline);
        }
        if spec.collapsed {
            row_attrs.push_str(" collapsed=\"1\"");
        }
        let n = block.cells_in_row(r).max(style_cols);
        let mut cells = String::new();
        for c in 0..n {
            let text = block.cell(r, c).unwrap_or("");
            let col = table.col_id(c);
            let at = CellRef { row: id, col };
            let style = table.style_of(at);
            if text.is_empty() {
                // A styled empty cell is written only where its row or column wouldn't
                // give it that style anyway.
                let fallback = match row_style {
                    Some(s) => s,
                    None if col.is_inserted() => StyleId(0),
                    None => table.file_styles().column(col.0),
                };
                if style == fallback {
                    continue;
                }
            }
            let result = self.sheet.computed.get(at).map(str::to_owned);
            write_cell(
                &mut cells,
                r,
                c,
                text,
                style,
                result.as_deref(),
                self.styles,
            );
        }
        if cells.is_empty() && row_attrs.is_empty() {
            return;
        }
        let _ = write!(self.buf, "<row r=\"{}\"{row_attrs}", r + 1);
        if cells.is_empty() {
            self.buf.push_str("/>");
        } else {
            self.buf.push('>');
            self.buf.push_str(&cells);
            self.buf.push_str("</row>");
        }
    }

    /// The file's rows, in file order, with changed and formula cells written anew and the
    /// others copied as written. Reads up to and including `</sheetData>`.
    fn rows_in_order(
        &mut self,
        out: &mut dyn Write,
        reader: &mut Reader<&[u8]>,
        src: &[u8],
        p: &str,
    ) -> Result<()> {
        let mut edits = self.sheet.table.file_edits();
        let heights = self.heights();
        let mut next_row = 0u64;
        let mut events = 0u32;
        loop {
            events += 1;
            if events.is_multiple_of(4096) && self.cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let start = reader.buffer_position() as usize;
            let event = reader.read_event().map_err(xml_error)?;
            let end = reader.buffer_position() as usize;
            match &event {
                Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"row" => {
                    let empty = matches!(event, Event::Empty(_));
                    let raw = std::str::from_utf8(&src[start..end])
                        .map_err(|_| xml_error("not UTF-8"))?;
                    let r = num::<u64>(e, b"r").map_or(next_row, |r| r.saturating_sub(1));
                    next_row = r + 1;
                    // Rows the file leaves out but that have edits now.
                    while let Some((&er, _)) = edits.first_key_value() {
                        if er >= r {
                            break;
                        }
                        let cols = edits.pop_first().expect("a first entry").1;
                        self.edited_row(er, &cols, &heights, p);
                    }
                    let extra = edits.remove(&r).unwrap_or_default();
                    self.file_row(reader, src, e, raw, empty, r, extra, &heights)?;
                    self.progress.add_written(1);
                    self.flush(out, false)?;
                }
                Event::End(e) if e.local_name().as_ref() == b"sheetData" => break,
                Event::Eof => return Err(xml_error("the sheet data ends early")),
                _ => self.push_bytes(&src[start..end])?,
            }
        }
        for (r, cols) in std::mem::take(&mut edits) {
            self.edited_row(r, &cols, &heights, p);
        }
        Ok(())
    }

    /// A row the file doesn't have, with the edited cells `cols`.
    fn edited_row(&mut self, r: u64, cols: &[u32], heights: &HashMap<RowId, f64>, p: &str) {
        let mut cells = String::new();
        for &c in cols {
            self.fresh_cell(r, c, &mut cells);
        }
        if cells.is_empty() {
            return;
        }
        let _ = write!(self.buf, "<{p}row r=\"{}\"", r + 1);
        if let Some(h) = heights.get(&RowId::source(r)) {
            let _ = write!(self.buf, " ht=\"{h}\" customHeight=\"1\"");
        }
        self.buf.push('>');
        self.buf.push_str(&cells);
        let _ = write!(self.buf, "</{p}row>");
    }

    /// The cell at file row `r`, column `c`, written from the model.
    fn fresh_cell(&mut self, r: u64, c: u32, out: &mut String) {
        let table = &self.sheet.table;
        let at = CellRef {
            row: RowId::source(r),
            col: ColId::source(c),
        };
        let text = table
            .cell_at(at)
            .map(|t| t.into_owned())
            .unwrap_or_default();
        let style = table.style_of(at);
        let result = self.sheet.computed.get(at).map(str::to_owned);
        write_cell(out, r, c, &text, style, result.as_deref(), self.styles);
    }

    /// One `<row>` of the file, read to its end.
    #[allow(clippy::too_many_arguments)]
    fn file_row(
        &mut self,
        reader: &mut Reader<&[u8]>,
        src: &[u8],
        e: &BytesStart<'_>,
        raw_tag: &str,
        empty: bool,
        r: u64,
        extra: Vec<u32>,
        heights: &HashMap<RowId, f64>,
    ) -> Result<()> {
        let height = heights.get(&RowId::source(r)).copied();
        let mut sets: Vec<(&str, String)> = Vec::new();
        if let Some(h) = height {
            sets.push(("ht", h.to_string()));
            sets.push(("customHeight", "1".into()));
        }
        let rewrite = height.is_some() || !extra.is_empty();
        let tag = |empty: bool| -> String {
            let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
            let mut out = format!("<{name}");
            for a in e.attributes().flatten() {
                let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                // `spans` only hints at the row's columns: left out once they change.
                if key == "spans" || sets.iter().any(|(k, _)| *k == key) {
                    continue;
                }
                let v = crate::xml::value(a.clone())
                    .map(|v| v.into_owned())
                    .unwrap_or_default();
                out.push(' ');
                out.push_str(&key);
                out.push_str("=\"");
                escape(&v, &mut out);
                out.push('"');
            }
            for (k, v) in &sets {
                let _ = write!(out, " {k}=\"{v}\"");
            }
            out.push_str(if empty { "/>" } else { ">" });
            out
        };
        let mut extra = extra.into_iter().peekable();
        let open = if rewrite {
            tag(false)
        } else if empty {
            // `<row .../>` as `<row ...>` so cells can follow.
            format!("{}>", raw_tag.trim_end_matches("/>").trim_end())
        } else {
            raw_tag.to_owned()
        };
        let mut body = String::new();
        if !empty {
            let mut next_col = 0u32;
            loop {
                let start = reader.buffer_position() as usize;
                let event = reader.read_event().map_err(xml_error)?;
                let end = reader.buffer_position() as usize;
                match &event {
                    Event::Start(c) | Event::Empty(c) if c.local_name().as_ref() == b"c" => {
                        let col = attr(c, b"r")
                            .and_then(|v| cell_ref(v.as_bytes()))
                            .map_or(next_col, |(_, col)| col);
                        next_col = col + 1;
                        let cell_empty = matches!(event, Event::Empty(_));
                        let (raw_end, formula) = if cell_empty {
                            (end, None)
                        } else {
                            read_cell(reader)?
                        };
                        while let Some(&ec) = extra.peek() {
                            if ec >= col {
                                break;
                            }
                            extra.next();
                            self.fresh_cell(r, ec, &mut body);
                        }
                        let edited = extra.peek() == Some(&col);
                        if edited {
                            extra.next();
                        }
                        let keep_formula =
                            matches!(formula.as_deref(), Some("array" | "dataTable"));
                        if edited || (formula.is_some() && !keep_formula) {
                            self.fresh_cell(r, col, &mut body);
                        } else {
                            body.push_str(
                                std::str::from_utf8(&src[start..raw_end])
                                    .map_err(|_| xml_error("not UTF-8"))?,
                            );
                        }
                    }
                    Event::End(e) if e.local_name().as_ref() == b"row" => break,
                    Event::Eof => return Err(xml_error("a row ends early")),
                    _ => body.push_str(
                        std::str::from_utf8(&src[start..end])
                            .map_err(|_| xml_error("not UTF-8"))?,
                    ),
                }
            }
        }
        for c in extra {
            self.fresh_cell(r, c, &mut body);
        }
        if empty && body.is_empty() {
            if rewrite {
                self.buf.push_str(&tag(true));
            } else {
                self.buf.push_str(raw_tag);
            }
            return Ok(());
        }
        self.buf.push_str(&open);
        self.buf.push_str(&body);
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let _ = write!(self.buf, "</{name}>");
        Ok(())
    }
}

/// Read a `<c>`'s content to its end tag; returns where it ends and its formula's type
/// (`Some("")` for a plain formula, `Some("shared")`, `Some("array")`, …), if it has one.
fn read_cell(reader: &mut Reader<&[u8]>) -> Result<(usize, Option<String>)> {
    let mut formula = None;
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"f" => {
                formula = Some(attr(e, b"t").map(|t| t.into_owned()).unwrap_or_default());
            }
            Event::End(e) if e.local_name().as_ref() == b"c" => {
                return Ok((reader.buffer_position() as usize, formula));
            }
            Event::Eof => return Err(xml_error("a cell ends early")),
            _ => {}
        }
    }
}

fn skip_to_end(reader: &mut Reader<&[u8]>, name: &[u8]) -> Result<()> {
    let mut depth = 0;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(e) if e.local_name().as_ref() == name => depth += 1,
            Event::End(e) if e.local_name().as_ref() == name => {
                if depth == 0 {
                    return Ok(());
                }
                depth -= 1;
            }
            Event::Eof => return Err(xml_error("the part ends early")),
            _ => {}
        }
    }
}

fn prefix_of(e: &BytesStart<'_>) -> String {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    name.rsplit_once(':')
        .map_or(String::new(), |(p, _)| format!("{p}:"))
}

fn has_range_attr(e: &BytesStart<'_>) -> bool {
    e.attributes()
        .flatten()
        .any(|a| matches!(a.key.as_ref(), b"sqref" | b"ref" | b"activeCell"))
}

/// `A1:C3` or `B2` as an area.
fn parse_area(r: &str) -> Option<CellArea> {
    let (a, b) = r.split_once(':').unwrap_or((r, r));
    let ((r1, c1), (r2, c2)) = (cell_ref(a.as_bytes())?, cell_ref(b.as_bytes())?);
    Some(CellArea {
        first_row: r1.min(r2),
        first_col: c1.min(c2),
        last_row: r1.max(r2),
        last_col: c1.max(c2),
    })
}

/// Columns of the file's `<dimension>` (`A1:Q12859` gives 17).
fn file_dimension_cols(src: &[u8]) -> Option<u32> {
    let mut reader = Reader::from_reader(src);
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"dimension" => {
                let r = attr(&e, b"ref")?;
                let last = r.rsplit(':').next()?;
                return cell_ref(last.as_bytes()).map(|(_, c)| c + 1);
            }
            Event::Start(e) if e.local_name().as_ref() == b"sheetData" => return None,
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// The merged ranges a sheet has now (STYLE-4 keeps them following structure edits).
fn sheet_merges(table: &data_model::CsvTable) -> Vec<CellArea> {
    table.merges().to_vec()
}

/// A `<col>` element's attributes, comparable so equal neighbours join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ColAttrs {
    /// `f64` bits.
    width: Option<u64>,
    style: u32,
    hidden: bool,
    custom_width: bool,
    best_fit: bool,
    collapsed: bool,
    outline: u8,
}

impl ColAttrs {
    fn of(spec: Option<&ColSpec>, style: StyleId, width: Option<f64>) -> Self {
        let spec = spec.cloned().unwrap_or_default();
        Self {
            width: width.or(spec.width).map(f64::to_bits),
            style: style.0,
            hidden: spec.hidden,
            custom_width: width.is_some() || spec.custom_width,
            best_fit: width.is_none() && spec.best_fit,
            collapsed: spec.collapsed,
            outline: spec.outline,
        }
    }
}
