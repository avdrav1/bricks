# Architecture overview

Cronkite is a Linux-native office suite written in Rust: Brick, Slate, Frame, and Etch.

```text
Brick — spreadsheet / CSV editor
Slate  — pageless word processor: .txt, .md, .docx (docs/specs/slate.md, ADRs 0003, 0005)
Frame  — presentation app (working name), easier than PowerPoint: .pptx (docs/specs/frame.md, ADRs 0004, 0006)
Etch   — raster image editor, closer to Paint than Photoshop: PNG, JPEG, BMP (docs/specs/etch.md, ADR 0010)
```

## Guiding principle

> Share infrastructure, not product models prematurely.

Code becomes shared when at least two applications genuinely need the same abstraction. Until then it stays in the app that needs it. There is no universal document model.

## Structure

- **One monorepo, one Cargo workspace** (`Cargo.toml` at the root, one `Cargo.lock`). See [ADR 0001](../adr/0001-use-rust-monorepo.md).
- **Independently runnable applications** in `apps/`: `brick`, `slate`, `frame`, `etch`. Each is its own binary crate and builds and runs alone (`cargo run -p <app>`).
- **App-specific crates** live under their app: Brick's engine is in `apps/brick/crates/` (`csv-engine`, `formula`, `data-model`, `xlsx`, `grid`, `commands`, `file-format`, `jobs`), Etch's image model in `apps/etch/crates/image-model`. Their names describe what they do, not that they are shared: none of Brick's is used by Slate, Frame, or Etch.
- **Small shared infrastructure crates** in `crates/`:

| Crate | Purpose | Today |
| --- | --- | --- |
| `suite-core` | application-independent primitives: app identity, later common IDs, error and value types | `AppKind` with `display_name()` |
| `suite-ui` | UI conventions: later menus, command palette, toolbars, dialogs, themes, window chrome, shortcuts | `startup_message()` |
| `suite-platform` | OS integration, Linux and macOS (ADR 0011): later file dialogs, clipboard, desktop integration, drag and drop, Wayland/X11, system theme | `current()` platform check (`Linux`, `MacOs`) |
| `suite-file` | format-independent file infrastructure: later atomic writes, save/save-as, recent files, temp files, autosave and recovery | `document_name()` |

Format parsers (CSV, XLSX, DOCX, PPTX) never go in `suite-file`; they belong to the app that reads the format.

## Dependency direction

```mermaid
graph TD
    brick --> suite-ui
    brick --> suite-file
    brick --> suite-core
    brick --> brick-crates["apps/brick/crates/*"]
    slate --> suite-ui
    slate --> suite-platform
    slate --> suite-file
    slate --> suite-core
    slate --> slate-crates["apps/slate/crates/*"]
    frame --> suite-ui
    frame --> suite-platform
    frame --> suite-core
    frame --> frame-crates["apps/frame/crates/*"]
    etch --> suite-ui
    etch --> suite-platform
    etch --> suite-core
    etch --> etch-crates["apps/etch/crates/*"]
    suite-ui --> suite-core
```

Rules:

- `suite-core` depends on no other workspace crate.
- `suite-ui`, `suite-platform`, and `suite-file` may depend on `suite-core` and nothing else in the workspace. Today only `suite-ui` needs it.
- No shared crate depends on an application or on an application's crates.
- An app's crates never depend on another app's crates. If two apps need the same code, it moves to a shared crate first.
- Apps depend only on the shared crates they use.
- Only apps, `suite-ui`, and `suite-platform` may depend on the GUI toolkit (see below).

## Errors and logging

- Libraries return errors for recoverable conditions instead of panicking. Error types are plain enums or structs implementing `std::error::Error` (for example `suite_platform::Unsupported`, Brick's `SaveError`); no error framework.
- Binaries report startup errors on stderr as `<app>: <message>` and exit non-zero.
- Every app logs through [`tracing`](https://docs.rs/tracing) with a `tracing-subscriber` `fmt` subscriber writing to stderr (`INFO brick: application starting`). There is no logging wrapper. Stdout stays free for program output: Brick's benchmark modes report there.

## GUI toolkit

All four apps use GTK 4 through gtk4-rs, GTK 4.14 or newer ([ADR 0002](../adr/0002-gtk4-for-the-suite.md); Brick's original evaluation is `apps/brick/docs/decisions/0001-ui-toolkit.md`). `gtk` is declared once in `[workspace.dependencies]`. Apps may depend on it; `suite-ui` and `suite-platform` may once they hold code that needs it; `suite-core`, `suite-file`, and app engine crates never do. All four use it today: Brick, Slate (its SL0 viewer), Frame (its FR0 viewer), and Etch (its canvas; `image-model` stays toolkit-free).

## Versions and editions

- Brick and its crates share the workspace version (`[workspace.package] version`), which is Brick's release version; `scripts/ship.sh bump` changes it. Slate, Frame, Etch, and the `suite-*` crates set their own versions.
- The workspace uses edition 2021. Moving to edition 2024 is a separate change.

## Future crates (speculative)

These are likely candidates, listed so the shape is visible. None exists yet, and not every one will become its own crate; each is created only when implementation work shows the need, and shared only when two apps need it.

| Area | Possible crates |
| --- | --- |
| Suite-wide | `text-engine`, `rendering`, `undo`, `commands`, `pdf-export`, `charts`, `tables` |
| Brick | `brick-model`, `csv-engine`, `xlsx` (the last two exist today as Brick-only crates) |
| Slate | `document-model` and `docx` exist (`apps/slate/crates/`, SLATE-1 to SLATE-3) |
| Frame | `presentation-model`, `pptx`, `deck-edit` (exist today as Frame-only crates in `apps/frame/crates/`, ADRs 0004 and 0006) |
| Etch | `image-model` exists (`apps/etch/crates/`, ADR 0010); codecs and tools split out only once their boundaries are real |

Candidates already overlap with the apps' own crates (`commands` holds Brick's undo stack, `deck-edit` Frame's). Extracting one for the suite means generalizing it with a second app as its user, not moving it ahead of time.
