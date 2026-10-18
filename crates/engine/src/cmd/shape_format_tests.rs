//! `shape.adjust`, `shape.quickStyle`, `shape.altText` and `shape.presets` — the drag handles, the
//! Quick Styles gallery, the Accessibility pane and the shape gallery — plus `picture.crop` and
//! `picture.adjust` had no test. Each is one small handler over `edit_shapes`/`edit_geometry`, but
//! each decides something a user sees.

use deckcraft_model::{Shape, ShapeId, ShapeKind};
use serde_json::json;

use super::*;

/// An 8×8 red PNG.
fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68,
        65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

fn inserted(s: &mut Session, cmd: &str, p: Value) -> ShapeId {
    ShapeId(s.execute(cmd, &p).expect("insert")["id"].as_u64().expect("an id") as u32)
}

fn shape_of(s: &Session, id: ShapeId) -> Shape {
    s.doc().expect("a document").shape(id).cloned().expect("the shape")
}

#[test]
fn adjust_shape_walks_the_preset_handles() {
    let mut s = Session::with_new();
    let id = inserted(&mut s, "shape.insert", json!({"preset": "roundRect", "rect": [10, 10, 100, 60]}));
    let err = s.execute("shape.adjust", &json!({"index": 0})).unwrap_err();
    assert!(err.to_string().contains("missing `value`"), "{err}");
    let adj = |s: &Session| match shape_of(s, id).geom {
        Geom::Preset { adj, .. } => adj,
        other => panic!("expected a preset, got {other:?}"),
    };

    assert_eq!(s.execute("shape.adjust", &json!({"index": 0, "value": 40000})).unwrap(), json!(1));
    assert_eq!(adj(&s), vec![40000.0], "the first handle is the value given");

    s.execute("shape.adjust", &json!({"index": 2, "value": 7.5})).unwrap();
    let a = adj(&s);
    assert_eq!(a.len(), 3, "a later handle grows the list");
    assert_eq!(a[0], 40000.0, "and keeps the earlier ones");
    assert_eq!(a[2], 7.5);

    // The index is capped at 16, as PowerPoint's handle list is.
    s.execute("shape.adjust", &json!({"index": 99, "value": 1.0})).unwrap();
    let a = adj(&s);
    assert_eq!(a.len(), 17);
    assert_eq!(a[16], 1.0, "99 is handle 16");

    // Only the selected shape is adjusted.
    let other = inserted(&mut s, "shape.insert", json!({"preset": "roundRect", "rect": [10, 100, 100, 60]}));
    s.execute("shape.adjust", &json!({"index": 0, "value": 1000})).unwrap();
    match shape_of(&s, other).geom {
        Geom::Preset { adj, .. } => assert_eq!(adj, vec![1000.0]),
        other => panic!("expected a preset, got {other:?}"),
    }
    assert_eq!(adj(&s).len(), 17, "the shape selected before is untouched");
}

#[test]
fn quick_style_paints_the_shape_and_its_text_from_the_theme() {
    let mut s = Session::with_new();
    let id = inserted(&mut s, "shape.insert", json!({"preset": "rect", "rect": [10, 20, 100, 60]}));
    s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
    s.execute("shape.effects", &json!({"shadow": "outer"})).unwrap();
    assert_eq!(s.execute("shape.quickStyle", &json!({"index": 9})).unwrap(), json!(1));
    {
        let sh = shape_of(&s, id);
        assert_eq!(sh.style, Some(ShapeStyle::accent(SchemeSlot::Accent2)), "index 9 is row 1, second accent");
        assert!(sh.fill.is_none() && sh.line.is_none(), "the style supplies fill and line now");
        assert!(sh.effects.is_none(), "a quick style clears hand-made effects");
    }

    // The text follows the style too: a run colour of its own is dropped.
    let t = inserted(&mut s, "insert.textBox", json!({"rect": [10, 100, 200, 50], "text": "Hi"}));
    s.edit(|doc, _| {
        let sl = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let sh = sl.shapes.iter_mut().find(|x| x.id == t).expect("the text box");
        for p in &mut sh.text.as_mut().expect("text").paragraphs {
            for run in &mut p.runs {
                run.props.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Accent3)));
            }
        }
        Ok(())
    })
    .unwrap();
    s.execute("shape.quickStyle", &json!({"index": 0})).unwrap();
    {
        let sh = shape_of(&s, t);
        assert!(sh.text.as_ref().expect("text").paragraphs.iter().all(|p| p.runs.iter().all(|r| r.props.fill.is_none())), "the run colour is gone");
        let st = sh.style.as_ref().expect("a style");
        assert_eq!(st.fill_ref.1, deckcraft_model::ColorRef::scheme(SchemeSlot::Lt1), "row 0 fills light");
        assert_eq!(st.line_ref.1, deckcraft_model::ColorRef::scheme(SchemeSlot::Dk1), "and outlines in the first slot");
    }

    // The index is capped, so 99 is the last style, 41.
    let a = inserted(&mut s, "shape.insert", json!({"preset": "rect", "rect": [10, 200, 100, 60]}));
    s.execute("shape.quickStyle", &json!({"index": 99})).unwrap();
    let b = inserted(&mut s, "shape.insert", json!({"preset": "rect", "rect": [10, 260, 100, 60]}));
    s.execute("shape.quickStyle", &json!({"index": 41})).unwrap();
    assert_eq!(shape_of(&s, a).style, shape_of(&s, b).style, "99 is capped at 41");
}

