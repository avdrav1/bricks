//! `xl/styles.xml` on save (ADR 0010): the file's records stay as written; records the
//! pool gained are added before each list's end, its count updated. A workbook with no
//! styles part gets one written whole from the pool.

use super::cells::escape;
use data_model::{
    Align, Baseline, Border, Color, Edge, Fill, Font, HAlign, LineStyle, Pattern, StylePool,
    Underline, VAlign, Xf,
};
use std::fmt::Write as _;

fn color(tag: &str, c: Color, out: &mut String) {
    match c {
        Color::Auto => {
            let _ = write!(out, "<{tag} auto=\"1\"/>");
        }
        Color::Rgb(rgb) => {
            let _ = write!(out, "<{tag} rgb=\"FF{rgb:06X}\"/>");
        }
        Color::Theme { index, tint } if tint.value() != 0.0 => {
            let _ = write!(out, "<{tag} theme=\"{index}\" tint=\"{}\"/>", tint.value());
        }
        Color::Theme { index, .. } => {
            let _ = write!(out, "<{tag} theme=\"{index}\"/>");
        }
        Color::Indexed(n) => {
            let _ = write!(out, "<{tag} indexed=\"{n}\"/>");
        }
    }
}

pub(crate) fn font(f: &Font, out: &mut String) {
    out.push_str("<font>");
    if f.bold {
        out.push_str("<b/>");
    }
    if f.italic {
        out.push_str("<i/>");
    }
    if f.strike {
        out.push_str("<strike/>");
    }
    match f.underline {
        Underline::None => {}
        Underline::Single => out.push_str("<u/>"),
        Underline::Double => out.push_str("<u val=\"double\"/>"),
        Underline::SingleAccounting => out.push_str("<u val=\"singleAccounting\"/>"),
        Underline::DoubleAccounting => out.push_str("<u val=\"doubleAccounting\"/>"),
    }
    match f.baseline {
        Some(Baseline::Superscript) => out.push_str("<vertAlign val=\"superscript\"/>"),
        Some(Baseline::Subscript) => out.push_str("<vertAlign val=\"subscript\"/>"),
        None => {}
    }
    let _ = write!(out, "<sz val=\"{}\"/>", f64::from(f.size) / 100.0);
    if f.color != Color::Auto {
        color("color", f.color, out);
    }
    out.push_str("<name val=\"");
    escape(&f.name, out);
    out.push_str("\"/>");
    if let Some(family) = f.family {
        let _ = write!(out, "<family val=\"{family}\"/>");
    }
    if let Some(charset) = f.charset {
        let _ = write!(out, "<charset val=\"{charset}\"/>");
    }
    if let Some(scheme) = &f.scheme {
        out.push_str("<scheme val=\"");
        escape(scheme, out);
        out.push_str("\"/>");
    }
    out.push_str("</font>");
}

pub(crate) fn fill(f: &Fill, out: &mut String) {
    match f {
        Fill::Pattern { pattern, fg, bg } => {
            let name = match pattern {
                Pattern::None => "none",
                Pattern::Solid => "solid",
                Pattern::Other(name) => name,
            };
            out.push_str("<fill><patternFill patternType=\"");
            escape(name, out);
            out.push('"');
            if *fg == Color::Auto && *bg == Color::Auto {
                out.push_str("/></fill>");
                return;
            }
            out.push('>');
            if *fg != Color::Auto {
                color("fgColor", *fg, out);
            }
            if *bg != Color::Auto {
                color("bgColor", *bg, out);
            }
            out.push_str("</patternFill></fill>");
        }
        // A gradient Bricks made itself would come from a solid colour; it writes its first
        // stop as a solid fill.
        Fill::Gradient { first } => {
            out.push_str("<fill><patternFill patternType=\"solid\">");
            color("fgColor", *first, out);
            out.push_str("</patternFill></fill>");
        }
    }
}

fn edge(tag: &str, e: &Edge, out: &mut String) {
    if e.style == LineStyle::None {
        let _ = write!(out, "<{tag}/>");
        return;
    }
    let _ = write!(out, "<{tag} style=\"{}\">", e.style.name());
    color("color", e.color, out);
    let _ = write!(out, "</{tag}>");
}

