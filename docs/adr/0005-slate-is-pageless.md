# ADR 0005: Slate Is Pageless

Status: Accepted

Date: 2026-10-11

## Context

ADR 0003 left "the layout engine for editing and pagination" to SL1's decision story, and the SL0 viewer shows documents as a centred column a page wide (6.5 in at 96 dpi). The user decided Slate is pageless: a document is an endless column, as in Google Docs' pageless format, never split into pages. GTK 4 has no paginated layout engine, so pagination would have been Slate's largest custom component.

## Decision

- **No pages, in any view or milestone.** Slate lays a document out as one continuous column. It never computes page breaks from content, never draws page boundaries, and has no page layout view, page setup, headers, footers, or page numbers. Paginated printing and PDF export are out of scope.
- **The text width is a user setting, not a document property:** View → Text Width with Narrow (480 px), Medium (624 px, the default and SL0's width), Wide (816 px), and Full (the window's width less a 24 px margin each side). It applies to every window at once and is remembered in the user's config directory (`$XDG_CONFIG_HOME/slate/`), never written to a document. A window narrower than the chosen width narrows the column.
- **Page breaks stay content.** The model keeps `Block::PageBreak` (ADR 0003); the view shows it as a thin marker line in the flow, named "Page break" to screen readers, so a user can see and, in SL1, delete it. DOCX page setup (sections, page size, margins, columns, headers, footers) stays out of the model and survives SL1's patched save.
- **SLATE-5 decides only the editing engine** for this flow, GtkTextView or a custom Pango flow, measuring typing latency and frame time on a 150,000-word document (about 300 pages' worth) at Full width, and the time to re-wrap it when the width changes.

## Consequences

- SL1 drops pagination: no page-break computation, widow and orphan control, or per-page headers. Both SLATE-5 candidates become simpler; GtkTextView, which can't paginate, stays a real option.
- Documents look the same in every format: a `.docx` made in Word shows as a flowing column, as the SL0 viewer already shows it.
- Users who need printed pages export from another program; Slate keeps their `.docx` page setup intact for it.
- **Revisit if:** users need to print from Slate. Printing would add a paginated print path (GtkPrintOperation with Pango), not a paged editing view.
