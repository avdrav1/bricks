# Keyboard navigation checklist (GRID-3)

Each row is LibreOffice Calc 26.8.0.3's behavior, recorded by `scripts/lo_navigation.py` on a 200-row, 6-column sheet full of data with default settings. `cargo test -p grid --test lo_parity` replays every row through the grid's navigation and requires the same cursor cell and selection. Page Up/Down replay with LibreOffice's page height (the rows one Page Down moved from A1), since the two apps show different numbers of rows per screen.

| Check | Start | Keys | LibreOffice cursor | LibreOffice selection |
| --- | --- | --- | --- | --- |
| Down moves one row | C3 | DOWN | C4 | C4:C4 |
| Up moves one row | C3 | UP | C2 | C2:C2 |
| Left moves one column | C3 | LEFT | B3 | B3:B3 |
| Right moves one column | C3 | RIGHT | D3 | D3:D3 |
| Up in row 1 stays | C1 | UP | C1 | C1:C1 |
| Left in column A stays | A3 | LEFT | A3 | A3:A3 |
| Tab moves right | C3 | TAB | D3 | D3:D3 |
| Shift+Tab moves left | C3 | SHIFT+TAB | B3 | B3:B3 |
| Shift+Tab in column A | A3 | SHIFT+TAB | A3 | A3:A3 |
| Enter moves down | C3 | RETURN | C4 | C4:C4 |
| Shift+Enter moves up | C3 | SHIFT+RETURN | C2 | C2:C2 |
| Enter after Tabs returns to the column where tabbing started | B3 | TAB, TAB, RETURN | B4 | B4:B4 |
| An arrow key forgets the tab start column | B3 | TAB, DOWN, RETURN | C5 | C5:C5 |
| Home goes to column A | C3 | HOME | A3 | A3:A3 |
| End goes to the last column with data | C3 | END | F3 | F3:F3 |
| Ctrl+Home goes to A1 | D50 | CTRL+HOME | A1 | A1:A1 |
| Ctrl+End goes to the last cell with data | B3 | CTRL+END | F200 | F200:F200 |
| Page Down moves one screen down | A1 | PAGEDOWN | A23 | A23:A23 |
| Page Down twice | C3 | PAGEDOWN, PAGEDOWN | C47 | C47:C47 |
| Page Up moves one screen up | C80 | PAGEUP | C58 | C58:C58 |
| Page Up near the top stops at row 1 | C5 | PAGEUP | C1 | C1:C1 |
| Shift+Down extends; the cursor stays | C3 | SHIFT+DOWN | C3 | C3:C4 |
| Shift+arrows build a block | C3 | SHIFT+DOWN, SHIFT+DOWN, SHIFT+RIGHT | C3 | C3:D5 |
| Shift+arrows extend up and left | C5 | SHIFT+UP, SHIFT+LEFT, SHIFT+LEFT | C5 | A4:C5 |
| Extending back over the cursor shrinks then flips | C3 | SHIFT+DOWN, SHIFT+UP, SHIFT+UP | C3 | C2:C3 |
| An arrow key collapses the selection | C3 | SHIFT+DOWN, SHIFT+DOWN, DOWN | C4 | C4:C4 |
| Tab moves within a selected range | B2 | SHIFT+DOWN, SHIFT+RIGHT, TAB | C2 | B2:C3 |
| Tab wraps to the next row of the range | B2 | SHIFT+DOWN, SHIFT+RIGHT, TAB, TAB | B3 | B2:C3 |
| Tab cycles back to the start of the range | B2 | SHIFT+DOWN, SHIFT+RIGHT, TAB, TAB, TAB, TAB | B2 | B2:C3 |
| Enter moves down within a range | B2 | SHIFT+DOWN, SHIFT+RIGHT, RETURN | B3 | B2:C3 |
| Enter wraps to the next column of the range | B2 | SHIFT+DOWN, SHIFT+RIGHT, RETURN, RETURN | C2 | B2:C3 |
| Shift+Tab moves backwards within a range | B2 | SHIFT+DOWN, SHIFT+RIGHT, SHIFT+TAB | C3 | B2:C3 |
| Shift+Enter moves backwards within a range | B2 | SHIFT+DOWN, SHIFT+RIGHT, SHIFT+RETURN | C3 | B2:C3 |
| Shift+Home selects to column A | C3 | SHIFT+HOME | C3 | A3:C3 |
| Shift+End selects to the last data column | C3 | SHIFT+END | C3 | C3:F3 |
| Shift+Ctrl+Home selects to A1 | C3 | SHIFT+CTRL+HOME | C3 | A1:C3 |
| Shift+Ctrl+End selects to the last data cell | C3 | SHIFT+CTRL+END | C3 | C3:F200 |
| Shift+Page Down extends one screen | C3 | SHIFT+PAGEDOWN | C3 | C3:C25 |
| Shift+Page Up extends one screen up | C80 | SHIFT+PAGEUP | C80 | C58:C80 |

Out of scope here: Ctrl+arrow (jump to data edges), Alt+Page Up/Down (screen left/right), and moving past the last row or column of data (Calc has empty cells there; this grid shows only the file's rows and columns).
