#!/usr/bin/env python3
"""Frame's built-in blank deck (FRAME-14): a 16:9 presentation with one title slide, the
Office theme, one master, and five layouts (Title Slide, Title and Content, Section
Header, Two Content, Title Only), as PowerPoint's blank presentation has them.

Usage: python3 scripts/blank_deck.py [OUT]
       (default apps/frame/crates/pptx/data/blank.pptx, which `pptx::blank()` includes)

The package is written byte for byte the same on every run: fixed timestamps, fixed part
order.
"""
import sys
import zipfile
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "apps/frame/crates/pptx/data/blank.pptx"

DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
NS_A = "http://schemas.openxmlformats.org/drawingml/2006/main"
NS_R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS_P = "http://schemas.openxmlformats.org/presentationml/2006/main"
NS = f'xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:p="{NS_P}"'
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "application/vnd.openxmlformats-officedocument.presentationml"

# Slide size: 13.333 x 7.5 inches.
CX, CY = 12192000, 6858000

LAYOUTS = [
    ("Title Slide", "title"),
    ("Title and Content", "obj"),
    ("Section Header", "secHead"),
    ("Two Content", "twoObj"),
    ("Title Only", "titleOnly"),
]


def rels(items):
    body = "".join(
        f'<Relationship Id="{rid}" Type="{REL}/{kind}" Target="{target}"/>'
        for rid, kind, target in items
    )
    return f'{DECL}<Relationships xmlns="{PKG_REL}">{body}</Relationships>'


def xfrm(x, y, cx, cy):
    return f'<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'


def sp(id_, name, ph, frame="", body="", lst="", props=""):
    """A placeholder shape: `ph` the `<p:ph>` attributes, `frame` its `xfrm`, `body` its
    `bodyPr` attributes and children, `lst` its list style, `props` its paragraph."""
    return (
        f'<p:sp><p:nvSpPr><p:cNvPr id="{id_}" name="{name}"/><p:cNvSpPr>'
        f'<a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph{ph}/></p:nvPr></p:nvSpPr>'
        f'<p:spPr>{frame}</p:spPr><p:txBody>{body or "<a:bodyPr/>"}'
        f'<a:lstStyle>{lst}</a:lstStyle>{props or "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>"}'
        f"</p:txBody></p:sp>"
    )


def para(text):
    return f'<a:p><a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r><a:endParaRPr lang="en-US"/></a:p>'


def tree(shapes):
    return (
        '<p:cSld{name}><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/>'
        '<p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>'
        '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
        + "".join(shapes)
        + "</p:spTree></p:cSld>"
    )


def footers(first_id):
    """The date, footer, and slide number placeholders of a layout."""
    return [
        sp(first_id, "Date Placeholder", ' type="dt" sz="half" idx="10"'),
        sp(first_id + 1, "Footer Placeholder", ' type="ftr" sz="quarter" idx="11"'),
        sp(first_id + 2, "Slide Number Placeholder", ' type="sldNum" sz="quarter" idx="12"'),
    ]


def level(n, size, indent=True, bullet=True):
    mar = 228600 + 457200 * (n - 1)
    bu = (
        '<a:buFont typeface="Arial" panose="020B0604020202020204" pitchFamily="34" '
        'charset="0"/><a:buChar char="&#8226;"/>'
        if bullet
        else "<a:buNone/>"
    )
    space = '<a:spcBef><a:spcPts val="1000"/></a:spcBef>' if indent else ""
    return (
        f'<a:lvl{n}pPr marL="{mar if indent else 0}" indent="{-228600 if indent else 0}" '
        f'algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1">'
        f'<a:lnSpc><a:spcPct val="90000"/></a:lnSpc>{space}{bu}'
        f'<a:defRPr sz="{size}" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill>'
        f'<a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/>'
        f"</a:defRPr></a:lvl{n}pPr>"
    )


def plain_levels():
    """Nine levels of plain 18 pt text, 0.5 inch apart: the default and other styles."""
    out = ['<a:defPPr><a:defRPr lang="en-US"/></a:defPPr>']
    for n in range(1, 10):
        out.append(
            f'<a:lvl{n}pPr marL="{457200 * (n - 1)}" algn="l" defTabSz="914400" rtl="0" '
            f'eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:defRPr sz="1800" kern="1200">'
            f'<a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/>'
            f'<a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl{n}pPr>'
        )
    return "".join(out)


