//! Cell styles (ADR 0009): a workbook's pool of fonts, fills, borders, number formats, and
//! cell formats under the file's own indices, and each sheet's styles from the file as
//! Excel layers them (cell, then row, then column).

use crate::{ColId, CsvTable, RowId};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A cell format: an index into [`StylePool::xfs`], the file's `cellXfs` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct StyleId(pub u32);

/// A colour as the file writes it; [`StylePool::rgb`] resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// The application's colour (black text, no fill in Excel).
    #[default]
    Auto,
    /// `0xRRGGBB`.
    Rgb(u32),
    /// A theme colour, lightened or darkened by `tint` (-1.0..=1.0).
    Theme { index: u8, tint: Tint },
    /// An entry of the indexed palette (64 and 65 are the system's text and window
    /// colours).
    Indexed(u8),
}

/// A theme colour's tint, kept as the bits of its `f64` so styles can be hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Tint(u64);

impl Tint {
    pub fn new(tint: f64) -> Self {
        Self(tint.to_bits())
    }

    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    SingleAccounting,
    DoubleAccounting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Baseline {
    Superscript,
    Subscript,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Font {
    pub name: Box<str>,
    /// Size in hundredths of a point.
    pub size: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: Underline,
    pub strike: bool,
    pub color: Color,
    pub baseline: Option<Baseline>,
    /// The file's font family, scheme, and charset, written back unchanged.
    pub family: Option<u8>,
    pub scheme: Option<Box<str>>,
    pub charset: Option<u8>,
}

impl Default for Font {
    /// Excel's default font: Calibri 11 in the theme's text colour.
    fn default() -> Self {
        Self {
            name: "Calibri".into(),
            size: 1100,
            bold: false,
            italic: false,
            underline: Underline::None,
            strike: false,
            color: Color::Theme {
                index: 1,
                tint: Tint::default(),
            },
            baseline: None,
            family: Some(2),
            scheme: Some("minor".into()),
            charset: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Pattern {
    #[default]
    None,
    Solid,
    /// Any other pattern, by its name in the file (`gray125`, `darkGrid`, …); drawn as a
    /// blend of its colours.
    Other(Box<str>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Fill {
    Pattern {
        pattern: Pattern,
        fg: Color,
        bg: Color,
    },
    /// A gradient, drawn as its first stop's colour; kept in the file as written.
    Gradient { first: Color },
}

impl Default for Fill {
    fn default() -> Self {
        Fill::Pattern {
            pattern: Pattern::None,
            fg: Color::Auto,
            bg: Color::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineStyle {
    #[default]
    None,
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
    Double,
    Hair,
    MediumDashed,
    DashDot,
    MediumDashDot,
    DashDotDot,
    MediumDashDotDot,
    SlantDashDot,
}

impl LineStyle {
    /// The style named as in the file; `None` for an unknown name.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "thin" => Self::Thin,
            "medium" => Self::Medium,
            "thick" => Self::Thick,
            "dashed" => Self::Dashed,
            "dotted" => Self::Dotted,
            "double" => Self::Double,
            "hair" => Self::Hair,
            "mediumDashed" => Self::MediumDashed,
            "dashDot" => Self::DashDot,
            "mediumDashDot" => Self::MediumDashDot,
            "dashDotDot" => Self::DashDotDot,
            "mediumDashDotDot" => Self::MediumDashDotDot,
            "slantDashDot" => Self::SlantDashDot,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Thin => "thin",
            Self::Medium => "medium",
            Self::Thick => "thick",
            Self::Dashed => "dashed",
            Self::Dotted => "dotted",
            Self::Double => "double",
            Self::Hair => "hair",
            Self::MediumDashed => "mediumDashed",
            Self::DashDot => "dashDot",
            Self::MediumDashDot => "mediumDashDot",
            Self::DashDotDot => "dashDotDot",
            Self::MediumDashDotDot => "mediumDashDotDot",
            Self::SlantDashDot => "slantDashDot",
        }
    }
}

/// One side of a border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Edge {
    pub style: LineStyle,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Border {
    pub left: Edge,
    pub right: Edge,
    pub top: Edge,
    pub bottom: Edge,
    pub diagonal: Edge,
    pub diagonal_up: bool,
    pub diagonal_down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HAlign {
    /// Text left, numbers right.
    #[default]
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VAlign {
    Top,
    Center,
    #[default]
    Bottom,
    Justify,
    Distributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Align {
    pub horizontal: HAlign,
    pub vertical: VAlign,
    pub wrap: bool,
    pub shrink: bool,
    pub indent: u8,
    /// Degrees: 0–90 up, 91–180 down (90 + n is n degrees down), 255 stacked.
    pub rotation: u8,
}

/// A cell format: indices into the pool's fonts, fills, borders, and number formats.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Xf {
    pub num_fmt: u32,
    pub font: u32,
    pub fill: u32,
    pub border: u32,
    /// The cell style (`cellStyleXfs`) this format is based on.
    pub xf_id: u32,
    pub align: Align,
    pub locked: bool,
    pub hidden: bool,
    pub quote_prefix: bool,
}

/// How many records of each kind came from the file: those keep their indices on save,
/// the rest are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileCounts {
    pub fonts: u32,
    pub fills: u32,
    pub borders: u32,
    pub xfs: u32,
}

/// Every style record of a workbook (`xl/styles.xml` and its theme colours).
#[derive(Debug, Clone, PartialEq)]
pub struct StylePool {
    pub fonts: Vec<Font>,
    pub fills: Vec<Fill>,
    pub borders: Vec<Border>,
    /// Custom number format codes by id (164 and up, and any built-in id the file
    /// redefines).
    pub num_fmts: BTreeMap<u32, Box<str>>,
    pub xfs: Vec<Xf>,
    /// The theme's colour scheme in file order: dk1, lt1, dk2, lt2, accent1–6, hlink,
    /// folHlink.
    pub theme: [u32; 12],
    /// The file's own indexed palette, when it replaces the default one.
    pub indexed: Option<Vec<u32>>,
    pub from_file: FileCounts,
}

/// Office's default theme colours (dk1 … folHlink).
pub const DEFAULT_THEME: [u32; 12] = [
    0x000000, 0xFFFFFF, 0x44546A, 0xE7E6E6, 0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000, 0x5B9BD5,
    0x70AD47, 0x0563C1, 0x954F72,
];

/// Excel's default indexed palette (entries 0–63).
pub const DEFAULT_PALETTE: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000,
    0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000,
    0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC,
    0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF,
    0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF,
    0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600,
    0x666699, 0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399,
    0x333333,
];

impl Default for StylePool {
    /// A new workbook's styles, as Excel starts one: one font, the two fills every file
    /// has (none and gray125), an empty border, and one cell format.
    fn default() -> Self {
        Self {
            fonts: vec![Font::default()],
            fills: vec![
                Fill::default(),
                Fill::Pattern {
                    pattern: Pattern::Other("gray125".into()),
                    fg: Color::Auto,
                    bg: Color::Auto,
                },
            ],
            borders: vec![Border::default()],
            num_fmts: BTreeMap::new(),
            xfs: vec![Xf {
                locked: true,
                ..Xf::default()
            }],
            theme: DEFAULT_THEME,
            indexed: None,
            from_file: FileCounts {
                fonts: 1,
                fills: 2,
                borders: 1,
                xfs: 1,
            },
        }
    }
}

impl StylePool {
    /// Cell format `id`; an id past the end (a damaged file) reads as the default.
    pub fn xf(&self, id: StyleId) -> &Xf {
        self.xfs.get(id.0 as usize).unwrap_or(&self.xfs[0])
    }

    pub fn font(&self, xf: &Xf) -> &Font {
        self.fonts.get(xf.font as usize).unwrap_or(&self.fonts[0])
    }

    pub fn fill(&self, xf: &Xf) -> Option<&Fill> {
        self.fills.get(xf.fill as usize)
    }

    pub fn border(&self, xf: &Xf) -> Option<&Border> {
        self.borders.get(xf.border as usize)
    }

    /// The workbook's default font: that of cell format 0 (Excel's Normal style).
    pub fn default_font(&self) -> &Font {
        self.font(&self.xfs[0])
    }

    /// A custom number format's code; built-in ids (below 164) that the file doesn't
    /// redefine give `None`.
    pub fn custom_format(&self, id: u32) -> Option<&str> {
        self.num_fmts.get(&id).map(|c| &**c)
    }

    /// `color` as `0xRRGGBB`; `None` for an automatic colour (the application's own).
    pub fn rgb(&self, color: Color) -> Option<u32> {
        match color {
            Color::Auto => None,
            Color::Rgb(rgb) => Some(rgb),
            Color::Theme { index, tint } => {
                // Excel's theme indices swap the first two pairs: 0 is lt1, 1 is dk1.
                let slot = match index {
                    0 => 1,
                    1 => 0,
                    2 => 3,
                    3 => 2,
                    n => n as usize,
                };
                let base = *self.theme.get(slot)?;
                Some(apply_tint(base, tint.value()))
            }
            Color::Indexed(n) => {
                let palette = self.indexed.as_deref().unwrap_or(&DEFAULT_PALETTE);
                palette.get(n as usize).copied()
            }
        }
    }
}

impl StylePool {
    /// The id of a custom number format with `code`, added if new (ids from 164 up).
    pub fn num_fmt_id(&mut self, code: &str) -> u32 {
        if let Some((&id, _)) = self.num_fmts.iter().find(|(_, c)| &***c == code) {
            return id;
        }
        let id = self
            .num_fmts
            .keys()
            .next_back()
            .map_or(164, |&k| (k + 1).max(164));
        self.num_fmts.insert(id, code.into());
        id
    }

    /// The id of cell format `xf`: an equal one already in the pool, else a new one
    /// appended. The file's records keep their indices; the pool only grows.
    pub fn intern(&mut self, xf: Xf) -> StyleId {
        match self.xfs.iter().position(|x| *x == xf) {
            Some(i) => StyleId(i as u32),
            None => {
                self.xfs.push(xf);
                StyleId(self.xfs.len() as u32 - 1)
            }
        }
    }

    /// Style `id` with number format `code` instead of its own.
    pub fn with_num_fmt(&mut self, id: StyleId, code: &str) -> StyleId {
        let num_fmt = self.num_fmt_id(code);
        let xf = Xf {
            num_fmt,
            ..self.xf(id).clone()
        };
        self.intern(xf)
    }
}

/// Excel's tint: lighten (`tint` > 0) or darken a colour in HLS space.
pub fn apply_tint(rgb: u32, tint: f64) -> u32 {
    if tint == 0.0 {
        return rgb;
    }
    let channel = |shift: u32| f64::from((rgb >> shift) & 0xFF) / 255.0;
    let (h, l, s) = rgb_to_hls(channel(16), channel(8), channel(0));
    let l = if tint < 0.0 {
        l * (1.0 + tint)
    } else {
        l * (1.0 - tint) + tint
    };
    let (r, g, b) = hls_to_rgb(h, l.clamp(0.0, 1.0), s);
    let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

fn rgb_to_hls(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, l, 0.0);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, l, s)
}

fn hls_to_rgb(h: f64, l: f64, s: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0))
}

/// Rows `start..end` that share a style.
type Run = (u64, u64, StyleId);

/// A sheet's styles as the file stores them, by source row and column: cells' own styles
/// as runs per column, then row styles (`<row s customFormat>`), then column styles
/// (`<col style>`). Cells whose style equals what the row or column would give are not
/// stored, so memory follows style changes, not cells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileStyles {
    cells: Vec<Vec<Run>>,
    rows: Vec<Run>,
    /// `(first, last, style)` column ranges, inclusive, sorted.
    cols: Vec<(u32, u32, StyleId)>,
}

fn run_at(runs: &[Run], row: u64) -> Option<StyleId> {
    let i = runs.partition_point(|r| r.1 <= row);
    runs.get(i).filter(|r| r.0 <= row).map(|r| r.2)
}

/// The runs of `runs` (sorted, apart) that meet `rows`.
fn runs_in(runs: &[Run], rows: std::ops::Range<u64>) -> &[Run] {
    let first = runs.partition_point(|r| r.1 <= rows.start);
    let last = first + runs[first..].partition_point(|r| r.0 < rows.end);
    &runs[first..last]
}

impl FileStyles {
    /// The style of the cell at source row `row`, source column `col`.
    pub fn at(&self, row: u64, col: u32) -> StyleId {
        if let Some(s) = self.cell(row, col) {
            return s;
        }
        self.fallback(row, col)
    }

    /// The cell's own style, if the file gives it one other than its row's or column's.
    pub fn cell(&self, row: u64, col: u32) -> Option<StyleId> {
        self.cells.get(col as usize).and_then(|c| run_at(c, row))
    }

    /// Source column `col`'s cell runs that meet rows `rows`, sorted.
    pub(crate) fn cell_runs(&self, col: u32, rows: std::ops::Range<u64>) -> &[Run] {
        self.cells
            .get(col as usize)
            .map_or(&[][..], |runs| runs_in(runs, rows))
    }

    /// The row-style runs that meet rows `rows`, sorted.
    pub(crate) fn row_runs(&self, rows: std::ops::Range<u64>) -> &[Run] {
        runs_in(&self.rows, rows)
    }

    /// What a cell without its own style shows: its row's style, else its column's.
    fn fallback(&self, row: u64, col: u32) -> StyleId {
        run_at(&self.rows, row).unwrap_or_else(|| self.column(col))
    }

    /// Column `col`'s style (`<col style>`), else 0.
    pub fn column(&self, col: u32) -> StyleId {
        let i = self.cols.partition_point(|c| c.1 < col);
        self.cols
            .get(i)
            .filter(|c| c.0 <= col)
            .map_or(StyleId(0), |c| c.2)
    }

    /// Row `row`'s style, if the file gives it one.
    pub fn row(&self, row: u64) -> Option<StyleId> {
        run_at(&self.rows, row)
    }

    /// Columns that have cells with their own styles: those past it have none.
    pub fn width(&self) -> u32 {
        self.cells.len() as u32
    }

    /// Column styles as `(first, last, style)` ranges, inclusive, sorted.
    pub fn column_ranges(&self) -> &[(u32, u32, StyleId)] {
        &self.cols
    }

    /// Stored runs in all: what these styles cost.
    pub fn runs(&self) -> usize {
        self.cells.iter().map(Vec::len).sum::<usize>() + self.rows.len() + self.cols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs() == 0
    }
}

/// Builds [`FileStyles`] from a sheet read in file order: column styles first, then each
/// row's style before its cells, cells in row-major order.
#[derive(Debug, Default)]
pub struct FileStylesBuilder {
    styles: FileStyles,
}

impl FileStylesBuilder {
    /// `cols`: `(first, last, style)` inclusive ranges of `<col style>`, any order.
    pub fn new(mut cols: Vec<(u32, u32, StyleId)>) -> Self {
        cols.retain(|c| c.2 != StyleId(0) && c.0 <= c.1);
        cols.sort_unstable();
        Self {
            styles: FileStyles {
                cols,
                ..FileStyles::default()
            },
        }
    }

    /// Row `row` has its own style (`customFormat="1"`).
    pub fn row(&mut self, row: u64, style: StyleId) {
        push_run(&mut self.styles.rows, row, style, |_| false);
    }

    /// The cell at `row`, `col` has style `style` (its `s`, 0 if it has none).
    pub fn cell(&mut self, row: u64, col: u32, style: StyleId) {
        let styles = &mut self.styles;
        if style == styles.fallback(row, col) {
            return;
        }
        let col_style = styles.column(col);
        let rows = &styles.rows;
        // A gap between two runs of the same style can join them when every row in it
        // would show that style anyway.
        let gap_shows = |gap: std::ops::Range<u64>| {
            let first = rows.partition_point(|r| r.1 <= gap.start);
            let mut covered = gap.start;
            for r in &rows[first..] {
                if r.0 >= gap.end {
                    break;
                }
                if r.2 != style || (r.0 > covered && col_style != style) {
                    return false;
                }
                covered = r.1;
            }
            covered >= gap.end || col_style == style
        };
        let col = col as usize;
        if styles.cells.len() <= col {
            styles.cells.resize_with(col + 1, Vec::new);
        }
        push_run(&mut styles.cells[col], row, style, gap_shows);
    }

    pub fn finish(self) -> FileStyles {
        self.styles
    }
}

fn push_run(
    runs: &mut Vec<Run>,
    row: u64,
    style: StyleId,
    joins: impl Fn(std::ops::Range<u64>) -> bool,
) {
    if let Some(last) = runs.last_mut() {
        if last.2 == style && (last.1 == row || (last.1 < row && joins(last.1..row))) {
            last.1 = row + 1;
            return;
        }
    }
    runs.push((row, row + 1, style));
}

/// A column range's layout from the file (`<col>`), columns 0-based and inclusive.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColSpec {
    pub first: u32,
    pub last: u32,
    /// Width in characters of the default font's widest digit, as the file writes it.
    pub width: Option<f64>,
    pub custom_width: bool,
    pub hidden: bool,
    pub best_fit: bool,
    pub outline: u8,
    pub collapsed: bool,
}

/// A row's layout from the file (`<row>`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RowSpec {
    /// Height in points.
    pub height: Option<f64>,
    pub custom_height: bool,
    pub hidden: bool,
    pub outline: u8,
    pub collapsed: bool,
}

/// A rectangle of cells in all-rows positions and view columns, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellArea {
    pub first_row: u64,
    pub first_col: u32,
    pub last_row: u64,
    pub last_col: u32,
}

/// A sheet's layout from the file: default sizes, column and row specs, merged ranges.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileLayout {
    /// `<sheetFormatPr defaultRowHeight>` in points.
    pub default_row_height: Option<f64>,
    /// `<sheetFormatPr defaultColWidth>` in characters.
    pub default_col_width: Option<f64>,
    /// `<sheetFormatPr baseColWidth>` in characters (Excel's default is 8).
    pub base_col_width: Option<u32>,
    /// `<sheetView showGridLines="0">`: the sheet shows only its cells' borders.
    pub hide_grid_lines: bool,
    /// Sorted by `first`.
    pub cols: Vec<ColSpec>,
    /// Rows whose spec differs from the default, as `(start, end, spec)` runs of source
    /// rows, sorted: files often give every row the same custom height.
    rows: Vec<(u64, u64, RowSpec)>,
    pub merges: Vec<CellArea>,
}

