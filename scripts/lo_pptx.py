#!/usr/bin/env python3
"""LibreOffice side of Frame's PPTX tests (FRAME-1..3): build the fixture deck, and dump a
deck as LibreOffice reads it.

Usage: python3 scripts/lo_pptx.py fixture OUT   (store the fixture deck at OUT)
       python3 scripts/lo_pptx.py dump FILE     (print FILE's slides and shapes)
       python3 scripts/lo_pptx.py layouts FILE  (print FILE's layouts: `layout NAME` lines)

`dump` prints tab-separated lines. Lengths are EMU: LibreOffice's 1/100 mm times 360, so
they are exact to 360 EMU (LibreOffice rounds what it reads to 1/100 mm).
  size  W  H                          the slide size
  slide N  hidden=0|1                 a slide, in show order (N from 1)
  background  KIND                    none | solid RRGGBB | gradient | hatch | bitmap
  shape DEPTH KIND X Y W H ROT FLIPH FLIPV NAME
        a shape in z-order; DEPTH counts enclosing groups, KIND is LibreOffice's shape
        type without its module (CustomShape, TitleTextShape, GraphicObjectShape, ...).
        X Y W H is the unrotated frame (from the shape's transformation matrix: its
        centre minus half its size), ROT clockwise degrees as in PPTX.
  fill  none | solid RRGGBB ALPHA | gradient | hatch | bitmap     (ALPHA 0..100 opaque)
  line  none | solid RRGGBB WIDTH | dash RRGGBB WIDTH
  picture  MIME  WxH                  a picture's type and pixel size
  cell  ROW COL  FILL  T B L R        a table cell (FILL as `fill`; each border RRGGBB:WIDTH
                                      or -), then its text
  para  LEVEL  ALIGN  TEXT            a paragraph: outline level from 0, left | right |
                                      center | justify, text with \\n for line breaks
  run   SIZE B I U S COLOR FONT TEXT  a text run: points, bold, italic, underline and
                                      strikethrough (LibreOffice's enum values, 0 = none),
                                      RRGGBB or auto, font name
  notes  TEXT                         the slide's speaker notes

The fixture (apps/frame/crates/pptx/tests/data/fixture.pptx), written by Impress's
"Impress MS PowerPoint 2007 XML" filter and then patched where Impress can't write what
the tests need:
  1. Title slide: a title and a subtitle with bold, italic, underlined, struck, and
     coloured runs; notes.
  2. Title and content: bullets on three levels, a centred and a right-aligned paragraph.
     Its title's <a:xfrm> is removed, so it takes the layout's frame.
  3. Shapes: rectangles with solid fills and outlines, a rotated ellipse, a rounded
     rectangle flipped horizontally, a triangle, a line, a text box with mixed runs. Four
     fills/colours are patched to theme colours with tint, shade, lumMod/lumOff, alpha.
  4. A picture (a 40x30 PNG), a group of a rectangle and an ellipse, a 3x3 table with
     filled cells.
  5. Hidden: a title only slide.
Needs LibreOffice with Python UNO.
"""
import os
import re
import struct
import subprocess
import sys
import tempfile
import time
import zipfile
import zlib
from pathlib import Path

import uno
from com.sun.star.awt import Point, Size
from com.sun.star.beans import PropertyValue
from com.sun.star.table import BorderLine2

PORT = int(os.environ.get("LO_PORT", "2170"))  # Frame's range is 2170-2179
PARAGRAPH_BREAK = 0  # com.sun.star.text.ControlCharacter
LINE_BREAK = 1
ALIGN = {0: "left", 1: "right", 2: "justify", 3: "center"}
AUTOLAYOUT_TITLE, AUTOLAYOUT_ENUM, AUTOLAYOUT_ONLY_TITLE, AUTOLAYOUT_NONE = 0, 1, 19, 20

# Sentinel colours the fixture writes and patch() turns into theme colours.
PATCHES = {
    "123401": '<a:schemeClr val="accent2"><a:tint val="40000"/></a:schemeClr>',
    "123402": '<a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr>',
    "123403": '<a:schemeClr val="accent6"><a:lumMod val="60000"/><a:lumOff val="40000"/></a:schemeClr>',
    "123404": '<a:schemeClr val="accent5"><a:lumMod val="50000"/></a:schemeClr>',
    "123405": '<a:schemeClr val="accent4"><a:alpha val="50000"/></a:schemeClr>',
}


