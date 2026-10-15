# Etch

Fast, lightweight Linux-native raster image editor (user decision). Command `etch`. Etch sits where MacPaint, Microsoft Paint, and the simple end of Paint.NET sit, not beside GIMP, Photoshop, or Krita: "I just need to edit this image." Design: [ADR 0010](../adr/0010-etch-image-model.md) (image model, history, canvas).

## Principles

- **Simple.** Someone who launches Etch understands it at once: a menu bar, a tool column, the canvas, a colour row, and a status bar. No docks, no panel collections, plain words over graphics jargon.
- **Fast.** It starts quickly, opens ordinary images quickly, and drawing, zooming, panning, and changing tools respond at once. Interaction latency matters more than huge professional files: images up to 8K (7680×4320) stay practical.
- **Familiar.** The tools of a Paint program, where Paint users expect them: pencil, brush, eraser, fill, colour picker, line, rectangle, ellipse, text, selection, crop, zoom.
- **Native Linux.** GTK 4 like the rest of the suite (ADR 0002); developed on Arch, released for Arch, Ubuntu 24.04, and Fedora.

## Layout

```text
┌──────────────────────────────────────────────┐
│ File  Edit  View  Image  Tools               │
├───────┬──────────────────────────────────────┤
│ Tools │               Canvas                 │
├───────┴──────────────────────────────────────┤
│ Colors                       100%   1024×768 │
└──────────────────────────────────────────────┘
```

The canvas takes the window; permanent UI stays small. Menus appear as their commands arrive: there is no empty Image or Tools menu.

## Image model

One RGBA8 raster per document (straight alpha), whatever the file format: JPEG's lack of alpha never shapes the model; conversion happens when exporting. No layers in V0.1, but nothing in the document rules them out (ADR 0010). No native `.etch` format until Etch keeps something PNG can't (layers, editable text or shapes); PNG, JPEG, and BMP files are the documents.

## Files

- New, Open, Save, Save As, Export, Close, and recent files. PNG, JPEG, and BMP; WebP if the codec comes for free. New images save as PNG by default.
- Installing Etch makes it the default app for PNG, JPEG, and BMP, so double-clicking an image opens it in Etch (user decision); uninstalling gives the types back.
- **New Image** asks for width and height in pixels (remembered from the last new image) and a background: white, transparent, or the current background colour. Presets (1920×1080, 1280×720, 1024×1024) may come later.
- Unsaved changes are tracked; closing an edited image asks Save, Discard, or Cancel. Work is never silently dropped. Saving is atomic through `suite-file`; autosave and crash recovery come in ET3.

## Canvas