pub(crate) fn border(b: &Border, out: &mut String) {
    out.push_str("<border");
    if b.diagonal_up {
        out.push_str(" diagonalUp=\"1\"");
    }
    if b.diagonal_down {
        out.push_str(" diagonalDown=\"1\"");
    }
    out.push('>');
    edge("left", &b.left, out);
    edge("right", &b.right, out);
    edge("top", &b.top, out);
    edge("bottom", &b.bottom, out);
    edge("diagonal", &b.diagonal, out);
    out.push_str("</border>");
}

fn alignment(a: &Align, out: &mut String) {
    if *a == Align::default() {
        return;
    }
    out.push_str("<alignment");
    let h = match a.horizontal {
        HAlign::General => None,
        HAlign::Left => Some("left"),
        HAlign::Center => Some("center"),
        HAlign::Right => Some("right"),
        HAlign::Fill => Some("fill"),
        HAlign::Justify => Some("justify"),
        HAlign::CenterContinuous => Some("centerContinuous"),
        HAlign::Distributed => Some("distributed"),
    };
    if let Some(h) = h {
        let _ = write!(out, " horizontal=\"{h}\"");
    }
    let v = match a.vertical {
        VAlign::Bottom => None,
        VAlign::Top => Some("top"),
        VAlign::Center => Some("center"),
        VAlign::Justify => Some("justify"),
        VAlign::Distributed => Some("distributed"),
    };
    if let Some(v) = v {
        let _ = write!(out, " vertical=\"{v}\"");
    }
    if a.rotation != 0 {
        let _ = write!(out, " textRotation=\"{}\"", a.rotation);
    }
    if a.wrap {
        out.push_str(" wrapText=\"1\"");
    }
    if a.indent != 0 {
        let _ = write!(out, " indent=\"{}\"", a.indent);
    }
    if a.shrink {
        out.push_str(" shrinkToFit=\"1\"");
    }
    out.push_str("/>");
}

pub(crate) fn xf(x: &Xf, out: &mut String) {
    let _ = write!(
        out,
        "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\" xfId=\"{}\"",
        x.num_fmt, x.font, x.fill, x.border, x.xf_id
    );
    // Bricks' own formats say what they set, so cell styles under them don't show through.
    out.push_str(" applyNumberFormat=\"1\" applyFont=\"1\" applyFill=\"1\" applyBorder=\"1\"");
    if x.align != Align::default() {
        out.push_str(" applyAlignment=\"1\"");
    }
    if x.quote_prefix {
        out.push_str(" quotePrefix=\"1\"");
    }
    let protection = !x.locked || x.hidden;
    if protection {
        out.push_str(" applyProtection=\"1\"");
    }
    if x.align == Align::default() && !protection {
        out.push_str("/>");
        return;
    }
    out.push('>');
    alignment(&x.align, out);
    if protection {
        let _ = write!(
            out,
            "<protection locked=\"{}\" hidden=\"{}\"/>",
            u8::from(x.locked),
            u8::from(x.hidden)
        );
    }
    out.push_str("</xf>");
}

fn num_fmt(id: u32, code: &str, out: &mut String) {
    let _ = write!(out, "<numFmt numFmtId=\"{id}\" formatCode=\"");
    escape(code, out);
    out.push_str("\"/>");
}

/// The pool gained records the file doesn't have.
pub(crate) fn grew(pool: &StylePool, file_num_fmts: &[u32]) -> bool {
    let n = pool.from_file;
    pool.fonts.len() as u32 > n.fonts
        || pool.fills.len() as u32 > n.fills
        || pool.borders.len() as u32 > n.borders
        || pool.xfs.len() as u32 > n.xfs
        || pool.num_fmts.keys().any(|id| !file_num_fmts.contains(id))
}

