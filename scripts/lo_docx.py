#!/usr/bin/env python3
"""LibreOffice side of SLATE-3's tests: build the fixture document, and dump a .docx as
LibreOffice reads it.

Usage: python3 scripts/lo_docx.py fixture OUT   (store the fixture document at OUT)
       python3 scripts/lo_docx.py dump FILE     (print FILE's paragraphs and tables)

`dump` prints the body in document order, one line per item, tab-separated:
  para<TAB>STYLE<TAB>LIST<TAB>LABEL<TAB>ALIGN<TAB>BEFORE<TAB>AFTER<TAB>LINE<TAB>LEFT<TAB>FIRST<TAB>TEXT
                                     a paragraph: its paragraph style's display name, its
                                     list level (0-based) or `-` outside lists, its list
                                     label (`"2."`; LibreOffice gives `""` for bullets), its
                                     alignment (`start`, `center`, `end`, `justify`), then
                                     its paragraph style's space before and after, line
                                     spacing (`auto:PERCENT`, `exact:TWIPS`, or
                                     `atLeast:TWIPS`), and left and first-line indent (in
                                     twips, rounded; a hanging indent is negative), and its
                                     text; label and text as JSON strings
  run<TAB>FLAGS<TAB>FONT<TAB>SIZE<TAB>COLOR<TAB>TEXT
                                     the paragraph's text in runs of one look: FLAGS is
                                     `b`, `i`, `u`, `s` (bold, italic, underline,
                                     strikethrough) or `-`, FONT the font's name, SIZE in
                                     points, COLOR `RRGGBB` or `auto`; adjacent portions
                                     with the same look are joined
  table / row / cell / end           a table, its rows and cells (in cell-name order), and
                                     its end; a cell's paragraphs and nested tables follow
                                     its `cell` line
Spacing and indents are the paragraph style's (inherited through its parents and the
document defaults), not the paragraph's own: Slate's model holds a paragraph's style and
its alignment.
The fixture (apps/slate/crates/docx/tests/data/fixture.docx) has a title, headings 1-3,
runs that are bold, italic, underlined, struck, sized, coloured, and in the Strong and
Emphasis character styles (also inside a heading), centred, right-aligned, and justified
paragraphs, a quote, bullet and numbered lists with nested levels, a tab and a line break,
a hyperlink, an inline picture with alt text, a table with a bold header, a merged cell, a
two-paragraph cell and a nested table, and a page break.
Needs LibreOffice with Python UNO. LO_PORT picks the port (default 2160).
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import uno
from com.sun.star.beans import PropertyValue

PORT = int(os.environ.get("LO_PORT", "2160"))
ROOT = Path(__file__).resolve().parent.parent
PICTURE = ROOT / "apps/slate/crates/document-model/tests/data/picture.png"
PARAGRAPH_BREAK, LINE_BREAK = 0, 1  # com.sun.star.text.ControlCharacter


def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p


def connect():
    profile = tempfile.mkdtemp(prefix="lo-docx-")
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


def first_style(family, names):
    """The first of `names` that exists in the style family (names changed across
    LibreOffice versions)."""
    for n in names:
        if family.hasByName(n):
            return n
    sys.exit(f"none of the styles {names} exists")


def adjust(name):
    return uno.Enum("com.sun.star.style.ParagraphAdjust", name)


class Writer:
    """Appends paragraphs at the end of a text, each with its own style and runs."""

    def __init__(self, doc, text):
        self.doc, self.text = doc, text
        self.cursor = text.createTextCursor()
        self.cursor.gotoEnd(False)
        self.first = True

    def para(self, runs, style="Standard", **props):
        c = self.cursor
        if not self.first:
            self.text.insertControlCharacter(c, PARAGRAPH_BREAK, False)
        self.first = False
        c.ParaStyleName = style
        for k in ("ParaAdjust", "NumberingLevel", "BreakType"):
            c.setPropertyToDefault(k)
        c.NumberingStyleName = ""
        for k, v in props.items():
            c.setPropertyValue(k, v)
        if isinstance(runs, str):
            runs = [(runs, {})]
        for text, char in runs:
            for k, v in char.items():
                c.setPropertyValue(k, v)
            if text == "\n":
                self.text.insertControlCharacter(c, LINE_BREAK, False)
            else:
                self.text.insertString(c, text, False)
            for k in char:
                if k == "HyperLinkURL":  # not reset by setPropertyToDefault
                    c.HyperLinkURL = ""
                else:
                    c.setPropertyToDefault(k)
        return c

    def content(self, obj):
        self.text.insertTextContent(self.cursor, obj, False)


def build(ctx, doc):
    para_styles = doc.StyleFamilies.getByName("ParagraphStyles")
    char_styles = doc.StyleFamilies.getByName("CharacterStyles")
    numbering = doc.StyleFamilies.getByName("NumberingStyles")
    quote = first_style(para_styles, ["Quotations", "Quote"])
    strong = first_style(char_styles, ["Strong", "Strong Emphasis"])
    emphasis = first_style(char_styles, ["Emphasis"])
    bullets = first_style(numbering, ["Bullet •", "List Bullet", "List 1"])
    numbers = first_style(numbering, ["Numbering 123", "Numbering 1"])

    w = Writer(doc, doc.Text)
    w.para("Slate DOCX fixture", "Title")
    w.para("First heading", "Heading 1")
    w.para([
        ("Plain, ", {}), ("bold", {"CharWeight": 150.0}), (", ", {}),
        ("italic", {"CharPosture": uno.Enum("com.sun.star.awt.FontSlant", "ITALIC")}),
        (", ", {}), ("underlined", {"CharUnderline": 1}), (", ", {}),
        ("struck", {"CharStrikeout": 1}), (", ", {}),
        ("big", {"CharHeight": 18.0}), (", ", {}),
        ("red", {"CharColor": 0xC00000}), (", and ", {}),
        ("bold italic underlined", {
            "CharWeight": 150.0,
            "CharPosture": uno.Enum("com.sun.star.awt.FontSlant", "ITALIC"),
            "CharUnderline": 1}),
        (".", {}),
    ])
    w.para([("Character styles: ", {}), ("strong", {"CharStyleName": strong}),
            (" and ", {}), ("emphasis", {"CharStyleName": emphasis}), (".", {})])
    w.para("Second heading", "Heading 2")
    w.para([("A heading with ", {}), ("strong", {"CharStyleName": strong}),
            (" text and ", {}), ("not bold", {"CharWeight": 100.0}), (" text", {})],
           "Heading 3")
    w.para("A centred paragraph.", ParaAdjust=adjust("CENTER"))
    w.para("A right-aligned paragraph.", ParaAdjust=adjust("RIGHT"))
    w.para("A justified paragraph that is long enough to wrap onto a second line in a "
           "normal page width, so that the justification shows on its first line.",
           ParaAdjust=adjust("BLOCK"))
    w.para("A quotation from somebody wise.", quote)
    w.para("First bullet", NumberingStyleName=bullets, NumberingLevel=0)
    w.para("Nested bullet", NumberingStyleName=bullets, NumberingLevel=1)
    w.para("Second bullet", NumberingStyleName=bullets, NumberingLevel=0)
    w.para("First number", NumberingStyleName=numbers, NumberingLevel=0)
    w.para("Second number", NumberingStyleName=numbers, NumberingLevel=0)
    w.para("Nested number", NumberingStyleName=numbers, NumberingLevel=1)
    w.para("Third number", NumberingStyleName=numbers, NumberingLevel=0)
    w.para([("Before a tab\tafter it", {}), ("\n", {}), ("after a line break", {})])
    w.para([("Visit ", {}), ("the Slate page", {"HyperLinkURL": "https://example.com/slate"}),
            (" for more.", {})])

    # An inline picture with alt text, in its own paragraph.
    w.para("")
    provider = ctx.ServiceManager.createInstanceWithContext(
        "com.sun.star.graphic.GraphicProvider", ctx)
    graphic = provider.queryGraphic((prop("URL", PICTURE.as_uri()),))
    picture = doc.createInstance("com.sun.star.text.TextGraphicObject")
    picture.Graphic = graphic
    picture.AnchorType = uno.Enum("com.sun.star.text.TextContentAnchorType", "AS_CHARACTER")
    picture.Width, picture.Height = 3175, 2117  # 120 x 80 px at 96 dpi, in 1/100 mm
    picture.Description = "A blue picture"
    w.content(picture)
    w.para("Caption under the picture.")

    # A table: bold header, a merged cell, a two-paragraph cell, and a nested table.
    w.para("The table follows.")
    table = doc.createInstance("com.sun.star.text.TextTable")
    table.initialize(3, 3)
    w.content(table)
    for col, title in zip("ABC", ["Name", "Count", "Note"]):
        cell = table.getCellByName(f"{col}1")
        c = cell.createTextCursor()
        c.CharWeight = 150.0
        cell.insertString(c, title, False)
    table.getCellByName("A2").setString("Apples")
    table.getCellByName("B2").setString("3")
    cell = table.getCellByName("C2")
    c = cell.createTextCursor()
    cell.insertString(c, "First paragraph in a cell", False)
    cell.insertControlCharacter(c, PARAGRAPH_BREAK, False)
    cell.insertString(c, "Second paragraph", False)
    table.getCellByName("A3").setString("Merged across two columns")
    merge = table.createCursorByCellName("A3")
    merge.goRight(1, True)
    merge.mergeRange()
    cell = table.getCellByName("B3")  # C3 before the merge
    c = cell.createTextCursor()
    cell.insertString(c, "Outer cell", False)
    inner = doc.createInstance("com.sun.star.text.TextTable")
    inner.initialize(2, 2)
    cell.insertTextContent(c, inner, False)
    for name, text in zip(["A1", "B1", "A2", "B2"], ["in 1", "in 2", "in 3", "in 4"]):
        inner.getCellByName(name).setString(text)

    w.para("After the table.")
    w.para("After a page break.",
           BreakType=uno.Enum("com.sun.star.style.BreakType", "PAGE_BEFORE"))
    w.para("Third heading", "Heading 1")
    w.para("The end.")


def flags(portion):
    f = ""
    if portion.CharWeight > 100:
        f += "b"
    if portion.CharPosture.value != "NONE":
        f += "i"
    if portion.CharUnderline != 0:
        f += "u"
    if portion.CharStrikeout != 0:
        f += "s"
    return f or "-"


def look(portion):
    """A portion's look: flags, font, size in points, and colour."""
    color = portion.CharColor
    color = "auto" if color < 0 else f"{color & 0xFFFFFF:06X}"
    size = f"{portion.CharHeight:g}"
    return (flags(portion), portion.CharFontName, size, color)