THEME = (
    f'{DECL}<a:theme xmlns:a="{NS_A}" name="Office Theme"><a:themeElements>'
    '<a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>'
    '<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>'
    '<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>'
    '<a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2>'
    '<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>'
    '<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6>'
    '<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>'
    "</a:clrScheme>"
    '<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light" panose="020F0302020204030204"/>'
    '<a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont>'
    '<a:latin typeface="Calibri" panose="020F0502020204030204"/><a:ea typeface=""/><a:cs typeface=""/>'
    "</a:minorFont></a:fontScheme>"
    '<a:fmtScheme name="Office"><a:fillStyleLst>'
    '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:gradFill rotWithShape="1"><a:gsLst>'
    '<a:gs pos="0"><a:schemeClr val="phClr"><a:lumMod val="110000"/><a:satMod val="105000"/><a:tint val="67000"/></a:schemeClr></a:gs>'
    '<a:gs pos="50000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="103000"/><a:tint val="73000"/></a:schemeClr></a:gs>'
    '<a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="109000"/><a:tint val="81000"/></a:schemeClr></a:gs>'
    '</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>'
    '<a:gradFill rotWithShape="1"><a:gsLst>'
    '<a:gs pos="0"><a:schemeClr val="phClr"><a:satMod val="103000"/><a:lumMod val="102000"/><a:tint val="94000"/></a:schemeClr></a:gs>'
    '<a:gs pos="50000"><a:schemeClr val="phClr"><a:satMod val="110000"/><a:lumMod val="100000"/><a:shade val="100000"/></a:schemeClr></a:gs>'
    '<a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="99000"/><a:satMod val="120000"/><a:shade val="78000"/></a:schemeClr></a:gs>'
    '</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill></a:fillStyleLst>'
    '<a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>'
    '<a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>'
    '<a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:prstDash val="solid"/><a:miter lim="800000"/></a:ln></a:lnStyleLst>'
    '<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle>'
    '<a:effectStyle><a:effectLst><a:outerShdw blurRad="57150" dist="19050" dir="5400000" algn="ctr" rotWithShape="0">'
    '<a:srgbClr val="000000"><a:alpha val="63000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle>'
    "</a:effectStyleLst>"
    '<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    '<a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/><a:satMod val="170000"/></a:schemeClr></a:solidFill>'
    '<a:gradFill rotWithShape="1"><a:gsLst>'
    '<a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="93000"/><a:satMod val="150000"/><a:shade val="98000"/><a:lumMod val="102000"/></a:schemeClr></a:gs>'
    '<a:gs pos="50000"><a:schemeClr val="phClr"><a:tint val="98000"/><a:satMod val="130000"/><a:shade val="90000"/><a:lumMod val="103000"/></a:schemeClr></a:gs>'
    '<a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="63000"/><a:satMod val="120000"/></a:schemeClr></a:gs>'
    '</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill></a:bgFillStyleLst>'
    "</a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"
)

TITLE_BOX = (838200, 365125, 10515600, 1325563)
BODY_BOX = (838200, 1825625, 10515600, 4351338)


