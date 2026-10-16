#!/usr/bin/env python3
"""Documents in the fonts Office documents use most, for the look checks.

    python3 scripts/office_fonts.py docx OUT.docx [--stand-ins]
    python3 scripts/office_fonts.py pptx IN.pptx OUT.pptx

`docx` writes a Word document: a Cambria heading, then one long paragraph each in
Calibri, Arial, Times New Roman, Courier New, and Cambria, on a Letter page with 1-inch
margins (6.5 inches of text). Where those fonts aren't installed, fontconfig's metric
aliases draw them in Carlito, Liberation Sans, Liberation Serif, Liberation Mono, and
Caladea, with the same widths, so every line breaks in the same place. `--stand-ins`
names those stand-ins themselves, so the document draws in them wherever they're
installed or carried (the apps carry them on macOS and Windows).

`pptx` copies a deck with each of those Office fonts renamed to its stand-in, in every
part (theme, masters, layouts, slides).

The look checks compare their screenshots across systems (MAC-6's `check_mac_look.sh`,
WIN-4's `check_win_open.ps1`).
"""

import re
import sys
import zipfile

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


def para(font, size, text):
    return (f'<w:p><w:r><w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}"/>'
            f'<w:sz w:val="{size}"/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p>')


def docx(path, stand_ins=False):
    """Write the Word document to `path`, in the Office fonts or their stand-ins."""
    font = (lambda f: STAND_INS[f]) if stand_ins else (lambda f: f)
    paras = [para(font("Cambria"), 40, "Cambria heading, drawn in Caladea"),
             para(font("Calibri"), 22, "Calibri body, drawn in Carlito. " + BODY)]
    paras += [para(font(f), 22, f"{f}: {BODY}") for f in ("Arial", "Times New Roman", "Courier New")]
    paras += [para(font("Cambria"), 22, "Cambria body. " + BODY)]
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


def pptx(source, path):
    """Copy the deck at `source` to `path` with every Office font renamed to its stand-in."""
    typeface = re.compile(rb'typeface="(%s)"' % b"|".join(re.escape(f.encode()) for f in STAND_INS))
    with zipfile.ZipFile(source) as src, zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as out:
        for item in src.infolist():
            data = src.read(item)
            if item.filename.endswith(".xml"):
                data = typeface.sub(lambda m: b'typeface="%s"' % STAND_INS[m.group(1).decode()].encode(), data)
            out.writestr(item, data)


def main(argv):
    if len(argv) in (3, 4) and argv[1] == "docx" and argv[3:] in ([], ["--stand-ins"]):
        docx(argv[2], stand_ins=len(argv) == 4)
    elif len(argv) == 4 and argv[1] == "pptx":
        pptx(argv[2], argv[3])
    else:
        raise SystemExit("usage: office_fonts.py docx OUT.docx [--stand-ins] | pptx IN.pptx OUT.pptx")


if __name__ == "__main__":
    main(sys.argv)
