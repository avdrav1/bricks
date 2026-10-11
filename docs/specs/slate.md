# Slate

Fast, lightweight Linux-native word processor. Command `slate`. Design: [ADR 0003](../adr/0003-slate-document-model.md).

## Formats

| Format | SL0 (viewer) | SL1 (editor) |
| --- | --- | --- |
| `.txt` | open: UTF-8, UTF-16 with a BOM, else Windows-1252; one paragraph per line | edit, save in the encoding and line endings it was read with |
| `.md` | open and show formatted: headings, bold, italic, strikethrough, inline code, links, block quotes, ordered, bullet, and task lists, code blocks, tables, rules, images | edit formatted (WYSIWYG); save writes Markdown back; what Slate can't show (raw HTML, reference definitions, footnotes) is kept as written |
| `.docx` | open and show: paragraph styles (Title, headings, quotes), bold, italic, underline, strikethrough, font size and colour, alignment, lists with levels, tables, hyperlinks, inline pictures, page breaks | edit; save by patching the original package, so content Slate doesn't model survives |

Anything else is refused with a message naming the format (`.doc`, `.odt`, `.rtf`: "Save it as .docx first").

## SL0: viewer

- `slate FILE` opens it; File → Open (Ctrl+O) opens a file dialog filtered to `.txt`, `.md`, `.docx`; dropping a file on the window opens it. Each file opens in its own window.
- Reading runs off the UI thread; the window shows "Opening…" until it is done. A file that can't be read gives an alert naming the file and the reason, and the window closes.
- The document shows as a centred column the width of a page (6.5 in of text at 96 dpi), read-only, with the text selectable and copyable (Ctrl+C).
- The window title is the file name; the status bar shows the format and the word count.
- Accessibility: the document's text is exposed to screen readers.

## SL1: editor (not built yet)

Typing, formatting commands with undo, save and save-as with atomic writes, crash recovery, find and replace, and the page layout engine (a decision story with a spike, per [ADR 0002](../adr/0002-gtk4-for-the-suite.md)).

## Non-goals for now

Track changes, comments, mail merge, macros, fields beyond page numbers, collaboration.