/// A whole styles part for `pool`.
pub(crate) fn whole(pool: &StylePool) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
    );
    if !pool.num_fmts.is_empty() {
        let _ = write!(out, "<numFmts count=\"{}\">", pool.num_fmts.len());
        for (&id, code) in &pool.num_fmts {
            num_fmt(id, code, &mut out);
        }
        out.push_str("</numFmts>");
    }
    let _ = write!(out, "<fonts count=\"{}\">", pool.fonts.len());
    pool.fonts.iter().for_each(|f| font(f, &mut out));
    let _ = write!(out, "</fonts><fills count=\"{}\">", pool.fills.len());
    pool.fills.iter().for_each(|f| fill(f, &mut out));
    let _ = write!(out, "</fills><borders count=\"{}\">", pool.borders.len());
    pool.borders.iter().for_each(|b| border(b, &mut out));
    out.push_str("</borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    let _ = write!(out, "<cellXfs count=\"{}\">", pool.xfs.len());
    pool.xfs.iter().for_each(|x| xf(x, &mut out));
    out.push_str("</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>");
    out
}

/// The file's styles part with the records the pool gained added. `xml` is the part as
/// the file has it; `file_num_fmts` the number format ids it defines.
pub(crate) fn spliced(xml: &str, pool: &StylePool, file_num_fmts: &[u32]) -> Option<String> {
    let n = pool.from_file;
    let mut out = xml.to_owned();
    let add = |out: &mut String, list: &str, count: usize, records: String| -> Option<()> {
        if records.is_empty() {
            return Some(());
        }
        insert_records(out, list, count, &records)
    };
    let mut fmts = String::new();
    let new_fmts: Vec<_> = pool
        .num_fmts
        .iter()
        .filter(|(id, _)| !file_num_fmts.contains(id))
        .collect();
    for (&id, code) in &new_fmts {
        num_fmt(id, code, &mut fmts);
    }
    if !fmts.is_empty() && find_tag(&out, "numFmts").is_none() {
        // No list yet: it goes first in the style sheet, with the sheet's prefix.
        let open = find_tag(&out, "styleSheet")?;
        let prefix = prefix_of(&out[open.start..open.end], "styleSheet")?;
        out.insert_str(
            open.end,
            &format!("<{prefix}numFmts count=\"0\"></{prefix}numFmts>"),
        );
    }
    add(
        &mut out,
        "numFmts",
        file_num_fmts.len() + new_fmts.len(),
        fmts,
    )?;
    let mut records = String::new();
    pool.fonts[n.fonts as usize..]
        .iter()
        .for_each(|f| font(f, &mut records));
    add(&mut out, "fonts", pool.fonts.len(), records)?;
    let mut records = String::new();
    pool.fills[n.fills as usize..]
        .iter()
        .for_each(|f| fill(f, &mut records));
    add(&mut out, "fills", pool.fills.len(), records)?;
    let mut records = String::new();
    pool.borders[n.borders as usize..]
        .iter()
        .for_each(|b| border(b, &mut records));
    add(&mut out, "borders", pool.borders.len(), records)?;
    let mut records = String::new();
    pool.xfs[n.xfs as usize..]
        .iter()
        .for_each(|x| xf(x, &mut records));
    add(&mut out, "cellXfs", pool.xfs.len(), records)?;
    Some(out)
}

/// Where an element's start tag is (prefix allowed: `<x:fonts ...>`).
struct Tag {
    start: usize,
    /// Just past the start tag's `>`.
    end: usize,
    empty: bool,
}

fn find_tag(xml: &str, name: &str) -> Option<Tag> {
    let mut from = 0;
    while let Some(i) = xml[from..].find('<').map(|i| i + from) {
        from = i + 1;
        let rest = &xml[i + 1..];
        if rest.starts_with('/') || rest.starts_with('?') || rest.starts_with('!') {
            continue;
        }
        let name_end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
        let tag_name = &rest[..name_end];
        let local = tag_name.rsplit(':').next().unwrap_or(tag_name);
        if local != name {
            continue;
        }
        let close = rest.find('>')?;
        return Some(Tag {
            start: i,
            end: i + 1 + close + 1,
            empty: rest[..close].ends_with('/'),
        });
    }
    None
}

