#!/usr/bin/env python3
"""LibreOffice side of SLATE-3's tests: build the fixture document, and dump a .docx as
LibreOffice reads it.

Usage: python3 scripts/lo_docx.py fixture OUT   (store the fixture document at OUT)
       python3 scripts/lo_docx.py dump FILE     (print FILE's paragraphs and tables)
       python3 scripts/lo_docx.py spacing OUT   (write the spacing document at OUT:
                                                 apps/slate/crates/docx/tests/data/
                                                 spacing.docx; no LibreOffice needed)
       python3 scripts/lo_docx.py borders OUT   (write the borders document at OUT:
                                                 apps/slate/crates/docx/tests/data/
                                                 borders.docx; no LibreOffice needed)
       python3 scripts/lo_docx.py lists OUT     (write the lists document at OUT:
                                                 apps/slate/crates/docx/tests/data/
                                                 lists.docx; no LibreOffice needed)

`dump` prints the body in document order, one line per item, tab-separated:
  para<TAB>STYLE<TAB>LIST<TAB>LABEL<TAB>ALIGN<TAB>BEFORE<TAB>AFTER<TAB>LINE<TAB>LEFT<TAB>RIGHT<TAB>FIRST<TAB>CONTEXT<TAB>TOP<TAB>BOTTOM<TAB>BORDERLEFT<TAB>BORDERRIGHT<TAB>FILL<TAB>NUMLEFT<TAB>NUMFIRST<TAB>MARKERJC<TAB>SUFFIX<TAB>MARKERFONT<TAB>TEXT
                                     a paragraph: its paragraph style's display name, its
                                     list level (0-based) or `-` outside lists, its list
                                     marker (its label, `"2."`, or a bullet's character as
                                     its level has it, a symbol font's unmapped), its
                                     alignment (`start`, `center`, `end`, `justify`), then
                                     its space before and after, line spacing
                                     (`auto:PERCENT`, `exact:TWIPS`, or `atLeast:TWIPS`),
                                     left, right, and first-line indent (in twips, rounded;
                                     a hanging indent is negative), whether space between
                                     paragraphs of its style is left out (`1` or `0`), its
                                     top, bottom, left, and right borders (`-` for none, or
                                     `STYLE:WIDTH:DISTANCE:RRGGBB`: LibreOffice's line style
                                     in lower case, `solid`, `double`, ..., the line's whole
                                     width and its distance from the text in twips,
                                     rounded), its fill (`-` or `RRGGBB`), its numbering
                                     level's left and first-line indents (twips), its
                                     marker's alignment (`left`, `center`, `right`) and
                                     what follows it (`tab`, `space`, `nothing`), and a
                                     bullet's font (`-` for none, and for a number), each
                                     `-` outside lists, and its text; label and text as
                                     JSON strings
  run<TAB>FLAGS<TAB>FONT<TAB>SIZE<TAB>COLOR<TAB>TEXT
                                     the paragraph's text in runs of one look: FLAGS is
                                     `b`, `i`, `u`, `s` (bold, italic, underline,
                                     strikethrough) or `-`, FONT the font's name, SIZE in
                                     points, COLOR `RRGGBB` or `auto`; adjacent portions
                                     with the same look are joined
  table / row / cell / end           a table, its rows and cells (in cell-name order), and
                                     its end; a cell's paragraphs and nested tables follow
                                     its `cell` line
Spacing, indents, borders, and fill are the paragraph's own (ADR 0009), which LibreOffice
resolves through its style, its parents, and the document defaults, without its numbering
level's indents (NUMLEFT and NUMFIRST). LibreOffice reads
Word's line between paragraphs bordered together as the top border of each but the first,
the first's bottom border left off.
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
    """A paragraph's alignment, spacing, line spacing, indents, and contextual spacing."""
    adjust = p.ParaAdjust
    align = ALIGN[adjust] if 0 <= adjust < len(ALIGN) else f"adjust{adjust}"
    line = p.ParaLineSpacing
    mode = LINE.get(line.Mode, f"mode{line.Mode}")
    value = str(line.Height) if line.Mode == 0 else twips(line.Height)
    return [align, twips(p.ParaTopMargin), twips(p.ParaBottomMargin), f"{mode}:{value}",
            twips(p.ParaLeftMargin), twips(p.ParaRightMargin), twips(p.ParaFirstLineIndent),
            "1" if p.ParaContextMargin else "0"]


