# 0001: UI toolkit

- Story: DEC-1
- Status: proposed
- Date: 2026-10-05

## Question

Which UI toolkit does `app` use to render the grid and host the native shell (menus, dialogs, IME, accessibility)?

## Options

GTK4 and egui were spiked; they are the two finalists named in the execution plan (SP-1). Slint and Iced were not spiked.

Both spikes draw the same custom virtual grid: 30 columns, 22 px rows, cells generated on demand, an f64 scroll offset, one clip per column, and a shaped-text cache that drops entries unused for one frame. That cache policy is egui's built-in galley cache, and the GTK spike reimplements it so both toolkits do the same work. Frame time and latency come from the 1600x1000 window, 1M-row runs. The other runs are in the Spike section.

| Option | Measured result | Cost / risk |
| --- | --- | --- |
| GTK4 (gtk4-rs 0.11, custom `Widget::snapshot` + Pango, GSK default renderer) | Holds 60 fps with 0 missed vsyncs in both scroll and jump modes. Frame CPU p50 1.9 ms (scroll) and 4.5 ms (jump). Input to frame p50 5.4 ms, p99 7.2 ms. Peak RSS 133–136 MiB. Text: system font fallback and bidi work (Arabic, Hebrew, CJK, Devanagari, emoji all render). | At 3800x2080, jumps that redraw every cell run at 30 fps (frame CPU p50 18.1 ms). 14.4 ms of that is Pango shaping in `snapshot`, and changing the GSK renderer does not help. Needs system GTK ≥ 4.14 and `libgtk-4-dev` in CI. Subclassing in gtk-rs takes more boilerplate. |
| egui (eframe 0.36, wgpu 30, custom painter) | Holds 60 fps (1 missed vsync out of 962 frames in scroll, 0 in jump). Frame CPU p50 1.1 ms (scroll) and 3.5 ms (jump). Input to frame p50 3.8 ms, p99 5.1 ms. Peak RSS 160 MiB. | Default fonts draw Arabic, Hebrew, and CJK as tofu because there is no system font fallback, so we would have to load fonts ourselves. Even with Noto fonts loaded there is no bidi: a two-word Hebrew cell shows its words in the wrong order (screenshot, plus epaint 0.36 `font.rs:829` "TODO: heed bidi characters"). Menus and widgets are egui-drawn, not native. At 3800x2080, jumps that redraw every cell miss 11% of vsyncs (frame CPU p50 12.7 ms). |

