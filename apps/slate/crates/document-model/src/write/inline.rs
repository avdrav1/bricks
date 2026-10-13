//! Inline Markdown: a paragraph's runs as emphasis, strikethrough, code spans, links, raw
//! HTML, and text escaped just enough to read back as written.
//!
//! Looks open and close at run boundaries like a stack: the look that lasts longest opens
//! first, so nested looks nest. Whitespace at a run's edges stays outside the delimiters a
//! boundary opens or closes (CommonMark won't open `**` before a space or close it after
//! one), so that whitespace reads back in the outer look. Spaces and tabs a paragraph or
//! line would lose at its ends are written as character references.

use crate::{ImageFormat, ImageSource, Run};

/// Where the inline text goes, for what it can't hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// A paragraph or list item: line breaks are hard breaks.
    Body,
    /// An ATX heading: one line, so a line break is a space, and `#` is escaped.
    Heading,
    /// A pipe-table cell: one line, so a line break is `<br>`, and `|` is escaped.
    Cell,
}

/// The delimiters of strikethrough, bold, and italic (a [`Look`]'s flags, in order), the
/// order they open in when they last equally long.
const DELIMITERS: [&str; 3] = ["~~", "**", "*"];

/// What Markdown can show of a run's look.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Look<'r> {
    flags: [bool; 3],
    code: bool,
    raw: bool,
    link: Option<&'r str>,
}

struct Piece<'r> {
    text: String,
    look: Look<'r>,
}

/// `runs` as inline Markdown, with `\n` between the lines it takes.
pub(super) fn inline(runs: &[Run], mode: Mode) -> String {
    let pieces = pieces(runs, mode);
    let mut w = Inline::new(mode);
    // How many pieces from each one on keep each look (within one link).
    let mut lasts = vec![[0usize; 3]; pieces.len() + 1];
    for k in (0..pieces.len()).rev() {
        let continues = pieces
            .get(k + 1)
            .is_some_and(|n| n.look.link == pieces[k].look.link);
        let next = if continues { lasts[k + 1] } else { [0; 3] };
        lasts[k] = std::array::from_fn(|f| {
            if pieces[k].look.flags[f] {
                1 + next[f]
            } else {
                0
            }
        });
    }
    let mut stack: Vec<usize> = Vec::new();
    let mut link: Option<&str> = None;
    let mut pending = "";
    for (k, piece) in pieces.iter().enumerate() {
        let last_piece = k + 1 == pieces.len();
        let look = &piece.look;
        let (lead, core, trail) = if look.code || look.raw {
            ("", piece.text.as_str(), "")
        } else {
            split_whitespace(&piece.text)
        };
        if look.link != link {
            w.close(&mut stack, 0);
            w.text(pending, false);
            if let Some(dest) = link {
                w.close_link(dest);
            }
            if look.link.is_some() {
                w.open_link();
            }
            link = look.link;
        } else {
            // Close the looks the piece doesn't have (and those opened after them).
            let keep = stack
                .iter()
                .position(|f| !look.flags[*f])
                .unwrap_or(stack.len());
            w.close(&mut stack, keep);
            w.text(pending, false);
            if core.is_empty() {
                // Whitespace only: open nothing before it.
                pending = lead;
                continue;
            }
        }
        w.text(lead, false);
        if core.is_empty() {
            pending = trail;
            continue;
        }
        let mut opening: Vec<usize> = (0..3)
            .filter(|f| look.flags[*f] && !stack.contains(f))
            .collect();
        opening.sort_by_key(|f| std::cmp::Reverse(lasts[k][*f]));
        for f in opening {
            w.push_delimiter(DELIMITERS[f]);
            stack.push(f);
        }
        if look.code {
            w.code(core);
        } else if look.raw {
            w.raw(core);
        } else {
            w.text(core, last_piece && trail.is_empty());
        }
        pending = trail;
    }
    w.close(&mut stack, 0);
    w.text(pending, true);
    if let Some(dest) = link {
        w.close_link(dest);
    }
    w.finish()
}

/// Image alt text, escaped to read back as written.
pub(super) fn alt(text: &str) -> String {
    let mut w = Inline::new(Mode::Heading);
    w.in_link = true;
    w.line_start = false;
    w.text(&text.replace('\n', " "), false);
    w.out
}

