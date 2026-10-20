//! `mc:AlternateContent` in a shape tree: a choice we cannot read must survive a save.
//!
//! A block holds a choice in a namespace we don't know and a fallback every reader shows. The
//! fallback is what the shape shows; the choice must come back when the deck is saved, or the
//! file loses content the moment it is opened and written again.

use std::io::{Cursor, Read, Write};

use deckcraft_model::Presentation;
use deckcraft_model::defaults;

/// A block whose choice uses a namespace we don't know, and a fallback with a shape of its own.
const BLOCK: &str = r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p99="http://example.org/deckcraft/2026/p99" Requires="p99"><p:sp><p:nvSpPr><p:cNvPr id="70" name="Choice shape"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Choice branch</a:t></a:r></a:p></p:txBody><p99:mark val="kept"/></p:sp></mc:Choice><mc:Fallback><p:sp><p:nvSpPr><p:cNvPr id="71" name="Fallback shape"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Fallback branch</a:t></a:r></a:p></p:txBody></p:sp></mc:Fallback></mc:AlternateContent>"#;

/// The same block, with a picture in the fallback (the poster of an object we cannot read).
const POSTER_BLOCK: &str = r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p99="http://example.org/deckcraft/2026/p99" Requires="p99"><p:sp><p:nvSpPr><p:cNvPr id="80" name="Choice shape"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Choice branch</a:t></a:r></a:p></p:txBody><p99:mark val="kept"/></p:sp></mc:Choice><mc:Fallback><p:pic><p:nvPicPr><p:cNvPr id="81" name="Poster"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rIdPoster"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></mc:Fallback></mc:AlternateContent>"#;

fn png(v: u8) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([v, v, v])))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

