//! `edit.pasteText` — Paste and Match Formatting: replaces the selected text (keeping its
//! formatting), or makes a text box when nothing is being edited.

use deckcraft_model::ShapeId;
use serde_json::json;

use super::*;

fn text_box(s: &mut Session, text: &str) -> ShapeId {
    ShapeId(s.execute("insert.textBox", &json!({"rect": [100.0, 100.0, 300.0, 100.0], "text": text})).unwrap()["id"].as_u64().unwrap() as u32)
}

fn caret(s: &mut Session, id: ShapeId, anchor: (usize, usize), at: (usize, usize)) {
    s.select(|_, sel| sel.text = Some(crate::TextSel { shape: id, anchor, caret: at, ..Default::default() })).unwrap();
}

fn body(s: &Session, id: ShapeId) -> deckcraft_model::text::TextBody {
    s.doc().unwrap().shape(id).cloned().unwrap().text.unwrap_or_default()
}

#[test]
fn pasting_with_no_insertion_point_makes_a_text_box_a_quarter_in() {
    let mut s = Session::with_new();
    let n = s.doc().unwrap().history.undo.len();
    let r = s.execute("edit.pasteText", &json!({"text": "Hello"})).unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    assert_eq!(s.doc().unwrap().selection.shapes, vec![id]);
    let sh = s.doc().unwrap().shape(id).cloned().unwrap();
    assert_eq!(sh.text.as_ref().unwrap().text(), "Hello");
    let x = sh.xfrm.unwrap();
    assert_eq!((x.x, x.y, x.w, x.h), (240.0, 216.0, 480.0, 28.8), "a quarter in on a 960×540 slide, shrunk to its one line");
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1, "the whole paste is one step");
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Paste and Match Formatting");
}

#[test]
fn pasting_into_the_edited_text_replaces_the_selection_and_keeps_its_formatting() {
    let mut s = Session::with_new();
    let id = text_box(&mut s, "Hi");
    s.edit(|doc, _| {
        let sl = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let sh = sl.shapes.iter_mut().find(|x| x.id == id).expect("the text box");
        for p in &mut sh.text.as_mut().expect("text").paragraphs {
            for run in &mut p.runs {
                run.props.bold = Some(true);
            }
        }
        Ok(())
    })
    .unwrap();

    // Over a selection: the text is replaced and takes the formatting at its start.
    caret(&mut s, id, (0, 0), (0, 2));
    assert_eq!(s.execute("edit.pasteText", &json!({"text": "Yo"})).unwrap(), json!({"caret": [0, 2]}));
    let t = body(&s, id);
    assert_eq!(t.text(), "Yo");
    assert!(t.paragraphs[0].runs.iter().all(|r| r.props.bold == Some(true)), "{:?}", t.paragraphs[0].runs);

    // With no `text`, the clipboard is the source; at a caret the text goes in as typed.
    s.clipboard.plain = " there".to_string();
    caret(&mut s, id, (0, 2), (0, 2));
    assert_eq!(s.execute("edit.pasteText", &json!({})).unwrap(), json!({"caret": [0, 8]}));
    assert_eq!(body(&s, id).text(), "Yo there");
}
