//! `commands.list`: the deck of every command an agent can call, with the session's take on
//! whether the selection lets it run.

use serde_json::{Value, json};

use super::*;

fn listing(s: &mut Session, filter: &str) -> Vec<Value> {
    s.execute("commands.list", &json!({"filter": filter})).unwrap().as_array().cloned().unwrap()
}

/// The one entry with this exact id (`filter` may match neighbours).
fn one(s: &mut Session, id: &str) -> Value {
    let mut found: Vec<Value> = listing(s, id).into_iter().filter(|c| c["id"] == id).collect();
    assert_eq!(found.len(), 1, "{id}: {found:?}");
    found.remove(0)
}

#[test]
fn every_command_comes_back_with_its_label_params_and_state() {
    let mut s = Session::with_new();
    let all = listing(&mut s, "");
    assert_eq!(all.len(), s.commands().len(), "no filter lists every command");
    assert!(all.len() > 300, "the deck is the usual long list: {}", all.len());
    for c in &all {
        assert!(c["id"].as_str().is_some_and(|i| !i.is_empty()), "{c}");
        assert!(c["label"].as_str().is_some_and(|l| !l.is_empty()), "{c}");
        assert!(c["params"].is_string(), "{c}");
        assert!(c["enabled"].is_boolean(), "{c}");
        assert!(c["menu"].is_array(), "{c}");
    }
    let list = one(&mut s, "commands.list");
    assert_eq!(list["label"], "List Commands");
    assert!(list["params"].as_str().unwrap().contains("filter"), "{list}");
}

#[test]
fn the_filter_is_a_case_insensitive_substring_of_id_or_label() {
    let mut s = Session::with_new();
    let charts = listing(&mut s, "chart.");
    assert!(charts.len() >= 3, "{charts:?}");
    for want in ["chart.type", "chart.data", "chart.options"] {
        assert!(charts.iter().any(|c| c["id"] == want), "{want} in {charts:?}");
    }
    assert!(charts.iter().all(|c| c["id"].as_str().unwrap().contains("chart.")), "{charts:?}");
    assert_eq!(listing(&mut s, "CHART.TYPE").len(), 1, "an id matches whatever its case");
    let label = listing(&mut s, "match formatting");
    assert_eq!(label.len(), 1, "{label:?}");
    assert_eq!(label[0]["id"], "edit.pasteText", "a label matches too");
    assert!(listing(&mut s, "zzz").is_empty());
}

#[test]
fn enabled_follows_the_selection_and_a_refusal_says_why() {
    let mut s = Session::with_new();
    let doc = one(&mut s, "document.inspect");
    assert_eq!(doc["enabled"], true);
    assert!(doc.get("disabled_reason").is_none(), "a runnable command gives no reason: {doc}");

    let compose = one(&mut s, "text.compose");
    assert_eq!(compose["enabled"], false);
    assert_eq!(compose["disabled_reason"], "no text insertion point");

    let fill = one(&mut s, "shape.fill");
    assert_eq!(fill["enabled"], false);
    assert_eq!(fill["disabled_reason"], "nothing selected");
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 100, 60]})).unwrap();
    let fill = one(&mut s, "shape.fill");
    assert_eq!(fill["enabled"], true, "{fill}");
    assert!(fill.get("disabled_reason").is_none(), "{fill}");
}
