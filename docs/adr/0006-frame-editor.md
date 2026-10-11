# ADR 0006: Frame's Editor: Direct Editing With One Inspector, Plus an Outline View

Status: Accepted

Date: 2026-10-11

## Context

FR1 turns Frame's viewer into an editor. The user decided Frame must be easier to use than PowerPoint, and chose both directions offered: direct editing on the slide, simplified as Keynote does it, and an outline view for writing a deck as titles and bullets. Frame still opens and saves only `.pptx` (ADR 0004), so decks made in PowerPoint, with free-form layouts, must stay editable, and nothing Frame doesn't model may be lost. The product rules are in `docs/specs/frame.md` ("Easier than PowerPoint").

## Decision

- **Edits are commands on the presentation**, in a new toolkit-free crate `apps/frame/crates/deck-edit`: each command stores the operation and its inverse (Bricks' rule: undo stores operations, not snapshots), and names the slide part, shape, and properties it changes so FR1's save patches only those (as Bricks ADR 0010 does for XLSX). ADR 0004's model gains provenance per property, own or inherited from the layout or master, so the inspector can show inherited values and an edit writes a slide-level override without touching the layout.
- **The outline is a view of the slides, not a second document.** `deck-edit` projects each slide to its title placeholder's text and its body placeholder's paragraphs with their levels; outline edits become the same commands as canvas edits (set paragraph text, set level, split or merge slides, add a slide with the Title and Content layout), so both views share one undo history and one save path. Text in any other shape is left out of the projection and is never written by it.
- **Snapping is computed outside the toolkit.** `deck-edit` returns, for a dragged frame in EMU and a tolerance in EMU (6 screen pixels at the current scale), the snapped frame and the guide lines to draw, from the slide's edges and centre, the layout's placeholder frames, and the other shapes' frames, plus equal-spacing candidates. Unit tests cover it without GTK.
- **The inspector is one GTK sidebar** that rebuilds its rows from the selection's kind (none, text, shape, picture, table) and applies each change as a command immediately. Formatting has no dialogs.
- **New decks start from a built-in `.pptx`** embedded in the binary (16:9, a plain theme, the standard layouts: Title Slide, Title and Content, Title Only, Two Content, Blank). File → New opens a copy, so a new deck and an opened deck read, edit, and save through the same code: there is no from-scratch PPTX writer.

## Consequences

- Canvas and outline can't drift apart, and undo works across both.
- Decks with free-form slides still edit fully on the canvas; the outline shows their titles and leaves their other text alone, which the outline view must make visible (the spec's "how many other items" line).
- The model grows provenance; read-time resolution (ADR 0004) stays for values Frame doesn't edit.
- "Easier than PowerPoint" is checked, not asserted: the spec's reference task is counted in Frame by its check script and in PowerPoint by hand.
- **Revisit if:** users need the outline to cover text outside title and body placeholders, or patched saves can't express an edit (for example, a layout change that PowerPoint stores by rewriting the slide).