# com.sun.star.table.BorderLineStyle by value, as the dump names them.
BORDER_STYLES = ["solid", "dotted", "dashed", "double", "thinthick_smallgap",
                 "thinthick_mediumgap", "thinthick_largegap", "thickthin_smallgap",
                 "thickthin_mediumgap", "thickthin_largegap", "embossed", "engraved", "outset",
                 "inset", "fine_dashed", "double_thin", "dash_dot", "dash_dot_dot"]


def color(c):
    return f"{c & 0xFFFFFF:06X}"


def para_frame(p):
    """A paragraph's top, bottom, left, and right borders (`-`, or the line's style, its
    whole width and its distance from the text in twips, and its colour) and its fill
    (`-` or its colour)."""
    out = []
    for side in ("Top", "Bottom", "Left", "Right"):
        line = p.getPropertyValue(f"{side}Border")
        if line.LineWidth == 0 or line.LineStyle == 0x7FFF:
            out.append("-")
            continue
        style = (BORDER_STYLES[line.LineStyle] if line.LineStyle < len(BORDER_STYLES)
                 else f"style{line.LineStyle}")
        distance = p.getPropertyValue(f"{side}BorderDistance")
        out.append(f"{style}:{twips(line.LineWidth)}:{twips(distance)}:{color(line.Color)}")
    solid = p.FillStyle.value == "SOLID"
    out.append(color(p.FillColor) if solid else "-")
    return out


# com.sun.star.style.NumberingType's CHAR_SPECIAL: a bullet.
BULLET = 6
# com.sun.star.text.HoriOrientation (a level's Adjust) and LabelFollowedBy, as the dump
# names them.
MARKER_JC = {1: "right", 2: "center", 3: "left"}
MARKER_SUFFIX = {0: "tab", 1: "space", 2: "nothing"}


def level_props(p):
    """The properties of the paragraph's numbering level."""
    return {x.Name: x.Value for x in p.NumberingRules.getByIndex(p.NumberingLevel)}


def marker_label(p):
    """A list item's marker: its list label, or a bullet's character (LibreOffice gives no
    label for a bullet), as the level has it (a symbol font's characters unmapped)."""
    props = level_props(p)
    if props.get("NumberingType") == BULLET:
        return props.get("BulletChar") or ""
    return p.getPropertyValue("ListLabelString") or ""


def list_level(p, listed):
    """A list item's level: its left and first-line indents (twips), its marker's alignment
    and suffix, and a bullet's font (`-` for none or a number); `-` for each outside lists."""
    if not listed:
        return ["-"] * 5
    props = level_props(p)
    font = props.get("BulletFontName") if props.get("NumberingType") == BULLET else None
    return [twips(props.get("IndentAt", 0)), twips(props.get("FirstLineIndent", 0)),
            MARKER_JC.get(props.get("Adjust"), f"adjust{props.get('Adjust')}"),
            MARKER_SUFFIX.get(props.get("LabelFollowedBy"), "?"), font or "-"]


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
        label = marker_label(p) if listed else ""
        out.append("\t".join(["para", display_name(p), level, json.dumps(label, ensure_ascii=False),
                              *para_look(p), *para_frame(p), *list_level(p, listed),
                              json.dumps(p.getString(), ensure_ascii=False)]))
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


# The spacing document (SLATE-26): WordprocessingML written by hand, so it holds what Word
# and Google Docs write and LibreOffice doesn't: `start` and `end` indents, automatic
# spacing, contextual spacing, a line without its rule.
W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
DOCX_CT = "application/vnd.openxmlformats-officedocument.wordprocessingml"
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

SPACING_STYLES = (
    f'<w:styles xmlns:w="{W_NS}">'
    "<w:docDefaults>"
    '<w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/>'
    '<w:sz w:val="22"/></w:rPr></w:rPrDefault>'
    '<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/>'
    "</w:pPr></w:pPrDefault>"
    "</w:docDefaults>"
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/>'
    "</w:style>"
    '<w:style w:type="paragraph" w:styleId="Spaced"><w:name w:val="Spaced"/>'
    '<w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="240" w:after="120" w:line="360" '
    'w:lineRule="auto"/><w:ind w:left="720" w:right="360" w:hanging="360"/></w:pPr></w:style>'
    '<w:style w:type="paragraph" w:styleId="Tight"><w:name w:val="Tight"/>'
    '<w:basedOn w:val="Spaced"/><w:pPr><w:contextualSpacing/><w:ind w:right="0"/></w:pPr>'
    "</w:style>"
    "</w:styles>"
)

