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
| Ctrl+S, Ctrl+Shift+S | Save, Save As |
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

Built (ETCH-2 to ETCH-4):

- **Window.** `etch` opens a window with a blank white 1024×768 image, titled Untitled ("Untitled 2" beside it, and so on), with a menu bar (File: New Image…, Close, Quit; Edit: Undo, Redo; View: Zoom In, Zoom Out, Actual Size, Fit to Window), the canvas, and a status bar (pointer position in image pixels, zoom, image size). Each image has its own window.
- **New Image.** Ctrl+N asks for a width and height (1 to 16384 pixels each; the last ones used, 1024×768 at first) and opens the white image in a new window.
- **Canvas.** The image sits centred on the window's backdrop with a one-pixel boundary, at 100%, or fitted with a margin when larger than the window. Ctrl+wheel zooms a step a notch around the pointer (a touchpad smoothly), Ctrl++ and Ctrl+- around the window's centre, through 10%, 12.5%, 16.7%, 25%, 33.3%, 50%, 66.7%, 100%, 150%, 200%, 300%, 400%, 600%, 800%, 1200%, 1600%, 2400%, 3200%; a zoom between steps (after Fit) goes to the next one. The wheel pans 48 screen pixels a notch (Shift+wheel sideways), and dragging with the middle button or with Space held moves the image with the pointer. The image can't be panned out of sight (16 pixels of it stay in); smaller than the window, it stays centred. Resizing the window keeps the point at its centre.
- **Drawing stays immediate.** The canvas redraws only the 256-pixel tiles a stroke changed. `etch --bench-stroke FRAMES` draws a stroke a segment a frame on a 7680×4320 image at 100% and prints its frame times: on a Ryzen 9 5900 under Hyprland, 1.2 ms p99 a frame (target 8 ms).
- **Tools.** A column at the window's left holds a button a tool, each with its icon and its name and key as the tooltip; one is on at a time, Pencil at first. A tool's key picks it too: P the pencil, E the eraser.
- **Colours and sizes.** Below the canvas, left of the status: the foreground colour's square over the background's (their hex values in its tooltip), the palette of Paint's 20 colours (a click makes one the foreground, a right-click the background; X swaps the two), and the current tool's Size, 1 to 100 pixels. Each tool keeps its own size: the pencil starts at 1, the eraser at 9. A tip a few screen pixels across or more shows as a ring around the pointer, where a drag would draw.
- **Pencil.** Dragging draws a line of the foreground colour at the pencil's size through every pixel the pointer crossed (a click draws one dot); each drag is one undo step.
- **Eraser.** Dragging erases a line at the eraser's size to transparency, whatever the image was; the canvas's checkerboard shows through. Each drag is one undo step.
- **Undo.** Ctrl+Z undoes the last stroke, Ctrl+Shift+Z and Ctrl+Y redo it; a new stroke after undoing drops what was undone.

## Non-goals for V0.1

Layers, masks, vector illustration, advanced brushes, PSD, CMYK, RAW, colour management, adjustment layers, advanced filters, animation, professional typography, plugins, AI generation, collaboration. Later candidates, once the basics are strong: layers, editable text and shapes, blur, sharpen, greyscale, brightness and contrast, hue and saturation, selection shapes, magic wand, arbitrary rotation, guides, rulers, a pixel grid, SVG import as raster, and `.etch`.
