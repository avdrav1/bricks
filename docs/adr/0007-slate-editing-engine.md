# ADR 0007: Slate's Editing Engine for the Pageless Flow

Status: Accepted

Date: 2026-10-11

## Context

SL1 turns Slate's viewer into an editor. ADR 0005 made Slate pageless, so the engine lays out one continuous column at a user-chosen width (Narrow 480, Medium 624, Wide 816, Full = window less 48 px) and never paginates. ADR 0003 left the choice between two engines to this story, measured on a 150,000-word document (about 300 pages) at Full width:

- **GtkTextView:** GTK's text widget, which the SL0 viewer already uses read-only.
- **Custom Pango flow:** Slate's own widget. It draws through `snapshot` with one `pango::Layout` per paragraph, as Bricks draws its grid (ADR 0002).

## Options

Both spikes show the same generated document: 150,000 words in 1,740 paragraphs (1,020,131 characters), with headings, two bullet levels and bold and italic runs. The looks are identical: Sans 11 pt, headings at 2.0/1.5/1.25x, 12 px between paragraphs, 28 px per bullet level, WordChar wrapping, and the column centred. Both read typed text through a GtkIMContext. Each reports a frame only once its snapshot shows the change.

| Option | Measured (1600x1000 window, Full = 1552 px) | Cost / risk |
| --- | --- | --- |
| GtkTextView | Typing p50 11.2 ms, p99 19.7 ms. Re-wrap: the visible text in 15.8 ms (p50), the whole document in 489 ms (p50) and 578 ms (max). Open: 309 ms to the first frame, 865 ms to the whole layout. Scroll frame CPU p50 6.5 ms. Peak RSS 233 MiB. | Comes with IME with preedit, AT-SPI text (TextBox role), selection, clipboard, bidi, drag and drop, and spell-check hooks. The `TextBuffer` is a second copy of the document: its offsets include list markers and U+FFFC anchors, so a two-way position map to the model is needed for every edit, undo and patched save, and its own undo must stay off. A table is a child widget with its own buffers per cell, so **no selection or copy can span body text and a table** (confirmed: select-all over a table yields `before\n\nafter`). 50 3x3 tables add 128 ms to open and 1–2 ms per scroll frame. Changing margins from inside size-allocate trips a GTK assertion (`onscreen_validated`), so width changes must be deferred to an idle. |
| Custom Pango flow | Typing p50 4.8 ms, p99 18.7 ms. Re-wrap: the visible text in 7.3 ms (p50), the whole document in 286 ms (p50) and 305 ms (max). Open: 234 ms to the first frame, 623 ms to the whole layout. Scroll frame CPU p50 8.0 ms. Peak RSS 268 MiB, all 1,740 layouts cached; an LRU would cap that. | The document model is the only copy: an edit relays out one paragraph and updates a Fenwick height index. Estimated heights (within 1.3% of exact) fill in until a 4 ms-sliced idle pass finishes, and the scroll position is anchored to a paragraph so text never jumps. AT-SPI text works on GTK 4.14 (`AccessibleText`, verified: contents, caret, word, line and paragraph at an offset, insert and delete events). Extents need GTK 4.16 and setting the caret from assistive tech needs 4.22, and the suite's minimum is 4.14 (ADR 0002). Everything GtkTextView provides has to be built: selection, clipboard, IME preedit drawing, bidi details, find, spell checking, undo commands, and tables and pictures drawn in the flow. The spike estimates 14–19 weeks, half of it for tables and pictures. |

At 3800x2080 (Full = 3752 px):

| Metric | GtkTextView | Custom Pango flow |
| --- | --- | --- |
| Typing p50 / p99 (ms) | 16.3 / 23.1 | 13.6 / 25.9 |
| Re-wrap: visible text p50 / max (ms) | 30.5 / 59.6 | 17.0 / 46.4 |
| Re-wrap: whole document p50 / max (ms) | 913 / 1008 | 341 / 367 |
| Open: whole layout (ms) | 1358 | 774 |
| Scroll frame CPU p50 / p99 (ms) | 21.4 / 26.2 | 26.3 / 30.7 |

These numbers are **software rendered** (see Spike). Typing and frame CPU include llvmpipe's rasterizing, which hides most of the engines' own difference at 3800x2080. Re-wrap and open are layout-bound, so they compare the engines fairly.

## Recommendation

**Custom Pango flow.**

