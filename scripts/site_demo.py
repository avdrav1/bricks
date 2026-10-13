#!/usr/bin/env python3
"""Demo files for the cronkite.cc screenshots (scripts/site_shots.sh), built by LibreOffice.

Usage: python3 scripts/site_demo.py OUTDIR

Writes one small story, a bike shop's quarter, in each app's formats:
- budget.xlsx: three sheets; the Budget sheet has a title, a styled header, currency
  formats, banded rows, =SUM totals per row and column, and a growth column in percent;
- report.docx: a title, headings, body text, a bulleted list, a table, and a chart picture;
- notes.md: Markdown with headings, emphasis, a task list, a quote, a table, and code;
- review.pptx: a deck from LibreOffice's Midnightblue template: a title slide, bullets, and
  a chart picture;
- chart.png: the chart (also dropped into the report and the deck).

Needs LibreOffice with Python UNO and ImageMagick (`magick`).
"""
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.awt import Point, Size
from com.sun.star.beans import PropertyValue
from com.sun.star.table import BorderLine2, CellRangeAddress

PORT = int(os.environ.get("LO_PORT", "2190"))
PARAGRAPH_BREAK = 0  # com.sun.star.text.ControlCharacter
AUTOLAYOUT_TITLE, AUTOLAYOUT_ENUM, AUTOLAYOUT_ONLY_TITLE = 0, 1, 19
TEMPLATE = Path("/usr/lib/libreoffice/share/template/common/presnt/Midnightblue.otp")

QUARTERS = ["Q1", "Q2", "Q3", "Q4"]
BUDGET = [
    ("Bike sales", [182400, 241900, 268300, 196200]),
    ("Repairs and servicing", [38600, 52100, 57400, 44900]),
    ("Parts and accessories", [41200, 49800, 55300, 47100]),
    ("Rentals", [9800, 21400, 26900, 11300]),
    ("Group rides and events", [2400, 6800, 8100, 3200]),
    ("Online store", [27300, 31600, 35200, 48700]),
]


def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p


def connect():
    profile = tempfile.mkdtemp(prefix="lo-demo-")
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


def chart(path):
    """A bar chart of revenue per quarter, as a PNG."""
    totals = [sum(v[i] for _, v in BUDGET) for i in range(4)]
    w, h, left, base, top = 1200, 700, 140, 600, 90
    bar, gap = 180, 70
    draw = ["-fill", "#14201a", "-pointsize", "34", "-annotate", "+140+60", "Revenue by quarter"]
    colours = ["#7fb8a4", "#3f9c7d", "#0b7a5a", "#5aa88c"]
    for i, t in enumerate(totals):
        x = left + 40 + i * (bar + gap)
        y = base - int((base - top - 30) * t / max(totals))
        draw += ["-fill", colours[i], "-draw", f"rectangle {x},{y} {x + bar},{base}",
                 "-fill", "#14201a", "-pointsize", "26", "-annotate", f"+{x + 30}+{y - 14}",
                 f"${t / 1000:.0f}k", "-pointsize", "28", "-annotate", f"+{x + 66}+{base + 44}",
                 QUARTERS[i]]
    draw += ["-stroke", "#9aaba1", "-strokewidth", "2", "-draw", f"line {left},{base} {w - 60},{base}"]
    subprocess.run(["magick", "-size", f"{w}x{h}", "xc:white", "-font", "Liberation-Sans",
                    *draw, str(path)], check=True)


def border(colour, width):
    return BorderLine2(Color=colour, InnerLineWidth=0, OuterLineWidth=width, LineDistance=0,
                       LineStyle=0, LineWidth=width)