SPACING_NUMBERING = (
    f'<w:numbering xmlns:w="{W_NS}">'
    '<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/>'
    '<w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/>'
    '<w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>'
    '<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>'
    "</w:numbering>"
)

# (style, the paragraph's own pPr, text) for each paragraph, in order.
SPACING_PARAGRAPHS = [
    ("Normal", "", "Normal: the document defaults' 8 pt after and 1.08 lines."),
    ("Normal", '<w:spacing w:before="120" w:after="0"/>', "Own space: 6 pt before, none after."),
    ("Normal", '<w:spacing w:line="480" w:lineRule="auto"/>', "Own double line spacing."),
    ("Normal", '<w:spacing w:line="300" w:lineRule="exact"/>', "Own exact 15 pt lines."),
    ("Normal", '<w:spacing w:line="400" w:lineRule="atLeast"/>', "Own lines at least 20 pt."),
    ("Normal", '<w:spacing w:line="360"/>', "A line without its rule: 1.5 lines."),
    ("Normal", '<w:ind w:left="1440" w:firstLine="360"/>', "Own left 1 in, first line 0.25 in."),
    ("Normal", '<w:ind w:start="720" w:end="1080" w:hanging="540"/>',
     "Start, end, and hanging: left 0.5 in, right 0.75 in, hanging 0.375 in."),
    ("Normal", '<w:ind w:left="-360" w:right="-720"/>', "Negative indents into the margins."),
    ("Spaced", "", "The Spaced style's own look, nothing direct."),
    ("Spaced", '<w:spacing w:before="0"/><w:ind w:firstLine="0"/>',
     "Spaced, with no space before and no hanging line of its own; left and right kept."),
    ("Spaced", '<w:spacing w:after="480" w:line="240" w:lineRule="auto"/><w:ind w:left="0"/>',
     "Spaced, overridden: 24 pt after, single lines, no left indent."),
    ("Tight", "", "Tight, contextual: no space between it"),
    ("Tight", "", "and the next Tight paragraph."),
    ("Tight", "<w:contextualSpacing w:val=\"0\"/>", "Tight with contextual spacing turned off."),
    ("Normal", "<w:contextualSpacing/>", "Normal with its own contextual spacing."),
    ("Normal", '<w:spacing w:before="100" w:beforeAutospacing="1" w:after="100" '
     'w:afterAutospacing="1"/>', "Automatic spacing: 14 pt before and after."),
    ("Normal", '<w:spacing w:after="0"/><w:ind w:left="1080" w:hanging="360"/>',
     "A list item with its own indents and spacing.", "1"),
    ("Normal", "", "A list item with its level's indents.", "1"),
]


def spacing_ppr(style, own, num=None):
    """A `w:pPr` with the style, the list, and `own` (in schema order) in it."""
    pstyle = f'<w:pStyle w:val="{style}"/>' if style != "Normal" else ""
    numpr = f'<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num}"/></w:numPr>' if num else ""
    inner = pstyle + numpr + own
    return f"<w:pPr>{inner}</w:pPr>" if inner else ""


def spacing_paragraph(style, own, text, num=None):
    return f"<w:p>{spacing_ppr(style, own, num)}<w:r><w:t>{text}</w:t></w:r></w:p>"


def spacing_document():
    body = "".join(spacing_paragraph(*p) for p in SPACING_PARAGRAPHS)
    cell = spacing_paragraph("Normal", '<w:spacing w:after="0"/><w:ind w:left="200"/>',
                             "A cell paragraph with its own indent and no space after.")
    body += ("<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"pct\"/></w:tblPr>"
             "<w:tblGrid><w:gridCol w:w=\"9360\"/></w:tblGrid>"
             f"<w:tr><w:tc><w:tcPr><w:tcW w:w=\"9360\" w:type=\"dxa\"/></w:tcPr>{cell}</w:tc></w:tr>"
             "</w:tbl>")
    body += spacing_paragraph("Normal", "", "The end.")
    sect = ('<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" '
            'w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>')
    return f'<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}"><w:body>{body}{sect}</w:body></w:document>'


