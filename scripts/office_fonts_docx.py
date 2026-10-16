#!/usr/bin/env python3
"""Write office-fonts.docx: a Word document in the fonts Office documents use most.

    python3 scripts/office_fonts_docx.py OUT.docx

A Cambria heading, then one long paragraph each in Calibri, Arial, Times New Roman,
Courier New, and Cambria, on a Letter page with 1-inch margins (6.5 inches of text). Where
those fonts aren't installed, fontconfig's metric aliases draw them in Carlito, Liberation
Sans, Liberation Serif, Liberation Mono, and Caladea, with the same widths, so every line
breaks in the same place. The look checks compare its screenshots across systems
(MAC-6's `check_mac_look.sh`, WIN-4's `check_win_open.ps1`).
"""

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


def para(font, size, text):
    return (f'<w:p><w:r><w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}"/>'
            f'<w:sz w:val="{size}"/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p>')


def write(path):
    paras = [para("Cambria", 40, "Cambria heading, drawn in Caladea"),
             para("Calibri", 22, "Calibri body, drawn in Carlito. " + BODY)]
    paras += [para(f, 22, f"{f}: {BODY}") for f in ("Arial", "Times New Roman", "Courier New")]
    paras += [para("Cambria", 22, "Cambria body. " + BODY)]
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


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: office_fonts_docx.py OUT.docx")
    write(sys.argv[1])
