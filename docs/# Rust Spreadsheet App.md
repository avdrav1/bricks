# Rust Spreadsheet App

## 1. Product Summary

A fast, lightweight spreadsheet application for Linux, written primarily in Rust.

The initial product focuses on one thing exceptionally well:

> **Open, browse, edit, sort, filter, search, and save large CSV files significantly faster than LibreOffice Calc.**

The application should feel familiar to general spreadsheet users while avoiding the complexity and visual weight of traditional office suites.

CSV editing is the entry point, not the final destination. Over time, the product should evolve into a serious Linux-native alternative to Excel and LibreOffice Calc.

---

# 2. Product Vision

Build a modern spreadsheet application that combines:

- the familiarity of a traditional spreadsheet
- the startup speed of a lightweight text editor
- the performance advantages of Rust
- first-class Linux desktop integration
- excellent handling of large datasets
- eventual compatibility with Excel files and spreadsheet workflows

The long-term ambition is:

> **A fast, modern, native Linux spreadsheet capable of becoming a credible Excel/LibreOffice alternative.**

---

# 3. Target User

## Primary User

General spreadsheet users on Linux.

Users should not need to:

- understand databases
- know SQL
- use command-line tools
- know programming
- understand CSV internals

Someone familiar with Excel or LibreOffice should be able to open the application and immediately understand how to use it.

---

# 4. Core Product Principles

## 4.1 Fast by Default

Performance is a product feature.

Opening, scrolling, searching, sorting, filtering, and editing should remain responsive even with very large files.

Initial performance target:

**CSV files up to approximately 1 GB should remain usable.**

---

## 4.2 Lightweight

The interface should avoid the complexity of traditional office software.

Prefer:

- simple menus
- a small toolbar
- contextual controls
- keyboard shortcuts
- an uncluttered grid

Avoid a large Excel-style ribbon for the initial product.

---

## 4.3 Familiar

Despite being lightweight, standard spreadsheet behaviors should work as users expect.

Examples:

- Ctrl+C / Ctrl+V
- Ctrl+Z / Ctrl+Shift+Z
- Ctrl+F
- clicking column headers
- selecting ranges
- dragging column widths
- sorting columns
- filtering columns
- arrow-key navigation
- Enter moves through cells

---

## 4.4 Native Linux Experience

Linux is not a secondary platform.

The application should integrate naturally with the Linux desktop, including:

- system file dialogs
- clipboard
- keyboard shortcuts
- drag and drop
- system themes
- high-DPI displays
- Wayland
- standard desktop menus

Arch Linux is the initial development and distribution target.

Other distributions should remain feasible.

---

# 5. Version 0.1 Goal

Version 0.1 should prove the fundamental product thesis:

> A Rust spreadsheet can provide a dramatically faster and cleaner experience for working with large CSV files than traditional Linux office software.

It is not yet an Excel replacement.

---

# 6. V0.1 Feature Set

## File Operations

Users can:

- open CSV files
- create new CSV files
- save files
- Save As
- drag a CSV file onto the application to open it
- reopen recent files

Supported delimiters should include at minimum:

- comma
- tab
- semicolon
- pipe

Delimiter detection should happen automatically where practical.

Users should also be able to override the detected delimiter.

---

# 7. Spreadsheet Grid

The primary application surface is a virtualized spreadsheet grid.

Users can:

- select cells
- select ranges
- select rows
- select columns
- edit individual cells
- edit cells using the keyboard
- resize columns
- resize rows
- insert rows
- delete rows
- insert columns
- delete columns

The grid should not create a heavyweight UI object for every cell.

Only visible cells and a small surrounding buffer should need to be rendered.

This is essential for large-file performance.

---

# 8. Navigation

Support familiar spreadsheet navigation.

Minimum keyboard behaviors:

- Arrow keys — move selection
- Enter — move down
- Shift+Enter — move up
- Tab — move right
- Shift+Tab — move left
- Ctrl+A — select data
- Ctrl+C — copy
- Ctrl+X — cut
- Ctrl+V — paste
- Delete — clear cells
- Ctrl+Z — undo
- Ctrl+Shift+Z — redo
- Ctrl+F — search

Navigation should remain responsive regardless of dataset size.

---

# 9. Copy and Paste

Users should be able to copy and paste:

- single cells
- rows
- columns
- rectangular ranges

Clipboard interoperability should work with:

- the application itself
- LibreOffice Calc
- Excel where possible
- plain-text applications

Tab/newline-separated clipboard content should map naturally onto spreadsheet ranges.

---

# 10. Undo and Redo

Undo/redo is a V0.1 requirement.

Actions that should be undoable include:

