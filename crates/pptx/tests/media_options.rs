//! Media playback options survive a PPTX round trip (#74): the timing tree's `p:video`/`p:audio`
//! nodes carry volume, the slide span, hide-while-not-playing, loop, rewind and the automatic
//! start — the options PowerPoint keeps there, which `p14:media` (trim, fades, bookmarks) does
//! not hold.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{MediaClip, Presentation, Shape, ShapeId, ShapeKind, defaults};

fn deck(clip: MediaClip) -> Presentation {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, deckcraft_model::LayoutType::Blank).unwrap_or_default();
    let mut slide = defaults::new_slide(&mut p, layout).expect("slide");
    let (name, mime) = if clip.video { ("clip.mp4", "video/mp4") } else { ("clip.m4a", "audio/mp4") };
    let media = p.add_media(name, mime, vec![0u8; 64]).expect("media");
    let mut shape = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Narration".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 80.0, 80.0)),
        ..Default::default()
    };
    shape.kind = ShapeKind::Media(MediaClip { media, ..clip });
    slide.shapes.push(shape);
    p.slides.push(Arc::new(slide));
    p
}

fn round(p: &Presentation) -> Presentation {
    let bytes = deckcraft_pptx::export(p).expect("export");
    deckcraft_pptx::import(&bytes).expect("import")
}

fn clip_of(p: &Presentation) -> &MediaClip {
    let s = p.slides.first().expect("slide").shapes.first().expect("shape");
    match &s.kind {
        ShapeKind::Media(m) => m,
        _ => panic!("not a media shape: {:?}", s.kind),
    }
}

/// The slide part's XML of an exported deck.
fn slide_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    for i in 0..zip.len() {
        let name = zip.by_index(i).expect("entry").name().to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            let mut data = vec![];
            zip.by_index(i).expect("entry").read_to_end(&mut data).expect("read");
            return String::from_utf8(data).expect("xml");
        }
    }
    panic!("no slide part");
}

/// The file id of the media shape in an exported deck.
fn shape_id(xml: &str) -> u32 {
    xml.split("<p:pic>")
        .find(|s| s.contains("Narration"))
        .and_then(|s| s.split("<p:cNvPr id=\"").nth(1))
        .and_then(|s| s.split('"').next())
        .and_then(|s| s.parse::<u32>().ok())
        .expect("the shape's file id")
}

/// Import the deck with `node` - a `p:audio`/`p:video` element - patched into the slide's
/// timing tree, the way a file authored outside this repo carries it. `{spid}` in the node is
/// the media shape's file id.
fn import_with_timing(node: &str) -> Presentation {
    let p = deck(MediaClip { video: node.contains("<p:video"), volume: 1.0, ..Default::default() });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = slide_xml(&bytes);
    let timing = format!(
        "<p:timing><p:tnLst><p:par><p:cTn id=\"1\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"><p:childTnLst>\
{node}\
</p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>",
        node = node.replace("{spid}", &shape_id(&xml).to_string())
    );
    let patched = xml.replacen("</p:sld>", &format!("{timing}</p:sld>"), 1);

    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_string();
        let mut data = vec![];
        entry.read_to_end(&mut data).expect("read");
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            data = patched.clone().into_bytes();
        }
        out.start_file(&name, opts).expect("start");
        out.write_all(&data).expect("write");
    }
    let bytes = out.finish().expect("finish").into_inner();
    deckcraft_pptx::import(&bytes).expect("import")
}

/// Every option the dialog offers, all preserved through save, open in PowerPoint's format and
/// reopen here.
#[test]
fn media_options_round_trip() {
    for video in [false, true] {
        let p = deck(MediaClip {
            video,
            autoplay: true,
            loop_play: true,
            rewind: true,
            play_across_slides: true,
            hide_while_not_playing: true,
            full_screen: video,
            volume: 0.35,
            trim_start_ms: 400,
            trim_end_ms: 1200,
            fade_in_ms: 100,
            fade_out_ms: 150,
            bookmarks: vec![("Cue".into(), 700)],
            ..Default::default()
        });
        let back = round(&p);
        let c = clip_of(&back);
        assert!(c.autoplay, "autoplay survives");
        assert!(c.loop_play, "loop survives");
        assert!(c.rewind, "rewind survives");
        assert!(c.play_across_slides, "play across slides survives");
        assert!(c.hide_while_not_playing, "hide while not playing survives");
        assert_eq!(c.full_screen, video, "video full screen survives");
        assert!((c.volume - 0.35).abs() < 1e-6, "volume survives, got {}", c.volume);
        assert_eq!((c.trim_start_ms, c.trim_end_ms, c.fade_in_ms, c.fade_out_ms), (400, 1200, 100, 150));
        assert_eq!(c.bookmarks, [("Cue".into(), 700)]);
    }
}