/// A Markdown image of `alt` and `source`; an embedded picture becomes a `data:` URI.
pub(super) fn image(alt_text: &str, source: &ImageSource) -> String {
    let dest = match source {
        ImageSource::Link(url) => destination(url),
        ImageSource::Embedded { format, bytes } => {
            let mime = match format {
                ImageFormat::Png => "image/png",
                ImageFormat::Jpeg => "image/jpeg",
                ImageFormat::Gif => "image/gif",
                ImageFormat::Other => "application/octet-stream",
            };
            format!("data:{mime};base64,{}", base64(bytes))
        }
    };
    format!("![{}]({dest})", alt(alt_text))
}

/// A link destination as written between `(` and `)`.
pub(super) fn destination(url: &str) -> String {
    if url.is_empty() {
        return "<>".to_owned();
    }
    let mut out = String::with_capacity(url.len() + 2);
    if url.chars().any(|c| c == ' ' || c.is_control()) {
        out.push('<');
        for c in url.chars() {
            if matches!(c, '<' | '>' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('>');
        return out;
    }
    let mut depth = 0i32;
    let balanced = url.chars().all(|c| {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        depth >= 0
    }) && depth == 0;
    let chars: Vec<char> = url.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let escape = match c {
            '\\' => chars.get(i + 1).is_none_or(|n| n.is_ascii_punctuation()),
            '(' | ')' => !balanced,
            '<' | '`' => true,
            '&' => entity_like(&chars[i + 1..]),
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The runs' Markdown looks, neighbouring runs that look the same in Markdown joined.
fn pieces(runs: &[Run], mode: Mode) -> Vec<Piece<'_>> {
    let mut pieces: Vec<Piece<'_>> = Vec::with_capacity(runs.len());
    for run in runs.iter().filter(|r| !r.text.is_empty()) {
        let s = &run.style;
        let on = |v: Option<bool>| v == Some(true);
        let look = Look {
            flags: [on(s.strike), on(s.bold), on(s.italic)],
            code: s.code,
            raw: s.raw && !s.code,
            link: s.link.as_deref(),
        };
        match pieces.last_mut() {
            Some(last) if last.look == look => last.text.push_str(&run.text),
            _ => pieces.push(Piece {
                text: run.text.clone(),
                look,
            }),
        }
    }
    if mode == Mode::Heading {
        for p in &mut pieces {
            if p.text.contains('\n') {
                p.text = p.text.replace('\n', " ");
            }
        }
    }
    pieces
}

/// `s` as its leading whitespace, the rest up to its trailing whitespace, and that.
fn split_whitespace(s: &str) -> (&str, &str, &str) {
    let core = s.trim_start();
    let lead = &s[..s.len() - core.len()];
    let trimmed = core.trim_end();
    (lead, trimmed, &core[trimmed.len()..])
}

/// `&` followed by what reads as a character reference: `#123;`, `#x1F;`, or `name;`.
fn entity_like(rest: &[char]) -> bool {
    let (digits, start): (fn(&char) -> bool, usize) = match rest {
        ['#', 'x' | 'X', ..] => (char::is_ascii_hexdigit, 2),
        ['#', ..] => (char::is_ascii_digit, 1),
        _ => (char::is_ascii_alphanumeric, 0),
    };
    let n = rest[start..].iter().take_while(|c| digits(c)).count();
    n > 0 && rest.get(start + n) == Some(&';')
}

/// `[^label]` (a footnote reference) at the start of `rest`, not followed by what would
/// make it a link or a definition; `end` says nothing follows `rest` in the paragraph.
fn footnote_reference(rest: &[char], end: bool) -> bool {
    if rest.get(1) != Some(&'^') {
        return false;
    }
    let label = rest[2..]
        .iter()
        .take_while(|c| !matches!(c, ']' | '[' | '\\') && !c.is_whitespace())
        .count();
    if label == 0 || rest.get(2 + label) != Some(&']') {
        return false;
    }
    match rest.get(3 + label) {
        Some(c) => !matches!(c, '(' | '[' | ':'),
        None => end,
    }
}

struct Inline {
    mode: Mode,
    out: String,
    /// Nothing but block prefixes precedes on this line.
    line_start: bool,
    /// The last character written is a space or tab of the text.
    space_tail: bool,
    in_link: bool,
}

impl Inline {
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            out: String::new(),
            line_start: true,
            space_tail: false,
            in_link: false,
        }
    }

    fn finish(mut self) -> String {
        self.protect_space_tail();
        self.out
    }

    /// A space or tab ending a line would be dropped: write it as a reference.
    fn protect_space_tail(&mut self) {
        if self.space_tail {
            match self.out.pop() {
                Some('\t') => self.out.push_str("&#9;"),
                Some(_) => self.out.push_str("&#32;"),
                None => {}
            }
            self.space_tail = false;
        }
    }

    fn push_delimiter(&mut self, d: &str) {
        self.out.push_str(d);
        self.line_start = false;
        self.space_tail = false;
    }

    fn close(&mut self, stack: &mut Vec<usize>, keep: usize) {
        while stack.len() > keep {
            let f = stack.pop().expect("longer than keep");
            self.push_delimiter(DELIMITERS[f]);
        }
    }

    fn open_link(&mut self) {
        self.push_delimiter("[");
        self.in_link = true;
    }

    fn close_link(&mut self, dest: &str) {
        self.in_link = false;
        let dest = destination(dest);
        self.push_delimiter("](");
        self.out.push_str(&dest);
        self.out.push(')');
    }

    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
        self.line_start = false;
        self.space_tail = false;
    }

    fn code(&mut self, text: &str) {
        let mut text = text.replace(['\n', '\r'], " ");
        if self.mode == Mode::Cell {
            text = text.replace('|', "\\|");
        }
        let mut longest = 0;
        let mut run = 0;
        for c in text.chars() {
            run = if c == '`' { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        let fence = "`".repeat(longest + 1);
        let pad = text.starts_with('`')
            || text.ends_with('`')
            || (text.starts_with(' ') && text.ends_with(' ') && text.chars().any(|c| c != ' '));
        let pad = if pad { " " } else { "" };
        self.out.push_str(&fence);
        self.out.push_str(pad);
        self.out.push_str(&text);
        self.out.push_str(pad);
        self.out.push_str(&fence);
        self.line_start = false;
        self.space_tail = false;
    }

    fn hard_break(&mut self) {
        match self.mode {
            Mode::Body => {
                self.protect_space_tail();
                self.out.push_str("\\\n");
                self.line_start = true;
            }
            Mode::Heading => self.text_char(' ', None, None),
            Mode::Cell => {
                self.protect_space_tail();
                self.out.push_str("<br>");
                self.line_start = false;
            }
        }
    }

    /// Write plain text, escaped; `end` says nothing follows it in the paragraph.
    fn text(&mut self, text: &str, end: bool) {
        if text.is_empty() {
            return;
        }
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            if c == '\n' {
                self.hard_break();
                i += 1;
                continue;
            }
            if self.line_start {
                if c == ' ' || c == '\t' {
                    self.out.push_str(if c == ' ' { "&#32;" } else { "&#9;" });
                    self.line_start = false;
                    self.space_tail = false;
                    i += 1;
                    continue;
                }
                self.line_start = false;
                match c {
                    '#' | '>' | '-' | '+' | '=' | '*' | '_' | '~' => {
                        self.out.push('\\');
                        self.out.push(c);
                        self.space_tail = false;
                        i += 1;
                        continue;
                    }
                    '0'..='9' => {
                        let digits = chars[i..].iter().take_while(|d| d.is_ascii_digit()).count();
                        if digits <= 9 && matches!(chars.get(i + digits), Some('.' | ')')) {
                            self.out.extend(&chars[i..i + digits]);
                            self.out.push('\\');
                            self.out.push(chars[i + digits]);
                            self.space_tail = false;
                            i += digits + 1;
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            if c == '['
                && !self.in_link
                && prev != Some('!')
                && footnote_reference(&chars[i..], end)
            {
                self.out.push('[');
                self.space_tail = false;
                i += 1;
                continue;
            }
            if c == '&' && entity_like(&chars[i + 1..]) {
                self.out.push_str("\\&");
                self.space_tail = false;
                i += 1;
                continue;
            }
            self.text_char(c, prev, next);
            i += 1;
        }
    }

    fn text_char(&mut self, c: char, prev: Option<char>, next: Option<char>) {
        let spaced = prev.is_some_and(char::is_whitespace) && next.is_some_and(char::is_whitespace);
        let escape = match c {
            // Before punctuation it would escape it; before a space, the reference a
            // line-end space may become.
            '\\' => !next.is_some_and(char::is_alphanumeric),
            '`' | '[' => true,
            '*' | '~' => !spaced,
            '_' => {
                let inner = prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric);
                !spaced && !inner
            }
            ']' => self.in_link,
            '<' => next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?')),
            '|' => self.mode == Mode::Cell,
            '#' => self.mode == Mode::Heading,
            _ => false,
        };
        if escape {
            self.out.push('\\');
        }
        self.out.push(c);
        self.space_tail = c == ' ' || c == '\t';
    }
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_pads_to_four() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"\xff\xfe\xfd"), "//79");
    }
}
