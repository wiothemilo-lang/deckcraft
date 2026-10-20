//! Slide transitions (`p:transition`) and animations (`p:timing`).

use std::collections::HashMap;

use deckcraft_model::anim::{AnimClass, AnimStart, Animation, TextBuild, Transition};
use deckcraft_model::{Shape, ShapeId, ShapeKind};

use super::shapes::IdCtx;
use super::{Imp, Part, dml};
use crate::maps;
use crate::xml::El;

fn ms_attr(e: &El, name: &str) -> Option<u32> {
    let v = e.attr(name)?.trim();
    if v == "indefinite" {
        return None;
    }
    v.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 86_400_000.0) as u32)
}

pub fn transition(imp: &mut Imp, part: &Part, t: &El) -> Option<Transition> {
    let effect = t.elements().find(|e| !matches!(e.local(), "sndAc" | "extLst"));
    let mut tr = Transition { advance_on_click: t.bool("advClick").unwrap_or(true), advance_after_ms: ms_attr(t, "advTm"), ..Default::default() };
    match effect.and_then(maps::transition_from_xml) {
        Some((k, o)) => {
            tr.kind = k;
            tr.option = o;
        }
        None => match effect {
            Some(e) => {
                tr.kind = e.local().to_string();
                tr.raw = Some(part.keep(t));
            }
            None => tr.kind = "none".into(),
        },
    }
    let default_dur = deckcraft_model::anim::TRANSITIONS.iter().find(|x| x.0 == tr.kind).map(|x| x.3).unwrap_or(1000);
    // `spd` defaults to "fast" in the schema; PowerPoint omits both attributes for 0.5 s.
    tr.duration_ms = ms_attr(t, "dur").unwrap_or(match t.attr("spd") {
        Some("med") => 750,
        Some("slow") => default_dur.max(1000),
        _ => 500,
    });
    if let Some(snd) = t.path(&["sndAc", "stSnd", "snd"]) {
        tr.sound = dml::media_ref(imp, part, snd);
        tr.sound_loop = t.path(&["sndAc", "stSnd"]).and_then(|s| s.attr("loop")).is_some_and(|v| v == "1" || v == "true");
    }
    tr.stop_sound = t.path(&["sndAc", "endSnd"]).is_some();
    if tr.kind == "none" && tr.advance_on_click && tr.advance_after_ms.is_none() && tr.sound.is_none() && !tr.stop_sound {
        return None;
    }
    Some(tr)
}

struct Found<'a> {
    ctn: &'a El,
    trigger: Option<u32>,
}

fn collect<'a>(e: &'a El, trigger: Option<u32>, out: &mut Vec<Found<'a>>, depth: usize) {
    if depth > 64 || out.len() > 10_000 {
        return;
    }
    for c in e.elements() {
        if c.is("cTn") && c.has_attr("presetClass") {
            out.push(Found { ctn: c, trigger });
            continue;
        }
        let mut trig = trigger;
        if c.is("seq")
            && let Some(ctn) = c.child("cTn")
            && ctn.attr("nodeType") == Some("interactiveSeq")
        {
            trig = ctn.path(&["stCondLst", "cond", "tgtEl", "spTgt"]).and_then(|s| s.u32("spid"));
        }
        collect(c, trig, out, depth + 1);
    }
}

fn max_dur(e: &El, depth: usize) -> Option<u32> {
    if depth > 64 {
        return None;
    }
    let mut best: Option<u32> = None;
    for c in e.elements() {
        if c.is("cTn")
            && let Some(d) = ms_attr(c, "dur")
            && d > 1
        {
            best = Some(best.map_or(d, |b: u32| b.max(d)));
        }
        if let Some(d) = max_dur(c, depth + 1) {
            best = Some(best.map_or(d, |b| b.max(d)));
        }
    }
    best
}

