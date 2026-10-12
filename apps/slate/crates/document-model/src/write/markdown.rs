//! The Markdown writer: blocks that didn't change since the file was read are copied from
//! its source byte for byte; only the others are written fresh.
//!
//! Matching: the blocks are matched to the source's in order (common start and end, then
//! the longest common subsequence of equal blocks in between; a middle over 16 million
//! block pairs counts as changed). Blocks that shared a source line (a picture inside a
//! paragraph) are copied together or not at all. A copied block brings the text between
//! it and the next source block; a block written fresh after one gets the separation it
//! needs (a blank line, or one line break between items of a tight list). A matched block
//! is still written fresh where its source would read otherwise in its new place: a
//! numbered item that would join the list before it with another number, a block whose
//! indentation would put it at another list level (or into a footnote), and a block using
//! a link reference definition that was deleted.
//!
//! Fresh blocks: `#` headings (a Title is a level-1 heading), `>` quotes per level,
//! `-` bullets, numbered items (`1.`, or `1)` where a new list must not continue the one
//! before), `- [ ]`/`- [x]` tasks, nested items and a list item's further paragraphs and
//! code indented to their item's content column, fenced code, pipe tables (column
//! alignment from the header cells, a cell's blocks joined with `<br>`), `---` rules,
//! `<div style="break-after: page"></div>` for page breaks, raw blocks as written, and
//! images as `![alt](url)`, reusing the source of an equal image the file had and writing
//! an embedded picture as a `data:` URI. Inline looks: see the `inline` module.
//!
//! Not representable, so dropped: font sizes, colours, underline, link titles, image
//! sizes, alignment outside tables. What doesn't read back as written: empty paragraphs
//! (none written), a line break at a paragraph's start or end or inside a heading (a
//! space) or a code span (a space) or a table cell (`<br>`, raw HTML), a Title (a level-1
//! heading), a page break (a raw HTML block), an embedded picture (a `data:` link), a
//! table without a header row (its first row becomes one), cells spanning columns, a code
//! block whose text doesn't end in a line break, a table inside a list item (its cells'
//! paragraphs keep the item's level), a list item whose first block is a nested list
//! (`- - a`), emphasis whose inner edge is punctuation glued to a word outside it
//! (`a**.b**c`, `**`code`**s`, `~~**a**~~b`), looks that overlap without nesting and
//! touch (`*a***b**`), and whitespace at the edge of a look, which reads back in the outer
//! look. Blocks in containers the model doesn't keep (a heading or code block in a quote, a
//! quote in a list item) are written outside them and read back the same.

use super::inline::{self, Mode};
use super::{encode, ending, push_with_ending, Unencodable};
use crate::markdown::MarkdownSource;
use crate::{
    match_blocks, Align, Block, CodeBlock, Image, ListItem, ListKind, Paragraph, ParagraphStyle,
    RawKind, Table, TextInfo,
};

const PAGE_BREAK: &str = "<div style=\"break-after: page\"></div>";

