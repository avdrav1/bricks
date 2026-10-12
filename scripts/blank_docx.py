#!/usr/bin/env python3
"""Slate's built-in blank Word document (SLATE-18): one empty paragraph, and the styles a
new document needs (Normal, Title, Heading 1 to 6, Quote, Hyperlink) with Word's default
look: Calibri 11 pt, 8 pt after each paragraph. No page setup: Word and LibreOffice use
their own default page size for the user's locale.

Usage: python3 scripts/blank_docx.py [OUT]
       (default apps/slate/crates/docx/data/blank.docx, which `docx::blank()` includes)

The package is written byte for byte the same on every run: fixed timestamps, fixed part
order.
"""
import sys
import zipfile
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "apps/slate/crates/docx/data/blank.docx"

DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
NS_W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
NS_R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "application/vnd.openxmlformats-officedocument.wordprocessingml"

CONTENT_TYPES = (
    DECL
    + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    f'<Override PartName="/word/document.xml" ContentType="{CT}.document.main+xml"/>'
    f'<Override PartName="/word/styles.xml" ContentType="{CT}.styles+xml"/>'
    "</Types>"
)

PACKAGE_RELS = (
    DECL
    + f'<Relationships xmlns="{PKG_REL}">'
    f'<Relationship Id="rId1" Type="{NS_R}/officeDocument" Target="word/document.xml"/>'
    "</Relationships>"
)

DOCUMENT_RELS = (
    DECL
    + f'<Relationships xmlns="{PKG_REL}">'
    f'<Relationship Id="rId1" Type="{NS_R}/styles" Target="styles.xml"/>'
    "</Relationships>"
)

DOCUMENT = (
    DECL
    + f'<w:document xmlns:w="{NS_W}" xmlns:r="{NS_R}">'
    "<w:body><w:p/></w:body>"
    "</w:document>"
)


def paragraph_style(sid, name, ppr="", rpr="", default=False, based=True):
    attrs = ' w:default="1"' if default else ""
    based_on = '<w:basedOn w:val="Normal"/><w:next w:val="Normal"/>' if based else ""
    return (
        f'<w:style w:type="paragraph"{attrs} w:styleId="{sid}">'
        f'<w:name w:val="{name}"/>{based_on}<w:qFormat/>'
        + (f"<w:pPr>{ppr}</w:pPr>" if ppr else "")
        + (f"<w:rPr>{rpr}</w:rPr>" if rpr else "")
        + "</w:style>"
    )


HEADING_SIZES = [32, 26, 24, 22, 22, 22]  # half-points

STYLES = (
    DECL
    + f'<w:styles xmlns:w="{NS_W}">'
    "<w:docDefaults>"
    '<w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/>'
    '<w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault>'
    '<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault>'
    "</w:docDefaults>"
    + paragraph_style("Normal", "Normal", default=True, based=False)
    + paragraph_style(
        "Title",
        "Title",
        ppr='<w:spacing w:after="0" w:line="240" w:lineRule="auto"/><w:contextualSpacing/>',
        rpr='<w:spacing w:val="-10"/><w:kern w:val="28"/><w:sz w:val="56"/><w:szCs w:val="56"/>',
    )
    + "".join(
        paragraph_style(
            f"Heading{n}",
            f"heading {n}",
            ppr=f'<w:keepNext/><w:keepLines/><w:spacing w:before="{240 if n == 1 else 40}" w:after="0"/>'
            f'<w:outlineLvl w:val="{n - 1}"/>',
            rpr=f'<w:b/><w:bCs/><w:color w:val="2F5496"/><w:sz w:val="{size}"/><w:szCs w:val="{size}"/>',
        )
        for n, size in enumerate(HEADING_SIZES, start=1)
    )
    + paragraph_style(
        "Quote",
        "Quote",
        ppr='<w:spacing w:before="200"/><w:ind w:left="864" w:right="864"/><w:jc w:val="center"/>',
        rpr='<w:i/><w:iCs/><w:color w:val="404040"/>',
    )
    + '<w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont">'
    '<w:name w:val="Default Paragraph Font"/><w:uiPriority w:val="1"/><w:semiHidden/></w:style>'
    '<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/>'
    '<w:basedOn w:val="DefaultParagraphFont"/>'
    '<w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>'
    "</w:styles>"
)

PARTS = [
    ("[Content_Types].xml", CONTENT_TYPES),
    ("_rels/.rels", PACKAGE_RELS),
    ("word/document.xml", DOCUMENT),
    ("word/_rels/document.xml.rels", DOCUMENT_RELS),
    ("word/styles.xml", STYLES),
]


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name, text in PARTS:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, text.encode("utf-8"))
    print(out)


if __name__ == "__main__":
    main()
