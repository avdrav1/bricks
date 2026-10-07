#!/usr/bin/env python3
"""Record LibreOffice Calc's keyboard-navigation behavior for GRID-3.

Drives a headless Calc (UNO + the built-in UITest service) through every scenario below on
a 200-row x 6-column sheet full of data, and records where the cell cursor and the
selection end up. Writes:
  crates/grid/tests/data/lo_navigation.tsv   replayed by `cargo test -p grid --test lo_parity`
  docs/navigation-checklist.md               the side-by-side checklist

Usage: python3 scripts/lo_navigation.py   (needs LibreOffice with Python UNO)
Default Calc settings are used: Enter moves the selection down.
"""
import os
import subprocess
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.beans import PropertyValue

ROOT = Path(__file__).resolve().parent.parent
ROWS, COLS = 200, 6
PORT = 2094

# (id, what it checks, start cell, keys). Keys use UITest names; sequences run in order.
SCENARIOS = [
    ("arrow-down", "Down moves one row", "C3", "DOWN"),
    ("arrow-up", "Up moves one row", "C3", "UP"),
    ("arrow-left", "Left moves one column", "C3", "LEFT"),
    ("arrow-right", "Right moves one column", "C3", "RIGHT"),
    ("arrow-up-at-top", "Up in row 1 stays", "C1", "UP"),
    ("arrow-left-at-a", "Left in column A stays", "A3", "LEFT"),
    ("tab", "Tab moves right", "C3", "TAB"),
    ("shift-tab", "Shift+Tab moves left", "C3", "SHIFT+TAB"),
    ("shift-tab-at-a", "Shift+Tab in column A", "A3", "SHIFT+TAB"),
    ("enter", "Enter moves down", "C3", "RETURN"),
    ("shift-enter", "Shift+Enter moves up", "C3", "SHIFT+RETURN"),
    ("tab-tab-enter", "Enter after Tabs returns to the column where tabbing started", "B3", "TAB TAB RETURN"),
    ("tab-arrow-enter", "An arrow key forgets the tab start column", "B3", "TAB DOWN RETURN"),
    ("home", "Home goes to column A", "C3", "HOME"),
    ("end", "End goes to the last column with data", "C3", "END"),
    ("ctrl-home", "Ctrl+Home goes to A1", "D50", "CTRL+HOME"),
    ("ctrl-end", "Ctrl+End goes to the last cell with data", "B3", "CTRL+END"),
    ("page-down", "Page Down moves one screen down", "A1", "PAGEDOWN"),
    ("page-down-twice", "Page Down twice", "C3", "PAGEDOWN PAGEDOWN"),
    ("page-up", "Page Up moves one screen up", "C80", "PAGEUP"),
    ("page-up-near-top", "Page Up near the top stops at row 1", "C5", "PAGEUP"),
    ("shift-down", "Shift+Down extends; the cursor stays", "C3", "SHIFT+DOWN"),
    ("shift-down-right", "Shift+arrows build a block", "C3", "SHIFT+DOWN SHIFT+DOWN SHIFT+RIGHT"),
    ("shift-up-left", "Shift+arrows extend up and left", "C5", "SHIFT+UP SHIFT+LEFT SHIFT+LEFT"),
    ("shift-back", "Extending back over the cursor shrinks then flips", "C3", "SHIFT+DOWN SHIFT+UP SHIFT+UP"),
    ("arrow-after-select", "An arrow key collapses the selection", "C3", "SHIFT+DOWN SHIFT+DOWN DOWN"),
    ("tab-in-range", "Tab moves within a selected range", "B2", "SHIFT+DOWN SHIFT+RIGHT TAB"),
    ("tab-wraps-in-range", "Tab wraps to the next row of the range", "B2", "SHIFT+DOWN SHIFT+RIGHT TAB TAB"),
    ("tab-cycles-range", "Tab cycles back to the start of the range", "B2", "SHIFT+DOWN SHIFT+RIGHT TAB TAB TAB TAB"),
    ("enter-in-range", "Enter moves down within a range", "B2", "SHIFT+DOWN SHIFT+RIGHT RETURN"),
    ("enter-wraps-in-range", "Enter wraps to the next column of the range", "B2", "SHIFT+DOWN SHIFT+RIGHT RETURN RETURN"),
    ("shift-tab-in-range", "Shift+Tab moves backwards within a range", "B2", "SHIFT+DOWN SHIFT+RIGHT SHIFT+TAB"),
    ("shift-enter-in-range", "Shift+Enter moves backwards within a range", "B2", "SHIFT+DOWN SHIFT+RIGHT SHIFT+RETURN"),
    ("shift-home", "Shift+Home selects to column A", "C3", "SHIFT+HOME"),
    ("shift-end", "Shift+End selects to the last data column", "C3", "SHIFT+END"),
    ("shift-ctrl-home", "Shift+Ctrl+Home selects to A1", "C3", "SHIFT+CTRL+HOME"),
    ("shift-ctrl-end", "Shift+Ctrl+End selects to the last data cell", "C3", "SHIFT+CTRL+END"),
    ("shift-page-down", "Shift+Page Down extends one screen", "C3", "SHIFT+PAGEDOWN"),
    ("shift-page-up", "Shift+Page Up extends one screen up", "C80", "SHIFT+PAGEUP"),
    ("shift-space", "Shift+Space selects the cursor's row", "C3", "SHIFT+SPACE"),
    ("ctrl-space", "Ctrl+Space selects the cursor's column", "C3", "CTRL+SPACE"),
    ("shift-ctrl-space", "Shift+Ctrl+Space selects everything", "C3", "SHIFT+CTRL+SPACE"),
    ("rows-extend", "Shift+Down after selecting a row adds rows", "C3", "SHIFT+SPACE SHIFT+DOWN"),
    ("cols-extend", "Shift+Right after selecting a column adds columns", "C3", "CTRL+SPACE SHIFT+RIGHT"),
    ("row-then-arrow", "An arrow key after selecting a row collapses it", "C3", "SHIFT+SPACE DOWN"),
    ("row-shift-left", "Shift+Left after selecting a row", "C3", "SHIFT+SPACE SHIFT+LEFT"),
    ("col-shift-down", "Shift+Down after selecting a column", "C3", "CTRL+SPACE SHIFT+DOWN"),
    ("col-shift-up", "Shift+Up after selecting a column", "C3", "CTRL+SPACE SHIFT+UP"),
    ("all-then-tab", "Tab with everything selected moves within it", "C3", "SHIFT+CTRL+SPACE TAB"),
]