/// A video with only the automatic start set keeps every other option at its default.
#[test]
fn sparse_options_round_trip() {
    let p = deck(MediaClip { video: true, autoplay: true, volume: 1.0, ..Default::default() });
    let back = round(&p);
    let c = clip_of(&back);
    assert!(c.autoplay);
    assert!(c.video);
    assert!(!c.loop_play && !c.rewind && !c.play_across_slides && !c.hide_while_not_playing);
    assert!((c.volume - 1.0).abs() < 1e-6);
}

/// A clip with nothing set writes no timing node at all.
#[test]
fn default_clip_writes_no_timing_tree() {
    let p = deck(MediaClip { volume: 1.0, ..Default::default() });
    let xml = slide_xml(&deckcraft_pptx::export(&p).expect("export"));
    assert!(!xml.contains("<p:audio"), "{xml}");
    assert!(!xml.contains("<p:video"), "{xml}");
    assert!(!xml.contains("p:timing"), "{xml}");
}

/// A timing tree the way PowerPoint writes one is read back, for files authored outside this
/// repo: the options are read onto the clip the node targets.
#[test]
fn powerpoint_style_timing_is_read() {
    let back = import_with_timing(
        "<p:audio><p:cMediaNode vol=\"35000\" numSld=\"999\" showWhenStopped=\"0\">\
<p:cTn id=\"2\" fill=\"remove\" display=\"0\" masterRel=\"sameClick\" repeatCount=\"indefinite\">\
<p:stCondLst><p:cond delay=\"0\"/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl></p:cMediaNode></p:audio>",
    );
    let c = clip_of(&back);
    assert!((c.volume - 0.35).abs() < 1e-6, "volume read, got {}", c.volume);
    assert!(c.play_across_slides, "999 slides spans slides");
    assert!(c.hide_while_not_playing, "showWhenStopped=0 hides");
    assert!(c.loop_play && c.rewind && c.autoplay);
}

/// A volume that is not a number is ignored: the clip keeps the full volume instead of a
/// number that is not one.
#[test]
fn a_nan_volume_is_ignored() {
    let back = import_with_timing(
        "<p:audio><p:cMediaNode vol=\"NaN\">\
<p:cTn id=\"2\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl></p:cMediaNode></p:audio>",
    );
    let c = clip_of(&back);
    assert!(c.volume.is_finite(), "the volume is a number, got {}", c.volume);
    assert!((c.volume - 1.0).abs() < 1e-6, "the volume stays the default, got {}", c.volume);
}

/// `showWhenStopped` is an XML boolean: "false" means the same as "0".
#[test]
fn show_when_stopped_reads_both_boolean_spellings() {
    let hidden = import_with_timing(
        "<p:audio><p:cMediaNode showWhenStopped=\"false\">\
<p:cTn id=\"2\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl></p:cMediaNode></p:audio>",
    );
    assert!(clip_of(&hidden).hide_while_not_playing, "\"false\" hides the clip while it is not playing");
    let shown = import_with_timing(
        "<p:audio><p:cMediaNode showWhenStopped=\"true\">\
<p:cTn id=\"2\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl></p:cMediaNode></p:audio>",
    );
    assert!(!clip_of(&shown).hide_while_not_playing, "\"true\" leaves the clip on screen");
}

/// A clip that starts on a click is not automatic, whatever its delay says.
#[test]
fn a_click_to_play_clip_is_not_automatic() {
    let back = import_with_timing(
        "<p:video><p:cMediaNode>\
<p:cTn id=\"2\"><p:stCondLst><p:cond evt=\"onClick\" delay=\"0\"/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl></p:cMediaNode></p:video>",
    );
    let c = clip_of(&back);
    assert!(!c.autoplay, "a click condition waits for the click");
}

