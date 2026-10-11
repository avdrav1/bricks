//! Small helpers over quick-xml for reading package parts: attributes by local name, so
//! prefixed producers (`x:c`) read like plain ones.

use quick_xml::events::BytesStart;
use std::borrow::Cow;

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

/// Attribute `name` parsed as a number.
pub(crate) fn num<T: std::str::FromStr>(e: &BytesStart<'_>, name: &[u8]) -> Option<T> {
    attr(e, name)?.trim().parse().ok()
}

/// Attribute `name` as an XML boolean (`1`/`true`); `None` if absent.
pub(crate) fn flag(e: &BytesStart<'_>, name: &[u8]) -> Option<bool> {
    attr(e, name).map(|v| matches!(v.trim(), "1" | "true"))
}

/// An element that is on when present unless `val` says otherwise (`<b/>`, `<b val="0"/>`).
pub(crate) fn on(e: &BytesStart<'_>) -> bool {
    flag(e, b"val").unwrap_or(true)
}
