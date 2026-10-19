//! `file.render`: the open slide as a PNG an agent can look at.

use serde_json::{Value, json};

use super::*;

fn rendered(s: &mut Session, p: Value) -> Value {
    s.execute("file.render", &p).unwrap()
}

#[test]
fn a_slide_renders_to_a_png_at_the_slide_size() {
    let mut s = Session::with_new();
    let n = s.doc().unwrap().history.undo.len();
    let v = rendered(&mut s, json!({}));
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(960), Some(540)));
    let png = base64_decode(v["png"].as_str().unwrap()).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "the payload is a PNG");
    assert_eq!(s.doc().unwrap().history.undo.len(), n, "rendering edits nothing");

    // `scale` scales the image; `slide` picks which one.
    s.execute("slide.new", &json!({})).unwrap();
    let half = rendered(&mut s, json!({"scale": 0.5}));
    assert_eq!((half["width"].as_u64(), half["height"].as_u64()), (Some(480), Some(270)));
    assert!(rendered(&mut s, json!({"slide": 0})).get("png").is_some());
    let err = s.execute("file.render", &json!({"slide": 9})).unwrap_err().to_string();
    assert!(err.contains("no slide 9"), "{err}");
}
