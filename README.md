# Spreadsheet

A fast, lightweight spreadsheet for Linux, written in Rust. V0.1 goal: open, browse, edit, sort, filter, search, and save ~1 GB CSV files faster than LibreOffice Calc.

## Start here

1. Install Rust stable (`rustup`), Python 3, GTK 4.14+ dev files (`gtk4` on Arch, `libgtk-4-dev` on Debian/Ubuntu), and Node (for `npx wrangler` when shipping).
2. `cargo test --workspace` should pass on a fresh clone.
3. Paste the product spec into `docs/SPEC.md`.
4. `python3 scripts/gen_corpus.py` builds test files in `corpus/` (about 5 GB at full size; use `--max-mb 100` to start small).
5. Open the repo in Claude Code and say **"next story"**.

## Run it

```sh
cargo run --release -p spreadsheet -- path/to/file.csv
```

Scroll with the wheel, scrollbars, arrows, Page Up/Down, Home/End. Double-click a cell to edit it (Enter applies, Esc cancels). Ctrl+S saves over the file atomically: untouched rows are written byte for byte, and a crash mid-save leaves the original intact. The title shows unsaved edits and save progress. The delimiter (comma, tab, semicolon, pipe) is detected on open; the dropdown in the header bar overrides it without a restart (save edits first). Encodings: UTF-8 (with or without BOM), UTF-16 LE/BE, and Windows-1252 ("Latin-1") are detected on open and kept on save, BOM included.

## Layout

```
crates/csv-engine    parsing, dialects, encodings, row index
crates/data-model    cells, edit overlay, types, sort/filter views
crates/grid          viewport, selection, navigation (no toolkit)
crates/commands      undo/redo
crates/file-format   atomic save
app/                 the desktop app (GTK4, ADR 0001)
BACKLOG.md           the work queue, read by scripts/backlog.py
docs/                spec, benchmarks, architecture decisions
site/                download page deployed to Cloudflare Pages
scripts/             backlog helper, corpus generator, release pipeline
.claude/skills/      next-story and ship-it skills for Claude Code
```

## Claude Code skills

- **next-story**: picks the next unblocked story from `BACKLOG.md`, writes a failing test from its acceptance criterion, implements it, verifies fmt/clippy/tests/benchmarks, updates the backlog, and commits locally. Stops at milestone gates and decision reviews for your approval.
- **ship-it**: preflight, version bump, release build, upload to Cloudflare R2, deploy the download page to Cloudflare Pages, tag. Asks before anything leaves your machine. Needs `.env.ship` (see `.env.ship.example`).

## License

MIT OR Apache-2.0 (open question in the plan; change before the first release if needed).