impl FileLayout {
    /// The spec of source column `col`, if the file has one.
    pub fn col(&self, col: u32) -> Option<&ColSpec> {
        let i = self.cols.partition_point(|c| c.last < col);
        self.cols.get(i).filter(|c| c.first <= col)
    }

    pub fn row(&self, row: u64) -> Option<&RowSpec> {
        let i = self.rows.partition_point(|r| r.1 <= row);
        self.rows.get(i).filter(|r| r.0 <= row).map(|r| &r.2)
    }

    /// Source row `row` has `spec`; rows come in increasing order.
    pub fn push_row(&mut self, row: u64, spec: RowSpec) {
        if let Some(last) = self.rows.last_mut() {
            if last.1 == row && last.2 == spec {
                last.1 = row + 1;
                return;
            }
        }
        self.rows.push((row, row + 1, spec));
    }

    /// The rows with a spec, as `(start, end, spec)` runs of source rows.
    pub fn row_runs(&self) -> &[(u64, u64, RowSpec)] {
        &self.rows
    }
}

impl CsvTable {
    /// Styles and layout read from the file (STYLE-1). The file's merges become the
    /// table's merged ranges (STYLE-4), sorted; one-cell merges are dropped, as they merge
    /// nothing.
    pub fn set_file_styles(&mut self, styles: FileStyles, mut layout: FileLayout) {
        layout
            .merges
            .retain(|m| m.first_row != m.last_row || m.first_col != m.last_col);
        layout.merges.sort_by_key(|m| (m.first_row, m.first_col));
        self.file_styles = Arc::new(styles);
        self.put_merges(Arc::new(layout.merges.clone()));
        self.layout = Arc::new(layout);
    }

