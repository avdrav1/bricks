#!/usr/bin/env python3
"""LibreOffice side of XLSX-1's tests: build the fixture workbook, and dump a workbook as
LibreOffice reads it.

Usage: python3 scripts/lo_xlsx.py fixture OUT   (store the fixture workbook at OUT)
       python3 scripts/lo_xlsx.py dump FILE     (print FILE's sheets and cells)
       python3 scripts/lo_xlsx.py styles FILE   (print FILE's cell styles and sheet layout)

`dump` prints, per sheet, `== name`, then per non-empty cell
`A1<TAB>kind<TAB>value<TAB>formula`. `kind` is `value`, `text`, or
`formula:<value|text|error>`; `value` is repr(float) for numbers (dates as serial numbers,
booleans as 1.0/0.0), the text for text and errors. `formula` is Excel syntax without `=`:
LibreOffice's API grammar (`$Data.B2`, `;` between arguments) turned into `Data!B2` and
`,`, with spaces removed outside strings.

The fixture (apps/brick/crates/xlsx/tests/data/fixture.xlsx) has three sheets:
- Data: a header row and 50 rows of integers, decimals, zero-padded codes stored as text,
  dates, TRUE/FALSE, a sparse column with gaps, an empty row, and =1/0; plus a bold cell, a
  comment, and a chart (content Brick drops, for XLSX-2).
- Q1 Sales: =Data!B2*2 down 10 rows, =SUM(Data!B2:B51), =SUM(Rates) over the named range
  Rates = Data!$C$2:$C$51, and =ROW()*2.
- Summary: a bold title merged over A1:B1, =AVERAGE('Q1 Sales'!A1:A10), and =BAHTTEXT(1), a
  function Brick doesn't implement (its result comes from the file).
- Styles (STYLE-1): fonts, colours, fills, every border style, alignments, wrap, rotation,
  indent, number formats, merged ranges, column widths, row heights, a hidden row and
  column, a conditional format, a validation list, and a hyperlink.

`styles` prints, per sheet, `== name`, then per cell of the used area
`A1<TAB>font=NAME|PT|b|i|u<n>|s<n>|RRGGBB<TAB>fill=RRGGBB<TAB>border=T,B,L,R<TAB>align=H|V|wrap|ROT|INDENT<TAB>fmt=CODE`,
where each border side is `STYLE:WIDTH:RRGGBB` (LibreOffice's BorderLineStyle number and
width in 1/100 mm, `-` for none), colours are `auto`/`none` when unset, and `fmt` is the
number format code. Then `merge A1:B1` per merged range, `col C WIDTH HIDDEN` and
`row 5 HEIGHT HIDDEN` (1/100 mm, 0/1) for columns and rows of the used area.
Needs LibreOffice with Python UNO.
"""
import os
import re
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

import uno
from com.sun.star.awt import Rectangle
from com.sun.star.beans import PropertyValue
from com.sun.star.table import BorderLine2, CellAddress, CellRangeAddress

PORT = int(os.environ.get("LO_PORT", "2105"))  # set it to run several at once
# Dump at most this many rows of each sheet (big real-world workbooks dump slowly).
MAX_ROWS = int(os.environ.get("LO_MAX_ROWS", "0")) or None
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
    build_styles(doc, sheets)
    doc.calculateAll()


def border(style, width, color=0):
    line = BorderLine2()
    line.Color, line.LineStyle, line.LineWidth = color, style, width
    return line


