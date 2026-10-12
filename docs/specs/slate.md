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
- The document shows as one pageless column (see Pageless) at the chosen text width, centred, read-only, with the text selectable by dragging and copyable as plain text (Ctrl+A, Ctrl+C). Text is in one document font, Liberation Sans (metric-compatible with Arial) or else the system's sans, whatever the desktop font (user decision, SLATE-11: variable desktop fonts such as Adwaita Sans lay out twice as slowly).
- The window title is the file name; the status bar shows the format and the word count.
- Accessibility: the document's text is exposed to screen readers.

## SL1: editor (in progress)

Typing, formatting commands with undo, save and save-as with atomic writes, crash recovery, and find and replace, in Slate's own pageless text engine, a custom Pango flow ([ADR 0007](../adr/0007-slate-editing-engine.md); pageless per [ADR 0005](../adr/0005-slate-is-pageless.md)).

Built so far (SLATE-6, SLATE-12), in paragraphs, list items, and code blocks, at the top level and in table cells (tables nested in a cell aren't editable):

- Typing goes through the input method. Backspace and Delete remove a character, or join paragraphs at a paragraph's start or end; Backspace at a list item's start leaves the list. Enter splits the paragraph (after a heading the new one is normal text; in a list it starts a new item, and on an empty item it leaves the list); Shift+Enter breaks the line. Tab and Shift+Tab nest and un-nest list items.
- The caret moves with the arrows, Ctrl+Left and Ctrl+Right by word, Home and End along the wrapped line, and Ctrl+Home and Ctrl+End to the document's ends; a click puts it where it lands. Up and Down go to the line above or below wherever it is, into and out of table cells; Left and Right cross from cell to cell. In a cell, Tab goes to the next cell and Shift+Tab to the one before, and past the last cell Tab leaves the table. A cell's paragraphs never join a neighbouring cell's or the text around the table.
- Ctrl+B and Ctrl+I toggle bold and italic over the selection, or for what's typed next. Ctrl+0 makes normal text and Ctrl+1 to Ctrl+6 headings. Ctrl+Shift+8 toggles a bulleted list and Ctrl+Shift+7 a numbered one, renumbering as items come and go. The menu's Edit and Format sections do the same. Plain text has no formatting.
- Every step undoes (Ctrl+Z) and redoes (Ctrl+Shift+Z, Ctrl+Y); typing undoes a word at a time.

## Non-goals for now

Track changes, comments, mail merge, macros, fields, collaboration. Pages: a page layout view, page setup, headers and footers, page numbers, and paginated printing or PDF export ([ADR 0005](../adr/0005-slate-is-pageless.md)).
