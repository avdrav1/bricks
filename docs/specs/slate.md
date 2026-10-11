# Slate

Fast, lightweight Linux-native word processor. Command `slate`. Design: [ADR 0003](../adr/0003-slate-document-model.md) (document model and readers), [ADR 0005](../adr/0005-slate-is-pageless.md) (pageless).

## Pageless

Slate has no pages (user decision). A document is one continuous column that scrolls without end: no page boundaries, page size, margins, headers, footers, or page numbers on screen, and the layout never paginates. The column's width is the user's choice, View → Text Width: Narrow (480 px), Medium (624 px, the default), Wide (816 px), or Full (the window's width less a 24 px margin each side). The choice applies to every window, is remembered between sessions, and is never stored in documents. A window narrower than the chosen width narrows the column to fit. Pictures scale down to fit the column; tables take the column's width.

A `.docx`'s page breaks show as a thin marker line in the flow ("Page break" to screen readers) and stay in the model, so SL1 saves them back. Its page setup (size, margins, sections, columns, headers, footers) isn't shown and survives saving, because the save patches the original package.

## Formats

| Format | SL0 (viewer) | SL1 (editor) |
| --- | --- | --- |
| `.txt` | open: UTF-8, UTF-16 with a BOM, else Windows-1252; one paragraph per line | edit, save in the encoding and line endings it was read with |
| `.md` | open and show formatted: headings, bold, italic, strikethrough, inline code, links, block quotes, ordered, bullet, and task lists, code blocks, tables, rules, images | edit formatted (WYSIWYG); save writes Markdown back; what Slate can't show (raw HTML, reference definitions, footnotes) is kept as written |
| `.docx` | open and show: paragraph styles (Title, headings, quotes), bold, italic, underline, strikethrough, font size and colour, alignment, lists with levels, tables, hyperlinks, inline pictures, page breaks (as markers in the flow) | edit; save by patching the original package, so content Slate doesn't model survives |

Anything else is refused with a message naming the format (`.doc`, `.odt`, `.rtf`: "Save it as .docx first").

## SL0: viewer

- `slate FILE` opens it; File → Open (Ctrl+O) opens a file dialog filtered to `.txt`, `.md`, `.docx`; dropping a file on the window opens it. Each file opens in its own window.
- Reading runs off the UI thread; the window shows "Opening…" until it is done. A file that can't be read gives an alert naming the file and the reason, and the window closes.
- The document shows as one pageless column (see Pageless) at the chosen text width, centred, read-only, with the text selectable and copyable (Ctrl+C).
- The window title is the file name; the status bar shows the format and the word count.
- Accessibility: the document's text is exposed to screen readers.

## SL1: editor (not built yet)

Typing, formatting commands with undo, save and save-as with atomic writes, crash recovery, find and replace, and the editing engine for the pageless flow (a decision story with a spike, per [ADR 0002](../adr/0002-gtk4-for-the-suite.md)). See [ADR 0005](../adr/0005-slate-is-pageless.md).

## Non-goals for now

Track changes, comments, mail merge, macros, fields, collaboration. Pages: a page layout view, page setup, headers and footers, page numbers, and paginated printing or PDF export ([ADR 0005](../adr/0005-slate-is-pageless.md)).
