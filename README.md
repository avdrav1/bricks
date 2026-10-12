# Cronkite

A Linux-native office suite written in Rust: Brick, Slate, and Frame. One repository, one Cargo workspace, three independently runnable applications. Install instructions: <https://cronkite.cc>.

## Applications

### Brick

A fast CSV editor evolving toward a general spreadsheet application (`apps/brick`, command `brick`).

- Initial product promise: open large CSV files faster and more smoothly than LibreOffice.
- Long-term goal: become a serious Linux-native spreadsheet alternative.

Brick is the most advanced of the three: it ships today (GTK 4), opens ~1 GB CSV files and `.xlsx` workbooks, and has formulas and cell styles. Install, usage, and its own layout: [`apps/brick/README.md`](apps/brick/README.md).

### Slate

A lightweight Linux-native word processor (`apps/slate`, command `slate`). Scaffold only.

### Frame

A lightweight Linux-native presentation application (`apps/frame`, command `frame`). Scaffold only. Frame is a working name.

## Layout

```
apps/brick/           Brick: the app, its own crates (apps/brick/crates/), and its docs
apps/slate/           Slate
apps/frame/           Frame
crates/suite-core     app-independent primitives (AppKind)
crates/suite-ui       UI conventions shared by the apps
crates/suite-platform OS integration (Linux)
crates/suite-file     format-independent file infrastructure
docs/architecture/    how the suite is put together
docs/adr/             suite-wide architecture decisions
docs/specs/           one product spec per app
BACKLOG.md            Brick's work queue, read by scripts/backlog.py
scripts/, packaging/  tooling and packaging (desktop entries and icons for Brick, Slate, and Frame)
sites/brick/          Brick's download page and installer (brick.cronkite.cc)
sites/slate/          Slate's download page and installer (slate.cronkite.cc)
sites/frame/          Frame's download page and installer (frame.cronkite.cc)
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
```

Logs go to stderr.

## License

MIT OR Apache-2.0: see `LICENSE` and `LICENSE-APACHE`.
