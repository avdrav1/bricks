# Cronkite

A Linux-native office suite written in Rust: Bricks, Slate, and Frame. One repository, one Cargo workspace, three independently runnable applications. Install instructions: <https://cronkite.cc>.

## Applications

### Bricks

A fast CSV editor evolving toward a general spreadsheet application (`apps/bricks`, command `bricks`).

- Initial product promise: open large CSV files faster and more smoothly than LibreOffice.
- Long-term goal: become a serious Linux-native spreadsheet alternative.

Bricks is the most advanced of the three: it ships today (GTK 4), opens ~1 GB CSV files and `.xlsx` workbooks, and has formulas and cell styles. Install, usage, and its own layout: [`apps/bricks/README.md`](apps/bricks/README.md).

### Slate

A lightweight Linux-native word processor (`apps/slate`, command `slate`). Scaffold only.

### Frame

A lightweight Linux-native presentation application (`apps/frame`, command `frame`). Scaffold only. Frame is a working name.

## Layout

```
apps/bricks/          Bricks: the app, its own crates (apps/bricks/crates/), and its docs
apps/slate/           Slate
apps/frame/           Frame
crates/suite-core     app-independent primitives (AppKind)
crates/suite-ui       UI conventions shared by the apps
crates/suite-platform OS integration (Linux)
crates/suite-file     format-independent file infrastructure
docs/architecture/    how the suite is put together
docs/adr/             suite-wide architecture decisions
docs/specs/           one product spec per app
BACKLOG.md            Bricks' work queue, read by scripts/backlog.py
scripts/, packaging/  Bricks' tooling and packaging
sites/bricks/         Bricks' download page and installer (bricks.cronkite.cc)
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
cargo run -p bricks
cargo run -p slate
cargo run -p frame
```

Logs go to stderr.

## License

MIT OR Apache-2.0: see `LICENSE` and `LICENSE-APACHE`.