# com.sun.star.style.ParagraphAdjust as the dump names it: LEFT, RIGHT, BLOCK, CENTER,
# STRETCH, START, END.
ALIGN = ["start", "end", "justify", "center", "justify", "start", "end"]
# com.sun.star.style.LineSpacingMode: PROP, MINIMUM, LEADING, FIX.
LINE = {0: "auto", 1: "atLeast", 3: "exact"}


def twips(hundredths_mm):
    return str(round(hundredths_mm * 1440 / 2540))


def para_look(p):
    """A paragraph's alignment, and its style's spacing, line spacing, and indents."""
    adjust = p.ParaAdjust
    align = ALIGN[adjust] if 0 <= adjust < len(ALIGN) else f"adjust{adjust}"
    style = STYLES.getByName(p.ParaStyleName)
    line = style.ParaLineSpacing
    mode = LINE.get(line.Mode, f"mode{line.Mode}")
    value = str(line.Height) if line.Mode == 0 else twips(line.Height)
    return [align, twips(style.ParaTopMargin), twips(style.ParaBottomMargin),
            f"{mode}:{value}", twips(style.ParaLeftMargin), twips(style.ParaFirstLineIndent)]


def dump_text(text, out):
    paras = text.createEnumeration()
    while paras.hasMoreElements():
        p = paras.nextElement()
        if p.supportsService("com.sun.star.text.TextTable"):
            out.append("table")
            row = None
            for name in p.getCellNames():
                r = "".join(ch for ch in name if ch.isdigit())
                if r != row:
                    out.append("row")
                    row = r
                out.append("cell")
                dump_text(p.getCellByName(name), out)
            out.append("end")
            continue
        listed = p.NumberingIsNumber is True and bool(p.ListId)
        level = str(p.NumberingLevel) if listed else "-"
        label = p.getPropertyValue("ListLabelString") or "" if listed else ""
        out.append("\t".join(["para", display_name(p), level, json.dumps(label, ensure_ascii=False),
                              *para_look(p), json.dumps(p.getString(), ensure_ascii=False)]))
        runs = []
        for portion in portions(p):
            s = portion.getString()
            if not s:
                continue
            f = look(portion)
            if runs and runs[-1][0] == f:
                runs[-1][1] += s
            else:
                runs.append([f, s])
        for f, s in runs:
            out.append("\t".join(["run", *f, json.dumps(s, ensure_ascii=False)]))


