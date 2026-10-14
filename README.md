# Cronkite

A Linux-native office suite written in Rust: Brick, Slate, Frame, and Etch. One repository, one Cargo workspace, four independently runnable applications. Install instructions: <https://cronkite.cc>.

```text
Brick — spreadsheet / CSV editor
Slate — word processor
Frame — presentation application
Etch  — raster image editor
```

## Applications

### Brick

A fast CSV editor evolving toward a general spreadsheet application (`apps/brick`, command `brick`).

- Initial product promise: open large CSV files faster and more smoothly than LibreOffice.
- Long-term goal: become a serious Linux-native spreadsheet alternative.

Brick is the most advanced of the four: it ships today (GTK 4), opens ~1 GB CSV files and `.xlsx` workbooks, and has formulas and cell styles. Install, usage, and its own layout: [`apps/brick/README.md`](apps/brick/README.md).

### Slate

A lightweight, pageless Linux-native word processor for .txt, .md, and .docx (`apps/slate`, command `slate`). Spec: [`docs/specs/slate.md`](docs/specs/slate.md).

### Frame

A lightweight Linux-native presentation application for .pptx, easier to use than PowerPoint (`apps/frame`, command `frame`). Frame is a working name. Spec: [`docs/specs/frame.md`](docs/specs/frame.md).

### Etch

A fast, approachable Linux-native raster image editor, closer to Paint than to Photoshop (`apps/etch`, command `etch`): draw, erase, crop, resize, annotate screenshots, and save PNG, JPEG, and BMP. In progress: the ET0 canvas prototype. Spec: [`docs/specs/etch.md`](docs/specs/etch.md).

## Layout

```
apps/brick/           Brick: the app, its own crates (apps/brick/crates/), and its docs
apps/slate/           Slate
apps/frame/           Frame
apps/etch/            Etch
crates/suite-core     app-independent primitives (AppKind)
crates/suite-ui       UI conventions shared by the apps
crates/suite-platform OS integration (Linux)
crates/suite-file     format-independent file infrastructure
docs/architecture/    how the suite is put together
docs/adr/             suite-wide architecture decisions
docs/specs/           one product spec per app
BACKLOG.md            every app's work queue, read by scripts/backlog.py
scripts/, packaging/  tooling and packaging (desktop entries and icons for the apps)
sites/brick/          Brick's download page and installer (brick.cronkite.cc)
sites/slate/          Slate's download page and installer (slate.cronkite.cc)
sites/frame/          Frame's download page and installer (frame.cronkite.cc)
sites/etch/           Etch's download page and installer (etch.cronkite.cc)
sites/cronkite/       the suite's install page (cronkite.cc)
```

Shared crates stay small; code becomes shared only when at least two apps genuinely need it. See [`docs/architecture/overview.md`](docs/architecture/overview.md).

## Development

Needs Rust stable (pinned by `rust-toolchain.toml`) and GTK 4.14+ dev files (`gtk4` on Arch, `libgtk-4-dev` on Debian/Ubuntu): the suite's toolkit is GTK 4 ([ADR 0002](docs/adr/0002-gtk4-for-the-suite.md)).

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all
```

Run one application:

```sh
cargo run -p brick
cargo run -p slate
cargo run -p frame
cargo run -p etch
```

Logs go to stderr.

## License

MIT OR Apache-2.0: see `LICENSE` and `LICENSE-APACHE`.