def master():
    shapes = [
        sp(2, "Title Placeholder 1", ' type="title"', xfrm(*TITLE_BOX),
           '<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" '
           'anchor="ctr"><a:normAutofit/></a:bodyPr>',
           props=para("Click to edit Master title style")),
        sp(3, "Text Placeholder 2", ' type="body" idx="1"', xfrm(*BODY_BOX),
           '<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0">'
           "<a:normAutofit/></a:bodyPr>",
           props=para("Click to edit Master text styles")),
        sp(4, "Date Placeholder 3", ' type="dt" sz="half" idx="2"', xfrm(838200, 6356350, 2743200, 365125),
           '<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"/>',
           '<a:lvl1pPr algn="l"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/>'
           "</a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"),
        sp(5, "Footer Placeholder 4", ' type="ftr" sz="quarter" idx="3"', xfrm(4038600, 6356350, 4114800, 365125),
           '<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"/>',
           '<a:lvl1pPr algn="ctr"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/>'
           "</a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"),
        sp(6, "Slide Number Placeholder 5", ' type="sldNum" sz="quarter" idx="4"', xfrm(8610600, 6356350, 2743200, 365125),
           '<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"/>',
           '<a:lvl1pPr algn="r"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/>'
           "</a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"),
    ]
    ids = "".join(
        f'<p:sldLayoutId id="{2147483649 + i}" r:id="rId{i + 1}"/>' for i in range(len(LAYOUTS))
    )
    body = "".join(level(n, s) for n, s in zip(range(1, 10), [2800, 2400, 2000, 1800, 1800, 1800, 1800, 1800, 1800]))
    title_style = (
        '<a:titleStyle><a:lvl1pPr algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" '
        'hangingPunct="1"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPct val="0"/>'
        '</a:spcBef><a:buNone/><a:defRPr sz="4400" kern="1200"><a:solidFill><a:schemeClr val="tx1"/>'
        '</a:solidFill><a:latin typeface="+mj-lt"/><a:ea typeface="+mj-ea"/><a:cs typeface="+mj-cs"/>'
        "</a:defRPr></a:lvl1pPr></a:titleStyle>"
    )
    return (
        f'{DECL}<p:sldMaster {NS}>'
        + tree(shapes).replace("{name}", "")
        .replace("<p:cSld>", '<p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>')
        + '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" '
        'accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" '
        'folHlink="folHlink"/>'
        f"<p:sldLayoutIdLst>{ids}</p:sldLayoutIdLst>"
        f"<p:txStyles>{title_style}<p:bodyStyle>{body}</p:bodyStyle>"
        f"<p:otherStyle>{plain_levels()}</p:otherStyle></p:txStyles></p:sldMaster>"
    )


def layout(name, kind):
    title = sp(2, "Title 1", ' type="title"')
    if kind == "title":
        shapes = [
            sp(2, "Title 1", ' type="ctrTitle"', xfrm(1524000, 1122363, 9144000, 2387600),
               '<a:bodyPr anchor="b"/>', '<a:lvl1pPr algn="ctr"><a:defRPr sz="6000"/></a:lvl1pPr>'),
            sp(3, "Subtitle 2", ' type="subTitle" idx="1"', xfrm(1524000, 3602038, 9144000, 1655762),
               "<a:bodyPr/>",
               '<a:lvl1pPr marL="0" indent="0" algn="ctr"><a:buNone/><a:defRPr sz="2400"/></a:lvl1pPr>'
               '<a:lvl2pPr marL="457200" indent="0" algn="ctr"><a:buNone/><a:defRPr sz="2000"/></a:lvl2pPr>'),
        ]
    elif kind == "obj":
        shapes = [title, sp(3, "Content Placeholder 2", ' idx="1"')]
    elif kind == "secHead":
        shapes = [
            sp(2, "Title 1", ' type="title"', xfrm(831850, 1709738, 10515600, 2852737),
               '<a:bodyPr anchor="b"/>', '<a:lvl1pPr><a:defRPr sz="6000"/></a:lvl1pPr>'),
            sp(3, "Text Placeholder 2", ' type="body" idx="1"', xfrm(831850, 4589463, 10515600, 1500187),
               "<a:bodyPr/>",
               '<a:lvl1pPr marL="0" indent="0"><a:buNone/><a:defRPr sz="2400"><a:solidFill>'
               '<a:schemeClr val="tx1"><a:tint val="75000"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>'),
        ]
    elif kind == "twoObj":
        shapes = [
            title,
            sp(3, "Content Placeholder 2", ' sz="half" idx="1"', xfrm(838200, 1825625, 5181600, 4351338)),
            sp(4, "Content Placeholder 3", ' sz="half" idx="2"', xfrm(6172200, 1825625, 5181600, 4351338)),
        ]
    else:
        shapes = [title]
    shapes += footers(len(shapes) + 2)
    return (
        f'{DECL}<p:sldLayout {NS} type="{kind}" preserve="1">'
        + tree(shapes).replace("{name}", f' name="{name}"')
        + '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>'
    )


def slide():
    shapes = [
        sp(2, "Title 1", ' type="ctrTitle"'),
        sp(3, "Subtitle 2", ' type="subTitle" idx="1"'),
    ]
    return (
        f'{DECL}<p:sld {NS}>'
        + tree(shapes).replace("{name}", "")
        + '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>'
    )


