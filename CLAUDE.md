# CLAUDE.md

Cronkite: a Linux-native office suite in Rust: one Cargo workspace, four apps. Repo: `github.com/avdrav1/cronkite`; web pages: `sites/<name>/` (cronkite.cc, brick.cronkite.cc, slate.cronkite.cc, frame.cronkite.cc). Suite architecture: `docs/architecture/overview.md`; suite ADRs: `docs/adr/`.

- **Brick** (`apps/brick`, command `brick`): a fast, lightweight spreadsheet. V0.1 is the best large-CSV editor on Linux; Excel compatibility comes later. Product spec: `apps/brick/docs/SPEC.md`. Work queue: `BACKLOG.md`.
- **Slate** (`apps/slate`, command `slate`): pageless word processor for .txt, .md, and .docx: one endless column, never paginated, drawn by its own Pango flow. Spec: `docs/specs/slate.md`; design: `docs/adr/0003-slate-document-model.md`, `docs/adr/0005-slate-is-pageless.md`, `docs/adr/0007-slate-editing-engine.md`. SL0 builds a read-only viewer.
- **Frame** (`apps/frame`, command `frame`): presentation app for .pptx, easier to use than PowerPoint: direct editing with one inspector, plus an outline view. Spec: `docs/specs/frame.md`; design: `docs/adr/0004-frame-presentation-model.md`, `docs/adr/0006-frame-editor.md`. FR0 builds a viewer with a slideshow.
- **Etch** (`apps/etch`, command `etch`): raster image editor, closer to Paint than to Photoshop: pencil, brush, eraser, fill, shapes, text, selection, crop, resize, rotate; PNG, JPEG, BMP, edited as one RGBA8 image with tiled undo. Spec: `docs/specs/etch.md`; design: `docs/adr/0010-etch-image-model.md`. ET0 builds a canvas prototype.

Everything below about stories, invariants, crates, benchmarks, and ADRs is Brick's unless it says otherwise.

## How we work

- Build one story at a time with the `next-story` skill (`scripts/backlog.py next <app>`: each app has its own milestone chain in `BACKLOG.md`). Ship releases with the `ship-it` skill (`scripts/ship.sh <app> …`).
- A story is done only when its acceptance criterion is proven by a test or a benchmark, not by reading the code.
- Commit format: `<STORY-ID>: <imperative summary>`. One story per branch: `story/<id>-<slug>`.
- Never push, merge, tag, or deploy without asking first.

## Architecture invariants (do not break these)

1. **Raw values are authoritative.** Type inference affects sort, filter, and display only. `00123` stays `00123` on disk.
2. **Never materialize the whole file.** No `Vec<Vec<String>>` of all rows. Source stays indexed; edits live in the overlay.
3. **Layers stay separate:** source (csv-engine) -> overlay (data-model, with formula) -> view (sort/filter) -> grid -> app. Lower crates never depend on higher ones, and nothing below `app` depends on the UI toolkit.
4. **Nothing slow on the UI thread.** Parsing, indexing, sort, filter, search, save, inference run as background jobs with progress and cancel.
5. **Every mutation is an undoable command** that stores the operation, not a snapshot.
6. **Saving is atomic:** temp file in the same directory, verify, fsync, rename. A crash never corrupts the original.
7. **Filters never drop rows on save.** Sorting changes row order; filtering changes only the view.
8. **Non-goals stay out:** charts, pivots, macros, scripting, plugins, collaboration. Formulas, multiple sheets, XLSX (ADRs 0006-0008), and cell styles (ADRs 0009-0010) are in scope from M5. Brick keeps the charts, pivots, and macros it can't edit when it saves.

## Suite rules

