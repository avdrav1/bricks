//! Small helpers over quick-xml: attributes by local name (so `w:val` and `val` read the
//! same), WordprocessingML's on/off values, and skipping elements.

use crate::DocxError;
use quick_xml::events::{BytesRef, BytesStart};
use quick_xml::name::QName;
use quick_xml::Reader;
use std::borrow::Cow;

pub(crate) type XmlReader<'a> = Reader<&'a [u8]>;

pub(crate) fn reader(xml: &str) -> XmlReader<'_> {
    Reader::from_str(xml)
}

/// The value of attribute `name` (local name), unescaped; `None` if absent or malformed.
pub(crate) fn attr<'a>(e: &'a BytesStart<'_>, name: &[u8]) -> Option<Cow<'a, str>> {
    let a = e
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)?;
    match a.value {
        Cow::Borrowed(raw) => {
            let text = std::str::from_utf8(raw).ok()?;
            quick_xml::escape::unescape(text).ok()
        }
        Cow::Owned(raw) => {
            let text = String::from_utf8(raw).ok()?;
            let value = quick_xml::escape::unescape(&text).ok()?.into_owned();
            Some(Cow::Owned(value))
        }
    }
}

/// `w:val`, the attribute most WordprocessingML properties carry.
pub(crate) fn val<'a>(e: &'a BytesStart<'_>) -> Option<Cow<'a, str>> {
    attr(e, b"val")
}

/// Attribute `name` parsed as a number.
pub(crate) fn num<T: std::str::FromStr>(e: &BytesStart<'_>, name: &[u8]) -> Option<T> {
    attr(e, name)?.trim().parse().ok()
}

/// An on/off property: on when present unless `w:val` turns it off (`0`, `false`, `off`).
pub(crate) fn on(e: &BytesStart<'_>) -> bool {
    !matches!(val(e).as_deref(), Some("0" | "false" | "off" | "none"))
}

/// The local name of an element: `p` for `w:p`.
pub(crate) fn local<'a>(e: &'a BytesStart<'_>) -> &'a [u8] {
    let name = e.name().into_inner();
    match name.iter().position(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// Skip the rest of the element `e` just started.
pub(crate) fn skip(r: &mut XmlReader<'_>, e: &BytesStart<'_>) -> Result<(), DocxError> {
    let name = e.name().into_inner().to_vec();
    r.read_to_end(QName(&name))?;
    Ok(())
}

/// The text of a character or entity reference (`&amp;`, `&#x41;`).
pub(crate) fn reference(r: &BytesRef<'_>) -> Option<String> {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return Some(c.to_string());
    }
    let name: &[u8] = r;
    let s = match name {
        b"amp" => "&",
        b"lt" => "<",
        b"gt" => ">",
        b"quot" => "\"",
        b"apos" => "'",
        _ => return None,
    };
    Some(s.to_owned())
}