Not exercised in either spike: IME composition, the portal file dialog, screen readers, and theme following. In GTK these come from the toolkit (`GtkIMMulticontext`, `GtkFileDialog`, built-in AT-SPI with `GtkAccessibleText` from 4.14, GMenu/`PopoverMenuBar`). egui covers them with winit IME events, the `rfd` crate, and AccessKit (`accesskit_unix` is already in the spike's dependency tree). Their quality in practice is unverified.

## Spike

`spikes/dec-1/` holds `gtk4-grid/`, `egui-grid/`, the shared `common.rs` (data, viewport math, stdout protocol), the `measure.py` driver, and the `results-*.json` files.

- **Hardware:** AMD Ryzen 9 5900 (12 cores), 125 GB RAM, RTX 3080 (NVIDIA 615.71.09), 3840x2160 at 60 Hz, Hyprland 0.56.2, GTK 4.22.5, Rust 1.99. This is not the reference box in docs/BENCHMARKS.md. The desktop was in use during the runs, so there is some compositor noise.
- **Setup:** each window floats at a fixed size, is pinned so it stays visible, and runs with vsync on.
- **Frame time:** an in-app autopilot runs 1000 frames, and the first 0.5 s after placement are discarded.
  - *scroll* advances 37 px per frame, so new rows appear every frame.
  - *jump* moves to a random row every frame, which simulates scrollbar drag with nothing cacheable.
  - Frame CPU is the toolkit's own number: GTK from frame-clock before-paint to after-paint; egui from `Frame::info().cpu_usage`, which excludes vsync wait.
- **Input latency:** 80 Page_Down presses injected with Hyprland `send_shortcut` over the IPC socket, spaced 80–150 ms at random. The clock starts just before the socket write (CLOCK_MONOTONIC, shared with the apps). It stops at the first finished frame whose top row changed.
  - GTK's endpoint is after-paint, which comes after GSK render and swap.
  - egui's endpoint is a wgpu paint callback recorded last in the render pass, which comes before queue submit and present. This favors egui by its submit/present cost.
  - Neither number includes the compositor's wait for scanout, which adds up to one vblank for both.

| Run | Toolkit | Mode | Frame interval p50 / p99 (ms) | Missed vsync | Frame CPU p50 / p99 (ms) |
| --- | --- | --- | --- | --- | --- |
| 1M rows, 1600x1000 | gtk4 | scroll | 16.7 / 18.4 | 0/967 | 1.9 / 3.4 |
| 1M rows, 1600x1000 | gtk4 | jump | 16.7 / 18.9 | 0/965 | 4.5 / 6.6 |
| 1M rows, 1600x1000 | egui | scroll | 16.6 / 17.6 | 1/962 | 1.1 / 1.9 |
| 1M rows, 1600x1000 | egui | jump | 16.7 / 18.4 | 0/962 | 3.5 / 5.1 |
| 1M rows, 3800x2080 | gtk4 | scroll | 16.6 / 19.9 | 0/966 | 6.4 / 8.8 |
| 1M rows, 3800x2080 | gtk4 | jump | 33.2 / 38.0 | 918/981 | 18.1 / 22.9 |
| 1M rows, 3800x2080 | egui | scroll | 16.6 / 21.0 | 6/961 | 3.2 / 5.4 |
| 1M rows, 3800x2080 | egui | jump | 16.9 / 34.9 | 110/962 | 12.7 / 17.5 |
| 10M rows, 1600x1000 | gtk4 | scroll | 16.7 / 17.8 | 0/964 | 2.0 / 3.0 |
| 10M rows, 1600x1000 | gtk4 | jump | 16.7 / 19.6 | 4/965 | 4.3 / 6.8 |
| 10M rows, 1600x1000 | egui | scroll | 16.6 / 17.5 | 0/961 | 1.1 / 1.7 |
| 10M rows, 1600x1000 | egui | jump | 16.7 / 18.4 | 0/961 | 3.6 / 5.2 |

| Run | Toolkit | Input to frame p50 / p95 / p99 / max (ms) |
| --- | --- | --- |
| 1M rows, 1600x1000 | gtk4 | 5.4 / 6.7 / 7.2 / 7.2 |
| 1M rows, 1600x1000 | egui | 3.8 / 4.7 / 5.1 / 5.9 |
| 1M rows, 3800x2080 | gtk4 | 17.8 / 20.4 / 21.2 / 22.9 |
| 1M rows, 3800x2080 | egui | 12.1 / 14.2 / 14.9 / 15.2 |
| 10M rows, 1600x1000 | gtk4 | 6.2 / 9.1 / 10.4 / 15.3 |
| 10M rows, 1600x1000 | egui | 3.9 / 5.0 / 5.4 / 5.9 |

Other observations:

- The event reaches the app's key handler in 0.5–0.7 ms p50 for both.
- Cold start (spawn to first frame) is 260–346 ms for both. It includes GPU driver init.
- Row count does not change frame cost: 1M and 10M rows land within noise of each other.
- GTK renderer sweep at 3800x2080 in jump mode, before the text cache was added (frame CPU p50): default 19.5 ms, ngl 21.1 ms, gl 21.4 ms, vulkan 19.7 ms, cairo 33.6 ms. `snapshot` alone took about 16 ms in every case, so the cost is text shaping, not rendering.

To rerun (opens windows on the current Hyprland session):

```sh
cd spikes/dec-1
cargo build --release
python3 measure.py --rows 1000000 --win 1600x1000 --out results-1m-1600x1000.json
python3 measure.py --rows 1000000 --win 3800x2080 --out results-1m-3800x2080.json
python3 measure.py --rows 10000000 --win 1600x1000 --out results-10m-1600x1000.json
# Text probe: DEC1_UNICODE=1 fills column E with mixed scripts;
# add DEC1_UNICODE_FONTS=1 to load Noto fallbacks into egui.
DEC1_UNICODE=1 target/release/gtk4-grid
```

## Decision

Proposed: **GTK4 via gtk4-rs**, with the grid as a custom-drawn widget (`Widget::snapshot`). The `grid` crate stays toolkit-free and owns viewport math.

1. **Text correctness on arbitrary CSV content.** Pango, HarfBuzz, and fontconfig give font fallback and bidi out of the box. egui needs bundled fonts and still lays RTL text out in the wrong order. A spreadsheet that opens whatever file it is given cannot show tofu or reorder words in a cell.
2. **Native shell for a Linux-first product.** GTK provides real menus, the portal file dialog, IME, AT-SPI, and system theme. These are the SP-1 exit criteria the execution plan cares about, and egui would cover them with drawn or third-party substitutes.

Both toolkits meet the frame-time and latency bar at 1M and 10M rows in a normal-sized window. egui's 1.3–1.8x lower frame CPU is real, but it does not cross the 60 fps line except in the 4K full-redraw case, and neither toolkit passes that case.

## Consequences

- **Easy:** native dialogs, menus, IME, accessibility, and theming come from the toolkit. Text renders correctly for any script the system has fonts for.
- **Hard:** frames that redraw every cell are bound by Pango shaping at about 5 µs per fresh cell. At 3800x2080 with every cell new, GTK hits 30 fps. GRID-1/GRID-2 need a plan for fast scrollbar drags on large windows. Options: reuse per-row render nodes across frames, fill cells progressively within a frame budget, or a cheaper text path for ASCII-only cells. That is a GRID-1 implementation concern, not a blocker for this decision.
- **CI:** CI needs `libgtk-4-dev` (Ubuntu 24.04 ships 4.14, which matches the `v4_14` feature) and adds GTK to the AUR package's runtime dependencies.
- **Revisit if:** GRID-1 cannot hold 60 fps for scrolling at 2M rows on a maximized 4K window after node reuse, or gtk4-rs friction (subclassing, lifetimes in signal closures) measurably slows feature work in M1.
