# ADR 0002: Use GTK 4 for the Suite

Status: Accepted

Date: 2026-10-11

## Context

Bricks chose GTK 4 after spiking it against egui (Bricks ADR 0001, `apps/bricks/docs/decisions/0001-ui-toolkit.md`): both held 60 fps on a 1M-row virtual grid, and GTK gave system font fallback, bidi text, IME, portal file dialogs, and AT-SPI accessibility from the toolkit instead of from extra crates. Bricks has shipped on it since v0.1 (gtk4-rs 0.11, GTK 4.14 or newer, so Ubuntu 24.04 is the oldest supported desktop).

Slate and Frame have no toolkit yet. The plan after ADR 0001 was a comparative prototype covering all three apps' needs before choosing one for the suite. The user decided GTK 4 is fine for all three without that prototype.

## Decision

- Bricks, Slate, and Frame all use GTK 4 through gtk4-rs, with the same minimum: GTK 4.14 (`features = ["v4_14"]`).
- The `gtk` dependency is declared once in `[workspace.dependencies]` so every app builds against the same gtk4-rs version.
- Toolkit code stays at the top of the stack. Apps may depend on `gtk`. `suite-ui` (shared widgets, menus, dialogs, theme) and `suite-platform` (clipboard, file dialogs, drag and drop through GDK) may depend on it once they hold code that needs it. `suite-core`, `suite-file`, and app engine crates (such as Bricks' `csv-engine`, `data-model`, `formula`, `xlsx`, `grid`) never do.
- Custom content (Bricks' grid, Slate's pages, Frame's slides) is drawn the way Bricks draws its grid: a custom widget's `snapshot` with Pango for text and GSK for rendering.

## Consequences

Benefits:

- One toolkit across the suite: shared dialogs, menus, theme, shortcuts, and window chrome can move into `suite-ui` once two apps need them.
- Every app gets IME, accessibility, portal file dialogs, system theme, and font fallback from the toolkit, as Bricks does.
- Bricks needs no migration, and what it learned (custom snapshot drawing, Pango layout caching, headless check scripts under sway) carries over.

Risks:

- No prototype tested Slate's or Frame's hardest parts. GTK has no paginated document layout engine (`GtkTextView` isn't one), so Slate's page layout, line breaking across pages, and print/PDF output are custom work on Pango. Frame's slide canvas, shape editing, and full-screen playback are custom drawing too.
- The whole suite depends on GTK's release cycle and on gtk4-rs keeping up with it.
- Builds need GTK 4.14+ dev files for every app, not only Bricks.

Mitigations:

- The first Slate and Frame stories that draw content should measure it the way DEC-1 did (frame time, input latency on long documents and dense slides) and record the numbers in an ADR.
- Engine crates stay toolkit-free, so layout and models can be tested headless and a renderer swap stays confined to the app and `suite-ui`.
