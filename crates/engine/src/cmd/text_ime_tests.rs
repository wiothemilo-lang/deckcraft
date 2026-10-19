//! `text.compose` / `text.commit`: what an input method shows before it commits, and the one
//! undo step the whole composition leaves behind.

use deckcraft_model::ShapeId;
use serde_json::json;

use super::*;

fn text_box(s: &mut Session, text: &str) -> ShapeId {
    ShapeId(s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 100.0], "text": text})).unwrap()["id"].as_u64().unwrap() as u32)
}

fn caret(s: &mut Session, id: ShapeId, anchor: (usize, usize), at: (usize, usize)) {
    s.select(|_, sel| sel.text = Some(crate::TextSel { shape: id, anchor, caret: at, ..Default::default() })).unwrap();
}

fn text(s: &Session, id: ShapeId) -> String {
    s.doc().unwrap().shape(id).cloned().unwrap().text.unwrap_or_default().text()
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

#[test]
fn a_preedit_is_typed_in_place_replaced_and_cancelled_without_undo_steps() {
    let mut s = Session::with_new();
    let id = text_box(&mut s, "Hi");
    let n = undo_len(&s);
    let err = s.execute("text.compose", &json!({"text": "x"})).unwrap_err().to_string();
    assert!(err.contains("no text insertion point"), "without a caret the command is not even available: {err}");
    assert_eq!(undo_len(&s), n);

    // With one, `text` is what the preedit carries.
    caret(&mut s, id, (0, 2), (0, 2));
    let err = s.execute("text.compose", &json!({})).unwrap_err().to_string();
    assert!(err.contains("missing `text`"), "{err}");
    assert_eq!(undo_len(&s), n);
    assert_eq!(s.execute("text.compose", &json!({"text": "に"})).unwrap(), json!({"composing": true, "start": [0, 2], "len": 1}));
    assert_eq!(text(&s, id), "Hiに");
    assert!(s.doc().unwrap().composition.is_some());
    assert_eq!(undo_len(&s), n, "a live preedit is not an undo step");

    // A second preedit replaces the first; the control characters of the IME stream are dropped.
    assert_eq!(s.execute("text.compose", &json!({"text": "日本\u{7}\n"})).unwrap(), json!({"composing": true, "start": [0, 2], "len": 2}));
    assert_eq!(text(&s, id), "Hi日本", "the new preedit replaces the old one");

    // An empty preedit cancels: the presentation goes back to before the composition began.
    assert_eq!(s.execute("text.compose", &json!({"text": ""})).unwrap(), json!({"composing": false}));
    assert_eq!(text(&s, id), "Hi");
    assert!(s.doc().unwrap().composition.is_none());
    assert_eq!(undo_len(&s), n);
}

#[test]
fn a_preedit_over_a_selection_replaces_it_and_keeps_the_active_range() {
    let mut s = Session::with_new();
    let id = text_box(&mut s, "Hi");
    caret(&mut s, id, (0, 0), (0, 2));
    assert_eq!(s.execute("text.compose", &json!({"text": "あ", "active": [0, 1]})).unwrap(), json!({"composing": true, "start": [0, 0], "len": 1}));
    assert_eq!(text(&s, id), "あ");
    let c = s.doc().unwrap().composition.clone().unwrap();
    assert_eq!(c.active, Some((0, 1)), "the part being converted is kept");

    // Committing replaces the preedit and makes the whole composition one undo step.
    assert_eq!(s.execute("text.commit", &json!({"text": "亜"})).unwrap(), json!({"caret": [0, 1]}));
    assert_eq!(text(&s, id), "亜");
    assert!(s.doc().unwrap().composition.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&s, id), "Hi", "undo takes the whole composition out");
}

#[test]
fn committing_without_a_preedit_types_at_the_insertion_point_as_one_step() {
    let mut s = Session::with_new();
    let id = text_box(&mut s, "Hi");
    caret(&mut s, id, (0, 2), (0, 2));
    let n = undo_len(&s);

    assert_eq!(s.execute("text.commit", &json!({"text": "よ"})).unwrap(), json!({"caret": [0, 3]}));
    assert_eq!(text(&s, id), "Hiよ");
    assert_eq!(undo_len(&s), n + 1, "the committed text is one step");
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Type Text");

    // Empty text with nothing composing changes nothing at all.
    assert_eq!(s.execute("text.commit", &json!({"text": ""})).unwrap(), json!({"caret": [0, 3]}));
    assert_eq!(undo_len(&s), n + 1);
    let err = s.execute("text.commit", &json!({})).unwrap_err().to_string();
    assert!(err.contains("missing `text`"), "{err}");

    // A composition in flight and its commit coalesce into one more step.
    s.execute("text.compose", &json!({"text": "じ"})).unwrap();
    assert_eq!(text(&s, id), "Hiよじ");
    assert_eq!(undo_len(&s), n + 1, "the live preedit is not a step yet");
    assert_eq!(s.execute("text.commit", &json!({"text": "字"})).unwrap(), json!({"caret": [0, 4]}));
    assert_eq!(text(&s, id), "Hiよ字");
    assert_eq!(undo_len(&s), n + 2, "the whole composition is one step");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&s, id), "Hiよ");
}
