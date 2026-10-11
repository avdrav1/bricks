#!/usr/bin/env python3
"""LibreOffice side of XLSX-1's tests: build the fixture workbook, and dump a workbook as
LibreOffice reads it.

Usage: python3 scripts/lo_xlsx.py fixture OUT   (store the fixture workbook at OUT)
       python3 scripts/lo_xlsx.py dump FILE     (print FILE's sheets and cells)

`dump` prints, per sheet, `== name`, then per non-empty cell
`A1<TAB>kind<TAB>value<TAB>formula`. `kind` is `value`, `text`, or
`formula:<value|text|error>`; `value` is repr(float) for numbers (dates as serial numbers,
booleans as 1.0/0.0), the text for text and errors. `formula` is Excel syntax without `=`:
LibreOffice's API grammar (`$Data.B2`, `;` between arguments) turned into `Data!B2` and
`,`, with spaces removed outside strings.

The fixture (crates/xlsx/tests/data/fixture.xlsx) has three sheets:
- Data: a header row and 50 rows of integers, decimals, zero-padded codes stored as text,
  dates, TRUE/FALSE, a sparse column with gaps, an empty row, and =1/0; plus a bold cell, a
  comment, and a chart (content Bricks drops, for XLSX-2).
- Q1 Sales: =Data!B2*2 down 10 rows, =SUM(Data!B2:B51), =SUM(Rates) over the named range
  Rates = Data!$C$2:$C$51, and =ROW()*2.
- Summary: a bold title merged over A1:B1, =AVERAGE('Q1 Sales'!A1:A10), and =BAHTTEXT(1), a
  function Bricks doesn't implement (its result comes from the file).
Needs LibreOffice with Python UNO.
"""
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.awt import Rectangle
from com.sun.star.beans import PropertyValue
from com.sun.star.table import CellAddress, CellRangeAddress

PORT = int(os.environ.get("LO_PORT", "2105"))  # set it to run several at once
RESULT_STRING = 2  # com.sun.star.sheet.FormulaResult.STRING


def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p