Zoom in, zoom out, fit to window, actual size, pan, the image centred when smaller than the window, a visible image boundary, and a checkerboard behind transparency. Zoom runs from 10% to 3200% and keeps the point under the pointer (or the window's centre, from the keyboard) fixed. At 100% and above pixels show as sharp squares (nearest neighbour); below, the image is smoothed.

## Tools

- **Pencil:** hard-edged, one colour, a size.
- **Brush:** freehand with a size and a hard or soft edge; not a brush engine.
- **Eraser:** erases to transparency (the model is always RGBA).
- **Fill:** floods the contiguous region of the clicked colour with the foreground; exact match first, tolerance later.
- **Colour picker:** a click sets the foreground colour from the image.
- **Line, Rectangle, Ellipse:** line width and colour; rectangles and ellipses outlined, filled, or both, previewed while dragged.
- **Text:** text, font family, size, colour, bold, italic; rasterized when committed. Editable text objects may come later; nothing in V0.1 depends on them.
- **Selection:** rectangular; drag to select, move the selected pixels, cut, copy, paste, delete, select all, deselect.

## Colours

A foreground and a background colour, a compact palette of familiar colours, a custom colour chooser, and hex entry. No colour management.

## Selection, clipboard, and image commands

- Pasted pixels float, movable, until committed. Copy puts the selection on the Linux clipboard as an image other apps can paste; paste takes images (screenshots) from other apps.
- **Crop to Selection** (Image menu): the canvas becomes the selection's bounds.
- **Resize Image:** width, height, keep aspect ratio; nearest neighbour or smooth.
- **Resize Canvas:** width, height, and an anchor of nine (↖ ↑ ↗ ← ● → ↙ ↓ ↘); new area transparent or the background colour.
- **Rotate** 90° either way and 180°; **Flip** horizontally and vertically. Arbitrary angles later.

## Undo

Every change undoes and redoes: strokes, erasing, fills, shapes, text, crop, resize, moving a selection, paste, delete, rotate, flip. History stores each step's operation and the pixels it replaced, never a copy of the whole image per stroke (ADR 0010).

## Shortcuts

| Keys | Command |
| --- | --- |
| Ctrl+N, Ctrl+O | New, Open |
| Ctrl+S, Ctrl+Shift+S, Ctrl+Shift+E | Save, Save As, Export |
| Ctrl+Z; Ctrl+Shift+Z or Ctrl+Y | Undo; Redo |
| Ctrl+X, Ctrl+C, Ctrl+V | Cut, Copy, Paste |
| Ctrl+A, Escape, Delete | Select All, Deselect, Delete selection |
| Ctrl++, Ctrl+- | Zoom In, Zoom Out |
| Ctrl+0 | Actual Size (100%) |
| Ctrl+9 | Fit to Window |
| P, B, E, F, I, T, R, L, S | Pencil, Brush, Eraser, Fill, Colour picker, Text, Rectangle, Line, Selection (Ellipse: O) |
| X | Swap the foreground and background colours |

Tool keys and X work only while no text is being typed (the size field included).

## Status bar

Pointer position in image pixels, the selection's size, zoom, and the image size: `x: 342  y: 191      Selection: 320×180      100%      1920×1080`.

## Milestones

The phases of the product plan, one milestone each (`BACKLOG.md`, chain `etch`):

- **ET0, canvas prototype:** window, canvas, New Image, zoom, pan, pencil, eraser, colours, undo and redo; drawing feels immediate and navigation smooth. Desktop entry and installer, so a release can ship.
- **ET1, a Paint replacement:** opening and saving PNG, JPEG, and BMP, brush, fill, colour picker, line, rectangle, ellipse, text.
- **ET2, image manipulation:** rectangular selection and moving it, cut, copy, and paste through the clipboard, crop, resize image, resize canvas, rotate, flip. ET2's gate is V0.1: a user can create or open an image, draw and erase, pick colours, draw shapes, add text, select and move regions, crop, resize, rotate or flip, copy and paste, undo, and save as a common format.
- **ET3, desktop polish:** drag and drop, recent files, a better colour chooser, every tool key, crash recovery, clearer errors.

## ET0: canvas prototype

Built (ETCH-2 to ETCH-5):

- **Window.** `etch` opens a window with a blank white 1024×768 image, titled Untitled ("Untitled 2" beside it, and so on), with a menu bar (File: New Image…, Close, Quit; Edit: Undo, Redo; View: Zoom In, Zoom Out, Actual Size, Fit to Window), the canvas, and a status bar (pointer position in image pixels, zoom, image size). Each image has its own window.
- **New Image.** Ctrl+N asks for a width and height (1 to 16384 pixels each; the last ones used, 1024×768 at first) and opens the white image in a new window.
- **Canvas.** The image sits centred on the window's backdrop with a one-pixel boundary, at 100%, or fitted with a margin when larger than the window. Ctrl+wheel zooms a step a notch around the pointer (a touchpad smoothly), Ctrl++ and Ctrl+- around the window's centre, through 10%, 12.5%, 16.7%, 25%, 33.3%, 50%, 66.7%, 100%, 150%, 200%, 300%, 400%, 600%, 800%, 1200%, 1600%, 2400%, 3200%; a zoom between steps (after Fit) goes to the next one. The wheel pans 48 screen pixels a notch (Shift+wheel sideways), and dragging with the middle button or with Space held moves the image with the pointer. The image can't be panned out of sight (16 pixels of it stay in); smaller than the window, it stays centred. Resizing the window keeps the point at its centre.
- **Drawing stays immediate.** The canvas redraws only the 256-pixel tiles a stroke changed. `etch --bench-stroke FRAMES` draws a stroke a segment a frame on a 7680×4320 image at 100% and prints its frame times: on a Ryzen 9 5900 under Hyprland, 1.2 ms p99 a frame (target 8 ms).
- **Tools.** A column at the window's left holds a button a tool, each with its icon and its name and key as the tooltip; one is on at a time, Pencil at first. A tool's key picks it too: P the pencil, E the eraser.
- **Colours and sizes.** Below the canvas, left of the status: the foreground colour's square over the background's (their hex values in its tooltip), the palette of Paint's 20 colours (a click makes one the foreground, a right-click the background; X swaps the two), and the current tool's Size, 1 to 100 pixels. Each tool keeps its own size: the pencil starts at 1, the eraser at 9. A tip a few screen pixels across or more shows as a ring around the pointer, where a drag would draw.
- **Pencil.** Dragging draws a line of the foreground colour at the pencil's size through every pixel the pointer crossed (a click draws one dot); each drag is one undo step.
- **Eraser.** Dragging erases a line at the eraser's size to transparency, whatever the image was; the canvas's checkerboard shows through. Each drag is one undo step.
- **Undo.** Ctrl+Z undoes the last stroke, Ctrl+Shift+Z and Ctrl+Y redo it; a new stroke after undoing drops what was undone.
- **Install.** A per-user curl installer, as the other apps have (`sites/etch/install.sh`: `curl -fsSL https://etch.cronkite.cc/install.sh | sh`, `| sh -s -- --uninstall`), with no sudo. It checks the release against its sha256 and puts `etch` in `~/.local/bin` and its menu entry and icon (`dev.cronkite.Etch`, the app ID) in `~/.local/share`. Since ETCH-6 it also makes Etch the default app for PNG, JPEG, and BMP (see ET1). GTK 4 comes from the system; without it, the installer says how to get it. The download page is `sites/etch/` (etch.cronkite.cc), and cronkite.cc lists Etch as an early preview.

## ET1: a Paint replacement

Built so far (ETCH-6 to ETCH-11):

- **Opening.** `etch FILE...` opens each image in a window of its own, titled with the file's name; File → Open… (Ctrl+O) picks one through the desktop's file dialog, filtered to images (PNG, JPEG, BMP; All files is the other filter). Open always opens a new window, as in Slate and Frame: an Untitled window stays as it is. A file already open brings its window forward. The file is read and decoded off the UI thread while its window says "Opening…", so other windows stay usable; then the image shows at 100%, or fitted when it's larger than the window.
- **Formats.** PNG, JPEG, and BMP, told by the file's content, not its name: a GIF named `.png` is a GIF. Images decode to the one RGBA8 raster. PNG keeps its alpha, fully transparent pixels included (their colour too). JPEG has none, and every BMP opens opaque: the fourth byte of a 32-bit BMP is unused in most files, so Etch reads it as Paint does. Images over 16384 pixels a side are refused before their pixels are decoded.
- **When a file can't be opened,** an alert names it and says why, and its window closes: "Cannot open “photo.png”" over "It's a damaged or incomplete PNG image (the file ends too soon).", "It's a GIF image. Etch opens PNG, JPEG, and BMP images.", "It isn't an image Etch can open: PNG, JPEG, or BMP.", "The file is empty.", "The file doesn't exist.", "You don't have permission to read it.", or "It's 20000×100 pixels; Etch opens images up to 16384 pixels a side." A JPEG missing its end (cut short) counts as damaged rather than opening grey at the bottom.
- **Fast.** An 8K PNG (7680×4320) shows within a second: `etch --bench-open FILE` prints the time from start to the first frame that drew the image.
- **The default app for images.** The installer makes Etch the default for PNG, JPEG, and BMP in `~/.config/mimeapps.list` (user decision), replacing another app's default for them, so double-clicking an image opens it in Etch; every other type's default stays. Uninstalling drops Etch's defaults (the types go back to whatever the desktop picks).
- **Saving.** File → Save (Ctrl+S) writes the image over its file, in the format the file was opened in; an image never saved asks where first, as Save As does, offering Untitled.png (Untitled 2.png, and so on) in the Pictures folder. File → Save As… (Ctrl+Shift+S) asks for a file through the desktop's file dialog, filtered to PNG, JPEG, or BMP images (the image's own format first); the name's extension picks the format (.png; .jpg or .jpeg; .bmp, in any case), and the file becomes the window's: its title, what Save writes next, what opening it again brings forward. A name without one of those extensions alerts "Cannot save “drawing.gif”" over "Etch saves PNG (.png), JPEG (.jpg or .jpeg), and BMP (.bmp) images: end the name with one of them."
- **Export.** File → Export… (Ctrl+Shift+E) writes a copy the same way, in the format its extension picks, but the window keeps its own file, format, and unsaved changes: a JPEG for sharing while the PNG stays the document.
- **Formats written.** PNG keeps every pixel exactly, alpha included (and the colour of fully transparent pixels). JPEG (quality 90) and BMP (24-bit) can't be transparent, so their pixels are composited onto white; saving or exporting an image with any transparent or see-through pixel as either first asks "Save “photo.jpg” without transparency?" ("JPEG images can't be transparent: the image's transparent and see-through pixels become white in the file."), Cancel or Save. The window keeps its transparency after such a save.
- **Saving stays out of the way.** The image is copied when the save begins (18 ms for 8K on a Ryzen 9 5900), then encoded and written off the UI thread, so drawing goes on meanwhile; the file holds the image as it was when the save began. A save asked for while another runs waits for it. Writing is atomic through `suite-file`: a temp file beside the file, read back and compared, synced, then renamed over it, so a save that fails (a full disk, a folder without write permission) leaves the old file whole and alerts "Cannot save “photo.png”" with the reason ("You don't have permission to write there.").
- **Unsaved changes.** The window's title starts with "• " while the image differs from its file (or, never saved, from the blank it started as). It's the history that counts: undoing back to the image saved clears the mark, redoing sets it again. Closing such a window (its close button, File → Close, Ctrl+W) or quitting (Ctrl+Q asks in each window) asks "Save changes to “photo.png” before closing?" with Cancel, Discard, and Save: Cancel keeps the window, Discard closes it leaving the file as it was, Save saves (asking where for a new image) and then closes; a save that fails or is cancelled keeps the window open.
- **Brush.** The tool column's Brush (B) draws the foreground colour in a round tip, 8 pixels at first (its own size, as each tool keeps), with a hard or a soft edge: Hard and Soft show beside the size while the brush is the tool, Soft at first. A hard edge is solid to the rim; a soft one fades smoothly from the full colour at the centre to nothing at the rim. The brush blends over what's there (a see-through colour lets the image show through) rather than replacing it as the pencil does. Within one drag the paint never builds up: going back and forth slowly over the same pixels looks the same as one quick pass, so a slow soft stroke doesn't turn solid; a second drag over the first does blend over it. Each drag is one undo step.
- **Fill.** The tool column's Fill (F) fills the region the clicked pixel belongs to with the foreground colour, a right-click with the background (as in Paint): every pixel of exactly the clicked colour reachable from it through pixels' edges (not their corners), so a line one pixel wide, of any other colour however close, stops it. Fully transparent pixels count as one colour whatever colour they hide, so a transparent area fills whole. The fill replaces pixels rather than blending, and has no size, so the size hides. Each fill is one undo step, and history keeps only the 64-pixel tiles it changed. Filling all of an 8K image takes about 80 ms on a Ryzen 9 5900 (under 200 ms at worst), so the fill lands at once rather than in the background.
- **Colour picker.** The tool column's Colour Picker (I) makes the colour of the clicked pixel the foreground, a right-click the background, alpha included: a see-through pixel gives a see-through colour, and a fully transparent one a fully transparent colour. Then it goes back to the tool used before it, as Paint's does, so picking a colour and painting with it is I, a click, and a drag. Picking changes no pixel and no history. It has no size. The colour row shows a see-through colour over a checkerboard, and its tooltip and accessible description give it as `#RRGGBBAA` (`#RRGGBB` when opaque).
- **Line, Rectangle, and Ellipse.** The tool column's Line (L), Rectangle (R), and Ellipse (O) draw shapes by dragging: from the pixel pressed to the pixel under the pointer, a line between them, or the rectangle or ellipse filling the box they span. While the drag goes on the shape shows on the canvas over the image, exactly as it will land, but the image isn't touched (the title stays unmarked); the release draws it as one undo step, and Escape before the release drops it. Shift keeps a line to the nearest 45° and a box square, so a circle. Each has its own size, the outline's width (1 at first). Shapes are hard-edged like the pencil, not smoothed: a line is the pencil's line at that width; a rectangle's outline runs inside its box with square corners; an ellipse touches its box's four sides, its outline inside it too. Rectangles and ellipses are Outline, Filled, or Both, chosen beside the size while either is the tool (one choice for the two, as in Paint), Outline at first: the outline is the foreground colour, Both fills inside it with the background, and Filled is the whole shape in the foreground with no outline (as in Paint and Paint.NET). A see-through colour lays one coat over the image, as the brush does. History keeps only the 64-pixel tiles a shape changed, so an outline round a large image keeps its edge's tiles, not its middle.

## Non-goals for V0.1

Layers, masks, vector illustration, advanced brushes, PSD, CMYK, RAW, colour management, adjustment layers, advanced filters, animation, professional typography, plugins, AI generation, collaboration. Later candidates, once the basics are strong: layers, editable text and shapes, blur, sharpen, greyscale, brightness and contrast, hue and saturation, selection shapes, magic wand, arbitrary rotation, guides, rulers, a pixel grid, SVG import as raster, and `.etch`.