/// A deck whose first slide holds `block` in its shape tree.
fn deck_with(block: &str, poster: bool) -> Vec<u8> {
    let base = deckcraft_pptx::export(&defaults::new_presentation(None)).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(base)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        if name == "ppt/slides/slide1.xml" {
            let s = String::from_utf8(d).expect("utf8");
            d = s.replacen("</p:spTree>", &format!("{block}</p:spTree>"), 1).into_bytes();
        } else if name == "ppt/slides/_rels/slide1.xml.rels" && poster {
            let s = String::from_utf8(d).expect("utf8");
            d = s
                .replacen(
                    "</Relationships>",
                    r#"<Relationship Id="rIdPoster" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/poster.png"/></Relationships>"#,
                    1,
                )
                .into_bytes();
        }
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    if poster {
        out.start_file("ppt/media/poster.png", o).expect("start");
        out.write_all(&png(120)).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut f = z.by_name(name).unwrap_or_else(|_| panic!("no {name}"));
    let mut s = String::new();
    f.read_to_string(&mut s).expect("read");
    s
}

fn names(p: &Presentation) -> Vec<String> {
    p.slides.iter().flat_map(|s| s.shapes.iter()).map(|s| s.name.clone()).collect()
}

fn text_of(p: &Presentation, name: &str) -> String {
    let s = p.slides.iter().flat_map(|s| s.shapes.iter()).find(|s| s.name == name).unwrap_or_else(|| panic!("no shape {name}"));
    s.text.as_ref().map(|t| t.text()).unwrap_or_default()
}

/// A choice in a namespace we don't know comes back when the deck is saved; the fallback is what
/// the shape shows.
#[test]
fn an_unsupported_choice_survives_a_save() {
    let p = deckcraft_pptx::import(&deck_with(BLOCK, false)).expect("import");
    // The fallback is the shape: the choice is not a shape of its own.
    assert!(names(&p).contains(&"Fallback shape".to_string()), "{:?}", names(&p));
    assert!(!names(&p).contains(&"Choice shape".to_string()), "the choice became a shape");
    assert_eq!(text_of(&p, "Fallback shape"), "Fallback branch");

    let out = deckcraft_pptx::export(&p).expect("export");
    let slide = part(&out, "ppt/slides/slide1.xml");
    assert!(slide.contains("mc:AlternateContent"), "the block is gone: {slide}");
    assert!(slide.contains(r#"Requires="p99""#), "the choice is gone: {slide}");
    assert!(slide.contains("p99:"), "the choice's own content is gone: {slide}");
    assert!(slide.contains("Choice branch"), "the choice's text is gone: {slide}");
    assert!(slide.contains("mc:Fallback"), "the fallback is gone: {slide}");
    assert!(slide.contains("Fallback branch"), "the fallback's text is gone: {slide}");

    // A second round trip keeps both branches.
    let p2 = deckcraft_pptx::import(&out).expect("re-import");
    assert_eq!(text_of(&p2, "Fallback shape"), "Fallback branch");
    let out2 = deckcraft_pptx::export(&p2).expect("re-export");
    let slide2 = part(&out2, "ppt/slides/slide1.xml");
    assert!(slide2.contains("p99:") && slide2.contains("Choice branch") && slide2.contains("Fallback branch"), "{slide2}");
    assert_eq!(part(&out2, "ppt/slides/slide1.xml"), slide, "the second save differs from the first");
}

/// The same, with a picture in the fallback: the picture is the shape's preview and stays.
#[test]
fn an_unsupported_choice_with_a_poster_survives_a_save() {
    let p = deckcraft_pptx::import(&deck_with(POSTER_BLOCK, true)).expect("import");
    let s = p.slides.iter().flat_map(|s| s.shapes.iter()).find(|s| s.name == "Poster").expect("the poster");
    let deckcraft_model::ShapeKind::Opaque { preview: Some(m), .. } = &s.kind else { panic!("{:?}", s.kind) };
    assert_eq!(p.media(*m).expect("poster").name, "poster.png");

    let out = deckcraft_pptx::export(&p).expect("export");
    let slide = part(&out, "ppt/slides/slide1.xml");
    assert!(slide.contains(r#"Requires="p99""#) && slide.contains("Choice branch"), "the choice is gone: {slide}");
    assert!(slide.contains("mc:Fallback") && slide.contains("r:embed"), "the fallback is gone: {slide}");
    // The poster's relationship is one the slide has, under the id the slide gives it.
    let rels = part(&out, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("relationships/image") && rels.contains("media/"), "the poster is gone: {rels}");

    let p2 = deckcraft_pptx::import(&out).expect("re-import");
    let out2 = deckcraft_pptx::export(&p2).expect("re-export");
    assert!(part(&out2, "ppt/slides/slide1.xml").contains(r#"Requires="p99""#));
}

/// A choice whose part is not in the package cannot be written: it is dropped, and the fallback
/// goes on its own. An `mc:AlternateContent` needs a choice, so a block without one is not valid.
#[test]
fn a_choice_whose_part_is_gone_is_dropped_and_the_fallback_stays() {
    let pic = |id: u32, name: &str, rel: &str| {
        format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{name}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rel}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
        )
    };
    let block = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p99="http://example.org/deckcraft/2026/p99" Requires="p99">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>"#,
        pic(70, "Choice shape", "rIdGone"),
        pic(71, "Poster", "rIdPoster")
    );
    let bytes = deck_with(&block, true);
    // The choice's picture is not in the package.
    let p = deckcraft_pptx::import(&bytes).expect("import");
    assert!(names(&p).contains(&"Poster".to_string()), "{:?}", names(&p));

    let out = deckcraft_pptx::export(&p).expect("export");
    let slide = part(&out, "ppt/slides/slide1.xml");
    assert!(!slide.contains("mc:AlternateContent"), "a block without a choice is not valid: {slide}");
    assert!(slide.contains("r:embed="), "the fallback is gone: {slide}");
    // The file still opens, and the poster is still there.
    let p2 = deckcraft_pptx::import(&out).expect("re-import");
    assert!(names(&p2).contains(&"Poster".to_string()), "{:?}", names(&p2));
}

/// A choice we do understand is read as before: its content is the shape, and the file keeps a
/// block of its own.
#[test]
fn a_supported_choice_is_still_read_as_its_content() {
    let block = r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" Requires="p14"><p:sp><p:nvSpPr><p:cNvPr id="90" name="Chosen shape"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Chosen branch</a:t></a:r></a:p></p:txBody></p:sp></mc:Choice><mc:Fallback><p:sp><p:nvSpPr><p:cNvPr id="91" name="Fallback shape"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Fallback branch</a:t></a:r></a:p></p:txBody></p:sp></mc:Fallback></mc:AlternateContent>"#;
    let p = deckcraft_pptx::import(&deck_with(block, false)).expect("import");
    assert!(names(&p).contains(&"Chosen shape".to_string()), "{:?}", names(&p));
    assert!(!names(&p).contains(&"Fallback shape".to_string()), "the fallback replaced the choice");
    assert_eq!(text_of(&p, "Chosen shape"), "Chosen branch");
    // The deck still saves.
    let out = deckcraft_pptx::export(&p).expect("export");
    let p2 = deckcraft_pptx::import(&out).expect("re-import");
    assert_eq!(text_of(&p2, "Chosen shape"), "Chosen branch");
}