def budget(desktop, path):
    doc = desktop.loadComponentFromURL("private:factory/scalc", "_blank", 0, (prop("Hidden", True),))
    sheets = doc.Sheets
    sheets.insertNewByName("Headcount", 1)
    sheets.insertNewByName("Assumptions", 2)
    s = sheets.getByIndex(0)
    s.Name = "Budget"
    formats = doc.NumberFormats
    locale = uno.createUnoStruct("com.sun.star.lang.Locale")

    def fmt(code):
        key = formats.queryKey(code, locale, False)
        return key if key != -1 else formats.addNew(code, locale)

    money, pct = fmt('"$"#,##0'), fmt("0.0%")
    s.getCellByPosition(0, 0).String = "Northwind Bikes: 2026 revenue budget"
    title = s.getCellRangeByName("A1:G1")
    title.merge(True)
    title.CharHeight, title.CharWeight = 16.0, 150.0
    s.getCellByPosition(0, 1).String = "All figures in US dollars"
    s.getCellByPosition(0, 1).CharColor = 0x5B6B62
    s.getCellByPosition(0, 1).CharPosture = uno.Enum("com.sun.star.awt.FontSlant", "ITALIC")
    head = ["Category", *QUARTERS, "Total", "Q4 vs Q1"]
    for c, text in enumerate(head):
        s.getCellByPosition(c, 3).String = text
    header = s.getCellRangeByName("A4:G4")
    header.CellBackColor, header.CharColor, header.CharWeight = 0x0B7A5A, 0xFFFFFF, 150.0
    first, last = 5, 4 + len(BUDGET)
    for i, (name, values) in enumerate(BUDGET):
        r = first + i
        s.getCellByPosition(0, r - 1).String = name
        for c, v in enumerate(values):
            s.getCellByPosition(c + 1, r - 1).Value = v
        s.getCellByPosition(5, r - 1).Formula = f"=SUM(B{r}:E{r})"
        s.getCellByPosition(6, r - 1).Formula = f"=E{r}/B{r}-1"
        if i % 2:
            s.getCellRangeByName(f"A{r}:G{r}").CellBackColor = 0xE3F2EC
    total = last + 1
    s.getCellByPosition(0, total - 1).String = "Total"
    for c in "BCDEF":
        s.getCellRangeByName(f"{c}{total}").Formula = f"=SUM({c}{first}:{c}{last})"
    s.getCellRangeByName(f"G{total}").Formula = f"=E{total}/B{total}-1"
    totals = s.getCellRangeByName(f"A{total}:G{total}")
    totals.CharWeight, totals.TopBorder = 150.0, border(0x14201A, 35)
    s.getCellRangeByName(f"B{first}:F{total}").NumberFormat = money
    s.getCellRangeByName(f"G{first}:G{total}").NumberFormat = pct
    for c in "BCDEFG":
        s.getCellRangeByName(f"{c}4:{c}{total}").HoriJustify = uno.Enum(
            "com.sun.star.table.CellHoriJustify", "RIGHT")
    s.getCellByPosition(0, total + 1).String = "Online store grew every quarter; Q4 is its best so far."
    s.getCellByPosition(0, total + 1).CharColor = 0x5B6B62
    widths = [6200, 2600, 2600, 2600, 2600, 2900, 2600]
    for c, w in enumerate(widths):
        s.Columns.getByIndex(c).Width = w

    h = sheets.getByName("Headcount")
    rows = [("Team", *QUARTERS), ("Sales floor", 6, 8, 9, 7), ("Workshop", 4, 5, 6, 5),
            ("Online", 2, 2, 3, 3), ("Events", 1, 2, 2, 1)]
    for r, row in enumerate(rows):
        for c, v in enumerate(row):
            cell = h.getCellByPosition(c, r)
            if isinstance(v, str):
                cell.String = v
            else:
                cell.Value = v
    h.getCellRangeByName("A1:E1").CharWeight = 150.0
    h.getCellByPosition(0, 5).String = "Total"
    for c in "BCDE":
        h.getCellRangeByName(f"{c}6").Formula = f"=SUM({c}2:{c}5)"
    h.Columns.getByIndex(0).Width = 4000

    a = sheets.getByName("Assumptions")
    for r, (k, v) in enumerate([("Average bike price", 1450), ("Service visits per week", 85),
                                ("Rental days per summer week", 120)]):
        a.getCellByPosition(0, r).String = k
        a.getCellByPosition(1, r).Value = v
    a.Columns.getByIndex(0).Width = 6500
    doc.CurrentController.setActiveSheet(s)
    doc.storeToURL(path.as_uri(), (prop("FilterName", "Calc MS Excel 2007 XML"),))
    doc.close(True)


def report(desktop, smgr, path, picture):
    doc = desktop.loadComponentFromURL("private:factory/swriter", "_blank", 0, (prop("Hidden", True),))
    text = doc.Text
    cursor = text.createTextCursor()
    first = [True]

    def para(style, s=""):
        if not first[0]:
            text.insertControlCharacter(cursor, PARAGRAPH_BREAK, False)
        first[0] = False
        cursor.ParaStyleName = style
        cursor.NumberingStyleName = ""
        if s:
            text.insertString(cursor, s, False)

    para("Title", "Northwind Bikes: third quarter review")
    para("Subtitle", "Prepared for the October planning meeting")
    para("Heading 1", "Summary")
    para("Text body", "The third quarter was our strongest yet. Revenue reached $451,200, up 12% on "
         "the second quarter, as summer rentals and the new workshop bays came into full use. "
         "Every line of the business grew, and the online store kept its steady climb.")
    para("Text body", "This review covers what went well, what we'd change, and where we plan to "
                      "spend in the fourth quarter.")
    para("Heading 1", "What went well")
    for item in ["Rentals nearly tripled since the first quarter, with weekend bookings full from June.",
                 "The two new workshop bays cut the wait for a service to three days.",
                 "Group rides brought in 140 new customers, a third of whom bought a bike."]:
        para("List", item)
        cursor.NumberingStyleName = "List 1"
    para("Heading 1", "Revenue by quarter")
    para("Text body", "Sales follow the season: they climb through spring, peak in summer, and ease "
                      "in the autumn. The online store is the exception.")
    para("Text body")
    graphic = smgr.createInstance("com.sun.star.graphic.GraphicProvider").queryGraphic(
        (prop("URL", picture.as_uri()),))
    image = doc.createInstance("com.sun.star.text.TextGraphicObject")
    image.Graphic = graphic
    image.AnchorType = uno.Enum("com.sun.star.text.TextContentAnchorType", "AS_CHARACTER")
    image.Size = Size(15000, 8750)
    text.insertTextContent(cursor, image, False)
    para("Heading 2", "By line of business")
    para("Text body")
    table = doc.createInstance("com.sun.star.text.TextTable")
    table.initialize(len(BUDGET) + 1, 3)
    text.insertTextContent(cursor, table, False)
    table.getCellByName("A1").String = "Line"
    table.getCellByName("B1").String = "Q3"
    table.getCellByName("C1").String = "Change on Q2"
    for i, (name, values) in enumerate(BUDGET):
        table.getCellByName(f"A{i + 2}").String = name
        table.getCellByName(f"B{i + 2}").String = f"${values[2]:,}"
        table.getCellByName(f"C{i + 2}").String = f"{values[2] / values[1] - 1:+.0%}"
    para("Heading 1", "Next quarter")
    para("Text body", "Winter is quiet on the shop floor, so the fourth quarter goes to the online "
                      "store and the workshop: a faster checkout, servicing booked online, and a "
                      "winter tune-up offer for everyone who bought a bike this year.")
    doc.storeToURL(path.as_uri(), (prop("FilterName", "MS Word 2007 XML"),))
    doc.close(True)


