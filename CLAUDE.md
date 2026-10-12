# CLAUDE.md

Cronkite: a Linux-native office suite in Rust: one Cargo workspace, three apps. Repo: `github.com/avdrav1/cronkite`; suite page: `sites/cronkite/` (cronkite.cc). Suite architecture: `docs/architecture/overview.md`; suite ADRs: `docs/adr/`.

- **Bricks** (`apps/bricks`, command `bricks`): a fast, lightweight spreadsheet. V0.1 is the best large-CSV editor on Linux; Excel compatibility comes later. Product spec: `apps/bricks/docs/SPEC.md`. Work queue: `BACKLOG.md`.
- **Slate** (`apps/slate`, command `slate`): pageless word processor for .txt, .md, and .docx: one endless column, never paginated, drawn by its own Pango flow. Spec: `docs/specs/slate.md`; design: `docs/adr/0003-slate-document-model.md`, `docs/adr/0005-slate-is-pageless.md`, `docs/adr/0007-slate-editing-engine.md`. SL0 builds a read-only viewer.
- **Frame** (`apps/frame`, command `frame`; working name): presentation app for .pptx, easier to use than PowerPoint: direct editing with one inspector, plus an outline view. Spec: `docs/specs/frame.md`; design: `docs/adr/0004-frame-presentation-model.md`, `docs/adr/0006-frame-editor.md`. FR0 builds a viewer with a slideshow.

Everything below about stories, invariants, crates, benchmarks, and ADRs is Bricks' unless it says otherwise.

## How we work

- Build one story at a time with the `next-story` skill. Ship releases with the `ship-it` skill.
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
8. **Non-goals stay out:** charts, pivots, macros, scripting, plugins, collaboration. Formulas, multiple sheets, XLSX (ADRs 0006-0008), and cell styles (ADRs 0009-0010) are in scope from M5. Bricks keeps the charts, pivots, and macros it can't edit when it saves.

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
| `suite-file` | format-independent file infrastructure (document names, atomic save) |

Bricks' crates, in `apps/bricks/crates/`:

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
| `bricks` (`apps/bricks`) | windows, menus, dialogs, OS integration (toolkit set by DEC-1) |

Slate's crates, in `apps/slate/crates/`:

| Crate | Owns |
| --- | --- |
| `document-model` | Slate's document model (ADR 0003): blocks, runs, plain-text reader with encoding and line-ending detection, Markdown reader (`pulldown-cmark`); writers: plain text in the file's encoding and line endings, Markdown copying unedited blocks' bytes from the source |
| `docx` | DOCX reader: `word/document.xml` with styles, numbering, hyperlinks, pictures, and tables into the document model; saving by patching the package read: unedited blocks' XML copied, edited and new ones written with the document's own styles and run looks, every other part copied |
| `document-edit` | edits as undoable commands on the document model (ADR 0007), in the document's blocks or a table cell's: text replacements holding the removed and inserted text, paragraph looks, list numbering, history that joins typing a word at a time |
| `flow` | the pageless flow's layout state (ADR 0007), toolkit-free: per-block heights in a Fenwick index, estimates replaced by time-sliced measuring, scroll anchoring, tables and pictures, hit-testing, selection, plain and accessible text, splicing in edited blocks; shapes text through a `Shaper` trait |
| `slate` (`apps/slate`) | the GTK 4 window: open, drop, alerts, menus, save and save as (atomic, off the UI thread, asking before closing with changes), and the page, which draws the flow (snapshot, Pango) and edits it: caret, input method, keys, undo, drag selection, copy, and accessible text |

Frame's crates, in `apps/frame/crates/`:

| Crate | Owns |
| --- | --- |
| `presentation-model` | slides, shapes, frames, text, fills, outlines, pictures, tables, and backgrounds with every value resolved (ADR 0004), where each look is set (own, layout, master, presentation, or an edit; ADR 0006) for runs, paragraphs, shapes, and backgrounds, each body's looks per level, the markup runs and paragraphs were read with, shapes' ids, placeholder slots, and whether they're grouped (for the save), and the deck's layouts with the new slide each makes; no dependencies |
| `pptx` | PPTX reading: package, slide order, layouts and each one's new slide and text box look, placeholder inheritance through layout and master, group transforms, text styles, theme colours and fonts, fills and outlines, autofit, pictures, tables with table styles, SmartArt drawings, notes; saving by patching a copy of the package read: text and its looks, shape boxes, fills and outlines, backgrounds, layout changes, inserted and deleted shapes, pictures with their media parts, tables, shrink to fit (ADR 0006); the built-in blank deck (`data/blank.pptx`) |
| `deck-edit` | edits as undoable commands on the presentation model (ADR 0006): text edits (typing, delete, Enter, line breaks), slide edits (new on a layout, duplicate, delete, move), shape box edits (move, resize) and snapping with smart guides, formatting (text and paragraph looks, fill, outline, background), layout changes, inserting (text boxes, shapes, pictures into placeholders or centred, tables) and deleting shapes, shrink to fit, the outline (projection and its edits), the canvas's keyboard flow (Ctrl+Enter's next placeholder, Tab and Shift+Tab levels), undo history with amended steps; toolkit-free |
| `frame` (`apps/frame`) | windows, slide list and slide commands, canvas and its slide renderer (GTK 4 snapshot, Pango), text editing on the canvas with PowerPoint's keyboard flow (caret in new slides' titles, Ctrl+Enter, Tab), selecting, dragging, resizing, and nudging shapes, the inspector sidebar, the Insert menu, picture drop and paste, shrinking text to fit, the outline view, save and save as, dialogs |

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo run -p bricks                      # or -p slate, -p frame
cargo test --release --workspace -- --ignored  # perf acceptance tests (timing asserted in release only)
python3 scripts/gen_corpus.py            # builds corpus/ (gitignored)
python3 scripts/fetch_delimiter_sample.py  # 50 real-world files into corpus/realworld/ (ENG-4 test)
python3 scripts/lo_navigation.py         # re-record GRID-3's checklist from LibreOffice (needs LibreOffice)
python3 scripts/lo_functions.py          # re-record formula results from LibreOffice (FML-5..8)
python3 scripts/lo_formats.py            # re-record number format results from LibreOffice (STYLE-2)
python3 scripts/lo_xlsx.py fixture|dump FILE  # LibreOffice fixture workbook and dumps (XLSX-*)
python3 scripts/lo_docx.py fixture|dump FILE  # Slate's LibreOffice fixture document and paragraph dumps (SLATE-3)
scripts/check_slate.sh [BINARY]          # Slate GUI check in docker: open, drop, show, refuse, text widths, select and copy, editing, tables, input method, accessibility, saving .txt, .md, and .docx (SLATE-4, -10, -11, -6, -12, -13, -14, -15, -7, -8; scripts/ime_helpers.py is the input method)
scripts/ship.sh slate build && scripts/ship.sh slate verify  # Slate's release tarball (dist/), started on Arch, Ubuntu 24.04, Fedora; scripts/check_install_slate.sh: its installer there, .docx and .md opening with it (SLATE-9)
cargo run --release -p xlsx --example gen_corpus_xlsx  # corpus/rows_1m.xlsx (BENCH-2)
cargo run --release -p xlsx --example gen_corpus_xlsx -- --styled   # corpus/styled_1m.xlsx (STYLE-3, BENCH-3)
python3 scripts/lo_pptx.py fixture|dump|layouts FILE  # LibreOffice fixture deck, dumps, and layout names (FRAME-*)
python3 scripts/blank_deck.py           # Frame's built-in blank deck: apps/frame/crates/pptx/data/blank.pptx (FRAME-14)
scripts/check_frame.sh [BINARY]          # Frame in a container: decks, drawing, Ctrl+O, drop, alerts, slideshow, editing, slides, save, shapes, inspector, insert, outline, new deck, with undo; the reference task counted against PowerPoint, read back by LibreOffice (FRAME-4 to FRAME-15)
python3 -m unittest discover -s scripts  # script tests (corpus generator, backlog helper)
cargo bench                              # prints the apps/bricks/docs/BENCHMARKS.md table (needs corpus/)
python3 scripts/backlog.py next [bricks|slate|frame]|status   # backlog helper; one milestone chain per app
```

## Performance

Targets live in `apps/bricks/docs/BENCHMARKS.md` and assume the reference box listed there. Report measured numbers in every perf-related commit. A change that regresses a benchmark more than 10% needs a note in BACKLOG.md.

## Decisions

Bricks' architecture decisions go in `apps/bricks/docs/decisions/NNNN-title.md` using `0000-template.md`; suite-wide ones in `docs/adr/`. Read the relevant ADR before touching that area. Spikes live in `spikes/<story-id>/` and are throwaway.