- Shared crates live in `crates/suite-*`; each app's own crates live under `apps/<app>/crates/`.
- `suite-core` depends on nothing in the workspace; within the workspace, `suite-ui`, `suite-platform`, and `suite-file` may depend only on `suite-core`. No shared crate depends on an app or an app's crates.
- The suite's UI toolkit is GTK 4 (`docs/adr/0002-gtk4-for-the-suite.md`), declared once as `gtk` in `[workspace.dependencies]`. Only apps, `suite-ui`, and `suite-platform` may depend on it; `suite-core`, `suite-file`, and engine crates stay toolkit-free.
- Code becomes shared only when at least two apps genuinely need it. Share infrastructure, not product models.
- Suite-wide decisions go in `docs/adr/NNNN-title.md`.

## Crates

| Crate | Owns |
| --- | --- |
| `suite-core` | `AppKind` and other app-independent primitives |
| `suite-ui` | UI conventions shared by the apps |
| `suite-platform` | OS integration (Linux) |
| `suite-file` | format-independent file infrastructure (document names, atomic save, crash recovery journals' directory: owner locks, atomic journal writes, crashed sessions) |

Brick's crates, in `apps/brick/crates/`:

| Crate | Owns |
| --- | --- |
| `csv-engine` | parsing, dialect and encoding detection, row index, CSV writing |
| `data-model` | cells, overlay, row map, inferred types, sort and filter views, cell styles |
| `formula` | formula language: parse, render, references, values, evaluation, functions, number formats; no dependencies |
| `xlsx` | XLSX import into indexed sheet copies with styles; save by patching the original package |
| `grid` | toolkit-free viewport, selection, navigation |
| `commands` | `Command` trait, undo/redo |
| `file-format` | CSV save (atomic through suite-file) |
| `jobs` | background jobs: shared pool, cancel tokens, results to the UI loop (ADR 0005) |
| `brick` (`apps/brick`) | windows, menus, dialogs, OS integration (toolkit set by DEC-1) |

Slate's crates, in `apps/slate/crates/`:

| Crate | Owns |
| --- | --- |
| `document-model` | Slate's document model (ADR 0003): blocks, runs as direct formatting over their paragraph's style, style sheets (ADR 0008: each style's resolved font, size, looks, alignment, spacing, line spacing, indents, tab stops, borders, shading; Slate's built-in sheet for Markdown and text), tab stops and their merging (ADR 0009), a Word document's page text width, default tab stop, and font table with embedded faces, Word numbering levels and their markers (Symbol and Wingdings mapped to Unicode), plain-text reader with encoding and line-ending detection, Markdown reader (`pulldown-cmark`); writers: plain text in the file's encoding and line endings, Markdown copying unedited blocks' bytes from the source |
| `docx` | DOCX reader: `word/document.xml` with styles (every paragraph style, custom ones included, through based-on and docDefaults, with theme fonts), tab stops and `w:defaultTabStop`, a paragraph's own spacing, indents, borders, and shading, the last section's page text width, the font table with embedded fonts (Word's obfuscated .odttf de-obfuscated), numbering with each level's marker text, alignment, suffix, indents, and look, hyperlinks, pictures, and tables into the document model; saving by patching the package read: unedited blocks' XML copied, edited and new ones written with the document's own styles and runs' direct formatting, every other part copied; the built-in blank document (`data/blank.docx`) |
| `document-edit` | edits as undoable commands on the document model (ADR 0007), in the document's blocks or a table cell's: text replacements holding the removed and inserted text, paragraph looks (style, alignment), run looks (font, size, bold, italic, underline, strikethrough, colour), the looks summary at the caret or over a selection, list numbering, history that joins typing a word at a time; find (cancellable, case-folding) and replacing one match or all of them as one step; replacing the whole document as one step (crash recovery) |
| `flow` | the pageless flow's layout state (ADR 0007), toolkit-free: per-block heights in a Fenwick index, estimates replaced by time-sliced measuring (metrics per font, size, and weight), scroll anchoring, paragraph space, indents, line spacing, tab stops, borders, and shading from each paragraph's style and its own, list markers placed by their numbering level (ADR 0008, 0009), tables and pictures, hit-testing, selection, plain and accessible text, splicing in edited blocks; shapes text through a `Shaper` trait |
| `slate` (`apps/slate`) | the GTK 4 window: open, drop, alerts, menus (Text Width, Page by default: a Word document's page width; ADR 0009), the format bar (style, font, size, B/I/U/S, colour, alignment; ADR 0008), the find bar, save and save as (atomic, off the UI thread, asking before closing with changes), crash recovery (journals of unsaved edits written off the UI thread, offered back after a crash), embedded fonts loaded for the process through fontconfig and fallbacks by the font table's family, and the page, which draws the flow (snapshot, Pango; each run in its style's font, size, and looks with its own on top; tab stops with leaders) and edits it: caret, input method, keys, undo, drag selection, copy, find's highlights and searches off the UI thread, and accessible text |

Frame's crates, in `apps/frame/crates/`:

| Crate | Owns |
| --- | --- |
| `presentation-model` | slides, shapes, frames, text, fills, outlines, pictures, tables, and backgrounds with every value resolved (ADR 0004), where each look is set (own, layout, master, presentation, or an edit; ADR 0006) for runs, paragraphs, shapes, and backgrounds, each body's looks per level, the markup runs and paragraphs were read with, shapes' ids, placeholder slots, and whether they're grouped (for the save), and the deck's layouts with the new slide each makes; no dependencies |
| `pptx` | PPTX reading: package, slide order, layouts and each one's new slide and text box look, placeholder inheritance through layout and master, group transforms, text styles, theme colours and fonts, fills and outlines, autofit, pictures, tables with table styles, SmartArt drawings, notes; saving by patching a copy of the package read: text and its looks, shape boxes, fills and outlines, backgrounds, layout changes, inserted and deleted shapes, pictures with their media parts, tables, shrink to fit (ADR 0006); the built-in blank deck (`data/blank.pptx`) |
| `deck-edit` | edits as undoable commands on the presentation model (ADR 0006): text edits (typing, delete, Enter, line breaks), slide edits (new on a layout, duplicate, delete, move), shape box edits (move, resize) and snapping with smart guides, formatting (text and paragraph looks, fill, outline, background), layout changes, inserting (text boxes, shapes, pictures into placeholders or centred, tables) and deleting shapes, shrink to fit, the outline (projection and its edits), the canvas's keyboard flow (Ctrl+Enter's next placeholder, Tab and Shift+Tab levels), undo history with amended steps; toolkit-free |
| `frame` (`apps/frame`) | windows, slide list and slide commands, canvas and its slide renderer (GTK 4 snapshot, Pango), text editing on the canvas with PowerPoint's keyboard flow (caret in new slides' titles, Ctrl+Enter, Tab), selecting, dragging, resizing, and nudging shapes, the inspector sidebar, the Insert menu, picture drop and paste, shrinking text to fit, the outline view, save and save as, dialogs |

Etch's crates, in `apps/etch/crates/`:

| Crate | Owns |
| --- | --- |
| `image-model` | Etch's image model (ADR 0010): the RGBA8 `Image` (straight alpha, at most 16384 px a side; new, or from decoded pixels), `Color`, pixel `Rect`s, the pencil (Bresenham lines stamped with a disc) and the eraser (the same tip writing transparent pixels), the brush (hard or soft round tip composited source-over, each pixel at the most coverage its stroke gave it), the fill (scanline flood fill, 4-connected, exact colour or within a tolerance: the largest channel difference, alpha included; with a fully transparent pixel alpha alone), shapes (`shape.rs`: line, rectangle, and ellipse, hard-edged, outlined, filled, or both, Shift's constraint; the line the pencil's, the ellipse Zingl's midpoint ellipse stamped with the pencil's disc; as one-colour rectangles a few a row, which the canvas previews and committing paints, one coat source-over), blends (`blend.rs`: given pixels laid over the image source-over at a point, clipped, the step keeping them so redo lays the same bytes: the text tool's rendered text, later pastes), the selection (`select.rs`: `Floating` pixels lifted off the image or from elsewhere, moved, and landed as one step replacing the pixels under them, the place they were lifted from cleared to transparency, the step keeping them; the rectangle a selection drag spans), clearing a rectangle to transparency, the Image menu's transforms (`transform.rs`: `Transform`, crop to a rectangle, resize (`resample.rs`: nearest neighbour, and smooth: a triangle filter weighting colour by alpha, ImageMagick's `-filter Triangle -resize` within a level; `par.rs` splits the new image's rows across the cores), resize the canvas (an `Anchor` of nine placing the pixels, the new area a colour), rotate a quarter either way or half and flip either way (`orient.rs`: `Turn`, `Flip`, exact, each with an inverse); each makes a new image from the old without touching it, so it can run off the UI thread, and `Document::install` takes its result), and `Document`: operations applied to the image, each returning the rect it changed, with undo (the 64-px tiles an operation replaced; a transform that loses pixels keeps the whole image on its step's other side, which undo and redo swap in; rotate and flip keep nothing and undo by their inverse), redo (replaying the operation), and history positions (equal positions, equal images: what a saved file is compared by); no dependencies |
| `image-io` | Etch's image files, toolkit-free: reading PNG, JPEG, and BMP (the pure-Rust `image` crate, those three codecs only) into an `Image`, the format told by content, PNG keeping its alpha, JPEG and BMP opaque, images over 16384 px a side refused before decoding, and each failure's reason in words (`OpenError`: unreadable, empty, unsupported format, damaged or truncated, too large); saving PNG (alpha kept exactly), JPEG (quality 90), and BMP (24-bit), the last two composited onto white, the format by the name's extension, written atomically through `suite_file::save_atomic` |
| `etch` (`apps/etch`) | windows (one per image) with the menu bar, the tool column (`tools.rs`: the tools, their keys, icons drawn from paths, driving the `win.tool` action, and `Settings`: tool, foreground and background, each tool's size, the brush's edge, the shapes' style, the text's font, the fill's tolerance, the tool a pick goes back to, the action they make: draw with a tip, fill, pick, drag a shape, type text, or select), the colour row (`colours.rs`: foreground/background indicator, Paint's palette, the size, the fill's Tolerance, the brush's Hard and Soft, the shapes' Outline, Filled, and Both, the text's font list, Bold, and Italic) and status bar (pointer position, the selection's size, zoom, size), New Image (size and background: white, transparent, or the background colour; the last ones remembered across runs by `settings.rs` in `$XDG_CONFIG_HOME/etch/settings`), opening files (`etch FILE...` and File → Open: read off the UI thread through `image-io` while the window says "Opening…", an alert naming a file that can't be opened), saving (`saving.rs`: Save, Save As, Export, the image copied on the UI thread and written off it, the question before writing transparency as JPEG or BMP, the title's "• " while unsaved, Save/Discard/Cancel on close and quit), the canvas (tile textures rebuilt only where an operation changed pixels, nearest neighbour from 100%, checkerboard, boundary, the tip's ring at the pointer, a dragged shape's preview drawn over the image from its rectangles, Escape dropping it, text being typed drawn over the image as it will land with its frame and caret, the selection's dashed outline and its floating pixels drawn over the image as they'll land), its toolkit-free view math (`view.rs`: zoom steps 10%–3200% around a point, pan limits), the pencil, brush, eraser, fill, colour picker, line, rectangle, and ellipse, the text tool (`text.rs`: text typed through the input method, laid out and rendered with Pango into straight RGBA, put on the image as a blend), the selection (select, move, nudge, Select All, Deselect, Delete), cut, copy, and paste through the clipboard, the Image menu (`image_menu.rs`: Crop to Selection, Resize Image… with its dialog (width, height, keep aspect ratio, Smooth or Nearest neighbour), Resize Canvas… with its dialog (width, height, the anchor's 3×3 toggle buttons, the new area transparent or the background colour), Rotate 90° Right and Left, Rotate 180°, Flip Horizontal and Vertical, and their keys; `Canvas::transform` runs a transform off the UI thread on a copy of the image, the canvas insensitive until its result is in, then shows the image anew, the view kept when the size didn't change), pan by wheel, middle button, or Space, Ctrl+wheel zoom, undo and redo, `--bench-stroke` and `--bench-open` |

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo run -p brick                       # or -p slate, -p frame, -p etch
cargo test --release --workspace -- --ignored  # perf acceptance tests (timing asserted in release only)
python3 scripts/gen_corpus.py            # builds corpus/ (gitignored)
python3 scripts/fetch_delimiter_sample.py  # 50 real-world files into corpus/realworld/ (ENG-4 test)
python3 scripts/lo_navigation.py         # re-record GRID-3's checklist from LibreOffice (needs LibreOffice)
python3 scripts/lo_functions.py          # re-record formula results from LibreOffice (FML-5..8)
python3 scripts/lo_formats.py            # re-record number format results from LibreOffice (STYLE-2)
python3 scripts/lo_xlsx.py fixture|dump FILE  # LibreOffice fixture workbook and dumps (XLSX-*)
python3 scripts/lo_docx.py fixture|dump|spacing|borders|lists FILE  # Slate's LibreOffice fixture document, paragraph dumps (SLATE-3), and the corpus documents for paragraphs' own spacing and indents (SLATE-26), borders and shading (SLATE-27), and list markers (SLATE-28)
scripts/check_slate.sh [BINARY]          # Slate GUI check in docker: open, drop, show, refuse, text widths, select and copy, editing, tables, input method, accessibility, saving .txt, .md, and .docx, new documents, a .docx's fonts and paragraph spacing measured against Pango, the format bar and saving its looks, find and replace, crash recovery, page width, tab stops, embedded fonts, paragraphs' own spacing and indents, borders and shading, list markers (SLATE-4, -10, -11, -6, -12, -13, -14, -15, -7, -8, -18, -20, -21, -22, -16, -17, -23, -24, -25, -26, -27, -28; scripts/ime_helpers.py is the input method)
scripts/ship.sh slate build && scripts/ship.sh slate verify  # Slate's release tarball (dist/), started on Arch, Ubuntu 24.04, Fedora; scripts/check_install_slate.sh: its installer there, .docx and .md opening with it (SLATE-9)
scripts/ship.sh frame build && scripts/ship.sh frame verify  # Frame's release tarball (dist/), opening and drawing a deck on Arch, Ubuntu 24.04, Fedora; scripts/check_install_frame.sh: its installer there, .pptx opening with it
scripts/ship.sh etch build && scripts/ship.sh etch verify  # Etch's release tarball (dist/), drawing strokes on Arch, Ubuntu 24.04, Fedora; scripts/check_install_etch.sh: its installer there, the menu entry starting it (ETCH-5), Etch the default for PNG, JPEG, and BMP, `gio open` opening a .png in it, other defaults kept, uninstall dropping Etch's (ETCH-6)
cargo run --release -p xlsx --example gen_corpus_xlsx  # corpus/rows_1m.xlsx (BENCH-2)
cargo run --release -p xlsx --example gen_corpus_xlsx -- --styled   # corpus/styled_1m.xlsx (STYLE-3, BENCH-3)
python3 scripts/lo_pptx.py fixture|dump|layouts FILE  # LibreOffice fixture deck, dumps, and layout names (FRAME-*)
python3 scripts/blank_deck.py           # Frame's built-in blank deck: apps/frame/crates/pptx/data/blank.pptx (FRAME-14)
python3 scripts/blank_docx.py           # Slate's built-in blank document: apps/slate/crates/docx/data/blank.docx (SLATE-18)
scripts/site_shots.sh [OUTDIR]          # cronkite.cc's gallery: the latest releases of Brick, Slate, and Frame screenshotted in a container on demo files (scripts/site_demo.py, LibreOffice) into sites/cronkite/shots (needs corpus/)
scripts/check_frame.sh [BINARY]          # Frame in a container: decks, drawing, Ctrl+O, drop, alerts, slideshow, editing, slides, save, shapes, inspector, insert, outline, new deck, with undo; the reference task counted against PowerPoint, read back by LibreOffice; the installer and a .pptx double-click (FRAME-4 to FRAME-15)
python3 -m unittest discover -s scripts  # script tests (corpus generator, backlog helper)
cargo bench                              # prints the apps/brick/docs/BENCHMARKS.md table (needs corpus/)
python3 scripts/backlog.py next [brick|slate|frame|etch]|status   # backlog helper; one milestone chain per app
(cd video && npm ci && node render.mjs)   # the explainer video (docs/video/explainer-script.md) from HTML scenes, with a synthesised techno soundtrack (video/soundtrack.js) and a voiceover in Microsoft's Irish voice, Connor (video/voice.mjs, cached in video/.voice/; needs the network) -> dist/video/; --cut short renders the under-a-minute cut (video/short.js; --cut short-hi for the Hindi one, narrated by Microsoft's hi-IN-MadhurNeural), --serve previews (?cut=short or ?cut=short-hi), --still SCENE@SEC checks a frame, --voice NAME picks the narrator
scripts/check_etch.sh [BINARY]           # Etch GUI check in docker: blank canvas, pencil, undo and redo, window resize, zoom around the pointer and sharp pixels, panning, New Image, the stroke benchmark (ETCH-2), the eraser over the checkerboard and the tool column (ETCH-3), palette colours, X, and sizes (ETCH-4), opening a PNG with transparency, Ctrl+O, and damaged or unsupported files' alerts (ETCH-6), saving (ETCH-7), the hard and soft brush (ETCH-8), the fill bucket (ETCH-9), the colour picker (ETCH-10), shapes previewed while dragged, landing as one undo step, Escape dropping them (ETCH-11), text typed on the canvas (tool keys typed as letters) going on with Enter or a click elsewhere as one undo step, Escape dropping it, Bold and the font list, an input method composing (scripts/ime_helpers.py) (ETCH-12), a transparent new image saved all alpha 0 and New Image's size remembered after a restart (ETCH-13), the fill's tolerance taking a near colour at 6 and not at 0 or 5 (ETCH-14), the selection: its size in the status, its dashed outline, moving its pixels as one undo step, nudging, Select All, Deselect, Delete (ETCH-15), cut, copy, and paste with another app (ETCH-16), Crop to Selection from the Image menu and Ctrl+Shift+X, saved exactly, one Ctrl+Z restoring size and pixels (ETCH-17), Resize Image's dialog (linked sides), nearest saved exactly and smooth's alpha-weighted edge, one Ctrl+Z restoring (ETCH-18), Resize Canvas's dialog (anchors pressed through AT-SPI), the old pixels placed by the anchor, the new area transparent or the background colour, shrinking about the centre, one Ctrl+Z restoring (ETCH-19)
sh apps/etch/crates/image-model/tests/data/resize/make_references.sh  # Resize Image's smooth fixture and its ImageMagick Triangle-filter references (ETCH-18; needs ImageMagick 7)
sh apps/etch/crates/image-io/tests/data/make_fixtures.sh  # image-io's PNG, JPEG, and BMP fixtures and their ImageMagick-decoded references (ETCH-6; needs ImageMagick 7)
cargo test --release -p etch --test open_8k -- --ignored --nocapture  # an 8K PNG shows within 1 s (ETCH-6; opens a window)
cargo test --release -p image-model -- --ignored --nocapture  # an 8K copy for saving (ETCH-7), 8K fills under 200 ms (ETCH-9), an 8K selection lifted, landed, and deleted under 200 ms (ETCH-15), and the Image menu's transforms on 8K: their copy and undo on the UI thread, the transform itself (crop ETCH-17, resize ETCH-18, resize canvas ETCH-19)
```

## Performance

Targets live in `apps/brick/docs/BENCHMARKS.md` and assume the reference box listed there. Report measured numbers in every perf-related commit. A change that regresses a benchmark more than 10% needs a note in BACKLOG.md.

## Decisions

Brick's architecture decisions go in `apps/brick/docs/decisions/NNNN-title.md` using `0000-template.md`; suite-wide ones in `docs/adr/`. Read the relevant ADR before touching that area. Spikes live in `spikes/<story-id>/` and are throwaway.
