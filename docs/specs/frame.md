# Frame

Fast, lightweight Linux-native presentation application, easier to use than PowerPoint (user decision). Command `frame`; "Frame" is a working name. Design: [ADR 0004](../adr/0004-frame-presentation-model.md) (model and reading), [ADR 0006](../adr/0006-frame-editor.md) (editor).

## Format

`.pptx` only (user decision). `.ppt`, `.odp`, and `.key` are refused with a message naming the format ("Save it as .pptx first"). `.pptm` opens like `.pptx`; its macros never run.

## FR0: viewer

- `frame FILE` opens it; File → Open (Ctrl+O) opens a file dialog filtered to `.pptx`; dropping a file on the window opens it. Each file opens in its own window.
- Reading runs off the UI thread; the window shows "Opening…" until it is done. A file that can't be read gives an alert naming the file and the reason, and the window closes.
- A sidebar lists the slides as numbered thumbnails; the main area shows the selected slide scaled to fit, letterboxed. Page Down, Page Up, arrows, Home, and End move between slides; clicking a thumbnail selects it. Hidden slides show dimmed.
- Slides draw what PowerPoint shows for: slide size, backgrounds (solid), shapes with preset geometry (rectangles, rounded rectangles, ellipses, lines, and others drawn as their bounding rectangle), solid fills and outlines, rotation and flips, text with fonts, sizes, bold, italic, underline, strikethrough, colours (including theme colours and their tints and shades), alignment, bullets and numbering with levels, vertical anchoring and insets, autofit's font scale, pictures (PNG, JPEG, GIF, BMP), tables (cell text, fills, borders), and groups. Placeholders take their position and text style from the slide's layout and master.
- What Frame can't draw yet (charts, SmartArt without a saved drawing, EMF/WMF/SVG pictures, video and embedded objects without a preview picture) draws as a labelled outline at its position, so nothing silently disappears. Gradients and patterns draw as one colour standing for them (a gradient's middle stop, a pattern's foreground); freeform and other preset shapes draw as their bounding rectangle.
- F5 starts a full-screen slideshow from the first slide, Shift+F5 from the current one; the slide fills the screen, letterboxed in black. Space, Enter, arrows, Page Up/Down, Backspace, and a click move; hidden slides are skipped. Escape, or going past the last slide, ends it, back in the window on the slide it showed.
- The window title is the file name; the status bar shows "Slide N of M".
- Accessibility: each slide's text is exposed as the canvas's accessible description.

## Easier than PowerPoint

Frame edits the same `.pptx` decks as PowerPoint with less to learn and fewer steps (user decision): direct editing on the slide, as in Keynote, plus an outline view for writing (user decision). The rules every FR1 feature follows:

- **No ribbon, one inspector.** The header bar holds New Slide, Insert (Text, Shape, Picture, Table), Outline, and Play. A sidebar inspector shows only what applies to the selection: with nothing selected, the slide (layout, background, hidden); with text, its font, size, bold, italic, underline, colour, alignment, bullets, and level; with a shape, its fill, outline, and geometry; with a table, its rows, columns, and cell fill. Changes apply as you make them; formatting never opens a dialog.
- **Things line up by themselves.** Moving or resizing a shape snaps its edges and centre to the slide's edges and centre, to the layout's placeholder positions, and to other shapes' edges and centres, within 6 screen pixels, drawing a guide line for each snap; three shapes in a row snap to equal spacing. Holding Alt turns snapping off. Arrow keys nudge the selection by one screen pixel, Shift+arrow by ten.
- **Layouts are pictures, and new slides guess right.** New Slide (Ctrl+M) adds a slide after the current one with its layout, or, after a title slide, the deck's Title and Content layout. The New Slide menu and the inspector's layout picker show the deck's own layouts as thumbnails. Changing a slide's layout moves its text into the new layout's matching placeholders.
- **Content goes where it belongs.** A picture dropped, pasted, or inserted fills the selected or first empty picture or content placeholder; with none, it lands centred, scaled to fit the slide. Text that overflows its placeholder shrinks to fit (PowerPoint's shrink on overflow, saved as `normAutofit`).
- **Writing in an outline.** View → Outline shows every slide's title and bullet text as one editable outline, as PowerPoint's outline view does: Enter after a title starts a new slide, Tab and Shift+Tab change a line's level (Tab on a title makes it a bullet of the slide before; Shift+Tab on a first-level bullet makes it a new slide's title). Slides added from the outline take the Title and Content layout. Text outside the title and body placeholders (text boxes, tables, SmartArt) isn't in the outline and is never changed by it; such a slide shows its title and how many other items it holds. The outline and the slide edit the same deck and share one undo history.
- **Starting is one step.** File → New (Ctrl+N), or `frame` with no file, opens Frame's built-in blank 16:9 deck with a title slide, ready to type; there is no template gallery.
- **Undo, not confirmation.** Every edit undoes and redoes; Frame doesn't ask "are you sure" for anything undo can reverse. The only dialogs are opening and saving files and alerts.

**Measured:** the reference task, a 4-slide deck (title slide "Quarterly review" with a subtitle; "Highlights" with three bullets, one at the second level; "Revenue" with a picture from a file; "Next steps" with two bullets; saved as `.pptx`), takes fewer input actions in Frame than in PowerPoint. Input actions are clicks, drags, and key presses other than the content's own characters. Frame's count comes from its check script; PowerPoint's (Microsoft 365, fastest documented path) is counted by hand once and recorded in the story's Notes.

## FR1: editor (in progress)

Editing slide text on the slide and in the outline view, moving and resizing shapes with snapping, formatting through the inspector, inserting text boxes, shapes, pictures, and tables, adding, deleting, duplicating, and reordering slides, new decks from the built-in blank deck, and save by patching the original package, all with undo. Speaker notes come later.

Built so far:

- **Slide text.** A click in a text box or placeholder puts the caret where it lands (Shift+click selects to it); the canvas outlines empty placeholders, dashed, so there's somewhere to click. Typing, Backspace, Delete, Enter (a new paragraph with the same level, alignment, and bullet; numbers renumber), Shift+Enter (a line break), the arrows, Home and End (with Ctrl, the whole text's), Shift with any of them to select, Ctrl+A, and Escape to stop. Typed text takes the look of the character before the caret, or of an empty paragraph's mark, so a placeholder keeps its layout's style. While text is edited the arrows move the caret; Page Up and Page Down stop editing and change slides.
- **Undo.** Ctrl+Z undoes and Ctrl+Shift+Z or Ctrl+Y redoes, a word of typing at a time, going to the slide the step changed.

## Non-goals for now

Animations and transitions (kept in the file, not played), audio and video playback, presenter view, collaboration.
