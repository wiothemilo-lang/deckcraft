//! Contributor-authored SmartArt without a cached drawing: the diagram data's image and link
//! relationships must survive save/reopen, including a native-format save in between (#203).

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_model::{ShapeId, defaults};

const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const LINK: &str = "https://example.invalid/owned-diagram";
const DATA: &str = r#"<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dgm:ptLst><dgm:pt modelId="0" type="doc"/><dgm:pt modelId="1"><dgm:spPr><a:blipFill><a:blip r:embed="rIdImage"/></a:blipFill></dgm:spPr><dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr><a:hlinkClick r:id="rIdLink"/></a:rPr><a:t>Owned diagram node</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst></dgm:dataModel>"#;

fn png() -> Vec<u8> {
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(3, 2, image::Rgb([40, 180, 90])))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).expect("entry");
            let name = file.name().to_string();
            let mut data = vec![];
            file.read_to_end(&mut data).expect("read");
            (name, data)
        })
        .collect()
}

fn package(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, data) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).expect("entry");
        zip.write_all(data).expect("write");
    }
    zip.finish().expect("finish").into_inner()
}

fn append_xml(files: &mut [(String, Vec<u8>)], part: &str, end: &str, content: &str) {
    let (_, data) = files.iter_mut().find(|(name, _)| name == part).expect("part");
    let xml = String::from_utf8(data.clone()).expect("xml");
    assert!(xml.contains(end));
    *data = xml.replacen(end, &format!("{content}{end}"), 1).into_bytes();
}

