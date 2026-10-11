# ADR 0004: Frame's Presentation Model and PPTX Reading

Status: Accepted

Date: 2026-10-11

## Context

Frame opens `.pptx` only (user decision). The first milestone, FR0, is a viewer with a slideshow; FR1 adds editing and saving. A PPTX slide rarely says everything about itself: placeholders take their position and text style from the slide layout, the layout from the slide master, and colours and fonts from the theme.

## Decision

- **A toolkit-free model**, `apps/frame/crates/presentation-model`: a `Presentation` has a slide size in EMU and slides in show order. A `Slide` has a background, shapes in z-order (the master's and layout's own shapes first, marked by layer, then the slide's), a hidden flag, and notes text. A `Shape` has a frame (EMU offset and size, rotation, horizontal and vertical flip), a geometry (a preset name), a fill, an outline, and optionally a text body, a picture, or a table; anything Frame can't draw is an `Unsupported` shape with its frame and a label (`Chart`, `SmartArt`, `Picture (EMF)`, `Video`). Text bodies hold paragraphs (alignment, level, bullet, spacing) of runs (text, font, size, bold, italic, underline, strikethrough, colour), plus anchoring, insets, and autofit's font scale.
- **Everything is resolved at read time**, in `apps/frame/crates/pptx` (`zip`, `quick-xml`): slide order from `presentation.xml`, placeholder frames and text styles through slide → layout → master (matching placeholders by `idx`, then `type`, then title or body family; a placeholder its layout has no match for, an orphan, inherits only its master text style, as LibreOffice reads it), the master's title, body, and other text styles, the presentation's default text style, theme colours (scheme colours with `lumMod`, `lumOff`, `tint`, `shade`, `alpha`) and theme fonts (`+mj-lt`, `+mn-lt`), and group transforms. The model holds final values, so the renderer and tests never walk inheritance.
- **The canvas is a custom GTK widget** drawing through `snapshot` with Pango and GSK, as ADR 0002 says, so thumbnails, the main view, and the slideshow share one renderer that scales EMU to pixels.
- **Reading runs off the UI thread** (`gio::spawn_blocking`).

## Consequences

- Rendering stays simple and testable: tests compare resolved values against LibreOffice's reading of the same files.
- Resolved values lose where they came from; FR1's editor needs to know whether a value is the slide's own or inherited, and FR1's save patches the original package (as Bricks ADR 0010) rather than writing the model out. FR1 adds that provenance to the model.
- Text layout uses Pango with the fonts the system has; decks made with fonts the system lacks wrap differently than in PowerPoint, as Slate's and Bricks' text does.
- **Revisit if:** FR1 needs inheritance at edit time in a way that read-time resolution can't express, or drawing a dense slide misses 16 ms per frame.