def write_package(path, document, styles, numbering=None):
    """A .docx at `path` of `document`, `styles`, and `numbering` (when given): byte for
    byte the same on every run."""
    import zipfile
    numbering_type = (f'<Override PartName="/word/numbering.xml" '
                      f'ContentType="{DOCX_CT}.numbering+xml"/>' if numbering else "")
    numbering_rel = (f'<Relationship Id="rId2" Type="{R_NS}/numbering" Target="numbering.xml"/>'
                     if numbering else "")
    parts = [
        ("[Content_Types].xml",
         '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
         '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
         '<Default Extension="xml" ContentType="application/xml"/>'
         f'<Override PartName="/word/document.xml" ContentType="{DOCX_CT}.document.main+xml"/>'
         f'<Override PartName="/word/styles.xml" ContentType="{DOCX_CT}.styles+xml"/>'
         f"{numbering_type}"
         "</Types>"),
        ("_rels/.rels",
         f'<Relationships xmlns="{PKG_NS}">'
         f'<Relationship Id="rId1" Type="{R_NS}/officeDocument" Target="word/document.xml"/>'
         "</Relationships>"),
        ("word/_rels/document.xml.rels",
         f'<Relationships xmlns="{PKG_NS}">'
         f'<Relationship Id="rId1" Type="{R_NS}/styles" Target="styles.xml"/>'
         f"{numbering_rel}"
         "</Relationships>"),
        ("word/document.xml", document),
        ("word/styles.xml", styles),
    ]
    if numbering:
        parts.append(("word/numbering.xml", numbering))
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, xml in parts:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, DECL + xml)


def write_spacing(path):
    """The spacing document: paragraphs with their own spacing, line spacing, and indents
    (Word's `start`, `end`, `hanging`, and negative ones), a style's overridden, contextual
    and automatic spacing, list items, and a table cell's paragraph. Byte for byte the same
    on every run."""
    write_package(path, spacing_document(), SPACING_STYLES, SPACING_NUMBERING)
    print(f"spacing -> {path}")


# The borders document (SLATE-27): paragraph borders and shading, a paragraph's own and its
# style's, written by hand as Word and Google Docs write them.
BORDERS_STYLES = (
    f'<w:styles xmlns:w="{W_NS}">'
    "<w:docDefaults>"
    '<w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/>'
    '<w:sz w:val="22"/></w:rPr></w:rPrDefault>'
    '<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/>'
    "</w:pPr></w:pPrDefault>"
    "</w:docDefaults>"
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/>'
    "</w:style>"
    '<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/>'
    '<w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="60"/></w:pPr>'
    '<w:rPr><w:sz w:val="52"/></w:rPr></w:style>'
    '<w:style w:type="paragraph" w:styleId="Boxed"><w:name w:val="Boxed"/>'
    '<w:basedOn w:val="Normal"/><w:pPr><w:pBdr>'
    '<w:top w:val="single" w:sz="8" w:space="4" w:color="2F5496"/>'
    '<w:left w:val="single" w:sz="8" w:space="4" w:color="2F5496"/>'
    '<w:bottom w:val="single" w:sz="8" w:space="4" w:color="2F5496"/>'
    '<w:right w:val="single" w:sz="8" w:space="4" w:color="2F5496"/>'
    '</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="DEEAF6"/></w:pPr></w:style>'
    '<w:style w:type="paragraph" w:styleId="BoxedDouble"><w:name w:val="Boxed Double"/>'
    '<w:basedOn w:val="Boxed"/><w:pPr><w:pBdr>'
    '<w:bottom w:val="double" w:sz="4" w:space="1" w:color="C00000"/>'
    "</w:pBdr></w:pPr></w:style>"
    "</w:styles>"
)


def border(side, val, sz=4, space=0, color="auto"):
    return f'<w:{side} w:val="{val}" w:sz="{sz}" w:space="{space}" w:color="{color}"/>'