def placeholder(page, kind):
    for i in range(page.getCount()):
        shape = page.getByIndex(i)
        if shape.ShapeType.endswith(kind):
            return shape
    raise RuntimeError(f"no {kind} on the page")


def deck(desktop, smgr, path, picture):
    doc = desktop.loadComponentFromURL(TEMPLATE.as_uri(), "_blank", 0,
                                       (prop("Hidden", True), prop("AsTemplate", True)))
    pages = doc.DrawPages
    # The template's second slide is on its content master (a title bar over a plain page).
    content = pages.getByIndex(min(1, pages.getCount() - 1)).MasterPage
    while pages.getCount() > 1:
        pages.remove(pages.getByIndex(1))
    p = pages.getByIndex(0)
    p.Layout = AUTOLAYOUT_TITLE
    placeholder(p, "TitleTextShape").String = "Third quarter review"
    placeholder(p, "SubtitleShape").String = "Northwind Bikes · October 2026"

    def slide(i, layout, title):
        page = pages.insertNewByIndex(i)
        page.MasterPage = content
        page.Layout = layout
        placeholder(page, "TitleTextShape").String = title
        return page

    def bullets(i, title, lines):
        page = slide(i, AUTOLAYOUT_ENUM, title)
        placeholder(page, "OutlinerShape").String = "\n".join(lines)

    bullets(0, "Highlights", ["Revenue of $451,200, up 12% on Q2",
                              "Rentals nearly tripled since Q1",
                              "Service wait down to three days",
                              "140 new customers from group rides"])
    page = slide(1, AUTOLAYOUT_ONLY_TITLE, "Revenue by quarter")
    graphic = smgr.createInstance("com.sun.star.graphic.GraphicProvider").queryGraphic(
        (prop("URL", picture.as_uri()),))
    title = placeholder(page, "TitleTextShape")
    top = title.Position.Y + title.Size.Height + 1000
    h = page.Height - top - 1100
    w = h * 12 // 7
    image = doc.createInstance("com.sun.star.drawing.GraphicObjectShape")
    page.add(image)
    image.Graphic = graphic
    image.Position, image.Size = Point((page.Width - w) // 2, top), Size(w, h)
    bullets(2, "Next quarter", ["Faster checkout in the online store",
                                "Book a service online",
                                "Winter tune-up for this year's buyers"])
    doc.storeToURL(path.as_uri(), (prop("FilterName", "Impress MS PowerPoint 2007 XML"),))
    doc.close(True)


NOTES = """# Shop notes: week 41

Quiet week on the floor, **busy week online**. Three things to settle before the planning
meeting on *Thursday*.

## To do

- [x] Order winter tyres for the rental fleet
- [x] Send the Q3 review to the team
- [ ] Price the winter tune-up
- [ ] Pick a booking tool for the workshop

## Ideas for the tune-up

> Keep it simple: one price, one hour, and a free coffee while you wait.

| Option | Price | Takes |
| --- | --- | --- |
| Basic check | $39 | 30 min |
| Full tune-up | $89 | 1 hour |
| Tune-up and wash | $109 | 90 min |

## Booking links

The shop page will link to the booking tool like this:

```html
<a href="https://book.northwind.bike/service">Book a service</a>
```

See [last year's notes](notes-2025.md) for what worked.
"""


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    picture = out / "chart.png"
    chart(picture)
    (out / "notes.md").write_text(NOTES)
    ctx = connect()
    smgr = ctx.ServiceManager
    desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    try:
        budget(desktop, out / "budget.xlsx")
        report(desktop, smgr, out / "report.docx", picture)
        deck(desktop, smgr, out / "review.pptx", picture)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass
    print(f"demo files -> {out}")


if __name__ == "__main__":
    main()