def portions(container):
    """A paragraph's text portions; a content control's (Word's `w:sdt`) are its own."""
    e = container.createEnumeration()
    while e.hasMoreElements():
        portion = e.nextElement()
        if portion.TextPortionType == "ContentControl":
            yield from portions(portion.ContentControl)
        else:
            yield portion


STYLES = None


def display_name(p):
    """The paragraph style's display name (the UI name, as Word names it)."""
    style = STYLES.getByName(p.ParaStyleName)
    return style.DisplayName


def dump(doc):
    global STYLES
    STYLES = doc.StyleFamilies.getByName("ParagraphStyles")
    out = []
    dump_text(doc.Text, out)
    return "\n".join(out) + "\n"


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("fixture", "dump"):
        sys.exit(__doc__)
    path = Path(sys.argv[2]).resolve()
    ctx = connect()
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    try:
        hidden = (prop("Hidden", True),)
        if sys.argv[1] == "fixture":
            doc = desktop.loadComponentFromURL("private:factory/swriter", "_blank", 0, hidden)
            build(ctx, doc)
            path.parent.mkdir(parents=True, exist_ok=True)
            doc.storeToURL(path.as_uri(), (prop("FilterName", "MS Word 2007 XML"),))
            print(f"fixture -> {path}")
        else:
            doc = desktop.loadComponentFromURL(path.as_uri(), "_blank", 0, hidden)
            sys.stdout.write(dump(doc))
        doc.close(True)
    finally:
        try:
            desktop.terminate()
        except Exception:
            pass


if __name__ == "__main__":
    main()
