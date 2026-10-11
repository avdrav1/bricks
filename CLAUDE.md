# CLAUDE.md

Cronkite: a Linux-native office suite in Rust: one Cargo workspace, three apps. Repo: `github.com/avdrav1/cronkite`; suite page: `sites/cronkite/` (cronkite.cc). Suite architecture: `docs/architecture/overview.md`; suite ADRs: `docs/adr/`.

- **Bricks** (`apps/bricks`, command `bricks`): a fast, lightweight spreadsheet. V0.1 is the best large-CSV editor on Linux; Excel compatibility comes later. Product spec: `apps/bricks/docs/SPEC.md`. Work queue: `BACKLOG.md`.
- **Slate** (`apps/slate`, command `slate`): word processor for .txt, .md, and .docx. Spec: `docs/specs/slate.md`; design: `docs/adr/0003-slate-document-model.md`. SL0 builds a read-only viewer.
- **Frame** (`apps/frame`, command `frame`; working name): presentation app for .pptx. Spec: `docs/specs/frame.md`; design: `docs/adr/0004-frame-presentation-model.md`. FR0 builds a viewer with a slideshow.

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
| `suite-file` | format-independent file infrastructure (document names) |

Bricks' crates, in `apps/bricks/crates/`:

| Crate | Owns |
| --- | --- |
| `csv-engine` | parsing, dialect and encoding detection, row index, CSV writing |
| `data-model` | cells, overlay, row map, inferred types, sort and filter views, cell styles |
| `formula` | formula language: parse, render, references, values, evaluation, functions, number formats; no dependencies |
| `xlsx` | XLSX import into indexed sheet copies with styles; save by patching the original package |
| `grid` | toolkit-free viewport, selection, navigation |
| `commands` | `Command` trait, undo/redo |
| `file-format` | atomic save |
| `jobs` | background jobs: shared pool, cancel tokens, results to the UI loop (ADR 0005) |
| `bricks` (`apps/bricks`) | windows, menus, dialogs, OS integration (toolkit set by DEC-1) |

Slate's crates, in `apps/slate/crates/`:

| Crate | Owns |
| --- | --- |
| `document-model` | Slate's document model (ADR 0003): blocks, runs, plain-text reader with encoding and line-ending detection, Markdown reader (`pulldown-cmark`) |
| `docx` | DOCX reader: `word/document.xml` with styles, numbering, hyperlinks, pictures, and tables into the document model |
| `slate` (`apps/slate`) | the GTK 4 viewer window: open, drop, the page as a read-only text view, alerts |

Frame's crates, in `apps/frame/crates/`:

| Crate | Owns |
| --- | --- |
| `presentation-model` | slides, shapes, frames, text, fills, outlines, pictures, tables, and backgrounds with every value resolved (ADR 0004); no dependencies |
| `pptx` | PPTX reading: package, slide order, placeholder inheritance through layout and master, group transforms, text styles, theme colours and fonts, fills and outlines, pictures, tables with table styles, SmartArt drawings, notes |
| `frame` (`apps/frame`) | windows, slide list, canvas and its slide renderer (GTK 4 snapshot, Pango), dialogs |

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
scripts/check_slate.sh [BINARY]          # Slate viewer GUI check in docker: open, drop, show, refuse (SLATE-4)
cargo run --release -p xlsx --example gen_corpus_xlsx  # corpus/rows_1m.xlsx (BENCH-2)
cargo run --release -p xlsx --example gen_corpus_xlsx -- --styled   # corpus/styled_1m.xlsx (STYLE-3, BENCH-3)
python3 scripts/lo_pptx.py fixture|dump FILE  # LibreOffice fixture deck and dumps (FRAME-*)
scripts/check_frame.sh [BINARY]          # Frame's viewer in a container: decks, Page Down, drawing, Ctrl+O, drop, alerts (FRAME-4)
python3 -m unittest discover -s scripts  # script tests (corpus generator, backlog helper)
cargo bench                              # prints the apps/bricks/docs/BENCHMARKS.md table (needs corpus/)
python3 scripts/backlog.py next [bricks|slate|frame]|status   # backlog helper; one milestone chain per app
```

## Performance

Targets live in `apps/bricks/docs/BENCHMARKS.md` and assume the reference box listed there. Report measured numbers in every perf-related commit. A change that regresses a benchmark more than 10% needs a note in BACKLOG.md.

## Decisions

Bricks' architecture decisions go in `apps/bricks/docs/decisions/NNNN-title.md` using `0000-template.md`; suite-wide ones in `docs/adr/`. Read the relevant ADR before touching that area. Spikes live in `spikes/<story-id>/` and are throwaway.