- **Performance.** Both are fast enough to type in on this machine. The flow re-wraps the whole document in 0.3 s instead of 0.5–1.0 s, shows the visible text sooner, and its time stays flat as the width grows.
- **The deciding difference is the model.** With GtkTextView every SL1 feature works against a second copy of the document, with a position map that has to absorb list markers and object anchors. Tables could never be selected or copied together with the text around them, which a word processor that edits `.docx` and Markdown tables can't accept. With the flow, the document model is drawn directly, edits and undo are model commands (the suite's rule), and SL1's patched DOCX save maps model positions without translation.
- **The cost is real.** The flow means 14–19 weeks of editor work, a GTK feature bump for complete accessibility, and owning text features GTK would otherwise give us.
- **If speed to a first editable SL1 matters more than tables,** GtkTextView is the boring choice. Its ceiling is the table limit and the second copy of the document, and leaving it later means rewriting the editor.

## Decision

Slate's editor is the custom Pango flow (user decision, 2026-10-11, on the software-rendered numbers above).

- **`apps/slate/crates/flow`** (new, toolkit-free): the document's layout state. It holds, per block, the cached height at a width; a Fenwick height index for y ↔ block; estimated heights until a time-sliced pass measures every block; scroll anchoring to a block and an offset; hit-testing; and caret and selection positions in model coordinates. Text shaping goes behind a small trait, so the crate depends on neither GTK nor Pango and its tests run without a display.
- **The widget in `apps/slate`** implements that trait with `pango::Layout`. It draws through `snapshot`, takes input through a `GtkIMMulticontext`, and implements `gtk::AccessibleText`.
- **Edits are document-model commands** with undo, as in Bricks. The flow never keeps a second copy of the text.
- **Layout runs on the main thread in 4 ms idle slices** below redraw priority, as in the spike. Per-thread Pango contexts are a later optimization, adopted only if measurements ask for it.

## Consequences

- SL1's engine work is split into backlog stories:
  - SLATE-11: the flow shows documents, tables and pictures included, and replaces the SL0 GtkTextView viewer, which is then removed.
  - SLATE-6: caret, typing, and formatting as undoable commands.
  - SLATE-12: editing inside tables.
  - SLATE-13: IME composition.
  - SLATE-14: selection and clipboard.
  - SLATE-15: accessibility.
- Full accessibility needs gtk4-rs `v4_16` (extents) and later `v4_22` (caret set by assistive tech). Raising Slate's minimum above GTK 4.14 drops Ubuntu 24.04's stock GTK (ADR 0002). Until that's decided, Slate ships contents, caret, and text-at-offset on 4.14.
- **Revisit if:** the flow's selection and IME work run well past the estimate, or the numbers rerun on working GPU rendering reverse the re-wrap result.

## Spike

`spikes/slate-5/` contains:
- `common.rs`: the document generator, the widths, and the stdout protocol.
- `textview/`, `pango-flow/`: the two spikes.
- `measure.py`: drives Hyprland. It floats each window at a fixed size, injects letters with `send_shortcut`, and times from just before the socket write to the first frame showing the edit.
- `results-*.json`: the numbers above. `results-tables50-*` holds the 50-table GtkTextView run.

To rerun: `cargo build --release && python3 measure.py` in `spikes/slate-5`.

- **Hardware:** AMD Ryzen 9 5900 (12 cores), RTX 3080, 3840x2160 at 60 Hz, Hyprland 0.56.2, GTK 4.24.1, gtk4-rs 0.11.5. **GSK ran on llvmpipe**: the NVIDIA kernel module (615.71.09) and nvidia-utils (615.78.08) didn't match, so Vulkan failed and GL fell back to software. This isn't the reference box. Rerun after the driver is fixed (a reboot) before accepting this ADR.
- **Method:**
  - *Typing:* 100 letters spaced 80–150 ms apart, with the cursor mid-document.
  - *Scroll:* 900 frames at 37 px per frame.
  - *Re-wrap:* the width switches Narrow ↔ Full every 90 frames, 10 switches in all. Timing runs from the switch to the first frame showing the new width, and to the moment every paragraph has its exact height.
  - *GtkTextView's "whole layout":* the moment its validate idle (priority 125) has drained. This was checked: the scroll range didn't change after it in any of the 22 switches.
- **Accessibility check (Pango flow):** AT-SPI read role `text`, the character count (1,021,870), and the caret offset, plus the character, word, sentence, line and paragraph at the caret, each in 0.1–0.3 ms. Typing and Backspace produced matching text-changed events. One cost: GTK's bridge appears to build the whole 1M-character string for a character count, about 5–10 ms per call.