/// A Markdown file of `blocks`, encoded per `info`. With `source`, every block equal to
/// one of the source's (matched in order) is copied byte for byte from the source text,
/// with the text between it and the next source block; only the others are written
/// fresh, in the file's line ending. Also returns the source of the file written, for the
/// next save (its blocks are `blocks`).
pub fn write_markdown(
    blocks: &[Block],
    info: &TextInfo,
    source: Option<&MarkdownSource>,
) -> Result<(Vec<u8>, MarkdownSource), Unencodable> {
    let nl = ending(info.line_ending);
    let kept = match source {
        Some(src) => {
            let mut kept = match_blocks(&src.blocks, blocks);
            keep_references(&mut kept, src);
            whole_lines(&mut kept, &src.starts);
            kept
        }
        None => vec![None; blocks.len()],
    };
    let mut w = Writer {
        out: String::new(),
        lists: Vec::new(),
        last: None,
    };
    let mut starts = Vec::with_capacity(blocks.len());
    let mut images = Vec::with_capacity(blocks.len());
    // The source index of the block before, if it was copied.
    let mut copied_before: Option<usize> = None;
    let mut tail_copied = false;
    for (j, block) in blocks.iter().enumerate() {
        let mut copy = kept[j].zip(source).map(|(i, src)| (src, i));
        let mut item = None;
        if let Some((src, i)) = copy {
            let line = line_at(&src.text, src.starts[i]);
            let parsed = match block {
                Block::ListItem(it) => parse_item(line, it),
                _ => None,
            };
            if let Block::ListItem(it) = block {
                item = Some(parsed.unwrap_or_else(|| w.plan_item(it)));
            }
            // Where what is before it isn't what was in the source, a block can read
            // otherwise; then it is written fresh, unless it shares its line with another
            // block, which needs it copied.
            let alone = (i == 0 || src.starts[i - 1] != src.starts[i])
                && src.starts.get(i + 1) != Some(&src.starts[i]);
            if alone && w.misplaced(block, line, parsed.as_ref()) {
                copy = None;
            }
        }
        match copy {
            Some((src, i)) => {
                let follows = copied_before.is_some_and(|p| p + 1 == i);
                if !follows {
                    w.separate(w.separator(block, item.as_ref()), nl);
                }
                let from = if w.out.is_empty() && i == 0 {
                    0
                } else {
                    src.starts[i]
                };
                let end = src.starts.get(i + 1).copied().unwrap_or(src.text.len());
                let at = w.out.len() + (src.starts[i] - from);
                starts.push(at);
                images.push(src.images[i].as_ref().and_then(|r| {
                    (r.start >= src.starts[i] && r.end <= end)
                        .then(|| r.start - src.starts[i] + at..r.end - src.starts[i] + at)
                }));
                w.out.push_str(&src.text[from..end]);
                w.update(block, item);
                copied_before = Some(i);
                tail_copied = i + 1 == src.blocks.len();
            }
            None => {
                copied_before = None;
                tail_copied = false;
                let item = match block {
                    Block::ListItem(it) => Some(w.plan_item(it)),
                    _ => None,
                };
                match w.fresh(block, item.as_ref(), source) {
                    Some(text) => {
                        w.separate(w.separator(block, item.as_ref()), nl);
                        let at = w.out.len();
                        push_with_ending(&mut w.out, &text, nl);
                        starts.push(at);
                        let indent = text.len() - text.trim_start_matches(' ').len();
                        images.push(
                            matches!(block, Block::Image(_)).then(|| at + indent..w.out.len()),
                        );
                        w.update(block, item);
                    }
                    None => {
                        starts.push(w.out.len());
                        images.push(None);
                    }
                }
            }
        }
    }
    if !tail_copied {
        let len = w.out.trim_end_matches([' ', '\t', '\n', '\r']).len();
        w.out.truncate(len);
        if info.final_newline && !w.out.is_empty() {
            w.out.push_str(nl);
        }
    }
    let bytes = encode(&w.out, info.encoding)?;
    let written = MarkdownSource {
        text: w.out,
        starts,
        images,
        blocks: blocks.to_vec(),
    };
    Ok((bytes, written))
}

/// Old blocks sharing a line (`starts`) are copied all, one after the other, or none.
fn whole_lines(kept: &mut [Option<usize>], starts: &[usize]) {
    let mut new_of = vec![None; starts.len()];
    for (j, i) in kept.iter().enumerate() {
        if let Some(i) = i {
            new_of[*i] = Some(j);
        }
    }
    let mut g = 0;
    while g < starts.len() {
        let mut end = g + 1;
        while end < starts.len() && starts[end] == starts[g] {
            end += 1;
        }
        let whole = (g..end).all(|i| new_of[i].is_some())
            && (g + 1..end).all(|i| new_of[i] == new_of[i - 1].map(|j| j + 1));
        if !whole {
            for slot in &mut new_of[g..end] {
                if let Some(j) = slot.take() {
                    kept[j] = None;
                }
            }
        }
        g = end;
    }
}

/// A link reference definition dropped takes the copies of the blocks that use it: their
/// source names its label (`[text][label]`, `[label][]`, `[label]`), so they are written
/// fresh, with the destination inline.
fn keep_references(kept: &mut [Option<usize>], src: &MarkdownSource) {
    let mut copied = vec![false; src.blocks.len()];
    for i in kept.iter().flatten() {
        copied[*i] = true;
    }
    let labels: Vec<String> = src
        .blocks
        .iter()
        .zip(&copied)
        .filter_map(|(b, copied)| match b {
            Block::Raw(raw) if raw.kind == RawKind::ReferenceDefinition && !copied => {
                let label = raw.source.trim_start().strip_prefix('[')?;
                let label = &label[..label.find("]:")?];
                Some(format!("[{}]", label.to_lowercase()))
            }
            _ => None,
        })
        .collect();
    if labels.is_empty() {
        return;
    }
    for slot in kept.iter_mut() {
        if let Some(i) = *slot {
            let end = src.starts.get(i + 1).copied().unwrap_or(src.text.len());
            let text = src.text[src.starts[i]..end].to_lowercase();
            if labels.iter().any(|l| text.contains(l.as_str())) {
                *slot = None;
            }
        }
    }
}