def connect():
    profile = tempfile.mkdtemp(prefix="lo-xlsx-")
    subprocess.Popen(
        ["soffice", "--headless", "--invisible", "--norestore", "--nologo",
         f"-env:UserInstallation=file://{profile}",
         f"--accept=socket,host=localhost,port={PORT};urp;"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", local)
    for _ in range(150):
        try:
            return resolver.resolve(
                f"uno:socket,host=localhost,port={PORT};urp;StarOffice.ComponentContext")
        except Exception:
            time.sleep(0.2)
    sys.exit("LibreOffice did not start")


def build(doc):
    sheets = doc.Sheets
    while sheets.getCount() > 1:
        sheets.removeByName(sheets.ElementNames[-1])
    sheets.getByIndex(0).Name = "Data"
    sheets.insertNewByName("Q1 Sales", 1)
    sheets.insertNewByName("Summary", 2)
    data = sheets.getByName("Data")

    for c, title in enumerate(["id", "amount", "rate", "code", "date", "active", "note", "check"]):
        data.getCellByPosition(c, 0).setString(title)
    data.getCellByPosition(0, 0).CharWeight = 150.0  # bold (com.sun.star.awt.FontWeight.BOLD)
    for i in range(1, 51):
        r = i  # row 0 is the header
        if i == 30:
            continue  # an empty row
        data.getCellByPosition(0, r).setValue(i)
        data.getCellByPosition(1, r).setValue(round(i * 12.5 + 0.25, 2))
        data.getCellByPosition(2, r).setValue(round(i / 100 + 0.005, 3))
        data.getCellByPosition(3, r).setString(f"{i * 7 % 1000:05d}")
        data.getCellByPosition(4, r).setFormula(f"2026-{(i % 12) + 1:02d}-{(i % 28) + 1:02d}")
        data.getCellByPosition(5, r).setFormula("TRUE" if i % 3 else "FALSE")
        if i % 4 == 0:
            data.getCellByPosition(6, r).setString(f"note {i}, with a comma")
    data.getCellByPosition(7, 1).setFormula("=1/0")

    anns = data.Annotations
    anns.insertNew(CellAddress(0, 0, 0), "The row number")
    rect = Rectangle(10000, 1000, 8000, 6000)
    data.Charts.addNewByName("Amounts", rect, (CellRangeAddress(0, 0, 0, 1, 10),), True, False)

    q1 = sheets.getByName("Q1 Sales")
    for n in range(2, 12):
        q1.getCellByPosition(0, n - 2).setFormula(f"=$Data.B{n}*2")
    doc.NamedRanges.addNewByName("Rates", "$Data.$C$2:$C$51", CellAddress(0, 0, 0), 0)
    q1.getCellByPosition(1, 0).setFormula("=SUM($Data.B2:B51)")
    q1.getCellByPosition(1, 1).setFormula("=SUM(Rates)")
    q1.getCellByPosition(1, 2).setFormula("=ROW()*2")

    summary = sheets.getByName("Summary")
    title = summary.getCellByPosition(0, 0)
    title.setString("Summary")
    title.CharWeight = 150.0
    summary.getCellRangeByName("A1:B1").merge(True)
    summary.getCellByPosition(0, 1).setFormula("=AVERAGE($'Q1 Sales'.A1:A10)")
    summary.getCellByPosition(0, 2).setFormula("=BAHTTEXT(1)")
    doc.calculateAll()


def excel_syntax(formula):
    """LibreOffice API grammar as Excel's: `$Data.B2` -> `Data!B2`, `;` -> `,`, no spaces."""
    out, i = [], 0
    sheet_ref = re.compile(r"\$?('(?:[^']|'')*'|[A-Za-z_][A-Za-z0-9_]*)\.(?=\$?[A-Za-z]{1,3}\$?[0-9])")
    while i < len(formula):
        c = formula[i]
        if c == '"':
            j = i + 1
            while j < len(formula):
                if formula[j] == '"':
                    if j + 1 < len(formula) and formula[j + 1] == '"':
                        j += 2
                        continue
                    break
                j += 1
            out.append(formula[i:j + 1])
            i = j + 1
            continue
        m = sheet_ref.match(formula, i)
        if m and (i == 0 or not (formula[i - 1].isalnum() or formula[i - 1] in "_.")):
            out.append(m.group(1) + "!")
            i = m.end()
            continue
        if c == ";":
            out.append(",")
        elif c != " ":
            out.append(c)
        i += 1
    text = "".join(out)
    # A range's second sheet prefix repeats the first: `Data!B2:Data!B51` -> `Data!B2:B51`.
    text = re.sub(r"((?:'(?:[^']|'')*'|[A-Za-z_][A-Za-z0-9_]*)!)(\$?[A-Za-z]{1,3}\$?[0-9]+):\1",
                  r"\1\2:", text)
    return text.lstrip("=")


def col_name(c):
    s = ""
    c += 1
    while c:
        c, r = divmod(c - 1, 26)
        s = chr(65 + r) + s
    return s


def dump(doc):
    lines = []
    for sheet in doc.Sheets:
        lines.append(f"== {sheet.Name}")
        cursor = sheet.createCursor()
        cursor.gotoEndOfUsedArea(False)
        end = cursor.RangeAddress
        for r in range(end.EndRow + 1):
            for c in range(end.EndColumn + 1):
                cell = sheet.getCellByPosition(c, r)
                kind = cell.Type.value  # com.sun.star.table.CellContentType
                ref = f"{col_name(c)}{r + 1}"
                if kind == "EMPTY":
                    continue
                if kind == "VALUE":
                    lines.append(f"{ref}\tvalue\t{cell.getValue()!r}\t")
                elif kind == "TEXT":
                    lines.append(f"{ref}\ttext\t{cell.getString()}\t")
                else:
                    result = cell.FormulaResultType2
                    if cell.getError() != 0:
                        k, v = "error", cell.getString()
                    elif result == RESULT_STRING:
                        k, v = "text", cell.getString()
                    else:
                        k, v = "value", repr(cell.getValue())
                    lines.append(f"{ref}\tformula:{k}\t{v}\t{excel_syntax(cell.getFormula())}")
    return "\n".join(lines) + "\n"


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("fixture", "dump"):
        sys.exit(__doc__)
    path = Path(sys.argv[2]).resolve()
    ctx = connect()
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    try:
        hidden = (prop("Hidden", True),)
        if sys.argv[1] == "fixture":
            doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, hidden)
            build(doc)
            path.parent.mkdir(parents=True, exist_ok=True)
            doc.storeToURL(path.as_uri(), (prop("FilterName", "Calc MS Excel 2007 XML"),))
            print(f"fixture -> {path}")
        else:
            doc = desktop.loadComponentFromURL(path.as_uri(), "_blank", 0, hidden)
            doc.calculateAll()
            sys.stdout.write(dump(doc))
        doc.close(True)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass


if __name__ == "__main__":
    main()