    /// The file's styles of this table.
    pub fn file_styles(&self) -> &FileStyles {
        &self.file_styles
    }

    /// The file's column and row layout and merged ranges.
    pub fn layout(&self) -> &FileLayout {
        &self.layout
    }

    /// The layout spec of column `id`, if it came from the file and has one.
    pub fn col_spec(&self, id: ColId) -> Option<&ColSpec> {
        (!id.is_inserted()).then(|| self.layout.col(id.0)).flatten()
    }

    /// The layout spec of row `id`, if it came from the file and has one.
    pub fn row_spec(&self, id: RowId) -> Option<&RowSpec> {
        (!id.is_inserted()).then(|| self.layout.row(id.0)).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(
        cols: Vec<(u32, u32, StyleId)>,
        rows: &[(u64, u32)],
        cells: &[(u64, u32, u32)],
    ) -> FileStyles {
        let mut b = FileStylesBuilder::new(cols);
        let mut rows = rows.iter().peekable();
        for &(r, c, s) in cells {
            while let Some(&&(row, style)) = rows.peek() {
                if row > r {
                    break;
                }
                b.row(row, StyleId(style));
                rows.next();
            }
            b.cell(r, c, StyleId(s));
        }
        for &(row, style) in rows {
            b.row(row, StyleId(style));
        }
        b.finish()
    }

    #[test]
    fn layers_cell_then_row_then_column() {
        let s = built(
            vec![(1, 2, StyleId(5))],
            &[(3, 7)],
            &[(0, 0, 1), (3, 1, 2), (4, 2, 0)],
        );
        assert_eq!(s.at(0, 0), StyleId(1), "a cell's own style");
        assert_eq!(s.at(1, 0), StyleId(0), "nothing: 0");
        assert_eq!(s.at(1, 1), StyleId(5), "the column's style");
        assert_eq!(s.at(3, 2), StyleId(7), "the row's style beats the column's");
        assert_eq!(s.at(3, 1), StyleId(2), "the cell's beats the row's");
        assert_eq!(
            s.at(4, 2),
            StyleId(0),
            "a cell with s=0 beats its column's style"
        );
    }

    #[test]
    fn runs_join_across_rows_that_show_the_same_style() {
        // Column 0 styled 4 on rows 0..1000 except row 500, which has no cell: with no row
        // or column style there, row 500 would show 0, so the runs stay apart.
        let mut cells: Vec<_> = (0..1000).filter(|&r| r != 500).map(|r| (r, 0, 4)).collect();
        let s = built(vec![], &[], &cells);
        assert_eq!(s.runs(), 2);
        assert_eq!(s.at(500, 0), StyleId(0));
        // With column style 4, the gap shows 4 anyway: one run, and cells equal to the
        // column's style aren't stored at all.
        cells.push((1000, 0, 3));
        let s = built(vec![(0, 0, StyleId(4))], &[], &cells);
        assert_eq!(s.runs(), 2, "the column style and row 1000's cell");
        assert_eq!(s.at(500, 0), StyleId(4));
        assert_eq!(s.at(1000, 0), StyleId(3));
    }

    #[test]
    fn a_gap_with_another_row_style_splits_runs() {
        let s = built(vec![(0, 0, StyleId(4))], &[(5, 9)], &[(2, 0, 3), (8, 0, 3)]);
        assert_eq!(s.at(5, 0), StyleId(9));
        assert_eq!(s.at(8, 0), StyleId(3));
        assert_eq!(s.at(7, 0), StyleId(4));
    }

    #[test]
    fn theme_colours_swap_the_first_pairs_and_tint() {
        let pool = StylePool::default();
        let theme = |index, tint| Color::Theme {
            index,
            tint: Tint::new(tint),
        };
        assert_eq!(pool.rgb(theme(0, 0.0)), Some(0xFFFFFF), "0 is lt1");
        assert_eq!(pool.rgb(theme(1, 0.0)), Some(0x000000), "1 is dk1");
        assert_eq!(pool.rgb(theme(4, 0.0)), Some(0x4472C4));
        // LibreOffice resolves tints with this float HLS arithmetic (Excel's own palette
        // shows D9E1F2 for lighter 80%); LibreOffice is the oracle.
        assert_eq!(pool.rgb(theme(4, 0.799_981_688_894_314_4)), Some(0xDAE3F3));
        assert_eq!(pool.rgb(theme(4, -0.249_977_111_117_893)), Some(0x2F5597));
        assert_eq!(pool.rgb(Color::Indexed(10)), Some(0xFF0000));
        assert_eq!(pool.rgb(Color::Indexed(64)), None, "the system text colour");
        assert_eq!(pool.rgb(Color::Auto), None);
    }
}