- cell edits
- paste operations
- row insertion
- row deletion
- column insertion
- column deletion
- sorting
- replace operations

The undo system should store operations rather than complete copies of the dataset whenever possible.

This is especially important for large files.

---

# 11. Search and Replace

Search should be a first-class capability.

Support:

- search entire file
- search current column
- next result
- previous result
- case sensitivity
- find and replace
- replace all

Later versions may add:

- regex
- fuzzy search
- advanced query syntax

Search operations should execute outside the UI thread.

---

# 12. Sorting

Users should be able to sort a column:

- ascending
- descending

Sorting changes the actual row order of the dataset.

Future versions should support multi-column sorting.

Sorting should understand inferred data types when possible.

For example:

`2, 10, 100`

should sort numerically rather than:

`10, 100, 2`

when the column is confidently inferred to contain numbers.

---

# 13. Filtering

Each column should support filtering.

Initial filter capabilities:

- exact match
- contains
- does not contain
- empty
- non-empty
- numeric comparisons
- text search

Multiple column filters should be combinable.

The UI should display how many rows remain visible, for example:

**14,392 of 2,103,814 rows**

Filters affect the active dataset view.

Saving while a filter is active should **not accidentally discard hidden rows**.

Filtering and sorting therefore need distinct internal representations even though sorting ultimately modifies row order.

---

# 14. Data Types

CSV files contain text, but the application should infer useful semantic types.

Potential inferred types:

- text
- integer
- decimal
- boolean
- date
- datetime

Type inference should improve:

- sorting
- filtering
- display

However:

> **Type inference must never silently change the original CSV value.**

For example:

`00123`

must not become:

`123`

simply because it resembles a number.

The raw stored CSV representation remains authoritative.

---

# 15. Large File Architecture

Supporting large files is a fundamental design requirement.

Target:

**1 GB CSV files should remain usable on typical desktop hardware.**

The architecture should also leave room for files larger than available RAM.

The implementation should therefore avoid assuming that the entire dataset must exist as millions of fully allocated cell objects.

---

# 16. Suggested Data Architecture

Conceptually:

```text
CSV File
   ↓
Parser / Indexer
   ↓
Tabular Data Model
   ↓
Edit Overlay
   ↓
Sort / Filter Layer
   ↓
Virtualized Grid
```

The application should separate:

### Source data

The original file representation.

### Logical data

The interpreted tabular structure.

### Edits

Changes made by the user.

### View

Sorting/filtering/visible-row state.

### Rendering

Only the cells currently visible on screen.

These should not be tightly coupled.

---

# 17. Edit Overlay

For large files, edits should initially be represented as changes layered over source data.

For example, changing:

`D1,532,817`

should not require copying or rebuilding one million rows immediately.

Conceptually:

```text
Original File
+
Cell Changes
+
Inserted Rows
+
Deleted Rows
+
Column Changes
=
Current Workbook State
```

A full output file can be generated when the user saves.

This architecture should make extremely large datasets practical.

---

# 18. Background Processing

Expensive operations must not block the interface.

Candidates include:

- initial parsing
- indexing
- sorting
- filtering
- searching
- saving
- type inference

The UI should remain responsive while these execute.

Operations should expose progress where useful and should ideally be cancellable.

---

# 19. File Saving

Saving should favor safety over raw speed.

Recommended strategy:

```text
Original
   ↓
Write temporary file
   ↓
Verify successful write
   ↓
Atomically replace original
```

A crash during saving should not corrupt the existing file.

---

# 20. Memory Strategy

The application should be designed to avoid requiring the entire CSV to be materialized in RAM.

Possible techniques include:

- streaming parsing
- file indexing
- memory mapping
- chunked storage
- columnar structures
- edit overlays
- lazy loading

The exact implementation should be determined during technical prototyping.

---

# 21. Rust Architecture

The codebase should be modular from the beginning.

Suggested workspace:

```text
spreadsheet/
│
├── crates/
│   ├── csv-engine/
│   ├── data-model/
│   ├── grid/
│   ├── commands/
│   └── file-format/
│
└── app/
```

### `csv-engine`

Responsibilities:

- parsing
- delimiter detection
- encoding handling
- CSV indexing
- CSV writing

### `data-model`

Responsibilities:

- tables
- rows
- columns
- cell values
- edits
- inferred types
- sorting
- filtering

### `grid`

Responsibilities:

- virtualization
- selection
- navigation
- visible cells
- resizing
- editing interaction

### `commands`

Responsibilities:

- undo/redo
- editing operations
- row operations
- column operations
- sorting

### `app`

Responsibilities:

- application windows
- menus
- dialogs
- tabs
- user preferences
- OS integration

This separation will make it much easier to add Excel features later.

