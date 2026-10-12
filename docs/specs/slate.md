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

Built so far (SLATE-6, SLATE-12, SLATE-13, SLATE-14, SLATE-15, SLATE-7), in paragraphs, list items, and code blocks, at the top level and in table cells (tables nested in a cell aren't editable):

- Typing goes through the input method. Text being composed (Chinese, Japanese, Korean) shows underlined at the caret, laid out with the paragraph but not in the document until it's committed; the input method can read the caret's paragraph and delete around the caret. Moving the caret or clicking ends a composition. Backspace and Delete remove a character, or join paragraphs at a paragraph's start or end; Backspace at a list item's start leaves the list. Enter splits the paragraph (after a heading the new one is normal text; in a list it starts a new item, and on an empty item it leaves the list); Shift+Enter breaks the line. Tab and Shift+Tab nest and un-nest list items.
- The caret moves with the arrows, Ctrl+Left and Ctrl+Right by word, Home and End along the wrapped line, and Ctrl+Home and Ctrl+End to the document's ends; a click puts it where it lands. Up and Down go to the line above or below wherever it is, into and out of table cells; Left and Right cross from cell to cell. In a cell, Tab goes to the next cell and Shift+Tab to the one before, and past the last cell Tab leaves the table. A cell's paragraphs never join a neighbouring cell's or the text around the table.
- Shift with any caret movement selects from where the selection started; Shift+click extends it; a double click selects a word and a triple click a paragraph; dragging selects too, and Ctrl+A selects everything. A selection that runs into a table from outside it (or from one cell into another) takes the whole table.
- Ctrl+X, Ctrl+C, and Ctrl+V (and Shift+Delete, Ctrl+Insert, Shift+Insert, and the Edit menu) cut, copy, and paste. Slate windows exchange rich text with its looks, lists, and tables (`application/x-slate-fragment`); other apps get plain text, a table's cells one a line, and their plain text pastes as paragraphs in the look at the caret. Cutting or deleting through a table removes the whole table; pasting a table into a cell pastes its cells' text. Cut and paste undo in one step.
- Ctrl+B and Ctrl+I toggle bold and italic over the selection, or for what's typed next. Ctrl+0 makes normal text and Ctrl+1 to Ctrl+6 headings. Ctrl+Shift+8 toggles a bulleted list and Ctrl+Shift+7 a numbered one, renumbering as items come and go. The menu's Edit and Format sections do the same. Plain text has no formatting.
- Every step undoes (Ctrl+Z) and redoes (Ctrl+Shift+Z, Ctrl+Y); typing undoes a word at a time.
- Screen readers (SLATE-15, through `GtkAccessibleText` on GTK 4.14): the page is a text box whose text is the document's paragraphs in order, table cells and pictures' alt text included, one a line. They get the caret's offset, the text at an offset by character, word, sentence, line, and paragraph, the selection, run attributes as drawn (family, size, weight, style, underline, strikethrough, colour) with defaults, and an insert or delete event naming exactly the text each edit, undo, or redo changed; page breaks are objects named "Page break". Still to come, because they need newer GTK than Ubuntu 24.04's 4.14 (ADR 0002, ADR 0007): character and range extents and the offset at a point, for magnifiers and pointer review (GTK 4.16, gtk4-rs `v4_16`), and screen readers setting the caret or the selection (GTK 4.22, `v4_22`).
- Saving (SLATE-7): Ctrl+S (File → Save) saves over the file, Ctrl+Shift+S (File → Save As…) saves as a .md or .txt file. Every save is atomic (`suite_file::save_atomic`: temp file, fsync, verify, rename) and runs off the UI thread; editing goes on meanwhile. A plain-text file keeps its encoding (UTF-8 with or without BOM, UTF-16 LE/BE, Windows-1252), line endings, and final newline; text its encoding can't hold is refused with an alert naming the character, and Save As then writes UTF-8. A Markdown file keeps the bytes of every block not edited, its blank lines and markers included; edited and new blocks are written fresh (`-` bullets, `#` headings, `**` and `*`), and what Markdown can't hold (font sizes, colours, underline, empty paragraphs) is dropped. A plain-text or Word document saved as Markdown, or Markdown saved as text, keeps showing as it was until reopened. The title starts with "• " while the document differs from its file (undoing back to the saved state clears it); closing a window or quitting then asks to save, discard, or cancel. Word documents can't be saved yet (SLATE-8): Save offers Save As.

## Non-goals for now

Track changes, comments, mail merge, macros, fields, collaboration. Pages: a page layout view, page setup, headers and footers, page numbers, and paginated printing or PDF export ([ADR 0005](../adr/0005-slate-is-pageless.md)).