/// The line starting at `start`, without its line break.
fn line_at(text: &str, start: usize) -> &str {
    let line = &text[start..];
    &line[..line.find(['\n', '\r']).unwrap_or(line.len())]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Bullet(char),
    Ordered { number: u64, delim: char },
}

/// A list item as written: its marker's column, its content's column, and its marker.
#[derive(Debug, Clone, Copy)]
struct ItemCtx {
    indent: usize,
    content: usize,
    marker: Marker,
}

/// The marker of a copied list item, read from its source line, if it is plainly there.
fn parse_item(line: &str, item: &ListItem) -> Option<ItemCtx> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    let rest = &line[indent..];
    let (marker, len) = match (rest.as_bytes().first()?, &item.kind) {
        (c @ (b'-' | b'*' | b'+'), ListKind::Bullet | ListKind::Task { .. }) => {
            (Marker::Bullet(char::from(*c)), 1)
        }
        (b'0'..=b'9', ListKind::Ordered { number, .. }) => {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            let delim = match rest.as_bytes().get(digits)? {
                b'.' => '.',
                b')' => ')',
                _ => return None,
            };
            (
                Marker::Ordered {
                    number: *number,
                    delim,
                },
                digits + 1,
            )
        }
        _ => return None,
    };
    let after = &rest[len..];
    let spaces = after.bytes().take_while(|b| *b == b' ').count();
    let content = if spaces == after.len() || spaces > 4 {
        indent + len + 1
    } else if spaces == 0 {
        return None;
    } else {
        indent + len + spaces
    };
    Some(ItemCtx {
        indent,
        content,
        marker,
    })
}

/// How a block is set off from the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sep {
    /// A line break (a blank line after is fine): list items of one list, a nested list.
    Line,
    /// One line break, no blank line: an empty item's first block, which a blank line
    /// would take out of the item.
    Direct,
    /// A blank line.
    Blank,
}

struct Writer<'b> {
    out: String,
    /// The last item at each list level around the position written (outermost first).
    lists: Vec<ItemCtx>,
    /// The last block written.
    last: Option<&'b Block>,
}

impl<'b> Writer<'b> {
    /// The content column of list level `indent` (1 = inside a top-level item).
    fn column(&self, indent: u8) -> usize {
        if indent == 0 {
            return 0;
        }
        self.lists
            .get(usize::from(indent) - 1)
            .or(self.lists.last())
            .map_or(0, |c| c.content)
    }

    /// How a fresh `item` is written here: under its parent, in the list before at its
    /// level when it continues it, else starting a list that doesn't join that one.
    fn plan_item(&self, item: &ListItem) -> ItemCtx {
        let level = usize::from(item.level);
        let indent = if level == 0 {
            0
        } else {
            self.lists
                .get(level - 1)
                .or(self.lists.last())
                .map_or(0, |c| c.content)
        };
        let before = self.lists.get(level).map(|c| c.marker);
        let marker = match (&item.kind, before) {
            (ListKind::Bullet | ListKind::Task { .. }, Some(Marker::Bullet(c))) => {
                Marker::Bullet(c)
            }
            (ListKind::Bullet | ListKind::Task { .. }, _) => Marker::Bullet('-'),
            (ListKind::Ordered { number, .. }, Some(Marker::Ordered { number: n, delim }))
                if *number == n.wrapping_add(1) =>
            {
                Marker::Ordered {
                    number: *number,
                    delim,
                }
            }
            (ListKind::Ordered { number, .. }, Some(Marker::Ordered { delim: '.', .. })) => {
                Marker::Ordered {
                    number: *number,
                    delim: ')',
                }
            }
            (ListKind::Ordered { number, .. }, _) => Marker::Ordered {
                number: *number,
                delim: '.',
            },
        };
        let width = match marker {
            Marker::Bullet(_) => 1,
            Marker::Ordered { number, .. } => number.to_string().len() + 1,
        };
        ItemCtx {
            indent,
            content: indent + width + 1,
            marker,
        }
    }