/// Flatten the timing tree into the Animation Pane list.
pub fn animations(imp: &mut Imp, part: &Part, ctx: &IdCtx, timing: &El, _shapes: &[Shape]) -> Vec<Animation> {
    let _ = (imp, part);
    let Some(tn) = timing.child("tnLst") else { return vec![] };
    let mut found = vec![];
    collect(tn, None, &mut found, 0);
    // Paragraph builds (`bldP build="p"`) by (spid, grpId).
    let by_para: Vec<(u32, String)> = timing
        .child("bldLst")
        .map(|b| {
            b.children_named("bldP")
                .filter(|p| p.attr("build") == Some("p"))
                .filter_map(|p| Some((p.u32("spid")?, p.attr("grpId").unwrap_or("0").to_string())))
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<(Animation, u32, String)> = vec![];
    for f in found {
        let c = f.ctn;
        let class = AnimClass::from_xml(c.attr("presetClass").unwrap_or("entr"));
        let preset = c.u32("presetID").unwrap_or(0);
        let sub = c.u32("presetSubtype").unwrap_or(0);
        let Some(tgt) = c.find("spTgt") else { continue };
        let Some(file_id) = tgt.u32("spid") else { continue };
        let Some(shape) = ctx.lookup(file_id) else { continue };
        let paragraph = tgt.path(&["txEl", "pRg"]).and_then(|p| p.u32("st"));
        let effect = maps::effect_for_preset(class, preset).unwrap_or_else(|| maps::fallback_effect(class));
        // PowerPoint writes Float Down as its own preset (47), not as Float with a direction.
        let (effect, option) = match (class, preset) {
            (AnimClass::Entrance, 47) => ("float", "d".to_string()),
            (AnimClass::Exit, 47) => ("floatOut", "d".to_string()),
            _ => (effect, maps::option_from_subtype(effect, sub)),
        };
        let start = match c.attr("nodeType") {
            Some("clickEffect") => AnimStart::OnClick,
            Some("afterEffect") => AnimStart::AfterPrevious,
            _ => AnimStart::WithPrevious,
        };
        let mut a = Animation {
            shape,
            class,
            effect: effect.to_string(),
            option,
            start,
            duration_ms: max_dur(c, 0).unwrap_or(0),
            delay_ms: c.path(&["stCondLst", "cond"]).and_then(|d| ms_attr(d, "delay")).unwrap_or(0),
            paragraph,
            trigger: f.trigger.and_then(|t| ctx.lookup(t)),
            preset_id: Some(preset),
            preset_subtype: Some(sub),
            auto_reverse: c.bool("autoRev").unwrap_or(false),
            // PowerPoint's "Rewind when done playing" is `fill="remove"` on the effect node.
            rewind: c.attr("fill") == Some("remove"),
            smooth_start: c.f64("accel").map(|v| v / 100_000.0).unwrap_or(0.0),
            smooth_end: c.f64("decel").map(|v| v / 100_000.0).unwrap_or(0.0),
            bounce_end: c.f64("bounceEnd").map(|v| (v / 100_000.0).clamp(0.0, 1.0)).unwrap_or(0.0),
            ..Default::default()
        };
        a.repeat = match c.attr("repeatCount") {
            Some("indefinite") => u32::MAX,
            Some(v) => v.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v / 1000.0).round().clamp(1.0, 10_000.0) as u32).unwrap_or(1),
            None => 1,
        };
        if let Some(m) = c.find("animMotion") {
            a.path = m.attr("path").map(String::from);
        }
        if let Some(r) = c.find("animRot") {
            a.amount = r.f64("by").map(|v| v / 60_000.0);
            if a.effect == "spin" && a.amount.is_some_and(|v| v < 0.0) {
                a.option = "counterClockwise".into();
            }
        } else if let Some(s) = c.find("animScale").and_then(|s| s.child("by")) {
            a.amount = s.f64("x").map(|v| v / 100_000.0);
        }
        if let Some(clr) = c.find("animClr").and_then(|x| x.child("to")) {
            a.color = dml::color(clr);
        }
        let grp = c.attr("grpId").unwrap_or("0").to_string();
        // Collapse a paragraph-by-paragraph build into one animation.
        if paragraph.is_some() && by_para.iter().any(|(s, g)| *s == file_id && *g == grp) {
            if out.last().is_some_and(|(l, lf, lg)| *lf == file_id && *lg == grp && l.text_build == TextBuild::ByParagraph) {
                continue;
            }
            a.text_build = TextBuild::ByParagraph;
            a.paragraph = None;
        }
        out.push((a, file_id, grp));
    }
    out.into_iter().map(|(a, _, _)| a).collect()
}

