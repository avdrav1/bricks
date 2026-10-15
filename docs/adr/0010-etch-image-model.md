# ADR 0010: Etch's Image Model, History, and Canvas

Status: Accepted

Date: 2026-10-14

## Context

Etch is the suite's raster image editor (docs/specs/etch.md): Paint, not Photoshop. V0.1 has one raster per document, no layers, and PNG, JPEG, and BMP as its files. Drawing must feel immediate on images up to 8K (7680×4320, 133 MB of RGBA), every change must undo, and history must not copy the whole image for each stroke. The suite's toolkit is GTK 4 (ADR 0002), and engine code stays toolkit-free.

## Decision

- **A toolkit-free crate, `apps/etch/crates/image-model`,** owns the pixels and everything that changes them: `Image` (width, height, RGBA8 rows, straight alpha, at most 16384 pixels a side), `Color`, pixel `Rect`s, the raster operations (pencil strokes first; brush, fill, shapes, crop, resize, rotate, and flip as they come), and `Document`, which holds the image and its history. The GTK app never writes pixels itself: tools call `Document` operations, which return the rectangle they changed. A `Document` holds one image today; layers, if they come, replace that field with a stack without changing how tools reach it.
- **RGBA8 straight alpha, always.** Files convert at the edges: JPEG and BMP read as opaque RGBA, and exporting to a format without alpha composites onto white then.
- **History stores operations and the pixels they replaced.** A step is the operation (a stroke's colour, width, and points; later a fill's seed and colour, a crop's rectangle) plus the image's 64-pixel tiles from before the operation touched them, saved the first time each tile is touched. Undo writes those tiles back; redo replays the operation, which is deterministic. A short stroke keeps a few tiles, not the image; operations that change the whole image and lose pixels (crop, resize, resize canvas) save the whole image once, which is what undo needs anyway (undo and redo swap it in, so neither recomputes), while exact ones (rotate, flip) save nothing: undo applies their inverse and redo applies them again (ETCH-20). A live stroke saves tiles as it goes, so drawing never waits on a snapshot.
- **The canvas is a custom GTK widget** (`snapshot`, GSK). The image is cut into 256-pixel tiles, each a `gdk::MemoryTexture` built the first time it's visible; an operation's changed rectangle drops only the tiles it overlaps, so a stroke rebuilds a tile or two per frame however large the image. Tiles draw with nearest-neighbour filtering at 100% and above (pixels as squares) and trilinear below. Zoom and pan are toolkit-free view math in the app (`view.rs`): a zoom keeps the image point under the pointer fixed, and panning stops before the image leaves the window.
- **Pencil lines are Bresenham's,** between the pixels the pointer reported, stamped with a disc for widths above one, so a one-pixel line is the classic Paint line with no gaps and no anti-aliasing.

## Consequences

- Tools and image commands are tested without a display: operations, undo, and redo are byte-for-byte checks in `image-model`.
- 8K images cost 133 MB for the pixels plus their tile textures while visible; history adds only touched tiles. Images above 16384 pixels a side are refused rather than slowing everything down.
- Redo depends on operations being deterministic; an operation that can't be replayed (one reading the clipboard, say) stores its result as tiles too.
- **Revisit if:** layers, editable text, or shapes arrive (the document grows a stack and a native format, spec), or a stroke on an 8K image misses a frame.