    /// The item continues the list before it at its level.
    fn continues(&self, item: &ItemCtx, level: u8) -> bool {
        match (
            self.lists.get(usize::from(level)).map(|c| c.marker),
            item.marker,
        ) {
            (Some(Marker::Bullet(a)), Marker::Bullet(b)) => a == b,
            (
                Some(Marker::Ordered {
                    number: n,
                    delim: a,
                }),
                Marker::Ordered { number, delim: b },
            ) => a == b && number == n.wrapping_add(1),
            _ => false,
        }
    }

    /// A block copied from a source line starting `line` (a list item's marker `item`, as
    /// read from it) would read otherwise after what is written: a numbered item would
    /// join the list before it with another number than its own, or the block would land
    /// at another list level (an indented code block taken into the list before it).
    fn misplaced(&self, block: &Block, line: &str, item: Option<&ItemCtx>) -> bool {
        let level = match block {
            Block::ListItem(it) => usize::from(it.level),
            _ => usize::from(self.nesting(block).unwrap_or(0)),
        };
        if let (Some(before), Some(ctx)) = (self.lists.get(level), item) {
            if let (
                Marker::Ordered {
                    number: n,
                    delim: a,
                },
                Marker::Ordered { number, delim: b },
            ) = (before.marker, ctx.marker)
            {
                if a == b && number != n.wrapping_add(1) {
                    return true;
                }
            }
        }
        let indented = line.starts_with([' ', '\t']);
        if indented
            && matches!(self.last, Some(Block::Raw(r)) if r.kind == RawKind::FootnoteDefinition)
        {
            // It would continue the footnote.
            return true;
        }
        if line.starts_with('\t') || line.trim_start().starts_with('>') {
            return false;
        }
        let indent = line.bytes().take_while(|b| *b == b' ').count();
        // Inside the content of the item it continues (if any)...
        let parent = match level {
            0 => 0,
            _ => match self.lists.get(level - 1) {
                Some(p) => p.content,
                None => return true,
            },
        };
        // ...and not inside the item before it at its own level.
        indent < parent || self.lists.get(level).is_some_and(|s| indent >= s.content)
    }

    /// The list level `block` continues inside an item: its indent, or for a picture right
    /// after an empty item (its first block), that item's.
    fn nesting(&self, block: &Block) -> Option<u8> {
        continuation(block).or_else(|| match (block, self.empty_item()) {
            (Block::Image(_), Some(level)) => Some(level),
            _ => None,
        })
    }

    /// The content level of the item just written, if it is empty (its first block, not a
    /// paragraph, comes next).
    fn empty_item(&self) -> Option<u8> {
        match self.last {
            Some(Block::ListItem(prev))
                if prev.paragraph.runs.is_empty()
                    && prev.paragraph.style == ParagraphStyle::Normal =>
            {
                Some(prev.level + 1)
            }
            _ => None,
        }
    }

    fn separator(&self, next: &Block, item: Option<&ItemCtx>) -> Sep {
        match (next, item) {
            (Block::ListItem(it), Some(ctx)) => {
                let level = usize::from(it.level);
                let nested = level > 0 && self.lists.len() >= level;
                if self.continues(ctx, it.level) || (nested && interrupts(it)) {
                    Sep::Line
                } else {
                    Sep::Blank
                }
            }
            _ if self.empty_item().is_some() && self.nesting(next) == self.empty_item() => {
                Sep::Direct
            }
            _ => Sep::Blank,
        }
    }

    /// End what is written so far as `sep` needs before the next block.
    fn separate(&mut self, sep: Sep, nl: &str) {
        if self.out.is_empty() {
            return;
        }
        let (broken, blank) = tail(&self.out);
        match sep {
            Sep::Line | Sep::Direct if !broken => self.out.push_str(nl),
            Sep::Direct if blank => {
                // Blank lines a copied block's source ended in go.
                let content = self.out.trim_end_matches([' ', '\t', '\r', '\n']).len();
                let line_end = self.out[content..]
                    .find(['\r', '\n'])
                    .map_or(self.out.len(), |i| content + i);
                self.out.truncate(line_end);
                self.out.push_str(nl);
            }
            Sep::Blank if !broken => {
                self.out.push_str(nl);
                self.out.push_str(nl);
            }
            Sep::Blank if !blank => self.out.push_str(nl),
            _ => {}
        }
    }