/// Media playback options from the timing tree's `p:video`/`p:audio` nodes (#74): volume,
/// play-across-slides, hide-while-not-playing, loop, rewind, video full screen and automatic start. The nodes
/// sit beside the main sequence and carry them for clips whose `p14:media` element only holds
/// trim, fades and bookmarks.
pub fn media_options(timing: &El, ctx: &IdCtx, shapes: &mut [Shape]) {
    // Index only timing nodes that target an imported shape. Walk the shape tree once rather
    // than searching it again for every node in a hostile timing part.
    let mut nodes = HashMap::new();
    collect_media(timing, ctx, &mut nodes, false, 0);
    apply_media(shapes, &nodes, 0);
}

fn collect_media<'a>(e: &'a El, ctx: &IdCtx, out: &mut HashMap<ShapeId, (&'a El, bool)>, waits_for_input: bool, depth: usize) {
    if depth > 64 || out.len() >= 10_000 {
        return;
    }
    let waits_for_input = waits_for_input
        || (e.is("cTn")
            && (matches!(e.attr("nodeType"), Some("interactiveSeq" | "clickEffect"))
                || e.child("stCondLst").is_some_and(|l| {
                    l.children_named("cond")
                        .any(|c| matches!(c.attr("evt"), Some("onClick" | "onDblClick" | "onMouseOver" | "onMouseOut" | "onNext" | "onPrev")))
                })));
    if matches!(e.local(), "video" | "audio")
        && let Some(id) = e.path(&["cMediaNode", "tgtEl", "spTgt"]).and_then(|t| t.u32("spid")).and_then(|s| ctx.lookup(s))
    {
        out.entry(id).or_insert((e, waits_for_input));
    }
    for c in e.elements() {
        collect_media(c, ctx, out, waits_for_input, depth + 1);
    }
}

fn apply_media(shapes: &mut [Shape], nodes: &HashMap<ShapeId, (&El, bool)>, depth: usize) {
    if depth > 32 {
        return;
    }
    for s in shapes {
        if let ShapeKind::Media(clip) = &mut s.kind
            && let Some((n, waits_for_input)) = nodes.get(&s.id)
            && let Some(mn) = n.child("cMediaNode")
        {
            if let Some(v) = mn.f64("vol") {
                clip.volume = (v / 100_000.0).clamp(0.0, 1.0);
            }
            if let Some(n) = mn.u32("numSld") {
                clip.play_across_slides = n > 1;
            }
            if let Some(b) = mn.bool("showWhenStopped") {
                clip.hide_while_not_playing = !b;
            }
            if n.is("video") {
                clip.full_screen = n.bool("fullScrn").unwrap_or(false);
            }
            if let Some(tn) = mn.child("cTn") {
                clip.loop_play = tn.attr("repeatCount") == Some("indefinite");
                clip.rewind = tn.attr("fill") == Some("remove");
                // Event-driven or ancestor click sequences do not start on slide entry. Only
                // a finite, nonnegative delay with no event/target is an automatic start.
                clip.autoplay = !waits_for_input
                    && tn.child("stCondLst").is_some_and(|l| {
                        l.children_named("cond")
                            .any(|c| c.attr("evt").is_none() && c.f64("delay").is_some_and(|d| d >= 0.0) && c.elements().next().is_none())
                    });
            }
        }
        if let Some(children) = s.children_mut() {
            apply_media(children, nodes, depth + 1);
        }
    }
}
