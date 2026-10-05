# CLAUDE.md

A fast, lightweight Linux spreadsheet in Rust. V0.1 is the best large-CSV editor on Linux; Excel compatibility comes later. Product spec: `docs/SPEC.md`. Work queue: `BACKLOG.md`.

## How we work

- Build one story at a time with the `next-story` skill. Ship releases with the `ship-it` skill.
- A story is done only when its acceptance criterion is proven by a test or a benchmark, not by reading the code.
- Commit format: `<STORY-ID>: <imperative summary>`. One story per branch: `story/<id>-<slug>`.
- Never push, merge, tag, or deploy without asking first.

## Architecture invariants (do not break these)

1. **Raw values are authoritative.** Type inference affects sort, filter, and display only. `00123` stays `00123` on disk.
2. **Never materialize the whole file.** No `Vec<Vec<String>>` of all rows. Source stays indexed; edits live in the overlay.
3. **Layers stay separate:** source (csv-engine) -> overlay (data-model) -> view (sort/filter) -> grid -> app. Lower crates never depend on higher ones, and nothing below `app` depends on the UI toolkit.
4. **Nothing slow on the UI thread.** Parsing, indexing, sort, filter, search, save, inference run as background jobs with progress and cancel.
5. **Every mutation is an undoable command** that stores the operation, not a snapshot.
6. **Saving is atomic:** temp file in the same directory, verify, fsync, rename. A crash never corrupts the original.
7. **Filters never drop rows on save.** Sorting changes row order; filtering changes only the view.
8. **V0.1 non-goals stay out:** formulas, charts, pivots, multiple sheets, macros, scripting, styling, plugins, collaboration.

## Crates

| Crate | Owns |
| --- | --- |
| `csv-engine` | parsing, dialect and encoding detection, row index, CSV writing |
| `data-model` | cells, overlay, row map, inferred types, sort and filter views |
| `grid` | toolkit-free viewport, selection, navigation |
| `commands` | `Command` trait, undo/redo |
| `file-format` | atomic save |
| `app` | windows, menus, dialogs, OS integration (toolkit set by DEC-1) |

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/gen_corpus.py            # builds corpus/ (gitignored)
cargo bench                              # after BENCH-1
python3 scripts/backlog.py next|status   # backlog helper
```

## Performance

Targets live in `docs/BENCHMARKS.md` and assume the reference box listed there. Report measured numbers in every perf-related commit. A change that regresses a benchmark more than 10% needs a note in BACKLOG.md.

## Decisions

Architecture decisions go in `docs/decisions/NNNN-title.md` using `0000-template.md`. Read the relevant ADR before touching that area. Spikes live in `spikes/<story-id>/` and are throwaway.
