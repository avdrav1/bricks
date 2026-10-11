//! FRAME-3: content Frame can't draw becomes a labelled shape, showing the picture the
//! file keeps for readers that can't play or edit it when there is one: a video's poster
//! frame, an embedded object's preview. Built in memory: no sample deck has all of these.

use presentation_model::{ImageFormat, ShapeKind};
use std::io::{Cursor, Write};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0";

fn emf() -> Vec<u8> {
    let mut b = vec![1, 0, 0, 0];
    b.resize(40, 0);
    b.extend_from_slice(b" EMF");
    b.resize(108, 0);
    b
}

fn deck() -> Vec<u8> {
    let rel = |id: &str, kind: &str, target: &str| {
        format!(
            r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{target}"/>"#
        )
    };
    let rels = |items: &[String]| {
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
            items.concat()
        )
    };
    let frame = |id: u32, name: &str| format!(r#"<p:cNvPr id="{id}" name="{name}"/>"#);
    let xfrm = r#"<a:xfrm><a:off x="100" y="200"/><a:ext cx="3000" cy="2000"/></a:xfrm>"#;
    let slide = format!(
        r#"<p:sld xmlns:a="a" xmlns:p="p" xmlns:r="r" xmlns:mc="mc"><p:cSld><p:spTree>
        <p:pic><p:nvPicPr>{}<p:cNvPicPr/><p:nvPr><a:videoFile r:link="rId9"/></p:nvPr></p:nvPicPr>
          <p:blipFill><a:blip r:embed="rId2"/></p:blipFill><p:spPr>{xfrm}</p:spPr></p:pic>
        <p:pic><p:nvPicPr>{}<p:cNvPicPr/><p:nvPr/></p:nvPicPr>
          <p:blipFill><a:blip r:embed="rId3"/></p:blipFill><p:spPr>{xfrm}</p:spPr></p:pic>
        <p:graphicFrame><p:nvGraphicFramePr>{}<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
          <p:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></p:xfrm>
          <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole">
            <mc:AlternateContent><mc:Choice Requires="v"><p:oleObj r:id="rId4"/></mc:Choice>
            <mc:Fallback><p:oleObj r:id="rId4"><p:embed/><p:pic><p:nvPicPr>{}<p:cNvPicPr/><p:nvPr/></p:nvPicPr>
              <p:blipFill><a:blip r:embed="rId2"/></p:blipFill><p:spPr/></p:pic></p:oleObj></mc:Fallback>
            </mc:AlternateContent></a:graphicData></a:graphic></p:graphicFrame>
        <p:graphicFrame><p:nvGraphicFramePr>{}<p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
          <p:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></p:xfrm>
          <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="c" r:id="rId5"/></a:graphicData></a:graphic></p:graphicFrame>
        </p:spTree></p:cSld></p:sld>"#,
        frame(2, "Video"),
        frame(3, "Metafile"),
        frame(4, "Workbook"),
        frame(9, "Ole preview"),
        frame(5, "Chart"),
    );
    let parts: Vec<(&str, Vec<u8>)> = vec![
        ("_rels/.rels", rels(&[rel("rId1", "officeDocument", "ppt/presentation.xml")]).into_bytes()),
        (
            "ppt/presentation.xml",
            br#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="6858000"/></p:presentation>"#.to_vec(),
        ),
        ("ppt/_rels/presentation.xml.rels", rels(&[rel("rId1", "slide", "slides/slide1.xml")]).into_bytes()),
        ("ppt/slides/slide1.xml", slide.into_bytes()),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            rels(&[
                rel("rId2", "image", "../media/poster.png"),
                rel("rId3", "image", "../media/picture.emf"),
                rel("rId4", "oleObject", "../embeddings/book.xlsx"),
                rel("rId5", "chart", "../charts/chart1.xml"),
            ])
            .into_bytes(),
        ),
        ("ppt/media/poster.png", PNG.to_vec()),
        ("ppt/media/picture.emf", emf()),
    ];
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn unsupported_content_is_labelled_with_its_preview() {
    let deck = pptx::read_from(Cursor::new(deck())).unwrap();
    let kinds: Vec<(String, Option<ImageFormat>)> = deck.slides[0]
        .shapes
        .iter()
        .map(|s| match &s.kind {
            ShapeKind::Unsupported { label, preview } => {
                (label.clone(), preview.as_ref().map(|p| p.format))
            }
            other => panic!("{}: {other:?}", s.name),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("Video".to_owned(), Some(ImageFormat::Png)),
            ("Picture (EMF)".to_owned(), None),
            ("Embedded object".to_owned(), Some(ImageFormat::Png)),
            ("Chart".to_owned(), None),
        ]
    );
    let video = &deck.slides[0].shapes[0];
    assert_eq!((video.frame.x, video.frame.cx), (100, 3000));
}