#[test]
fn alt_text_sets_the_description_and_marks_decorative() {
    let mut s = Session::with_new();
    let id = inserted(&mut s, "shape.insert", json!({"preset": "rect", "rect": [10, 10, 100, 60]}));
    assert_eq!(s.execute("shape.altText", &json!({"text": "A red square", "decorative": true})).unwrap(), json!(1));
    {
        let sh = shape_of(&s, id);
        assert_eq!(sh.descr, "A red square");
        assert!(sh.decorative, "marked decorative");
    }
    // Without `decorative` the flag is left alone, and no text clears the description.
    s.execute("shape.altText", &json!({"text": "A blue square"})).unwrap();
    assert!(shape_of(&s, id).decorative, "still decorative");
    s.execute("shape.altText", &json!({})).unwrap();
    {
        let sh = shape_of(&s, id);
        assert_eq!(sh.descr, "");
        assert!(sh.decorative);
    }
    // `decorative: false` takes the mark off again.
    s.execute("shape.altText", &json!({"text": "Square", "decorative": false})).unwrap();
    assert!(!shape_of(&s, id).decorative);
}

#[test]
fn the_shape_gallery_lists_every_preset_with_its_label_and_category() {
    let mut s = Session::with_new();
    let v = s.execute("shape.presets", &json!({})).unwrap();
    let list = v.as_array().expect("an array");
    assert!(list.len() > 20, "the gallery is the usual long list");
    assert!(
        list.iter().all(|i| i["name"].as_str().is_some_and(|x| !x.is_empty())
            && i["label"].as_str().is_some_and(|x| !x.is_empty())
            && i["category"].as_str().is_some_and(|x| !x.is_empty())),
        "every entry is complete"
    );
    let names: Vec<&str> = list.iter().filter_map(|i| i["name"].as_str()).collect();
    assert!(names.contains(&"rect"), "the plain rectangle is one of them");
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "names are unique");
}

#[test]
fn crop_and_corrections_land_on_picture_fills() {
    let mut s = Session::with_new();
    let id = inserted(&mut s, "insert.picture", json!({"name": "a.png", "data": base64_encode(&png())}));
    let fill = |s: &Session| match shape_of(s, id).kind {
        ShapeKind::Picture { fill } => fill,
        other => panic!("expected a picture, got {other:?}"),
    };

    assert_eq!(s.execute("picture.crop", &json!({"left": 0.25, "top": 0.1})).unwrap(), json!(1));
    assert_eq!(fill(&s).crop, [0.25, 0.1, 0.0, 0.0], "the edges that are not given stay flush");
    s.execute("picture.crop", &json!({"right": 2.0, "bottom": -20.0})).unwrap();
    assert_eq!(fill(&s).crop, [0.25, 0.1, 0.95, -10.0], "edges clamp to the range a crop can take");

    s.execute("picture.adjust", &json!({"brightness": 2.0, "contrast": -3.0, "saturation": 9.0, "grayscale": true, "transparency": 0.75})).unwrap();
    {
        let f = fill(&s);
        assert_eq!(f.adjust.brightness, 1.0, "brightness clamps to -1..1");
        assert_eq!(f.adjust.contrast, -1.0);
        assert_eq!(f.adjust.saturation, Some(4.0), "saturation clamps to 0..4");
        assert!(f.adjust.grayscale);
        assert_eq!(f.alpha, Some(0.25), "75% transparency is 25% opacity");
    }

    // A later call changes only what it passes; resetting the corrections keeps the transparency.
    s.execute("picture.adjust", &json!({"brightness": -0.5})).unwrap();
    {
        let f = fill(&s);
        assert_eq!(f.adjust.brightness, -0.5);
        assert_eq!(f.adjust.contrast, -1.0, "the other corrections stay");
    }
    s.execute("picture.adjust", &json!({"reset": true})).unwrap();
    {
        let f = fill(&s);
        assert_eq!(f.adjust, Default::default(), "reset clears the corrections");
        assert_eq!(f.alpha, Some(0.25), "but not the transparency");
    }

    // On a shape that is not a picture both are no-ops, like the rest of the Picture Format tab.
    inserted(&mut s, "shape.insert", json!({"preset": "rect", "rect": [10, 10, 100, 60]}));
    assert_eq!(s.execute("picture.crop", &json!({"left": 0.5})).unwrap(), json!(1));
    assert_eq!(s.execute("picture.adjust", &json!({"brightness": 0.5})).unwrap(), json!(1));
    assert_eq!(fill(&s).adjust, Default::default(), "the picture is untouched");
}