    /// Note the list context `block` leaves, after it is written.
    fn update(&mut self, block: &'b Block, item: Option<ItemCtx>) {
        match (block, item) {
            (Block::ListItem(it), Some(ctx)) => {
                let level = usize::from(it.level);
                self.lists.truncate(level);
                while self.lists.len() < level {
                    let pad = self.lists.last().copied().unwrap_or(ItemCtx {
                        indent: 0,
                        content: 0,
                        marker: Marker::Bullet('-'),
                    });
                    self.lists.push(pad);
                }
                self.lists.push(ctx);
            }
            _ => match self.nesting(block) {
                Some(k) => self.lists.truncate(usize::from(k)),
                None => self.lists.clear(),
            },
        }
        self.last = Some(block);
    }

    /// `block` written fresh, with `\n` line breaks; `None` for what writes nothing.
    fn fresh(
        &self,
        block: &Block,
        item: Option<&ItemCtx>,
        source: Option<&MarkdownSource>,
    ) -> Option<String> {
        match block {
            Block::Paragraph(p) => self.paragraph(p),
            Block::ListItem(it) => Some(self.list_item(it, item?)),
            Block::CodeBlock(code) => Some(self.code(code)),
            Block::Table(table) => table_text(table),
            Block::Image(image) => {
                let indent = self.nesting(block).map_or(0, |k| self.column(k));
                Some(format!(
                    "{}{}",
                    " ".repeat(indent),
                    image_text(image, source)
                ))
            }
            Block::PageBreak => Some(PAGE_BREAK.to_owned()),
            Block::Rule => Some("---".to_owned()),
            Block::Raw(raw) => {
                let text = raw.source.trim_end_matches(['\n', '\r']);
                (!text.is_empty()).then(|| text.to_owned())
            }
        }
    }

    fn paragraph(&self, p: &Paragraph) -> Option<String> {
        match p.style {
            ParagraphStyle::Normal => {
                if p.runs.is_empty() {
                    return None;
                }
                let prefix = " ".repeat(self.column(p.indent));
                Some(lines(
                    &inline::inline(&p.runs, Mode::Body),
                    &prefix,
                    &prefix,
                ))
            }
            ParagraphStyle::Quote => {
                if p.runs.is_empty() {
                    return None;
                }
                let prefix = "> ".repeat(usize::from(p.indent) + 1);
                Some(lines(
                    &inline::inline(&p.runs, Mode::Body),
                    &prefix,
                    &prefix,
                ))
            }
            ParagraphStyle::Heading(_) | ParagraphStyle::Title => Some(format!(
                "{}{}",
                " ".repeat(self.column(p.indent)),
                heading(p)
            )),
        }
    }

    fn list_item(&self, item: &ListItem, ctx: &ItemCtx) -> String {
        let p = &item.paragraph;
        let quote = if p.style == ParagraphStyle::Quote {
            "> ".repeat(usize::from(p.indent.saturating_sub(item.level)).max(1))
        } else {
            String::new()
        };
        let mut first = format!("{quote}{}", " ".repeat(ctx.indent));
        match ctx.marker {
            Marker::Bullet(c) => first.push(c),
            Marker::Ordered { number, delim } => {
                first.push_str(&number.to_string());
                first.push(delim);
            }
        }
        if let ListKind::Task { checked } = item.kind {
            first.push_str(if checked { " [x]" } else { " [ ]" });
        }
        let body = match p.style {
            ParagraphStyle::Heading(_) | ParagraphStyle::Title => heading(p),
            _ => inline::inline(&p.runs, Mode::Body),
        };
        if !body.is_empty() {
            first.push(' ');
        }
        let rest = format!("{quote}{}", " ".repeat(ctx.content));
        lines(&body, &first, &rest)
    }