def box(val="single", sz=4, space=4, color="auto", between=""):
    """A `w:pBdr` with the same line on all four sides, and `between` (a side's XML)."""
    sides = "".join(border(s, val, sz, space, color) for s in ["top", "left", "bottom", "right"])
    return f"<w:pBdr>{sides}{between}</w:pBdr>"


# Two paragraphs bordered together, with a red line between them.
GROUP = box(sz=8, space=1, color="2F5496", between=border("between", "single", 4, 1, "C00000"))


# The résumé's title rule, exactly as Google Docs writes it.
RESUME_RULE = ('<w:pBdr><w:bottom w:color="000000" w:space="1" w:sz="4" w:val="single"/>'
               "</w:pBdr>")
# Google Docs' paragraphs without borders or shading.
NO_BORDERS = ('<w:pBdr><w:top w:space="0" w:sz="0" w:val="nil"/>'
              '<w:left w:space="0" w:sz="0" w:val="nil"/>'
              '<w:bottom w:space="0" w:sz="0" w:val="nil"/>'
              '<w:right w:space="0" w:sz="0" w:val="nil"/>'
              '<w:between w:space="0" w:sz="0" w:val="nil"/></w:pBdr>'
              '<w:shd w:fill="auto" w:val="clear"/>')

# (style, the paragraph's own pPr, text) for each paragraph, in order.
BORDERS_PARAGRAPHS = [
    ("Title", RESUME_RULE, "Jane Q. Writer"),
    ("Normal", NO_BORDERS, "Nil borders and automatic shading draw nothing."),
    ("Normal", box(), "Boxed on all four sides, 4 pt from the text."),
    ("Normal", "<w:pBdr>" + border("bottom", "double", 6, 2, "C00000") + "</w:pBdr>",
     "A double rule below, red."),
    ("Normal", "<w:pBdr>" + border("top", "dotted", 12, 1, "00B050") + "</w:pBdr>",
     "A dotted rule above, green."),
    ("Normal", "<w:pBdr>" + border("left", "dashed", 8, 8, "7030A0")
     + border("right", "dashed", 8, 8, "7030A0") + '</w:pBdr><w:ind w:left="720" w:right="720"/>',
     "Dashed left and right, outside the indents."),
    ("Normal", GROUP, "The first of two boxed paragraphs, with a red line between."),
    ("Normal", GROUP, "The second of them: one box around both."),
    ("Normal", '<w:shd w:val="clear" w:color="auto" w:fill="FFE599"/>', "Shaded yellow."),
    ("Normal", '<w:shd w:val="solid" w:color="BDD7EE" w:fill="auto"/>',
     "Solid shading in the pattern's colour."),
    ("Normal", '<w:shd w:val="solid" w:color="auto" w:fill="auto"/>',
     "Solid shading in an automatic colour."),
    ("Normal", '<w:shd w:val="pct25" w:color="auto" w:fill="FFFFFF"/>',
     "A 25% pattern."),
    ("Boxed", "", "The Boxed style's box and fill."),
    ("Boxed", "", "A second Boxed paragraph: one box around both, no line between."),
    ("Boxed", '<w:pBdr><w:top w:val="nil"/>' + border("bottom", "none", 0, 0)
     + '</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="auto"/>',
     "Boxed, with its own nil top, no bottom, and no fill."),
    ("BoxedDouble", "", "Boxed Double: the Boxed style's box with a double bottom."),
    ("Normal", "<w:pBdr>" + border("bottom", "thick", 12, 1) + "</w:pBdr>", "A thick rule."),
    ("Normal", "<w:pBdr>" + border("bottom", "dotDash", 8, 1) + "</w:pBdr>",
     "A dot-dash rule draws dashed."),
    ("Normal", "<w:pBdr>" + border("bottom", "triple", 4, 1) + "</w:pBdr>",
     "A triple rule draws double."),
    ("Normal", "<w:pBdr>" + border("bottom", "wave", 6, 1) + "</w:pBdr>",
     "A wavy rule draws single."),
]