def pv(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p


def col_name(c):
    s = ""
    c += 1
    while c:
        c, r = divmod(c - 1, 26)
        s = chr(65 + r) + s
    return s


def cell(c, r):
    return f"{col_name(c)}{r + 1}"


def parse_cell(ref):
    letters = "".join(ch for ch in ref if ch.isalpha())
    col = 0
    for ch in letters:
        col = col * 26 + (ord(ch) - 64)
    return col - 1, int(ref[len(letters):]) - 1


def arrive(grid, start):
    """Put the cursor on `start` the way a user gets there: by arrowing onto it. UITest's
    SELECT action leaves a one-cell marked range, inside which Tab and Enter cycle in place,
    so it is only used to reach a neighbour."""
    c, r = parse_cell(start)
    if r > 0:
        grid.executeAction("SELECT", (pv("CELL", cell(c, r - 1)),))
        grid.executeAction("TYPE", (pv("KEYCODE", "DOWN"),))
    else:
        grid.executeAction("SELECT", (pv("CELL", cell(c, r + 1)),))
        grid.executeAction("TYPE", (pv("KEYCODE", "UP"),))


def connect(profile):
    proc = subprocess.Popen(
        ["soffice", "--headless", "--invisible", "--norestore", "--nologo",
         f"-env:UserInstallation=file://{profile}",
         f"--accept=socket,host=localhost,port={PORT};urp;"],
        env=dict(os.environ, SAL_USE_VCLPLUGIN="svp"),
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
    for _ in range(150):
        try:
            return proc, resolver.resolve(f"uno:socket,host=localhost,port={PORT};urp;StarOffice.ComponentContext")
        except Exception:  # noqa: BLE001 - office still starting
            time.sleep(0.2)
    proc.kill()
    raise SystemExit("LibreOffice did not start")


def main():
    with tempfile.TemporaryDirectory() as profile:
        proc, ctx = connect(profile)
        try:
            smgr = ctx.ServiceManager
            desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
            doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, ())
            sheet = doc.Sheets.getByIndex(0)
            sheet.getCellRangeByPosition(0, 0, COLS - 1, ROWS - 1).setDataArray(
                tuple(tuple(f"r{r}c{c}" for c in range(COLS)) for r in range(ROWS)))
            uitest = smgr.createInstanceWithContext("org.libreoffice.uitest.UITest", ctx)
            grid = uitest.getTopFocusWindow().getChild("grid_window")
            version = lo_version(smgr, ctx)
            rows = []
            for sid, what, start, keys in SCENARIOS:
                arrive(grid, start)
                for key in keys.split():
                    grid.executeAction("TYPE", (pv("KEYCODE", key),))
                state = {p.Name: p.Value for p in grid.getState()}
                cur = cell(int(state["CurrentColumn"]), int(state["CurrentRow"]))
                a = doc.getCurrentSelection().getRangeAddress()
                rng = f"{cell(a.StartColumn, a.StartRow)}:{cell(a.EndColumn, a.EndRow)}"
                rows.append((sid, what, start, keys, cur, rng))
                print(f"{sid:22} {start:4} {keys:42} -> cursor {cur:5} selection {rng}")
            doc.close(True)
        finally:
            proc.terminate()

    data = ROOT / "crates/grid/tests/data/lo_navigation.tsv"
    data.parent.mkdir(parents=True, exist_ok=True)
    with data.open("w") as f:
        f.write(f"# Recorded from LibreOffice {version} by scripts/lo_navigation.py on a {ROWS}x{COLS} sheet.\n")
        f.write("id\tstart\tkeys\tcursor\tselection\n")
        for sid, _, start, keys, cur, rng in rows:
            f.write(f"{sid}\t{start}\t{keys}\t{cur}\t{rng}\n")

    doc_path = ROOT / "docs/navigation-checklist.md"
    with doc_path.open("w") as f:
        f.write("# Keyboard navigation and selection checklist (GRID-3, GRID-4)\n\n")
        f.write(f"Each row is LibreOffice Calc {version}'s behavior, recorded by `scripts/lo_navigation.py` "
                f"on a {ROWS}-row, {COLS}-column sheet full of data with default settings. "
                "`cargo test -p grid --test lo_parity` replays every row through the grid's navigation and "
                "requires the same cursor cell and selection. Page Up/Down replay with LibreOffice's "
                "page height (the rows one Page Down moved from A1), since the two apps show different "
                "numbers of rows per screen. Whole-row and whole-column selections run to the sheet's "
                "edge in Calc (column XFD, row 1048576); the replay compares them within the data, where "
                "this grid's sheet ends.\n\n")
        f.write("| Check | Start | Keys | LibreOffice cursor | LibreOffice selection |\n| --- | --- | --- | --- | --- |\n")
        for _, what, start, keys, cur, rng in rows:
            f.write(f"| {what} | {start} | {keys.replace(' ', ', ')} | {cur} | {rng} |\n")
        f.write("\nOut of scope here: Ctrl+arrow (jump to data edges), Alt+Page Up/Down (screen left/right), "
                "and moving past the last row or column of data (Calc has empty cells there; this grid shows "
                "only the file's rows and columns). Mouse selection (click, Shift+click, drag, header "
                "clicks; GRID-4) cannot be driven through LibreOffice's UI-test API; it follows the same "
                "model and is covered by unit tests in `crates/grid/src/nav.rs`.\n")
    print(f"wrote {data.relative_to(ROOT)} and {doc_path.relative_to(ROOT)}")


def lo_version(smgr, ctx):
    cp = smgr.createInstanceWithContext("com.sun.star.configuration.ConfigurationProvider", ctx)
    node = cp.createInstanceWithArguments(
        "com.sun.star.configuration.ConfigurationAccess", (pv("nodepath", "/org.openoffice.Setup/Product"),))
    return node.getByName("ooSetupVersionAboutBox")


if __name__ == "__main__":
    main()