---

# 22. UI Direction

The interface should feel like a **modern lightweight spreadsheet**, not an office suite.

Suggested structure:

```text
┌──────────────────────────────────────────────┐
│ File  Edit  View  Data                      │
├──────────────────────────────────────────────┤
│ Search / lightweight actions                │
├─────┬──────────┬──────────┬──────────┬───────┤
│     │ Name     │ City     │ Revenue  │ ...   │
├─────┼──────────┼──────────┼──────────┼───────┤
│  1  │ Alice    │ Boston   │ 1200     │       │
│  2  │ David    │ Philly   │ 830      │       │
│  3  │ Maya     │ Chicago  │ 1520     │       │
│     │          │          │          │       │
│     │          │          │          │       │
├──────────────────────────────────────────────┤
│ 2,103,814 rows        CSV · UTF-8 · Saved   │
└──────────────────────────────────────────────┘
```

Avoid excessive permanent UI.

Advanced functionality should appear contextually.

---

# 23. Linux Platform

Initial development target:

**Arch Linux**

Initial priorities:

- Wayland compatibility
- X11 compatibility where practical
- native clipboard integration
- native file dialogs
- dark/light system theme
- high-DPI support
- desktop launcher
- file associations

Likely first distribution:

- curl installer (`curl -fsSL <site>/install.sh | sh`), per user, for Arch, Ubuntu, and Fedora (the AUR was dropped on 2026-10-10)

Future distribution:

- Flatpak
- potentially AppImage
- packages for major distributions

---

# 24. V0.1 Non-Goals

To keep the initial architecture manageable, V0.1 does not need:

- formulas
- charts
- pivot tables
- multiple worksheets
- macros
- scripting
- collaboration
- cloud sync
- conditional formatting
- advanced cell styling
- VBA
- real-time collaboration
- plugins

These should not prevent architectural planning for future versions.

---

# 25. Phase 2 — XLSX

Once the CSV editor is solid, the first major expansion should be:

## XLSX Import

Users should be able to open ordinary Excel workbooks.

Initial XLSX support could begin with:

- cell values
- multiple sheets
- basic formatting
- column widths
- row heights
- basic formulas

Unsupported Excel capabilities should generate warnings rather than silently altering workbook behavior.

---

# 26. Phase 3 — Spreadsheet Features

Following XLSX support:

### Formula engine

Examples:

```text
=SUM(A1:A20)
=AVERAGE(B:B)
=A1+B1
=IF(C2>100,"High","Low")
```

Architecture should eventually support:

- cell dependencies
- incremental recalculation
- formula errors
- cross-sheet references

---

# 27. Phase 4 — Full Spreadsheet

Possible later capabilities:

- rich formatting
- charts
- tables
- conditional formatting
- named ranges
- pivot tables
- data validation
- XLSX export
- print layouts
- advanced formulas
- multiple workbook windows
- plugins
- scripting

---

# 28. Proposed Roadmap

## Milestone 0 — Technical Prototype

Prove:

- extremely large CSV can be parsed
- virtualized grid works
- scrolling stays smooth
- individual cells can be edited
- modifications can be written back safely

The prototype does not need to look polished.

---

## Milestone 1 — CSV Viewer

Deliver:

- open CSV
- delimiter detection
- virtualized grid
- navigation
- column resizing
- large-file support

---

## Milestone 2 — CSV Editor

Add:

- cell editing
- copy/paste
- insert/delete rows
- insert/delete columns
- undo/redo
- saving

At this point the application becomes genuinely useful.

---

## Milestone 3 — Data Tools

Add:

- search
- replace
- filtering
- sorting
- type inference
- improved large-file operations

This represents the target **V0.1 release**.

---

## Milestone 4 — Desktop Polish

Add:

- recent files
- drag and drop
- system themes
- keyboard shortcut improvements
- crash recovery
- better errors
- packaging
- curl installer distribution

---

## Milestone 5 — Excel

Begin:

- XLSX parser/integration
- multiple sheets
- cell formatting
- initial formulas

---

# 29. Success Criteria for V0.1

The project succeeds if a general Linux user can open a large CSV and think:

> **“This is much nicer and faster than opening it in LibreOffice.”**

Specific benchmarks should eventually be established for:

- application startup
- time until first rows appear
- scrolling responsiveness
- search
- sort
- memory usage
- save performance

The headline benchmark should be:

> **A ~1 GB CSV can be opened, explored, edited, filtered, sorted, searched, and saved without the application becoming unusable.**

---

# 30. North Star

Do not begin by rebuilding Excel.

Begin by building the best large CSV editor on Linux.

Then progressively erase the reasons users need to open LibreOffice or Excel.