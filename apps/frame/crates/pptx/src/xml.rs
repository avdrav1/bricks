//! A small element tree over quick-xml. Package parts are small (a slide is kilobytes),
//! and inheritance needs to look at layouts and masters again and again, so each part is
//! parsed once into a tree.
//!
//! Element names are local names (`p:sp` is `sp`, `a:t` is `t`): PresentationML and
//! DrawingML never reuse a local name with a different meaning where Frame reads them.
//! Attributes keep whether they had a prefix, because `id` and `r:id` both occur on
//! `<p:sldId>`.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::borrow::Cow;

#[derive(Debug, Clone, Default)]
pub(crate) struct Element {
    pub name: String,
    attrs: Vec<Attr>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
struct Attr {
    prefixed: bool,
    local: String,
    value: String,
}

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    /// Unprefixed attribute `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| !a.prefixed && a.local == name)
            .map(|a| a.value.as_str())
    }

    /// Prefixed attribute with local name `name`: `r:id`, `r:embed`, whatever the prefix.
    pub fn rel_attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.prefixed && a.local == name)
            .map(|a| a.value.as_str())
    }

    /// Attribute `name` as a number.
    pub fn num<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.attr(name)?.trim().parse().ok()
    }

    /// Attribute `name` as an XML boolean (`1`, `true`).
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.attr(name).map(|v| matches!(v.trim(), "1" | "true"))
    }

    /// Child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The first child element named `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// Child elements named `name`.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// The element at `path` below this one, each step the first child of that name.
    pub fn path(&self, path: &[&str]) -> Option<&Element> {
        path.iter().try_fold(self, |e, name| e.child(name))
    }

    /// The text directly inside this element.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for n in &self.children {
            if let Node::Text(t) = n {
                out.push_str(t);
            }
        }
        out
    }
}

/// Parse a part into its root element.
pub(crate) fn parse(bytes: &[u8]) -> Result<Element, String> {
    let mut reader = Reader::from_reader(bytes);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(element(&e)?),
            Ok(Event::Empty(e)) => {
                let el = element(&e)?;
                close(&mut stack, &mut root, el);
            }
            Ok(Event::End(_)) => {
                let el = stack.pop().ok_or("an end tag without its start")?;
                close(&mut stack, &mut root, el);
            }
            Ok(Event::Text(t)) => {
                let text = t.xml10_content().map_err(|e| e.to_string())?;
                push_text(&mut stack, &text);
            }
            Ok(Event::CData(t)) => {
                let text = t.decode().map_err(|e| e.to_string())?;
                push_text(&mut stack, &text);
            }
            Ok(Event::GeneralRef(r)) => {
                let resolved = match r.resolve_char_ref().map_err(|e| e.to_string())? {
                    Some(c) => Cow::Owned(c.to_string()),
                    None => {
                        let name = r.decode().map_err(|e| e.to_string())?;
                        Cow::Borrowed(match name.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            other => return Err(format!("unknown entity &{other};")),
                        })
                    }
                };
                push_text(&mut stack, &resolved);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("at byte {}: {e}", reader.error_position())),
        }
    }
    if !stack.is_empty() {
        return Err("the part ends inside an element".into());
    }
    root.ok_or_else(|| "the part has no root element".into())
}

fn element(e: &BytesStart<'_>) -> Result<Element, String> {
    let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        // Namespace declarations aren't attributes of the content.
        let key = a.key;
        if key.as_ref() == b"xmlns" || key.prefix().is_some_and(|p| p.as_ref() == b"xmlns") {
            continue;
        }
        let raw = std::str::from_utf8(&a.value).map_err(|e| e.to_string())?;
        let value = quick_xml::escape::unescape(raw)
            .map_err(|e| e.to_string())?
            .into_owned();
        attrs.push(Attr {
            prefixed: key.prefix().is_some(),
            local: String::from_utf8_lossy(key.local_name().as_ref()).into_owned(),
            value,
        });
    }
    Ok(Element {
        name,
        attrs,
        children: Vec::new(),
    })
}

fn close(stack: &mut [Element], root: &mut Option<Element>, el: Element) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(Node::Element(el)),
        None => *root = Some(el),
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    let Some(parent) = stack.last_mut() else {
        return;
    };
    if let Some(Node::Text(t)) = parent.children.last_mut() {
        t.push_str(text);
    } else {
        parent.children.push(Node::Text(text.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_entities_and_spaces() {
        let root = parse(br#"<a:p xmlns:a="x"><a:t> a &amp; b &#x263A; </a:t></a:p>"#).unwrap();
        assert_eq!(root.name, "p");
        assert_eq!(root.child("t").unwrap().text(), " a & b \u{263a} ");
    }

    #[test]
    fn prefixed_attributes_stay_apart() {
        let root = parse(br#"<p:sldId xmlns:p="x" xmlns:r="y" id="256" r:id="rId2"/>"#).unwrap();
        assert_eq!(root.attr("id"), Some("256"));
        assert_eq!(root.rel_attr("id"), Some("rId2"));
    }
}