def prop(name, value):
    p = PropertyValue()
    p.Name, p.Value = name, value
    return p


def connect():
    profile = tempfile.mkdtemp(prefix="lo-pptx-")
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


def png(width, height):
    """A PNG of `width` x `height`: red left half, blue right half."""
    rows = b""
    for _ in range(height):
        rows += b"\0" + b"".join(
            (b"\xff\x00\x00" if x < width // 2 else b"\x00\x00\xff") for x in range(width))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def placeholder(page, kind):
    for i in range(page.getCount()):
        shape = page.getByIndex(i)
        if shape.ShapeType.endswith(kind):
            return shape
    raise RuntimeError(f"no {kind} on the page")


def write_runs(text, runs, cursor=None):
    """Append runs to `text`: each (string, {property: value}); "\\n" starts a paragraph,
    "\\v" breaks the line. A run's properties hold for that run only."""
    cursor = cursor or text.createTextCursorByRange(text.getEnd())
    defaults = {}
    for s, props in runs:
        if s == "\n":
            text.insertControlCharacter(cursor, PARAGRAPH_BREAK, False)
            continue
        if s == "\v":
            text.insertControlCharacter(cursor, LINE_BREAK, False)
            continue
        text.insertString(cursor, s, False)
        cursor.goLeft(len(s), True)
        for k in props:
            if k not in defaults:
                defaults[k] = cursor.getPropertyValue(k)
        for k, v in {**defaults, **props}.items():
            if k == "CharColor":
                cursor.CharComplexColor = None
            cursor.setPropertyValue(k, v)
        cursor.collapseToEnd()


def paragraphs(text, settings):
    """Set paragraph properties: settings[i] = {property: value} for paragraph i."""
    for i, para in enumerate(text.createEnumeration()):
        for k, v in settings.get(i, {}).items():
            para.setPropertyValue(k, v)


def shape(doc, page, kind, x, y, w, h, **props):
    s = doc.createInstance(f"com.sun.star.drawing.{kind}")
    page.add(s)
    s.Position, s.Size = Point(x, y), Size(w, h)
    # A new shape's colours are theme references: clear them, or FillColor is ignored.
    if "FillColor" in props:
        s.FillComplexColor = None
    if "LineColor" in props:
        s.LineComplexColor = None
    for k, v in props.items():
        s.setPropertyValue(k, v)
    return s


def custom(doc, page, geometry, x, y, w, h, mirror_x=False, **props):
    s = shape(doc, page, "CustomShape", x, y, w, h, **props)
    geo = [prop("Type", geometry)]
    if mirror_x:
        geo.append(prop("MirroredX", True))
    s.CustomShapeGeometry = tuple(geo)
    return s


def build(doc, smgr, picture_url):
    pages = doc.DrawPages
    bold, italic = {"CharWeight": 150.0}, {"CharPosture": uno.Enum("com.sun.star.awt.FontSlant", "ITALIC")}

    # 1: title slide.
    p1 = pages.getByIndex(0)
    p1.Layout = AUTOLAYOUT_TITLE
    write_runs(placeholder(p1, "TitleTextShape").Text, [("Frame fixture", {})])
    write_runs(placeholder(p1, "SubtitleShape").Text, [
        ("Bold", bold), (" and ", {}), ("italic", italic), (" and ", {}),
        ("underlined", {"CharUnderline": 1}), (", ", {}), ("struck", {"CharStrikeout": 1}),
        (", ", {}), ("red", {"CharColor": 0xC00000}), (" at 20 pt", {"CharHeight": 20.0})])
    notes = placeholder(p1.NotesPage, "NotesShape")
    notes.setString("Speaker notes for the title slide.")

    # 2: title and content with levels.
    p2 = pages.insertNewByIndex(0)
    p2.Layout = AUTOLAYOUT_ENUM
    write_runs(placeholder(p2, "TitleTextShape").Text, [("Levels and alignment", {})])
    body = placeholder(p2, "OutlinerShape").Text
    write_runs(body, [("First level", {}), ("\n", {}), ("Second level", {}), ("\n", {}),
                      ("Third level", {}), ("\n", {}), ("Back to first", {}), ("\n", {}),
                      ("Centred", {}), ("\n", {}), ("Right", {}), ("\v", {}), ("after a line break", {})])
    paragraphs(body, {1: {"NumberingLevel": 1}, 2: {"NumberingLevel": 2},
                      4: {"ParaAdjust": 3}, 5: {"ParaAdjust": 1}})
    placeholder(p2.NotesPage, "NotesShape").setString("Notes on levels.")

    # 3: shapes.
    p3 = pages.insertNewByIndex(1)
    p3.Layout = AUTOLAYOUT_ONLY_TITLE
    write_runs(placeholder(p3, "TitleTextShape").Text, [("Shapes", {})])
    custom(doc, p3, "rectangle", 1000, 4000, 5000, 3000, FillColor=0x4472C4, LineColor=0x203864,
           LineWidth=100)
    custom(doc, p3, "ellipse", 7000, 4000, 5000, 3000, FillColor=0x70AD47, LineStyle=uno.Enum(
        "com.sun.star.drawing.LineStyle", "NONE"), RotateAngle=3000)
    custom(doc, p3, "round-rectangle", 13000, 4000, 5000, 3000, mirror_x=True, FillColor=0x123401)
    custom(doc, p3, "isosceles-triangle", 19000, 4000, 5000, 3000, FillColor=0x123402)
    custom(doc, p3, "rectangle", 1000, 8500, 5000, 3000, FillColor=0x123403)
    custom(doc, p3, "rectangle", 7000, 8500, 5000, 3000, FillColor=0x123405)
    line = shape(doc, p3, "LineShape", 13000, 9000, 10000, 2000, LineColor=0xFF0000, LineWidth=50)
    line.RotateAngle = 0
    box = shape(doc, p3, "TextShape", 1000, 13000, 20000, 2000)
    write_runs(box.Text, [("Text box: ", {"CharHeight": 24.0}), ("bold blue", {**bold, "CharColor": 0x0070C0}),
                          (" and theme", {"CharColor": 0x123404}), (" in Liberation Serif", {"CharFontName": "Liberation Serif"})])

    # 4: picture, group, table.
    p4 = pages.insertNewByIndex(2)
    p4.Layout = AUTOLAYOUT_ONLY_TITLE
    write_runs(placeholder(p4, "TitleTextShape").Text, [("Picture, group, table", {})])
    graphic = smgr.createInstance("com.sun.star.graphic.GraphicProvider").queryGraphic(
        (prop("URL", picture_url),))
    shape(doc, p4, "GraphicObjectShape", 1000, 4000, 8000, 6000, Graphic=graphic)
    a = custom(doc, p4, "rectangle", 10000, 4000, 4000, 3000, FillColor=0xFFC000)
    b = custom(doc, p4, "ellipse", 12000, 6000, 4000, 3000, FillColor=0x7030A0)
    group = smgr.createInstance("com.sun.star.drawing.ShapeCollection")
    group.add(a)
    group.add(b)
    p4.group(group)
    table = shape(doc, p4, "TableShape", 1000, 11000, 15000, 4500)
    model = table.Model
    model.Rows.insertByIndex(0, 2)
    model.Columns.insertByIndex(0, 2)
    for i in range(3):
        model.Columns.getByIndex(i).Width = 5000
        model.Rows.getByIndex(i).Height = 1500
    for r in range(3):
        for c in range(3):
            cell = model.getCellByPosition(c, r)
            cell.setString(f"R{r + 1}C{c + 1}")
            if r != 1:
                cell.FillComplexColor = None
                cell.FillStyle = uno.Enum("com.sun.star.drawing.FillStyle", "SOLID")
                cell.FillColor = 0x305496 if r == 0 else 0xD9D9D9
    red = BorderLine2(Color=0xFF0000, InnerLineWidth=0, OuterLineWidth=35, LineDistance=0,
                      LineStyle=0, LineWidth=35)
    middle = model.getCellByPosition(1, 1)
    for side in ("TopBorder", "BottomBorder", "LeftBorder", "RightBorder"):
        middle.setPropertyValue(side, red)
    placeholder(p4.NotesPage, "NotesShape").setString("Notes on the picture slide.")

    # 5: hidden.
    p5 = pages.insertNewByIndex(3)
    p5.Layout = AUTOLAYOUT_ONLY_TITLE
    write_runs(placeholder(p5, "TitleTextShape").Text, [("Hidden slide", {})])
    p5.Visible = False


def patch(path):
    """Turn the sentinel colours into theme colours, and drop slide 2's title frame."""
    with zipfile.ZipFile(path) as z:
        parts = {name: z.read(name) for name in z.namelist()}
    found = set()
    for name, data in parts.items():
        if not re.fullmatch(r"ppt/slides/slide\d+\.xml", name):
            continue
        xml = data.decode()
        for sentinel, replacement in PATCHES.items():
            pattern = f'<a:srgbClr val="{sentinel}"/>'
            if pattern in xml:
                found.add(sentinel)
                xml = xml.replace(pattern, replacement)
        if "Levels and alignment" in xml:
            title = re.search(r'<p:sp>(?:(?!</p:sp>).)*?type="title".*?</p:sp>', xml, re.S)
            xfrm = re.search(r"<a:xfrm>.*?</a:xfrm>", title.group(0), re.S)
            xml = xml.replace(title.group(0), title.group(0).replace(xfrm.group(0), ""))
            found.add("xfrm")
        if "Picture, group, table" in xml:
            # Scale the group 1.5 times, move it, and rotate it 30 degrees: its children's
            # frames go through the group transform.
            group = re.search(r'<p:grpSpPr><a:xfrm><a:off x="\d+" y="\d+"/><a:ext cx="\d+" cy="\d+"/>', xml)
            xml = xml.replace(group.group(0), '<p:grpSpPr><a:xfrm rot="1800000"><a:off x="6120000" y="1440000"/>'
                                              '<a:ext cx="3240000" cy="2700000"/>')
            found.add("group")
        parts[name] = xml.encode()
    missing = (set(PATCHES) | {"xfrm", "group"}) - found
    if missing:
        sys.exit(f"fixture: nothing to patch for {sorted(missing)}")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in parts.items():
            z.writestr(name, data)


def hex_color(c):
    return "auto" if c in (-1, 0xFFFFFFFF) else f"{c & 0xFFFFFF:06X}"


def esc(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\r", "\\n").replace("\n", "\\n")


def emu(hmm):
    return round(hmm * 360)


def fill_of(obj):
    style = obj.FillStyle.value
    if style == "NONE":
        return "none"
    if style == "SOLID":
        return f"solid {hex_color(obj.FillColor)} {100 - obj.FillTransparence}"
    return style.lower()


def border_of(line):
    width = line.LineWidth or line.OuterLineWidth
    # LibreOffice reads <a:noFill/> borders as automatic-colour lines it doesn't draw.
    if width == 0 or line.Color in (-1, 0xFFFFFFFF):
        return "-"
    return f"{hex_color(line.Color)}:{emu(width)}"


def line_of(obj):
    style = obj.LineStyle.value
    if style == "NONE":
        return "none"
    return f"{style.lower()} {hex_color(obj.LineColor)} {emu(obj.LineWidth)}"


def frame_of(s):
    """The unrotated frame from the transformation matrix, and the clockwise rotation."""
    m = s.Transformation
    a, b, tx = m.Line1.Column1, m.Line1.Column2, m.Line1.Column3
    c, d, ty = m.Line2.Column1, m.Line2.Column2, m.Line2.Column3
    w, h = (a * a + c * c) ** 0.5, (b * b + d * d) ** 0.5
    cx, cy = tx + (a + b) / 2, ty + (c + d) / 2
    rot = (36000 - s.RotateAngle) % 36000 / 100 if hasattr(s, "RotateAngle") else 0
    return emu(cx - w / 2), emu(cy - h / 2), emu(w), emu(h), rot


def flips(s):
    x = y = 0
    if s.ShapeType == "com.sun.star.drawing.CustomShape":
        for p in s.CustomShapeGeometry:
            if p.Name == "MirroredX" and p.Value:
                x = 1
            if p.Name == "MirroredY" and p.Value:
                y = 1
    return x, y


def text_lines(text):
    out = []
    for para in text.createEnumeration():
        if not para.supportsService("com.sun.star.text.Paragraph"):
            continue
        level = para.NumberingLevel if isinstance(para.NumberingLevel, int) else 0
        level = max(level, 0)
        out.append(f"para\t{level}\t{ALIGN.get(para.ParaAdjust, para.ParaAdjust)}\t{esc(para.getString())}")
        for run in para.createEnumeration():
            if run.TextPortionType not in ("Text", "TextField") or not run.getString():
                continue
            bold = 1 if run.CharWeight >= 150 else 0
            ital = 0 if run.CharPosture.value == "NONE" else 1
            out.append(f"run\t{run.CharHeight:g}\t{bold}\t{ital}\t{run.CharUnderline}\t{run.CharStrikeout}"
                       f"\t{hex_color(run.CharColor)}\t{run.CharFontName}\t{esc(run.getString())}")
    return out


def dump_shape(s, depth, out):
    kind = s.ShapeType.rsplit(".", 1)[-1]
    x, y, w, h, rot = frame_of(s)
    fx, fy = flips(s)
    out.append(f"shape\t{depth}\t{kind}\t{x}\t{y}\t{w}\t{h}\t{rot:g}\t{fx}\t{fy}\t{esc(s.Name)}")
    if kind == "GroupShape":
        for i in range(s.getCount()):
            dump_shape(s.getByIndex(i), depth + 1, out)
        return
    info = s.getPropertySetInfo()
    if info.hasPropertyByName("FillStyle") and kind not in ("GraphicObjectShape", "TableShape"):
        out.append(f"fill\t{fill_of(s)}")
    if info.hasPropertyByName("LineStyle") and kind != "TableShape":
        out.append(f"line\t{line_of(s)}")
    if kind == "GraphicObjectShape" and s.Graphic is not None:
        px = s.Graphic.SizePixel
        out.append(f"picture\t{s.Graphic.MimeType}\t{px.Width}x{px.Height}")
    if kind == "TableShape":
        model = s.Model
        for r in range(model.Rows.getCount()):
            for c in range(model.Columns.getCount()):
                cell = model.getCellByPosition(c, r)
                borders = " ".join(border_of(cell.getPropertyValue(side)) for side in
                                   ("TopBorder", "BottomBorder", "LeftBorder", "RightBorder"))
                out.append(f"cell\t{r}\t{c}\t{fill_of(cell)}\t{borders}")
                out.extend(text_lines(cell.Text))
        return
    empty = info.hasPropertyByName("IsEmptyPresentationObject") and s.IsEmptyPresentationObject
    if not empty and s.supportsService("com.sun.star.drawing.Text"):
        lines = text_lines(s.Text)
        if any(line.startswith("run\t") for line in lines):
            out.extend(lines)


def background(page):
    bg = page.Background if page.getPropertySetInfo().hasPropertyByName("Background") else None
    if bg is None:
        bg = page.MasterPage.Background
    return "none" if bg is None else fill_of(bg)


def dump(doc):
    pages = doc.DrawPages
    first = pages.getByIndex(0)
    out = [f"size\t{emu(first.Width)}\t{emu(first.Height)}"]
    for n in range(pages.getCount()):
        page = pages.getByIndex(n)
        out.append(f"slide\t{n + 1}\thidden={0 if page.Visible else 1}")
        out.append(f"background\t{background(page)}")
        for i in range(page.getCount()):
            dump_shape(page.getByIndex(i), 0, out)
        notes = page.NotesPage
        for i in range(notes.getCount()):
            s = notes.getByIndex(i)
            if s.ShapeType.endswith("NotesShape") and s.getString():
                out.append(f"notes\t{esc(s.getString())}")
    return "\n".join(out) + "\n"


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("fixture", "dump", "layouts"):
        sys.exit(__doc__)
    path = Path(sys.argv[2]).resolve()
    ctx = connect()
    smgr = ctx.ServiceManager
    desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    try:
        hidden = (prop("Hidden", True),)
        if sys.argv[1] == "fixture":
            with tempfile.TemporaryDirectory() as tmp:
                picture = Path(tmp) / "picture.png"
                picture.write_bytes(png(40, 30))
                doc = desktop.loadComponentFromURL("private:factory/simpress", "_blank", 0, hidden)
                build(doc, smgr, picture.as_uri())
                path.parent.mkdir(parents=True, exist_ok=True)
                doc.storeToURL(path.as_uri(), (prop("FilterName", "Impress MS PowerPoint 2007 XML"),))
            patch(path)
            print(f"fixture -> {path}")
        elif sys.argv[1] == "layouts":
            # LibreOffice reads each of a deck's layouts as a master page of its name.
            doc = desktop.loadComponentFromURL(path.as_uri(), "_blank", 0, hidden)
            masters = doc.MasterPages
            for i in range(masters.getCount()):
                print(f"layout\t{masters.getByIndex(i).Name}")
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
