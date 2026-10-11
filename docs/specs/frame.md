# Frame

Fast, lightweight Linux-native presentation application. Command `frame`; "Frame" is a working name. Design: [ADR 0004](../adr/0004-frame-presentation-model.md).

## Format

`.pptx` only (user decision). `.ppt`, `.odp`, and `.key` are refused with a message naming the format ("Save it as .pptx first"). `.pptm` opens like `.pptx`; its macros never run.

## FR0: viewer

- `frame FILE` opens it; File → Open (Ctrl+O) opens a file dialog filtered to `.pptx`; dropping a file on the window opens it. Each file opens in its own window.
- Reading runs off the UI thread; the window shows "Opening…" until it is done. A file that can't be read gives an alert naming the file and the reason, and the window closes.
- A sidebar lists the slides as numbered thumbnails; the main area shows the selected slide scaled to fit, letterboxed. Page Down, Page Up, arrows, Home, and End move between slides; clicking a thumbnail selects it. Hidden slides show dimmed.
- Slides draw what PowerPoint shows for: slide size, backgrounds (solid), shapes with preset geometry (rectangles, rounded rectangles, ellipses, lines, and others drawn as their bounding rectangle), solid fills and outlines, rotation and flips, text with fonts, sizes, bold, italic, underline, strikethrough, colours (including theme colours and their tints and shades), alignment, bullets and numbering with levels, vertical anchoring and insets, autofit's font scale, pictures (PNG, JPEG, GIF, BMP), tables (cell text, fills, borders), and groups. Placeholders take their position and text style from the slide's layout and master.
- What Frame can't draw yet (charts, SmartArt without a saved drawing, EMF/WMF/SVG pictures, video and embedded objects without a preview picture) draws as a labelled outline at its position, so nothing silently disappears. Gradients and patterns draw as one colour standing for them (a gradient's middle stop, a pattern's foreground); freeform and other preset shapes draw as their bounding rectangle.
- F5 starts a full-screen slideshow from the first slide, Shift+F5 from the current one; Space, Enter, arrows, Page Up/Down move; Escape ends it.
- The window title is the file name; the status bar shows "Slide N of M".
- Accessibility: each slide's text is exposed as the canvas's accessible description.

## FR1: editor (not built yet)

Editing slide text and shapes with undo, adding, deleting, and reordering slides, save by patching the original package, speaker notes.

## Non-goals for now

Animations and transitions (kept in the file, not played), audio and video playback, presenter view, collaboration.