def build_styles(doc, sheets):
    sheets.insertNewByName("Styles", 3)
    st = sheets.getByName("Styles")
    cell = st.getCellByPosition
    locale = uno.createUnoStruct("com.sun.star.lang.Locale")
    locale.Language, locale.Country = "en", "US"
    formats = doc.NumberFormats

    def fmt(code):
        key = formats.queryKey(code, locale, False)
        return key if key != -1 else formats.addNew(code, locale)

    title = cell(0, 0)
    title.setString("Styled title")
    title.CharFontName, title.CharHeight, title.CharWeight = "Arial", 16.0, 150.0
    title.HoriJustify = uno.Enum("com.sun.star.table.CellHoriJustify", "CENTER")
    st.getCellRangeByName("A1:D1").merge(True)

    for c, name in enumerate(["kind", "a", "b", "c", "d", "e", "f", "g"]):
        h = cell(c, 1)
        h.setString(name)
        h.CharWeight, h.CellBackColor = 150.0, 0xFFFF00
        h.BottomBorder = border(0, 79)  # thick
    fonts = [
        ("italic", {"CharPosture": uno.Enum("com.sun.star.awt.FontSlant", "ITALIC")}),
        ("underline", {"CharUnderline": 1}),
        ("double underline", {"CharUnderline": 2}),
        ("strike", {"CharStrikeout": 1}),
        ("serif 14", {"CharFontName": "DejaVu Serif", "CharHeight": 14.0}),
        ("red", {"CharColor": 0xC00000}),
        ("bold blue", {"CharWeight": 150.0, "CharColor": 0x0070C0}),
    ]
    cell(0, 2).setString("fonts")
    for i, (text, props) in enumerate(fonts):
        c = cell(i + 1, 2)
        c.setString(text)
        for k, v in props.items():
            setattr(c, k, v)
    cell(0, 3).setString("fills")
    for i, color in enumerate([0xDDEBF7, 0xFCE4D6, 0xE2EFDA, 0x305496]):
        c = cell(i + 1, 3)
        c.setString(f"fill {i}")
        c.CellBackColor = color
    c.CharColor = 0xFFFFFF
    cell(0, 4).setString("borders")
    sides = [(0, 26), (0, 79), (0, 141), (2, 26), (1, 26), (16, 26), (0, 26)]
    for i, (style, width) in enumerate(sides):
        c = cell(i + 1, 4)
        c.setString(f"border {i}")
        color = 0xFF0000 if i == 6 else 0
        line = border(style, width, color)
        c.TopBorder = c.BottomBorder = c.LeftBorder = c.RightBorder = line
    cell(0, 5).setString("align")
    hori = ["LEFT", "CENTER", "RIGHT"]
    for i, h in enumerate(hori):
        c = cell(i + 1, 5)
        c.setString(h.lower())
        c.HoriJustify = uno.Enum("com.sun.star.table.CellHoriJustify", h)
    for i, v in enumerate([1, 2]):  # CellVertJustify2 TOP, CENTER
        c = cell(len(hori) + 1 + i, 5)
        c.setString(["top", "middle"][i])
        c.VertJustify = v
    wrap = cell(6, 5)
    wrap.setString("a long text that wraps inside its cell")
    wrap.IsTextWrapped = True
    rot = cell(7, 5)
    rot.setString("rotated")
    rot.RotateAngle = 4500
    ind = cell(1, 6)
    ind.setString("indented")
    ind.HoriJustify = uno.Enum("com.sun.star.table.CellHoriJustify", "LEFT")
    ind.ParaIndent = 706  # two indent steps (3.53 mm each)
    cell(0, 6).setString("indent")

    cell(0, 7).setString("formats")
    codes = [
        (1234.5, "#,##0.00"),
        (0.125, "0.00%"),
        (-1234.0, '_(* #,##0_);_(* \\(#,##0\\);_(* "-"??_);_(@_)'),
        (-12.5, "[RED]0.00;[BLUE]-0.00"),
        (46000.0, "YYYY-MM-DD"),
        (46000.0, "MMM D, YYYY"),
        (123456.0, "0.00E+00"),
        (0.75, "# ?/?"),
    ]
    for i, (value, code) in enumerate(codes):
        c = cell(1, 7 + i)
        c.setValue(value)
        c.NumberFormat = fmt(code)
        cell(2, 7 + i).setValue(value * 3)  # a plain neighbour
    st.getCellRangeByName("E8:G9").merge(True)
    cols = st.Columns
    cols.getByIndex(1).Width = 3000
    cols.getByIndex(6).Width = 5000
    cols.getByIndex(8).IsVisible = False
    rows = st.Rows
    rows.getByIndex(5).Height = 1500
    rows.getByIndex(15).IsVisible = False
    cell(8, 0).setString("hidden column")
    cell(0, 15).setString("hidden row")

    # Content Brick keeps but doesn't edit (STYLE-5): a conditional format, a validation
    # list, and a hyperlink.
    cond = st.getCellRangeByName("C8:C15")
    entries = cond.ConditionalFormat
    entries.addNew((prop("Operator", uno.Enum("com.sun.star.sheet.ConditionOperator", "GREATER")),
                    prop("Formula1", "1000"), prop("StyleName", "Bad")))
    cond.ConditionalFormat = entries
    val_range = st.getCellRangeByName("D8:D15")
    validation = val_range.Validation
    validation.Type = uno.Enum("com.sun.star.sheet.ValidationType", "LIST")
    validation.setFormula1('"yes";"no"')
    val_range.Validation = validation
    link = cell(0, 17)
    field = doc.createInstance("com.sun.star.text.TextField.URL")
    field.URL, field.Representation = "https://example.com/", "example"
    link.insertTextContent(link.getText().createTextCursor(), field, False)


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
        for r in range(min(end.EndRow + 1, MAX_ROWS or end.EndRow + 1)):
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


