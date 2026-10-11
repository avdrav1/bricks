# ADR 0001: Use a Rust Monorepo

Status: Accepted

Date: 2026-10-11

Suite-wide decisions live in `docs/adr/`. Bricks' own decisions keep their numbering in `apps/bricks/docs/decisions/`.

## Context

The suite has three applications: Bricks (spreadsheet, starting as a fast CSV editor, already shipping), Slate (word processor), and Frame (presentation app, working name). They will need some of the same infrastructure: application identity, OS integration, safe file handling, UI conventions. Their product models (a sheet of rows, a flowing document, a deck of slides) are different.

Bricks started alone in this repository as a Cargo workspace of an app and eight crates. Adding Slate and Frame means choosing between one repository for everything and one repository per application with shared crates published or vendored between them.

## Decision

- One repository holds all three applications and their shared code.
- One Cargo workspace builds all of it, with one `Cargo.lock` and one toolchain (`rust-toolchain.toml`).
- Each application is its own binary crate under `apps/` and runs alone: `cargo run -p bricks`, `-p slate`, `-p frame`.
- Shared code lives in small `suite-*` crates under `crates/` (`suite-core`, `suite-ui`, `suite-platform`, `suite-file`). An application's own crates live under that application (`apps/bricks/crates/`).

## Consequences

Benefits:

- Refactoring across applications is one change, checked by one `cargo build --workspace`.
- Shared infrastructure is written once and used directly, by path.
- Tooling is consistent: one toolchain, one fmt/clippy/test setup, one CI.
- A commit can change several apps and a shared crate atomically.
- No internal crate publishing, version pinning, or cross-repository dependency management.

Risks:

- Workspace build and test times grow with every app.
- Coupling is easy by accident: an app can reach into code it shouldn't.
- Shared crates can become dumping grounds for anything two places happen to use.

Mitigations:

- Strict crate boundaries: `suite-core` depends on nothing in the workspace; other shared crates depend only on `suite-core`; shared crates never depend on an application; an app's crates never depend on another app's (docs/architecture/overview.md).
- Independent application binaries, built and tested with `-p <app>` when only one app changes.
- Shared crates stay small, each with one stated purpose.
- Abstractions are extracted only when a second application needs them, not in advance.