/// A volume that is not a number is not written either: the clip keeps its full volume.
#[test]
fn a_nan_volume_is_not_written() {
    let p = deck(MediaClip { video: true, autoplay: true, volume: f64::NAN, ..Default::default() });
    let xml = slide_xml(&deckcraft_pptx::export(&p).expect("export"));
    assert!(xml.contains("vol=\"100000\""), "the full volume is written: {xml}");
}

/// Each option alone must cause a timing tree to be written, even without slide animations.
#[test]
fn individual_options_round_trip_without_animations() {
    let base = MediaClip { video: true, volume: 1.0, ..Default::default() };
    let cases = [
        MediaClip { autoplay: true, ..base.clone() },
        MediaClip { loop_play: true, ..base.clone() },
        MediaClip { rewind: true, ..base.clone() },
        MediaClip { play_across_slides: true, ..base.clone() },
        MediaClip { hide_while_not_playing: true, ..base.clone() },
        MediaClip { full_screen: true, ..base.clone() },
        MediaClip { volume: 0.25, ..base.clone() },
        MediaClip { volume: 0.0, ..base },
    ];
    for expected in cases {
        let p = deck(expected.clone());
        let back = round(&p);
        let actual = clip_of(&back);
        assert_eq!(actual.autoplay, expected.autoplay);
        assert_eq!(actual.loop_play, expected.loop_play);
        assert_eq!(actual.rewind, expected.rewind);
        assert_eq!(actual.play_across_slides, expected.play_across_slides);
        assert_eq!(actual.hide_while_not_playing, expected.hide_while_not_playing);
        assert_eq!(actual.full_screen, expected.full_screen);
        assert!((actual.volume - expected.volume).abs() < 1e-6);
    }
}

#[test]
fn video_full_screen_uses_the_standard_attribute() {
    let p = deck(MediaClip { video: true, full_screen: true, volume: 1.0, ..Default::default() });
    let xml = slide_xml(&deckcraft_pptx::export(&p).expect("export"));
    assert!(xml.contains("<p:video fullScrn=\"1\">"), "{xml}");
    for value in ["true", "1"] {
        let back = import_with_timing(&format!(
            "<p:video fullScrn=\"{value}\"><p:cMediaNode><p:cTn id=\"2\"/>\
<p:tgtEl><p:spTgt spid=\"{{spid}}\"/></p:tgtEl></p:cMediaNode></p:video>"
        ));
        assert!(clip_of(&back).full_screen);
    }
}

#[test]
fn grouped_clips_with_duplicate_model_ids_keep_their_own_options() {
    let mut p = deck(MediaClip { autoplay: true, volume: 0.25, ..Default::default() });
    let group_id = ShapeId(p.alloc_id().expect("group id"));
    let s = Arc::make_mut(&mut p.slides[0]);
    let mut second = s.shapes[0].clone();
    second.name = "Second".into();
    let ShapeKind::Media(clip) = &mut second.kind else { panic!("media") };
    clip.autoplay = false;
    clip.loop_play = true;
    clip.volume = 0.75;
    let first = s.shapes.remove(0);
    s.shapes.push(Shape {
        id: group_id,
        xfrm: Some(Xfrm::new(0.0, 0.0, 400.0, 400.0)),
        kind: ShapeKind::Group { children: vec![first, second], child: Xfrm::new(0.0, 0.0, 400.0, 400.0) },
        ..Default::default()
    });
    let back = round(&p);
    let children = back.slides[0].shapes[0].children();
    assert_eq!(children.len(), 2);
    assert_ne!(children[0].id, children[1].id, "the exporter gives duplicate ids separate file ids");
    let ShapeKind::Media(first) = &children[0].kind else { panic!("first clip") };
    let ShapeKind::Media(second) = &children[1].kind else { panic!("second clip") };
    assert!(first.autoplay && !first.loop_play);
    assert!(!second.autoplay && second.loop_play);
    assert!((first.volume - 0.25).abs() < 1e-6);
    assert!((second.volume - 0.75).abs() < 1e-6);
}