/// Add `records` at the end of list `list` and set its count to `count`.
fn insert_records(xml: &mut String, list: &str, count: usize, records: &str) -> Option<()> {
    let tag = find_tag(xml, list)?;
    let open = &xml[tag.start..tag.end];
    let prefix = prefix_of(open, list)?;
    // Records go in the list's namespace: with its prefix, if it has one.
    let prefixed;
    let records = if prefix.is_empty() {
        records
    } else {
        prefixed = records
            .replace("</", "\u{0}")
            .replace('<', &format!("<{prefix}"))
            .replace('\u{0}', &format!("</{prefix}"));
        &prefixed
    };
    let mut start_tag = set_count(open, count);
    let close = format!("</{prefix}{list}>");
    if tag.empty {
        // `<fonts count="0"/>`: open it up.
        start_tag = start_tag.trim_end_matches("/>").to_owned() + ">";
        let replaced = format!("{start_tag}{records}{close}");
        xml.replace_range(tag.start..tag.end, &replaced);
        return Some(());
    }
    let end = xml[tag.end..].find(&close)? + tag.end;
    xml.insert_str(end, records);
    xml.replace_range(tag.start..tag.end, &start_tag);
    Some(())
}

/// The namespace prefix of start tag `tag` for element `name` (`"x:"`, or empty).
fn prefix_of(tag: &str, name: &str) -> Option<String> {
    let full = tag[1..]
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()?;
    Some(full.strip_suffix(name)?.to_owned())
}

/// A start tag with its `count` attribute set (added if missing).
fn set_count(tag: &str, count: usize) -> String {
    if let Some(i) = tag.find(" count=\"") {
        let value_start = i + " count=\"".len();
        if let Some(len) = tag[value_start..].find('"') {
            return format!(
                "{}{count}{}",
                &tag[..value_start],
                &tag[value_start + len..]
            );
        }
    }
    let at = tag
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(tag.len());
    format!("{} count=\"{count}\"{}", &tag[..at], &tag[at..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_model::StyleId;

    const XML: &str = r#"<?xml version="1.0"?><x:styleSheet xmlns:x="m"><x:fonts count="1"><x:font><x:sz val="11"/></x:font></x:fonts><x:fills count="2"><x:fill/><x:fill/></x:fills><x:borders count="1"><x:border/></x:borders><x:cellXfs count="1"><x:xf numFmtId="0"/></x:cellXfs><x:dxfs count="1"><x:dxf><x:font><x:b/></x:font></x:dxf></x:dxfs></x:styleSheet>"#;

    #[test]
    fn new_records_go_at_each_lists_end_and_counts_follow() {
        let mut pool = StylePool::default();
        pool.fonts.push(Font {
            bold: true,
            ..Font::default()
        });
        let id = pool.with_num_fmt(StyleId(0), "0.000");
        assert_eq!(id, StyleId(1));
        let out = spliced(XML, &pool, &[]).unwrap();
        assert!(out.starts_with(r#"<?xml version="1.0"?><x:styleSheet xmlns:x="m"><x:numFmts count="1"><x:numFmt numFmtId="164" formatCode="0.000"/></x:numFmts><x:fonts count="2"><x:font><x:sz val="11"/></x:font><x:font><x:b/>"#), "{out}");
        assert!(
            out.contains(r#"<x:cellXfs count="2"><x:xf numFmtId="0"/><x:xf numFmtId="164""#),
            "{out}"
        );
        assert!(
            out.contains("<x:dxfs count=\"1\"><x:dxf><x:font><x:b/></x:font>"),
            "dxfs untouched: {out}"
        );
        assert!(
            out.contains(r#"<x:fills count="2"><x:fill/><x:fill/></x:fills>"#),
            "unchanged lists stay"
        );
    }

    #[test]
    fn a_whole_part_reads_back_as_the_same_pool() {
        let mut pool = StylePool::default();
        let id = pool.with_num_fmt(StyleId(0), "#,##0.00");
        assert_eq!(id, StyleId(1));
        let back = crate::styles::read_pool(Some(&whole(&pool)), None).unwrap();
        assert_eq!(back.xfs[1].num_fmt, pool.xfs[1].num_fmt);
        assert_eq!(back.custom_format(164), Some("#,##0.00"));
        assert_eq!(back.fonts, pool.fonts);
        assert_eq!(back.fills, pool.fills);
    }
}
