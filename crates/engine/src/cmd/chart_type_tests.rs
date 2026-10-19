//! `chart.type` — the Change Chart Type gallery of the Chart Design tab.

use deckcraft_model::ShapeId;
use deckcraft_model::chart::{Chart, ChartType};
use serde_json::{Value, json};

use super::*;

fn insert_chart(s: &mut Session) -> ShapeId {
    ShapeId(s.execute("insert.chart", &json!({})).unwrap()["id"].as_u64().unwrap() as u32)
}

fn chart_of(s: &Session, id: ShapeId) -> Chart {
    match s.doc().unwrap().shape(id).cloned().unwrap().kind {
        ShapeKind::Chart(c) => *c,
        other => panic!("expected a chart, got {other:?}"),
    }
}

fn series(names: &[&str]) -> Value {
    Value::Array(names.iter().map(|n| json!({"name": n, "values": [1.0, 2.0, 3.0]})).collect())
}

#[test]
fn the_gallery_switches_the_type_and_its_names() {
    let mut s = Session::with_new();
    let id = insert_chart(&mut s);
    assert_eq!(chart_of(&s, id).kind, ChartType::Column, "no `type` is a column chart");
    assert_eq!(chart_of(&s, id).series.len(), 3, "the sample column has three series");

    let n = s.doc().unwrap().history.undo.len();
    assert_eq!(s.execute("chart.type", &json!({"type": "pie"})).unwrap(), json!(1), "one chart changed");
    assert_eq!(chart_of(&s, id).kind, ChartType::Pie);
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1);
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Change Chart Type");

    // The names take the spellings the gallery uses: case, spaces and the Office aliases.
    s.execute("chart.type", &json!({"type": "Line With Markers"})).unwrap();
    assert_eq!(chart_of(&s, id).kind, ChartType::LineMarkers);
    s.execute("chart.type", &json!({"type": "donut"})).unwrap();
    assert_eq!(chart_of(&s, id).kind, ChartType::Doughnut);
    s.execute("chart.type", &json!({"type": "XY"})).unwrap();
    assert_eq!(chart_of(&s, id).kind, ChartType::Scatter);
    // An unknown name is a column chart, not an error.
    s.execute("chart.type", &json!({"type": "squiggly"})).unwrap();
    assert_eq!(chart_of(&s, id).kind, ChartType::Column);
}

#[test]
fn colours_vary_by_point_only_where_they_make_sense() {
    let mut s = Session::with_new();
    let id = insert_chart(&mut s);
    assert!(!chart_of(&s, id).vary_colors, "a column chart of three series does not vary");

    s.execute("chart.type", &json!({"type": "pie"})).unwrap();
    assert!(chart_of(&s, id).vary_colors, "a pie colours each point");
    s.execute("chart.type", &json!({"type": "column"})).unwrap();
    assert!(!chart_of(&s, id).vary_colors, "and they go off once it is not a pie");

    // A single series keeps the variants it had; a second one stops them.
    s.execute("chart.data", &json!({"series": series(&["Only"])})).unwrap();
    s.execute("chart.type", &json!({"type": "pie"})).unwrap();
    assert!(chart_of(&s, id).vary_colors);
    s.execute("chart.type", &json!({"type": "column"})).unwrap();
    assert!(chart_of(&s, id).vary_colors, "one series keeps the variants");
    s.execute("chart.data", &json!({"series": series(&["A", "B"])})).unwrap();
    s.execute("chart.type", &json!({"type": "column"})).unwrap();
    assert!(!chart_of(&s, id).vary_colors, "two series do not vary");
}

#[test]
fn a_waterfall_gets_its_gridlines() {
    let mut s = Session::with_new();
    let id = insert_chart(&mut s);
    assert!(chart_of(&s, id).gridlines, "a column chart shows its value gridlines");
    s.execute("chart.options", &json!({"gridlines": false})).unwrap();
    assert!(!chart_of(&s, id).gridlines);
    s.execute("chart.type", &json!({"type": "waterfall"})).unwrap();
    let c = chart_of(&s, id);
    assert_eq!(c.kind, ChartType::Waterfall);
    assert!(c.gridlines, "a waterfall turns the gridlines back on");
}

#[test]
fn it_refuses_what_is_not_a_chart_and_takes_an_id_for_one_that_is_not_selected() {
    let mut s = Session::with_new();
    let err = s.execute("chart.type", &json!({"type": "pie"})).unwrap_err().to_string();
    assert!(err.contains("nothing selected"), "{err}");
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 100, 60]})).unwrap();
    let err = s.execute("chart.type", &json!({"type": "pie"})).unwrap_err().to_string();
    assert!(err.contains("not a chart"), "{err}");

    // The chart can be named instead of selected.
    let id = insert_chart(&mut s);
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 100, 60]})).unwrap();
    s.execute("chart.type", &json!({"type": "pie", "id": id.0})).unwrap();
    assert_eq!(chart_of(&s, id).kind, ChartType::Pie);
}
