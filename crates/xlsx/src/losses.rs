//! What saving drops (XLSX-2): Bricks keeps a workbook's values, formulas, sheets, names, and
//! column widths. [`scan`] looks through the file's parts for everything else it holds, so
//! the app can say what a save over it would lose.

use memchr::memmem::Finder;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use zip::ZipArchive;

/// A kind of workbook content Bricks doesn't keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loss {
    CellFormatting,
    Charts,
    Images,
    Comments,
    MergedCells,
    ConditionalFormatting,
    DataValidation,
    Hyperlinks,
    PivotTables,
    Macros,
    Tables,
    ExternalLinks,
    HiddenSheets,
    ChartSheets,
}

impl Loss {
    /// Every loss, in declaration order (the order [`Losses::items`] lists them in).
    const ALL: [Loss; 14] = [
        Loss::CellFormatting,
        Loss::Charts,
        Loss::Images,
        Loss::Comments,
        Loss::MergedCells,
        Loss::ConditionalFormatting,
        Loss::DataValidation,
        Loss::Hyperlinks,
        Loss::PivotTables,
        Loss::Macros,
        Loss::Tables,
        Loss::ExternalLinks,
        Loss::HiddenSheets,
        Loss::ChartSheets,
    ];

    /// How the loss reads in a sentence; `n` counts charts and is ignored otherwise.
    pub fn text(self, n: u32) -> String {
        match self {
            Loss::CellFormatting => "cell formatting".into(),
            Loss::Charts if n == 1 => "1 chart".into(),
            Loss::Charts => format!("{n} charts"),
            Loss::Images => "images".into(),
            Loss::Comments => "comments".into(),
            Loss::MergedCells => "merged cells".into(),
            Loss::ConditionalFormatting => "conditional formatting".into(),
            Loss::DataValidation => "data validation".into(),
            Loss::Hyperlinks => "hyperlinks".into(),
            Loss::PivotTables => "pivot tables".into(),
            Loss::Macros => "macros".into(),
            Loss::Tables => "tables".into(),
            Loss::ExternalLinks => "links to other files".into(),
            Loss::HiddenSheets => "hidden sheets (shown)".into(),
            Loss::ChartSheets => "chart sheets".into(),
        }
    }
}

/// The content a workbook holds that a save would drop, in [`Loss`] declaration order. The
/// count is the number of charts for [`Loss::Charts`] and 1 for every other item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Losses {
    pub items: Vec<(Loss, u32)>,
}

impl Losses {
    /// True when a save keeps everything the workbook holds.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The items comma-joined: "cell formatting, 1 chart, comments, merged cells".
    pub fn sentence(&self) -> String {
        let texts: Vec<String> = self.items.iter().map(|&(l, n)| l.text(n)).collect();
        texts.join(", ")
    }
}

/// Bytes read from a part at a time; the end of each read is carried over to the next so a
/// marker split between two reads is still found.
const READ_BYTES: usize = 64 * 1024;

/// Markers in a worksheet part and what each means.
const SHEET_MARKERS: [(&str, Loss); 4] = [
    ("<mergeCell ", Loss::MergedCells),
    ("<conditionalFormatting", Loss::ConditionalFormatting),
    ("<dataValidation ", Loss::DataValidation),
    ("<hyperlink ", Loss::Hyperlinks),
];

/// Markers in `xl/styles.xml` of formatting a cell can show. Concrete markers, not the
/// `cellXfs` count, which writers fill with default styles. LibreOffice writes booleans as
/// `val="true"`, Excel as a bare element or `val="1"`.
const STYLE_MARKERS: [&str; 17] = [
    "<b/>",
    "<b val=\"1\"/>",
    "<b val=\"true\"/>",
    "<i/>",
    "<i val=\"1\"/>",
    "<i val=\"true\"/>",
    "<u/>",
    "<u val=",
    "<strike/>",
    "<strike val=\"1\"/>",
    "<strike val=\"true\"/>",
    "patternType=\"solid\"",
    "<left style=",
    "<right style=",
    "<top style=",
    "<bottom style=",
    "<numFmt ",
];

/// Number formats that show a cell as Bricks does: the two date formats Bricks writes itself,
/// and the plain ones LibreOffice declares in every file.
const PLAIN_FORMATS: [&str; 4] = ["yyyy-mm-dd", "yyyy-mm-dd hh:mm:ss", "General", "@"];

/// Markers in `xl/workbook.xml` of a sheet Bricks shows although the file hides it.
const HIDDEN_MARKERS: [&str; 2] = ["state=\"hidden\"", "state=\"veryHidden\""];