def borders_document():
    body = "".join(spacing_paragraph(*p) for p in BORDERS_PARAGRAPHS)
    cell = spacing_paragraph(
        "Normal", "<w:pBdr>" + border("bottom", "single", 8, 1, "70AD47")
        + '</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="E2EFDA"/>',
        "A cell paragraph with a rule below and a green fill.")
    body += ("<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"pct\"/></w:tblPr>"
             "<w:tblGrid><w:gridCol w:w=\"9360\"/></w:tblGrid>"
             f"<w:tr><w:tc><w:tcPr><w:tcW w:w=\"9360\" w:type=\"dxa\"/></w:tcPr>{cell}</w:tc></w:tr>"
             "</w:tbl>")
    body += spacing_paragraph("Normal", "", "The end.")
    sect = ('<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" '
            'w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>')
    return f'<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}"><w:body>{body}{sect}</w:body></w:document>'


def write_borders(path):
    """The borders document: the résumé's title rule, Google Docs' nil borders and automatic
    shading, a box, double, dotted, dashed, and thick lines, two boxed paragraphs with a line
    between, shading fills, a style's box and fill with nil sides over it, a style based on
    it, the styles drawn as their nearest, and a table cell's paragraph. Byte for byte the
    same on every run."""
    write_package(path, borders_document(), BORDERS_STYLES)
    print(f"borders -> {path}")


# The lists document (SLATE-28): list levels written by hand as Word and Google Docs write
# them, each paragraph's marker and indents from its level.
LISTS_STYLES = (
    f'<w:styles xmlns:w="{W_NS}">'
    "<w:docDefaults>"
    '<w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Sans" w:hAnsi="Liberation Sans"/>'
    '<w:sz w:val="22"/></w:rPr></w:rPrDefault>'
    '<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="240" w:lineRule="auto"/>'
    "</w:pPr></w:pPrDefault>"
    "</w:docDefaults>"
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/>'
    "</w:style>"
    '<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/>'
    '<w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0"/><w:ind w:left="720"/>'
    "<w:contextualSpacing/></w:pPr></w:style>"
    "</w:styles>"
)


def lvl(ilvl, fmt, text, left, hanging=None, first=None, jc="left", suff=None, rpr="",
        start=True):
    """A `w:lvl`: its format, text, alignment, suffix, indents, and run properties."""
    ind = f'w:left="{left}"' + (f' w:hanging="{hanging}"' if hanging is not None else "") + (
        f' w:firstLine="{first}"' if first is not None else "")
    return (f'<w:lvl w:ilvl="{ilvl}">' + ('<w:start w:val="1"/>' if start else "")
            + f'<w:numFmt w:val="{fmt}"/>' + (f'<w:suff w:val="{suff}"/>' if suff else "")
            + f'<w:lvlText w:val="{text}"/><w:lvlJc w:val="{jc}"/>'
            f"<w:pPr><w:ind {ind}/></w:pPr>" + (f"<w:rPr>{rpr}</w:rPr>" if rpr else "")
            + "</w:lvl>")


def abstract(n, *levels):
    return f'<w:abstractNum w:abstractNumId="{n}">{"".join(levels)}</w:abstractNum>'


# Google Docs' bullet levels, exactly as it writes them (no w:start).
GOOGLE_LEVEL = ('<w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="\u25cf"/>'
                '<w:lvlJc w:val="left"/><w:pPr><w:ind w:left="360" w:hanging="360"/></w:pPr>'
                '<w:rPr><w:u w:val="none"/></w:rPr></w:lvl>')
SYMBOL = '<w:rFonts w:ascii="Symbol" w:hAnsi="Symbol" w:hint="default"/>'
WINGDINGS = '<w:rFonts w:ascii="Wingdings" w:hAnsi="Wingdings" w:hint="default"/>'
COURIER = '<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" w:hint="default"/>'

LISTS_NUMBERING = (
    f'<w:numbering xmlns:w="{W_NS}">'
    + abstract(0, GOOGLE_LEVEL,
               lvl(1, "bullet", "\u25cb", 1080, 360, rpr='<w:u w:val="none"/>', start=False),
               lvl(2, "bullet", "\u25a0", 1800, 360, rpr='<w:u w:val="none"/>', start=False))
    + abstract(1, lvl(0, "bullet", "\uf0b7", 720, 360, rpr=SYMBOL),
               lvl(1, "bullet", "o", 1440, 360, rpr=COURIER),
               lvl(2, "bullet", "\uf0a7", 2160, 360, rpr=WINGDINGS),
               lvl(3, "bullet", "\uf0d8", 2880, 360, rpr=WINGDINGS))
    + abstract(2, lvl(0, "decimal", "%1.", 720, 360),
               lvl(1, "decimal", "%1.%2.", 1440, 720),
               lvl(2, "lowerLetter", "(%3)", 2160, 360))
    + abstract(3, lvl(0, "upperRoman", "%1.", 1080, 360, jc="right"))
    + abstract(4, lvl(0, "decimal", "%1)", 720, 360, suff="space"),
               lvl(1, "lowerRoman", "%2.", 1440, 360, suff="nothing"),
               lvl(2, "decimal", "", 2160, 360),
               lvl(3, "decimal", "Step %4:", 720, first=0))
    + "".join(f'<w:num w:numId="{n + 1}"><w:abstractNumId w:val="{n}"/></w:num>'
              for n in range(5))
    + "</w:numbering>"
)