    fn code(&self, code: &CodeBlock) -> String {
        let indent = " ".repeat(self.column(code.indent));
        let language = code.language.as_deref().unwrap_or("");
        let fence_char = if language.contains('`') { '~' } else { '`' };
        let mut longest = 0;
        let mut run = 0;
        for c in code.text.chars() {
            run = if c == fence_char { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        let fence = fence_char.to_string().repeat((longest + 1).max(3));
        let mut s = format!("{indent}{fence}{language}\n");
        if !code.text.is_empty() {
            let body = code.text.strip_suffix('\n').unwrap_or(&code.text);
            for line in body.split('\n') {
                if !line.is_empty() {
                    s.push_str(&indent);
                }
                s.push_str(line);
                s.push('\n');
            }
        }
        s.push_str(&indent);
        s.push_str(&fence);
        s
    }
}

/// A heading's line: `#` per level and its text.
fn heading(p: &Paragraph) -> String {
    let level = match p.style {
        ParagraphStyle::Heading(n) => usize::from(n.clamp(1, 6)),
        _ => 1,
    };
    let text = inline::inline(&p.runs, Mode::Heading);
    let hashes = "#".repeat(level);
    if text.is_empty() {
        hashes
    } else {
        format!("{hashes} {text}")
    }
}

/// `text`'s lines, the first after `first` and the others after `rest`.
fn lines(text: &str, first: &str, rest: &str) -> String {
    let mut out = String::with_capacity(text.len() + first.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(rest);
        } else {
            out.push_str(first);
        }
        out.push_str(line);
    }
    out
}

/// The list level a block continues inside an item (its indent), if it does.
fn continuation(block: &Block) -> Option<u8> {
    let indent = match block {
        Block::Paragraph(p) if p.style != ParagraphStyle::Quote => p.indent,
        Block::CodeBlock(code) => code.indent,
        _ => 0,
    };
    (indent > 0).then_some(indent)
}

/// A list item that can start a list right after a paragraph line.
fn interrupts(item: &ListItem) -> bool {
    !item.paragraph.runs.is_empty()
        && match item.kind {
            ListKind::Ordered { number, .. } => number == 1,
            _ => true,
        }
}

/// Whether `s` ends with a line break, and whether the line before that break is blank.
fn tail(s: &str) -> (bool, bool) {
    let rest = match s.strip_suffix("\r\n") {
        Some(r) => r,
        None => match s.strip_suffix(['\n', '\r']) {
            Some(r) => r,
            None => return (false, false),
        },
    };
    let line = &rest[rest.rfind(['\n', '\r']).map_or(0, |i| i + 1)..];
    (true, line.trim_matches([' ', '\t']).is_empty())
}

fn image_text(image: &Image, source: Option<&MarkdownSource>) -> String {
    if let Some(src) = source {
        let same =
            src.blocks.iter().zip(&src.images).find_map(|(b, r)| {
                matches!(b, Block::Image(i) if i == image).then_some(r.as_ref()?)
            });
        if let Some(range) = same {
            return src.text[range.clone()].to_owned();
        }
    }
    inline::image(&image.alt, &image.source)
}

fn table_text(table: &Table) -> Option<String> {
    let width = table
        .rows
        .iter()
        .map(|r| {
            r.cells
                .iter()
                .map(|c| c.span.max(1) as usize)
                .sum::<usize>()
        })
        .max()
        .filter(|w| *w > 0)?;
    let row_text = |row: &crate::TableRow| {
        let mut cells: Vec<String> = Vec::with_capacity(width);
        for cell in &row.cells {
            let parts: Vec<String> = cell.blocks.iter().map(cell_block).collect();
            cells.push(parts.join("<br>"));
            for _ in 1..cell.span.max(1) {
                cells.push(String::new());
            }
        }
        cells.resize(width, String::new());
        let mut s = String::from("|");
        for c in cells {
            s.push(' ');
            s.push_str(&c);
            s.push_str(" |");
        }
        s
    };
    let header = &table.rows[0];
    let mut aligns = Vec::with_capacity(width);
    for cell in &header.cells {
        let align = match cell.blocks.first() {
            Some(Block::Paragraph(p)) => p.align,
            _ => Align::Start,
        };
        aligns.push(align);
        aligns.extend((1..cell.span.max(1)).map(|_| Align::Start));
    }
    aligns.resize(width, Align::Start);
    let mut s = row_text(header);
    s.push_str("\n|");
    for align in aligns {
        s.push_str(match align {
            Align::Center => " :---: |",
            Align::End => " ---: |",
            Align::Start | Align::Justify => " --- |",
        });
    }
    for row in &table.rows[1..] {
        s.push('\n');
        s.push_str(&row_text(row));
    }
    Some(s)
}

/// A block inside a table cell, as the cell's one line holds it.
fn cell_block(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => inline::inline(&p.runs, Mode::Cell),
        Block::ListItem(item) => inline::inline(&item.paragraph.runs, Mode::Cell),
        Block::CodeBlock(code) => {
            let run = crate::Run {
                text: code.text.trim_end_matches('\n').to_owned(),
                style: crate::RunStyle {
                    code: true,
                    ..crate::RunStyle::default()
                },
            };
            inline::inline(&[run], Mode::Cell)
        }
        Block::Image(image) => inline::image(&image.alt, &image.source).replace('|', "\\|"),
        Block::Raw(raw) => raw.source.replace(['\n', '\r'], " "),
        Block::Table(_) | Block::PageBreak | Block::Rule => String::new(),
    }
}
