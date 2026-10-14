# ADR 0009: Word Documents Keep Their Layout in Slate's Pageless Column

Status: Accepted

Date: 2026-10-14

## Context

The user opened their résumé, a .docx exported from Google Docs, in Slate 0.2.0. It looked wrong next to Google Docs' own PDF.

- **Right-aligned dates and places drift and wrap.** Lines like "NAVA ⇥⇥⇥ Remote" use runs of tabs to reach Word's default tab stops, every 0.5″, on a 7.5″ text width (Letter, 0.5″ margins). Slate gives tabs Pango's default width of eight spaces, and the column is 6.5″ (Medium), so the tabs overflow and wrap. The title even has a tab in the middle of the name.
- **A font switches from serif to sans mid-sentence.** That run is in Cardo, which the .docx carries in `word/fonts/`. Slate ignores embedded fonts, and fontconfig falls back to a sans face for any family it doesn't know.
- **The rule under the name is missing.** Paragraph borders (`w:pBdr`) and shading (`w:shd`) aren't modelled.
- **The spacing is off.** Spacing and indents set on a paragraph itself (`w:spacing` and `w:ind` in its `pPr`) aren't modelled, only its style's (SLATE-19's deviation). The résumé sets them on 60 and 43 paragraphs.
- **The bullets are wrong.** Slate draws its own "•" where the numbering defines "●" and symbol-font glyphs, with their own indents.

## Decision (user, 2026-10-14)

1. **Word documents open at their page's text width.** Text Width gains **Page**: a Word document's page width minus its margins, from its last `w:sectPr`; Letter with 1″ margins when there is none. It becomes the default setting.
   - Under Page, Markdown and plain text use Medium. Narrow, Medium, Wide, and Full still apply to every document.
   - Slate stays pageless (ADR 0005): one column, no page breaks drawn as pages, no page view. Page only borrows the page's width, so lines and tab stops land where the author put them.
2. **Tab stops.** Tabs advance to the paragraph's own stops (`w:tabs` in its `pPr`, merged over its style's chain): left, centre, right, and decimal, with dot, hyphen, or underscore leaders, and `clear` removing an inherited stop. Past the last stop, they advance to multiples of `w:defaultTabStop` from `settings.xml` (0.5″ when absent).
   - Markdown and plain text keep a default stop of four spaces' width.
   - Saving writes paragraphs' tab stops back unchanged; they aren't edited yet.
3. **Embedded fonts and better fallbacks.**
   - Fonts a .docx carries (`w:embedRegular`, `w:embedBold`, `w:embedItalic`, `w:embedBoldItalic` in `fontTable.xml`) are loaded for the app's process while documents use them. Word's obfuscated `.odttf` is de-obfuscated with its `w:fontKey`.
   - A family neither installed nor embedded falls back by the font table's `w:family` (`roman` serif, `swiss` sans, `modern` monospace, others serif), so a serif document stays serif.
   - Loading fonts from a file is parsing untrusted data: fonts are only read through fontconfig and FreeType, as LibreOffice does, never installed for the user, and their parts are saved back unchanged.
4. **A paragraph's own spacing, indents, borders, and shading.** These become direct paragraph looks over the style, as runs' looks already are (ADR 0008): `Paragraph` gains optional overrides for space before and after, line spacing, left, right, and first-line indents, borders, and shading. They are drawn, kept on save, and written back for edited paragraphs.
   - Borders are top, bottom, left, right, and between, each with style (single, double, dotted, dashed), width, space, and colour. A shading fill colours the paragraph's box.
   - Styles' borders and shading come along through the style sheet.
5. **List markers from the numbering.** A list item's marker is its level's `w:lvlText`, drawn in the level's run font (symbol fonts like Symbol and Wingdings mapped to their Unicode characters), with the level's indents and the tab after it. Numbered formats keep today's counting.

## Consequences

- SL4 is planned as six stories: SLATE-23 (Page width), SLATE-24 (tab stops), SLATE-25 (embedded fonts and fallbacks), SLATE-26 (direct paragraph spacing and indents), SLATE-27 (borders and shading), and SLATE-28 (list markers).
- The proof is a committed fixture shaped like the résumé: tab-aligned lines on a 7.5″ text width, an embedded font, a bordered title, direct spacing and indents, and "●" bullets. It is built by a script, never from the user's own file, and checked against LibreOffice's dump and in Slate's GUI check.
- ADR 0005's "the column's width is the user's choice" now includes a choice that follows the document.
- **Revisit if:** documents that mix sections of different page widths need per-section widths. Page takes the last section's.