# (the paragraph's own pPr after its w:numPr, numId, ilvl, text) for each list item.
LISTS_PARAGRAPHS = [
    ("", 1, 0, "Google bullet: a filled circle at the margin, text at 0.25 in."),
    ("", 1, 1, "Google's second level: a hollow circle."),
    ("", 1, 2, "Google's third level: a square."),
    ("", 1, 0, "Google bullet again, long enough to wrap onto a second line, which starts at "
     "the level's left indent, under the first line's text and not under the bullet."),
    ('<w:rPr><w:color w:val="C00000"/><w:sz w:val="28"/></w:rPr>', 1, 0,
     "A bullet whose paragraph mark is red 14 pt: so is the bullet."),
    ("", 2, 0, "Word's Symbol bullet."),
    ("", 2, 1, "Word's Courier New o."),
    ("", 2, 2, "Word's Wingdings square."),
    ("", 2, 3, "Word's Wingdings arrowhead."),
    ("", 3, 0, "Decimal one."),
    ("", 3, 1, "Decimal one point one, a hanging indent of half an inch."),
    ("", 3, 2, "Lower letter in parentheses."),
    ("", 3, 2, "Lower letter, the second."),
    ("", 3, 0, "Decimal two."),
    ('<w:ind w:left="1440"/>', 3, 0, "Decimal three, with its own left indent over its level's."),
    ('<w:ind w:left="1080" w:hanging="720"/>', 3, 0,
     "Decimal four, with its own left and hanging indents."),
    ("", 4, 0, "Upper roman one, right-justified."),
    ("", 4, 0, "Upper roman two."),
    ("", 4, 0, "Upper roman three."),
    ("", 5, 0, "A number followed by a space."),
    ("", 5, 1, "A roman number followed by nothing."),
    ("", 5, 2, "A level whose text is empty: no marker."),
    ("", 5, 3, "A first-line indent: the marker at the indent, the text after a tab."),
]


def lists_paragraph(own, num, ilvl, text, style="Normal"):
    pstyle = f'<w:pStyle w:val="{style}"/>' if style != "Normal" else ""
    numpr = f'<w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num}"/></w:numPr>'
    return f"<w:p><w:pPr>{pstyle}{numpr}{own}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"


def lists_document():
    body = spacing_paragraph("Normal", "", "Lists as Word and Google Docs write them.")
    body += "".join(lists_paragraph(*p) for p in LISTS_PARAGRAPHS)
    body += lists_paragraph("", 3, 0, "Decimal five, in List Paragraph.", "ListParagraph")
    body += spacing_paragraph("Normal", "", "The end.")
    sect = ('<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" '
            'w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>')
    return f'<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}"><w:body>{body}{sect}</w:body></w:document>'


def write_lists(path):
    """The lists document: Google Docs' bullet levels exactly as it writes them, Word's
    Symbol, Courier New, and Wingdings bullets, decimal, multi-level, lettered, and
    right-justified roman numbers, a space and no suffix, a level without text, a first-line
    indent, own indents over a level's, and a style's indent under a level's. Byte for byte
    the same on every run."""
    write_package(path, lists_document(), LISTS_STYLES, LISTS_NUMBERING)
    print(f"lists -> {path}")


def main():
    commands = ("fixture", "dump", "spacing", "borders", "lists")
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    path = Path(sys.argv[2]).resolve()
    writers = {"spacing": write_spacing, "borders": write_borders, "lists": write_lists}
    if sys.argv[1] in writers:
        writers[sys.argv[1]](path)
        return
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
