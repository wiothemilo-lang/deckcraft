//! Files written before the SlideCraft → DeckCraft rename (`.slidecraft`): they carry the legacy
//! `mimetype` and the legacy `format` name in their manifest, and must still open.

use std::io::{Cursor, Write};

use super::*;

/// A package as it was written before the rename: `mimetype` and a manifest whose `format` is
/// `format`, holding `presentation` and its media.
fn package(format: &str, mimetype: &str, presentation: &Presentation) -> Vec<u8> {
    let m = Manifest { format: format.into(), version: VERSION, generator: "SlideCraft 0.9".into(), presentation: presentation.clone() };
    let json = serde_json::to_vec(&m).expect("the manifest serializes");
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        // The `mimetype` member is stored, not deflated, and comes first: that is what lets
        // [`sniff`] see the name in the first bytes of the file.
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("mimetype", stored).expect("mimetype");
        z.write_all(mimetype.as_bytes()).expect("the mimetype bytes");
        z.start_file("presentation.json", o).expect("presentation.json");
        z.write_all(&json).expect("the manifest bytes");
        // The media layout did not change with the name.
        for item in &presentation.media {
            if item.link.is_some() && item.data.is_empty() {
                continue;
            }
            z.start_file(format!("media/{}", item.id.0), o).expect("a media member");
            z.write_all(&item.data).expect("the media bytes");
        }
        z.finish().expect("the package closes");
    }
    buf.into_inner()
}

/// The manifest of a package, read back.
fn manifest_of(bytes: &[u8]) -> serde_json::Value {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("the package opens");
    let json = read_entry(&mut z, "presentation.json").expect("the manifest is there");
    serde_json::from_slice(&json).expect("the manifest is JSON")
}

#[test]
fn a_legacy_slidecraft_file_opens() {
    let p = Presentation::default();
    let bytes = package(LEGACY_FORMAT_NAME, LEGACY_MIMETYPE, &p);
    // The file the user picks is recognised by its name and by its bytes.
    assert!(sniff(&bytes), "the legacy mimetype is recognised");
    let back = load(&bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.slides.len(), p.slides.len());
    assert_eq!(serde_json::to_value(&back.slides).unwrap(), serde_json::to_value(&p.slides).unwrap());
}

#[test]
fn a_legacy_file_keeps_its_media() {
    let mut p = Presentation::default();
    let id = p.add_media("a.png", "image/png", vec![1, 2, 3, 4]).expect("media");
    let bytes = package(LEGACY_FORMAT_NAME, LEGACY_MIMETYPE, &p);
    let (back, damaged) = load_checked(&bytes).unwrap_or_else(|e| panic!("{e}"));
    assert!(damaged.is_empty(), "{damaged:?}");
    assert_eq!(back.media(id).map(|m| m.data.as_slice()), Some(&[1u8, 2, 3, 4][..]));
}

#[test]
fn a_legacy_file_saves_as_a_current_one() {
    let bytes = package(LEGACY_FORMAT_NAME, LEGACY_MIMETYPE, &Presentation::default());
    let back = load(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let again = save(&back).unwrap_or_else(|e| panic!("{e}"));
    // The new file carries the current names, and opens.
    assert!(sniff(&again));
    let mut z = zip::ZipArchive::new(Cursor::new(again.as_slice())).expect("the package opens");
    assert_eq!(read_entry(&mut z, "mimetype").unwrap(), MIMETYPE.as_bytes());
    assert_eq!(manifest_of(&again)["format"], FORMAT_NAME);
    assert!(load(&again).is_ok());
}

#[test]
fn a_current_file_still_opens() {
    let p = Presentation::default();
    let bytes = save(&p).unwrap_or_else(|e| panic!("{e}"));
    assert!(sniff(&bytes));
    assert!(load(&bytes).is_ok());
    // And it carries the current names.
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).expect("the package opens");
    assert_eq!(read_entry(&mut z, "mimetype").unwrap(), MIMETYPE.as_bytes());
    assert_eq!(manifest_of(&bytes)["format"], FORMAT_NAME);
}

#[test]
fn only_our_manifest_names_are_read() {
    // A file of somebody else's says so in its manifest, and is refused with that reason. The
    // names are compared whole: a near miss ("SlideCraft", "slidecrafter") is not ours.
    for name in ["powerpoint", "slidecrafter", "SlideCraft", ""] {
        let bytes = package(name, MIMETYPE, &Presentation::default());
        match load(&bytes) {
            Err(FormatError::NotOurs(found)) => assert_eq!(found, name, "{name:?}"),
            Ok(_) => panic!("{name:?} was read as ours"),
            Err(e) => panic!("{name:?}: {e}"),
        }
    }
}

#[test]
fn a_legacy_file_saves_as_a_legacy_one() {
    let p = Presentation::default();
    let bytes = save_legacy(&p).unwrap_or_else(|e| panic!("{e}"));
    // The file keeps the old names, so the name it is saved under matches what it holds.
    assert!(sniff(&bytes));
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).expect("the package opens");
    assert_eq!(read_entry(&mut z, "mimetype").unwrap(), LEGACY_MIMETYPE.as_bytes());
    assert_eq!(manifest_of(&bytes)["format"], LEGACY_FORMAT_NAME);
    assert!(load(&bytes).is_ok());
}

#[test]
fn sniff_and_load_agree_on_a_legacy_file() {
    // The legacy mimetype is what a name-based filter sees, and the manifest's name is what
    // the loader reads: the two must accept the same file.
    let bytes = package(LEGACY_FORMAT_NAME, LEGACY_MIMETYPE, &Presentation::default());
    assert!(sniff(&bytes));
    assert!(load(&bytes).is_ok());
    // A file carrying the legacy mimetype but somebody else's manifest is still not ours.
    let foreign = package("powerpoint", LEGACY_MIMETYPE, &Presentation::default());
    assert!(sniff(&foreign));
    assert!(matches!(load(&foreign), Err(FormatError::NotOurs(_))));
}