fn fixture() -> Vec<u8> {
    let mut files = entries(&deckcraft_pptx::export(&defaults::new_presentation(None)).expect("export"));
    append_xml(
        &mut files,
        "ppt/slides/slide1.xml",
        "</p:spTree>",
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="90" name="Owned diagram"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" r:dm="rIdDm" r:lo="rIdLo" r:qs="rIdQs" r:cs="rIdCs"/></a:graphicData></a:graphic></p:graphicFrame>"#,
    );
    let mut rels = String::new();
    let mut types = String::from(r#"<Default Extension="png" ContentType="image/png"/>"#);
    for (rid, stem, kind, tag) in [
        ("rIdDm", "data", "Data", "dataModel"),
        ("rIdLo", "layout", "Layout", "layoutDef"),
        ("rIdQs", "style", "Style", "styleDef"),
        ("rIdCs", "colors", "Colors", "colorsDef"),
    ] {
        let part = format!("ppt/diagrams/{stem}1.xml");
        let relation = if kind == "Style" { "QuickStyle" } else { kind };
        rels += &format!(r#"<Relationship Id="{rid}" Type="{RT}diagram{relation}" Target="../diagrams/{stem}1.xml"/>"#);
        types +=
            &format!(r#"<Override PartName="/{part}" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagram{kind}+xml"/>"#);
        let xml = if kind == "Data" {
            DATA.to_string()
        } else {
            format!(r#"<dgm:{tag} xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram"/>"#)
        };
        files.push((part, xml.into_bytes()));
    }
    append_xml(&mut files, "ppt/slides/_rels/slide1.xml.rels", "</Relationships>", &rels);
    append_xml(&mut files, "[Content_Types].xml", "</Types>", &types);
    files.push(("ppt/media/image1.png".into(), png()));
    files.push((
        "ppt/diagrams/_rels/data1.xml.rels".into(),
        format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="{RT}image" Target="../media/image1.png"/><Relationship Id="rIdLink" Type="{RT}hyperlink" Target="{LINK}" TargetMode="External"/></Relationships>"#).into_bytes(),
    ));
    package(&files)
}

fn resolve(source: &str, target: &str) -> String {
    let mut path: Vec<&str> =
        if target.starts_with('/') { vec![] } else { source.split('/').rev().skip(1).collect::<Vec<_>>().into_iter().rev().collect() };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                path.pop();
            }
            s => path.push(s),
        }
    }
    path.join("/")
}

fn assert_kept(bytes: &[u8]) {
    let files = entries(bytes);
    let parts: HashMap<_, _> = files.iter().map(|(name, data)| (name.as_str(), data.as_slice())).collect();
    assert_eq!(parts.len(), files.len(), "duplicate ZIP entries");
    for (name, data) in &files {
        if !name.ends_with(".rels") {
            continue;
        }
        let source = if name == "_rels/.rels" { String::new() } else { name.replace("/_rels/", "/").trim_end_matches(".rels").to_string() };
        let rels = deckcraft_pptx::parse_xml(data).expect("relationships");
        for rel in rels.root.elements().filter(|r| r.attr("TargetMode") != Some("External")) {
            let target = resolve(&source, rel.attr("Target").expect("target"));
            assert!(parts.contains_key(target.as_str()), "{name} has missing target {target}");
        }
    }
    let slide_rels = deckcraft_pptx::parse_xml(parts["ppt/slides/_rels/slide1.xml.rels"]).expect("slide rels");
    let data_rel = slide_rels.root.elements().find(|r| r.attr("Type") == Some(&format!("{RT}diagramData"))).expect("diagram data relationship lost");
    let data_name = resolve("ppt/slides/slide1.xml", data_rel.attr("Target").expect("target"));
    let data = std::str::from_utf8(parts[data_name.as_str()]).expect("data");
    assert!(data.contains("Owned diagram node") && data.contains("rIdImage") && data.contains("rIdLink"));
    let (folder, file) = data_name.rsplit_once('/').expect("data path");
    let rel_name = format!("{folder}/_rels/{file}.rels");
    let rels = deckcraft_pptx::parse_xml(parts[rel_name.as_str()]).expect("data relationships");
    let image = rels.root.elements().find(|r| r.attr("Id") == Some("rIdImage")).expect("image relation");
    assert_eq!(image.attr("Type"), Some(format!("{RT}image").as_str()));
    let image_name = resolve(&data_name, image.attr("Target").expect("image target"));
    assert_eq!(parts[image_name.as_str()], png(), "image bytes changed");
    let link = rels.root.elements().find(|r| r.attr("Id") == Some("rIdLink")).expect("link relation");
    assert_eq!(link.attr("Target"), Some(LINK));
    assert_eq!(link.attr("TargetMode"), Some("External"));
}

#[test]
fn smartart_data_keeps_its_image_and_hyperlink_on_repeated_saves() {
    let source = fixture();
    assert_kept(&source);
    let mut doc = deckcraft_pptx::import(&source).expect("import");
    for _ in 0..2 {
        let saved = deckcraft_pptx::export(&doc).expect("export");
        assert_kept(&saved);
        doc = deckcraft_pptx::import(&saved).expect("reopen");
    }
}

#[test]
fn native_save_keeps_nested_part_bytes_and_relationships() {
    use deckcraft_engine::cmd::file::{open_presentation, save_bytes};

    let doc = deckcraft_pptx::import(&fixture()).expect("import");
    let native = save_bytes(&doc, "deckcraft").expect("save native");
    let reopened = open_presentation("owned.deckcraft", &native).expect("reopen native");
    assert_kept(&deckcraft_pptx::export(&reopened).expect("export after native"));
}

#[test]
fn shared_parts_write_one_relationship_file_across_slides_and_objects() {
    let mut doc = deckcraft_pptx::import(&fixture()).expect("import");
    let mut copy = doc.slides[0].shapes.iter().find(|s| s.name == "Owned diagram").expect("diagram").clone();
    copy.id = ShapeId(doc.alloc_id().expect("id"));
    Arc::make_mut(&mut doc.slides[0]).shapes.push(copy.clone());
    copy.id = ShapeId(doc.alloc_id().expect("id"));
    let layout = doc.slides[0].layout;
    let mut slide = defaults::new_slide(&mut doc, layout).expect("slide");
    slide.shapes.push(copy);
    doc.slides.push(Arc::new(slide));
    assert_kept(&deckcraft_pptx::export(&doc).expect("export shared parts"));
}

#[test]
fn cyclic_part_relationships_are_bounded_and_round_trip() {
    let mut files = entries(&fixture());
    files.push((
        "ppt/media/_rels/image1.png.rels".into(),
        format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="back" Type="{RT}diagramData" Target="../diagrams/data1.xml"/></Relationships>"#).into_bytes(),
    ));
    let doc = deckcraft_pptx::import(&package(&files)).expect("import cycle");
    let saved = deckcraft_pptx::export(&doc).expect("export cycle");
    assert_kept(&saved);
    let reopened = deckcraft_pptx::import(&saved).expect("reopen cycle");
    assert_kept(&deckcraft_pptx::export(&reopened).expect("export cycle again"));
}

fn assert_fallback(source: &[u8]) {
    let doc = deckcraft_pptx::import(source).expect("import damaged graph");
    let saved = entries(&deckcraft_pptx::export(&doc).expect("safe export"));
    let (_, rels) = saved.iter().find(|(n, _)| n == "ppt/slides/_rels/slide1.xml.rels").expect("slide rels");
    let rels = std::str::from_utf8(rels).expect("rels XML");
    assert!(!rels.contains("diagramData"), "a partial diagram must not be written with dangling references");
    let (_, slide) = saved.iter().find(|(n, _)| n == "ppt/slides/slide1.xml").expect("slide");
    assert!(std::str::from_utf8(slide).expect("slide XML").contains("Owned diagram"), "the fallback shape is retained");
}

#[test]
fn a_missing_nested_image_uses_the_existing_safe_fallback() {
    let mut files = entries(&fixture());
    files.retain(|(name, _)| name != "ppt/media/image1.png");
    assert_fallback(&package(&files));
}

#[test]
fn deeply_nested_parts_use_the_existing_safe_fallback() {
    let mut files = entries(&fixture());
    let mut source = "ppt/media/image1.png".to_string();
    for i in 0..10 {
        let target = format!("ppt/diagrams/nested{i}.xml");
        let (folder, file) = source.rsplit_once('/').expect("source path");
        files.push((
            format!("{folder}/_rels/{file}.rels"),
            format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="child" Type="{RT}customXml" Target="/{}"/></Relationships>"#, target).into_bytes(),
        ));
        files.push((target.clone(), b"<owned/>".to_vec()));
        source = target;
    }
    assert_fallback(&package(&files));
}