/// Look through the parts of the workbook at `path` for content a save would drop.
pub fn scan(path: &Path) -> io::Result<Losses> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(path)?))?;
    let mut counts = [0u32; Loss::ALL.len()];
    let mut add = |loss: Loss, n: u32| counts[loss as usize] += n;
    for i in 0..zip.len() {
        let mut part = zip.by_index(i)?;
        let name = part.name().to_string();
        if is_part(&name, "xl/charts/chart", ".xml") {
            add(Loss::Charts, 1);
        } else if name.starts_with("xl/media/") {
            add(Loss::Images, 1);
        } else if is_part(&name, "xl/comments", ".xml") || name.starts_with("xl/threadedComments/")
        {
            add(Loss::Comments, 1);
        } else if name.starts_with("xl/pivotTables/") {
            add(Loss::PivotTables, 1);
        } else if name == "xl/vbaProject.bin" {
            add(Loss::Macros, 1);
        } else if name.starts_with("xl/tables/") {
            add(Loss::Tables, 1);
        } else if name.starts_with("xl/externalLinks/") {
            add(Loss::ExternalLinks, 1);
        } else if name.starts_with("xl/chartsheets/") {
            add(Loss::ChartSheets, 1);
        } else if is_part(&name, "xl/worksheets/sheet", ".xml") {
            let markers = SHEET_MARKERS.map(|(m, _)| m);
            let found = find_markers(&mut part, &markers)?;
            for ((_, loss), hit) in SHEET_MARKERS.iter().zip(found) {
                if hit {
                    add(*loss, 1);
                }
            }
        } else if name == "xl/styles.xml" {
            let mut styles = Vec::new();
            part.read_to_end(&mut styles)?;
            if has_formatting(&styles) {
                add(Loss::CellFormatting, 1);
            }
        } else if name == "xl/workbook.xml"
            && find_markers(&mut part, &HIDDEN_MARKERS)?.contains(&true)
        {
            add(Loss::HiddenSheets, 1);
        }
    }
    let items = Loss::ALL
        .iter()
        .filter(|&&l| counts[l as usize] > 0)
        .map(|&l| match l {
            Loss::Charts => (l, counts[l as usize]),
            _ => (l, 1),
        })
        .collect();
    Ok(Losses { items })
}

/// True for a part directly in the folder of `prefix` whose name starts with it and ends
/// with `suffix` (`xl/charts/chart1.xml`, not `xl/charts/_rels/chart1.xml.rels`).
fn is_part(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|middle| !middle.contains('/'))
}

/// Whether `styles` (the whole `xl/styles.xml`) holds a style marker, counting a `<numFmt `
/// only when its format code isn't a plain one.
fn has_formatting(styles: &[u8]) -> bool {
    if STYLE_MARKERS[..STYLE_MARKERS.len() - 1]
        .iter()
        .any(|m| memchr::memmem::find(styles, m.as_bytes()).is_some())
    {
        return true;
    }
    memchr::memmem::find_iter(styles, b"<numFmt ").any(|at| {
        let element = &styles[at..];
        let element = &element[..memchr::memchr(b'>', element).unwrap_or(element.len())];
        let Some(start) = memchr::memmem::find(element, b"formatCode=\"") else {
            return true;
        };
        let code = &element[start + "formatCode=\"".len()..];
        let code = &code[..memchr::memchr(b'"', code).unwrap_or(code.len())];
        !PLAIN_FORMATS
            .iter()
            .any(|plain| code.eq_ignore_ascii_case(plain.as_bytes()))
    })
}

/// Stream `part` in reads of [`READ_BYTES`] and report which `markers` occur in it.
fn find_markers(part: &mut impl Read, markers: &[&str]) -> io::Result<Vec<bool>> {
    let finders: Vec<Finder<'_>> = markers.iter().map(|m| Finder::new(m.as_bytes())).collect();
    let carry = markers.iter().map(|m| m.len()).max().unwrap_or(1) - 1;
    let mut found = vec![false; markers.len()];
    let mut buf = vec![0u8; carry + READ_BYTES];
    let mut have = 0;
    loop {
        let n = part.read(&mut buf[have..])?;
        if n == 0 {
            break;
        }
        have += n;
        for (hit, finder) in found.iter_mut().zip(&finders) {
            *hit = *hit || finder.find(&buf[..have]).is_some();
        }
        if found.iter().all(|&hit| hit) {
            break;
        }
        let keep_from = have.saturating_sub(carry);
        buf.copy_within(keep_from..have, 0);
        have -= keep_from;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hands out at most `step` bytes per read, as a decompressing reader may.
    struct Chunked<'a> {
        data: &'a [u8],
        step: usize,
    }

    impl Read for Chunked<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.data.len().min(self.step).min(out.len());
            out[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            Ok(n)
        }
    }

    #[test]
    fn a_marker_split_between_two_reads_is_found() {
        let marker = "<mergeCell ";
        for split in 1..marker.len() {
            let mut data = vec![b' '; READ_BYTES - split];
            data.extend_from_slice(marker.as_bytes());
            data.extend(std::iter::repeat_n(b' ', READ_BYTES));
            let mut part = Chunked {
                data: &data,
                step: READ_BYTES,
            };
            let found = find_markers(&mut part, &[marker, "<hyperlink "]).unwrap();
            assert_eq!(found, vec![true, false], "split after {split} bytes");
        }
    }

    #[test]
    fn number_formats_count_unless_plain() {
        assert!(!has_formatting(
            br#"<numFmt numFmtId="164" formatCode="General"/><numFmt numFmtId="165" formatCode="yyyy-mm-dd hh:mm:ss"/>"#
        ));
        assert!(has_formatting(
            br#"<numFmt numFmtId="164" formatCode="0.00%"/>"#
        ));
        assert!(has_formatting(br#"<font><b val="true"/></font>"#));
        assert!(!has_formatting(br#"<font><b val="false"/></font>"#));
    }
}