#[test]
fn media_and_animation_timing_round_trip_together() {
    let mut p = deck(MediaClip { video: true, full_screen: true, loop_play: true, volume: 0.25, ..Default::default() });
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.animations.push(deckcraft_model::anim::Animation { shape: slide.shapes[0].id, effect: "fade".into(), ..Default::default() });
    let back = round(&p);
    assert_eq!(back.slides[0].animations.len(), 1);
    assert_eq!(back.slides[0].animations[0].effect, "fade");
    let c = clip_of(&back);
    assert!(c.full_screen && c.loop_play);
    assert!(!c.autoplay);
    assert!((c.volume - 0.25).abs() < 1e-6);
}

#[test]
fn event_driven_or_invalid_start_conditions_are_not_autoplay() {
    let conditions = [
        "evt=\"onNext\" delay=\"0\"",
        "evt=\"onPrev\" delay=\"0\"",
        "evt=\"onMouseOver\" delay=\"0\"",
        "evt=\"onBegin\" delay=\"0\"",
        "delay=\"NaN\"",
        "delay=\"inf\"",
        "delay=\"-1\"",
        "delay=\"nonsense\"",
        "delay=\"indefinite\"",
        "",
    ];
    for condition in conditions {
        let back = import_with_timing(&format!(
            "<p:audio><p:cMediaNode><p:cTn id=\"2\"><p:stCondLst><p:cond {condition}/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{{spid}}\"/></p:tgtEl></p:cMediaNode></p:audio>"
        ));
        assert!(!clip_of(&back).autoplay, "{condition}");
    }
}

#[test]
fn a_parent_click_sequence_does_not_become_autoplay() {
    let back = import_with_timing(
        "<p:seq><p:cTn id=\"2\" nodeType=\"interactiveSeq\"><p:stCondLst>\
<p:cond evt=\"onClick\" delay=\"0\"/></p:stCondLst><p:childTnLst>\
<p:audio><p:cMediaNode><p:cTn id=\"3\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst></p:cTn>\
<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl></p:cMediaNode></p:audio>\
</p:childTnLst></p:cTn></p:seq>",
    );
    assert!(!clip_of(&back).autoplay);
}

#[test]
fn hostile_volume_is_finite_and_bounded_in_both_directions() {
    for (value, expected) in [("NaN", 1.0), ("inf", 1.0), ("-inf", 1.0), ("-100", 0.0), ("200000", 1.0)] {
        let back = import_with_timing(&format!(
            "<p:audio><p:cMediaNode vol=\"{value}\"><p:cTn id=\"2\"/>\
<p:tgtEl><p:spTgt spid=\"{{spid}}\"/></p:tgtEl></p:cMediaNode></p:audio>"
        ));
        assert_eq!(clip_of(&back).volume, expected, "{value}");
    }
    for (value, expected) in [(f64::NAN, 1.0), (f64::INFINITY, 1.0), (-0.5, 0.0), (1.5, 1.0)] {
        let p = deck(MediaClip { autoplay: true, volume: value, ..Default::default() });
        assert_eq!(clip_of(&round(&p)).volume, expected);
    }
}

#[test]
fn deeply_nested_media_timing_is_bounded() {
    let node = format!(
        "{}<p:audio><p:cMediaNode vol=\"25000\"><p:cTn id=\"2\"/>\
<p:tgtEl><p:spTgt spid=\"{{spid}}\"/></p:tgtEl></p:cMediaNode></p:audio>{}",
        "<p:par>".repeat(70),
        "</p:par>".repeat(70)
    );
    let back = import_with_timing(&node);
    assert_eq!(clip_of(&back).volume, 1.0, "unsupported nesting is ignored");
}

#[test]
fn linked_audio_and_video_keep_playback_options_without_fetching_media() {
    for video in [false, true] {
        let mut p = deck(MediaClip { video, autoplay: true, loop_play: true, full_screen: video, volume: 0.5, ..Default::default() });
        let id = clip_of(&p).media;
        let media = p.media.iter_mut().find(|m| m.id == id).expect("media");
        let link = if video { "https://example.invalid/owned/clip.mp4" } else { "https://example.invalid/owned/clip.m4a" };
        media.link = Some(link.into());
        media.data = Arc::new(Vec::new());
        let back = round(&p);
        let clip = clip_of(&back);
        assert!(clip.autoplay && clip.loop_play);
        assert_eq!(clip.full_screen, video);
        assert_eq!(clip.volume, 0.5);
        assert_eq!(back.media(clip.media).expect("linked media").link.as_deref(), Some(link));
    }
}