def hex_color(c, unset):
    return unset if c in (-1, 0xFFFFFFFF) else f"{c & 0xFFFFFF:06X}"


def side(line):
    if line.LineWidth == 0 and line.OuterLineWidth == 0:
        return "-"
    return f"{line.LineStyle}:{line.LineWidth}:{line.Color & 0xFFFFFF:06X}"


def styles(doc):
    lines = []
    formats = doc.NumberFormats
    for sheet in doc.Sheets:
        lines.append(f"== {sheet.Name}")
        cursor = sheet.createCursor()
        cursor.gotoEndOfUsedArea(False)
        end = cursor.RangeAddress
        merges = []
        for r in range(min(end.EndRow + 1, MAX_ROWS or end.EndRow + 1)):
            for c in range(end.EndColumn + 1):
                cell = sheet.getCellByPosition(c, r)
                ref = f"{col_name(c)}{r + 1}"
                font = [cell.CharFontName, f"{cell.CharHeight:g}",
                        "b" if cell.CharWeight > 100 else "",
                        "i" if cell.CharPosture.value == "ITALIC" else "",
                        f"u{cell.CharUnderline}", f"s{cell.CharStrikeout}",
                        hex_color(cell.CharColor, "auto")]
                fill = hex_color(cell.CellBackColor, "none")
                borders = ",".join(side(getattr(cell, f"{s}Border"))
                                   for s in ("Top", "Bottom", "Left", "Right"))
                align = [cell.HoriJustify.value, str(cell.VertJustify),
                         "wrap" if cell.IsTextWrapped else "", str(cell.RotateAngle),
                         str(cell.ParaIndent)]
                code = formats.getByKey(cell.NumberFormat).FormatString
                lines.append(f"{ref}\tfont={'|'.join(font)}\tfill={fill}\tborder={borders}"
                             f"\talign={'|'.join(align)}\tfmt={code}")
                if cell.IsMerged:
                    area = sheet.createCursorByRange(cell)
                    area.collapseToMergedArea()
                    a = area.RangeAddress
                    merges.append(f"merge {col_name(a.StartColumn)}{a.StartRow + 1}:"
                                  f"{col_name(a.EndColumn)}{a.EndRow + 1}")
        lines.extend(dict.fromkeys(merges))
        for c in range(end.EndColumn + 1):
            col = sheet.Columns.getByIndex(c)
            lines.append(f"col {col_name(c)} {col.Width} {int(not col.IsVisible)}")
        for r in range(min(end.EndRow + 1, MAX_ROWS or end.EndRow + 1)):
            row = sheet.Rows.getByIndex(r)
            lines.append(f"row {r + 1} {row.Height} {int(not row.IsVisible)}")
    return "\n".join(lines) + "\n"


# LibreOffice writes every colour as RGB; Excel writes theme colours with tints. These
# fills and that font colour become theme references, so the fixture covers them (the dump
# shows how LibreOffice resolves them against the file's theme).
THEME_COLORS = [
    ('<fgColor rgb="FFDDEBF7"/>', '<fgColor theme="4" tint="0.79998168889431442"/>'),
    ('<fgColor rgb="FFFCE4D6"/>', '<fgColor theme="4" tint="-0.249977111117893"/>'),
    ('<fgColor rgb="FFE2EFDA"/>', '<fgColor theme="9" tint="0.39997558519241921"/>'),
    ('<color rgb="FFFFFFFF"/>', '<color theme="0"/>'),
]


def theme_colors(path):
    with zipfile.ZipFile(path) as z:
        parts = [(info, z.read(info.filename)) for info in z.infolist()]
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for info, data in parts:
            if info.filename == "xl/styles.xml":
                text = data.decode()
                for old, new in THEME_COLORS:
                    if old not in text:
                        sys.exit(f"styles.xml has no {old}")
                    text = text.replace(old, new, 1)
                data = text.encode()
            z.writestr(info, data)


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("fixture", "dump", "styles"):
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
            theme_colors(path)
            print(f"fixture -> {path}")
        else:
            doc = desktop.loadComponentFromURL(path.as_uri(), "_blank", 0, hidden)
            doc.calculateAll()
            sys.stdout.write(dump(doc) if sys.argv[1] == "dump" else styles(doc))
        doc.close(True)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass


if __name__ == "__main__":
    main()