def presentation():
    return (
        f'{DECL}<p:presentation {NS} saveSubsetFonts="1">'
        '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>'
        '<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>'
        f'<p:sldSz cx="{CX}" cy="{CY}"/><p:notesSz cx="{CY}" cy="9144000"/>'
        f"<p:defaultTextStyle>{plain_levels()}</p:defaultTextStyle></p:presentation>"
    )


def content_types():
    over = [
        ("/ppt/presentation.xml", f"{CT}.presentation.main+xml"),
        ("/ppt/slideMasters/slideMaster1.xml", f"{CT}.slideMaster+xml"),
        ("/ppt/slides/slide1.xml", f"{CT}.slide+xml"),
        ("/ppt/presProps.xml", f"{CT}.presProps+xml"),
        ("/ppt/viewProps.xml", f"{CT}.viewProps+xml"),
        ("/ppt/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"),
        ("/ppt/tableStyles.xml", f"{CT}.tableStyles+xml"),
        ("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml"),
        ("/docProps/app.xml", "application/vnd.openxmlformats-officedocument.extended-properties+xml"),
    ] + [
        (f"/ppt/slideLayouts/slideLayout{i + 1}.xml", f"{CT}.slideLayout+xml")
        for i in range(len(LAYOUTS))
    ]
    body = (
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        + "".join(f'<Override PartName="{p}" ContentType="{t}"/>' for p, t in over)
    )
    return f'{DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{body}</Types>'


def parts():
    n = len(LAYOUTS)
    out = [
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", f'{DECL}<Relationships xmlns="{PKG_REL}">'
         f'<Relationship Id="rId3" Type="{REL}/extended-properties" Target="docProps/app.xml"/>'
         '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/'
         'metadata/core-properties" Target="docProps/core.xml"/>'
         f'<Relationship Id="rId1" Type="{REL}/officeDocument" Target="ppt/presentation.xml"/>'
         "</Relationships>"),
        ("docProps/app.xml", f'{DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/'
         '2006/extended-properties"><Application>Frame</Application><Slides>1</Slides></Properties>'),
        ("docProps/core.xml", f'{DECL}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/'
         'package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/">'
         "<dc:title></dc:title></cp:coreProperties>"),
        ("ppt/presentation.xml", presentation()),
        ("ppt/_rels/presentation.xml.rels", rels([
            ("rId1", "slideMaster", "slideMasters/slideMaster1.xml"),
            ("rId2", "slide", "slides/slide1.xml"),
            ("rId3", "presProps", "presProps.xml"),
            ("rId4", "viewProps", "viewProps.xml"),
            ("rId5", "theme", "theme/theme1.xml"),
            ("rId6", "tableStyles", "tableStyles.xml"),
        ])),
        ("ppt/presProps.xml", f"{DECL}<p:presentationPr {NS}/>"),
        ("ppt/viewProps.xml", f'{DECL}<p:viewPr {NS}><p:gridSpacing cx="76200" cy="76200"/></p:viewPr>'),
        ("ppt/tableStyles.xml", f'{DECL}<a:tblStyleLst xmlns:a="{NS_A}" '
         'def="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"/>'),
        ("ppt/theme/theme1.xml", THEME),
        ("ppt/slideMasters/slideMaster1.xml", master()),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels(
            [(f"rId{i + 1}", "slideLayout", f"../slideLayouts/slideLayout{i + 1}.xml") for i in range(n)]
            + [(f"rId{n + 1}", "theme", "../theme/theme1.xml")])),
    ]
    for i, (name, kind) in enumerate(LAYOUTS):
        out.append((f"ppt/slideLayouts/slideLayout{i + 1}.xml", layout(name, kind)))
        out.append((f"ppt/slideLayouts/_rels/slideLayout{i + 1}.xml.rels",
                    rels([("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")])))
    out.append(("ppt/slides/slide1.xml", slide()))
    out.append(("ppt/slides/_rels/slide1.xml.rels",
                rels([("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")])))
    return out


def write(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, text in parts():
            info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, text.encode("utf-8"))


if __name__ == "__main__":
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT
    write(out)
    print(f"blank deck -> {out}")
