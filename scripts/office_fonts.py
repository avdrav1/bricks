#!/usr/bin/env python3
"""A document and a deck in the fonts Office files use most, for the look checks.

    python3 scripts/office_fonts.py docx OUT.docx [--stand-ins]
    python3 scripts/office_fonts.py pptx OUT.pptx [--stand-ins]

`docx` writes a Word document: a Cambria heading, then one long paragraph each in
Calibri, Arial, Times New Roman, Courier New, and Cambria, on a Letter page with 1-inch
margins (6.5 inches of text). `pptx` writes a deck, Frame's built-in blank one with its
title slide holding the same title and paragraphs at 20 pt, left-aligned in a box 11.5
inches wide. Where those fonts aren't installed, fontconfig's metric aliases draw them
in Carlito, Liberation Sans, Liberation Serif, Liberation Mono, and Caladea, with the
same widths, so every line breaks in the same place. `--stand-ins` names those stand-ins
themselves, so the file draws in them wherever they're installed or carried (the apps
carry them on macOS and Windows).

The look checks compare their screenshots across systems (MAC-6's `check_mac_look.sh`,
WIN-4's `check_win_open.ps1`).
"""

import sys
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
BLANK_DECK = ROOT / "apps/frame/crates/pptx/data/blank.pptx"
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
OOXML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
BODY = ("Sphinx of black quartz, judge my vow. The quick brown fox jumps over the lazy dog "
        "while 0123456789 fly past; this paragraph runs long enough to wrap where Word "
        "wraps it when the font's widths match Word's own.")
STAND_INS = {
    "Calibri": "Carlito",
    "Cambria": "Caladea",
    "Arial": "Liberation Sans",
    "Times New Roman": "Liberation Serif",
    "Courier New": "Liberation Mono",
}
TITLE = "Cambria heading, drawn in Caladea"


def texts():
    """The paragraphs after the title, as (Office font, text)."""
    yield "Calibri", "Calibri body, drawn in Carlito. " + BODY
    for font in ("Arial", "Times New Roman", "Courier New"):
        yield font, f"{font}: {BODY}"
    yield "Cambria", "Cambria body. " + BODY


def chooser(stand_ins):
    return (lambda f: STAND_INS[f]) if stand_ins else (lambda f: f)


def docx(path, stand_ins=False):
    """Write the Word document to `path`, in the Office fonts or their stand-ins."""
    font = chooser(stand_ins)

    def para(name, size, text):
        return (f'<w:p><w:r><w:rPr><w:rFonts w:ascii="{name}" w:hAnsi="{name}"/>'
                f'<w:sz w:val="{size}"/></w:rPr><w:t xml:space="preserve">{escape(text)}</w:t></w:r></w:p>')

    paras = [para(font("Cambria"), 40, TITLE)]
    paras += [para(font(f), 22, text) for f, text in texts()]
    sect = ('<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:left="1440" w:right="1440" '
            'w:top="1440" w:bottom="1440"/></w:sectPr>')
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", HEAD + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
                   '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
                   '<Default Extension="xml" ContentType="application/xml"/>'
                   f'<Override PartName="/word/document.xml" ContentType="{OOXML}.document.main+xml"/></Types>')
        z.writestr("_rels/.rels", HEAD + f'<Relationships xmlns="{PKG}"><Relationship Id="rId1" '
                   f'Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>')
        z.writestr("word/document.xml", HEAD + f'<w:document xmlns:w="{W}"><w:body>'
                   + "".join(paras) + sect + "</w:body></w:document>")


def pptx(path, stand_ins=False):
    """Write the deck to `path`: Frame's blank deck with its title slide filled."""
    font = chooser(stand_ins)

    def run(name, size, text):
        return (f'<a:r><a:rPr lang="en-US" sz="{size}"><a:latin typeface="{name}"/></a:rPr>'
                f'<a:t>{escape(text)}</a:t></a:r>')

    def shape(xml, name, y, cy, paras):
        """`xml` with the placeholder `name` placed at `y` (EMU, `cy` tall, 11.5 inches
        wide) holding `paras`, which don't shrink to fit."""
        start = xml.index(f'name="{name}"')
        sp_pr = xml.index("<p:spPr></p:spPr>", start)
        end = xml.index("</p:txBody>", sp_pr) + len("</p:txBody>")
        body = (f'<p:spPr><a:xfrm><a:off x="838200" y="{y}"/><a:ext cx="10515600" cy="{cy}"/></a:xfrm></p:spPr>'
                f'<p:txBody><a:bodyPr><a:noAutofit/></a:bodyPr><a:lstStyle/>{paras}</p:txBody>')
        return xml[:sp_pr] + body + xml[end:]

    with zipfile.ZipFile(BLANK_DECK) as src, zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as out:
        for item in src.infolist():
            data = src.read(item)
            if item.filename == "ppt/slides/slide1.xml":
                xml = data.decode()
                xml = shape(xml, "Title 1", 300000, 900000,
                            f'<a:p><a:pPr algn="l"/>{run(font("Cambria"), 3200, TITLE)}</a:p>')
                xml = shape(xml, "Subtitle 2", 1300000, 5300000, "".join(
                    f'<a:p><a:pPr algn="l"/>{run(font(f), 2000, text)}</a:p>' for f, text in texts()))
                data = xml.encode()
            out.writestr(item, data)


def main(argv):
    if len(argv) in (3, 4) and argv[1] in ("docx", "pptx") and argv[3:] in ([], ["--stand-ins"]):
        (docx if argv[1] == "docx" else pptx)(argv[2], stand_ins=len(argv) == 4)
    else:
        raise SystemExit("usage: office_fonts.py docx|pptx OUT [--stand-ins]")


if __name__ == "__main__":
    main(sys.argv)
