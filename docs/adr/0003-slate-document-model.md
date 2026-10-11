# ADR 0003: Slate's Document Model and Formats

Status: Accepted

Date: 2026-10-11

## Context

Slate must open `.txt`, `.md`, and `.docx` (user decision) and show Markdown formatted, as a word processor does, not as source (user decision). The first milestone, SL0, is a read-only viewer; SL1 adds editing and saving. GTK 4 is the toolkit ([ADR 0002](0002-gtk4-for-the-suite.md)), which has no paginated layout engine.

## Decision

- **One document model for every format**, in a toolkit-free crate `apps/slate/crates/document-model`. A `Document` is a list of blocks: paragraphs (with a style: normal, title, heading 1-6, quote), list items (bullet, ordered, or task; level; number), code blocks, tables (rows of cells, each a list of blocks), images (bytes, format, alt text, size), page breaks, rules, and `Raw` blocks for source Slate can't model. A paragraph holds runs of text with bold, italic, underline, strikethrough, inline code, size, colour, and link. Paragraphs carry alignment.
- **Readers convert into the model**, one per format:
  - Plain text, in `document-model`: UTF-8 (BOM stripped), UTF-16 LE/BE with a BOM, otherwise Windows-1252 when the bytes aren't UTF-8 (`encoding_rs`). CRLF and CR read as line breaks. One paragraph per line. The reader records the encoding and line ending so SL1 saves the file as it came.
  - Markdown, in `document-model`: `pulldown-cmark` with tables, strikethrough, and task lists. Raw HTML, footnote definitions, and reference link definitions become `Raw` blocks holding their source text, so SL1 can write them back unchanged.
  - DOCX, in `apps/slate/crates/docx`: `zip` and `quick-xml` over `word/document.xml`, resolving paragraph and run properties through `word/styles.xml` (`basedOn` chains and document defaults), list kinds and levels through `word/numbering.xml`, hyperlinks and pictures through the part's relationships. Headers, footers, footnotes, comments, text boxes, and fields' codes are not shown in SL0.
- **The SL0 viewer uses a read-only `GtkTextView`** with one text tag per distinct run and paragraph look, tables as embedded grids, and images as embedded pictures. It is a viewer, not the editor: the layout engine for editing and pagination is decided in SL1 by a decision story with a spike that measures typing latency and frame time on a 300-page document, as ADR 0002 asks.
- **Reading runs off the UI thread** (`gio::spawn_blocking`); the window shows "Opening…" until the model arrives.

## Consequences

- One renderer serves all three formats, and SL1's editor edits one model.
- Markdown shows formatted, and the `Raw` blocks keep what it can't show; SL1 still has to keep Markdown's own syntax choices (`*` or `_`, setext or ATX headings) to avoid rewriting untouched text. A Markdown save that rewrites unedited blocks as written is a SL1 acceptance criterion.
- The model doesn't hold page geometry (margins, sections, columns, headers, footers): DOCX files show as a flowing column. SL1's save patches the original package, as Bricks' XLSX save does (Bricks ADR 0010), so what the model doesn't hold survives.
- `GtkTextView` may be replaced by SL1's engine; the model and readers don't depend on it.
- **Revisit if:** the SL1 spike shows the model needs positions into the original XML for patched saves, or a format needs a construct the block list can't express.
