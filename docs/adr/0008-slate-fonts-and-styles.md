# ADR 0008: Slate Draws Documents' Own Fonts and Styles

Status: Accepted

Date: 2026-10-13

## Context

Slate draws every document in one font, Liberation Sans 11 pt (SLATE-11). That was a user decision: with the desktop's variable font (Adwaita Sans), re-wrapping a 150,000-word document took p50 540 ms, over the 500 ms target. Its paragraph styles are a fixed enum, `ParagraphStyle { Normal, Title, Heading(1..6), Quote }`.

- **DOCX reading.** The reader maps a style's name onto that enum, so any other style (Subtitle, Caption, a company's own) reads as Normal. It bakes each run's resolved bold, italic, underline, strikethrough, size, and colour into `RunStyle`. It ignores fonts (`rFonts`, theme fonts), paragraph spacing, indents, and line spacing.
- **Saving.** The patched save keeps a run's font only because the template run's `rPr` is copied.
- **Commands.** The only formatting commands are bold, italic, Normal and Headings 1–6, and lists. The model already holds underline, strikethrough, size, colour, and alignment, but nothing sets them.

A word processor that opens Word documents has to show them in their fonts and styles and let the user choose them.

## Decision (user, 2026-10-13)

1. **Draw each document's own fonts.**
   - A run is drawn in the family its document names: DOCX `rFonts` (`ascii`/`hAnsi`, and the `asciiTheme` and `hAnsiTheme` theme slots read from `word/theme/theme1.xml`), resolved through the run style, paragraph style, and `docDefaults`.
   - Pango gets the family name. Fontconfig picks the face: its metric aliases give Carlito for Calibri, Caladea for Cambria, and Liberation Sans, Serif, and Mono for Arial, Times New Roman, and Courier New, so line breaks match Word where those fonts are installed. Otherwise fontconfig falls back to a similar family.
   - Markdown and plain text have no fonts, so they keep Liberation Sans 11 pt.
   - This supersedes SLATE-11's one-font rule for Word documents. Re-wrap and typing must still meet SL1's targets on a Word document that uses several fonts.
2. **Use each document's own paragraph styles.**
   - The model gains a style sheet, `Document::styles`. Each style has an id, a name, the style it's based on, the style that follows it, an outline level, a run look (font, size, bold, italic, underline, strikethrough, colour), and a paragraph look (alignment, space before and after, line spacing, left and first-line indent).
   - A paragraph names its style by id, replacing the `ParagraphStyle` enum.
   - A DOCX's style sheet is its `styles.xml`, custom styles included, with based-on chains resolved.
   - Markdown and plain text get Slate's built-in sheet: Normal, Title, Heading 1–6, Quote, and Code, in today's looks. So the Markdown writer still maps headings and quotes by outline level and name.
3. **A format bar over the page.**
   - It holds a style picker (the document's styles, each shown in its look), font family (searchable: the installed families plus any the document names), size, bold, italic, underline, strikethrough, colour, and alignment.
   - It shows the looks at the caret or across the selection; a look that varies across the selection shows blank.
   - A change applies to the selection, or to what's typed next, as one undo step.
   - Keys match Word's: Ctrl+U, Ctrl+L/E/R/J, and Ctrl+] and Ctrl+[ to step the size.
   - Plain-text files have no formatting, so the bar hides.
   - It follows the pattern of Brick's format bar (STYLE-6). A shared `suite-ui` font list is a follow-up: Brick, Frame, and Slate would then all use it.

## Consequences

- SL3 is planned as four stories:
  - SLATE-19: the style sheet and fonts in the model and the DOCX reader, checked against LibreOffice's dump.
  - SLATE-20: draw them. Fonts, style spacing, indents, and line spacing replace the flow's and the shaper's hard-coded heading tables.
  - SLATE-21: the format bar and its commands.
  - SLATE-22: save fonts, styles, and the new looks in `.docx`.
- `ParagraphStyle` goes everywhere in one cutover: flow, shaper, document-edit, the clipboard fragment, and the Markdown and DOCX writers. The clipboard fragment carries style names, so a paste into another document maps them by name, and falls back to Normal when that document has no style of that name.
- Re-wrap and typing performance are re-measured on a generated Word document using three font families. `apps/slate/tests/flow_perf.rs` gains it alongside the Markdown document.
- The GUI checks run in an Arch container with only DejaVu and WenQuanYi. SLATE-20's check adds the Liberation, Carlito, and Caladea fonts so substitution is exercised.
- The installer can't install fonts (no sudo). Slate's page and spec name the metric-compatible font packages per distribution.
- **Revisit if:** per-run fonts push whole-document re-wrap past 500 ms on the reference document. Then Slate would lay out on a worker thread with its own Pango context (ADR 0007's deferred optimization) before dropping any font.
