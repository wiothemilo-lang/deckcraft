//! A slide's animation sequence: click steps, effect start times and per-shape state over time.

use std::collections::{HashMap, HashSet};

use deckcraft_model::anim::TextBuild;
use deckcraft_model::style::Fill;
use deckcraft_model::{AnimClass, AnimStart, Animation, ColorScheme, Rgba, SchemeSlot, ShapeId, Slide, Xfrm};

use crate::AnimState;
use crate::effects::{EffectSpec, exit_base, motion_path};

/// Delay between paragraphs of a by-paragraph build that isn't on click (seconds).
pub const PARAGRAPH_STAGGER: f64 = 0.5;
const MAX_EFFECTS: usize = 100_000;
const MAX_PARAGRAPHS: usize = 1_000;
const MAX_ITERATIONS: f64 = 1.0e6;

/// Which sequence an effect belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sequence {
    /// The slide's main click sequence.
    Main,
    /// An interactive sequence started by clicking this shape.
    Trigger(ShapeId),
}

/// Timing of one (sub-)effect, for the Animation Pane timeline and the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectTiming {
    /// Index into `slide.animations`.
    pub anim: usize,
    /// Animated shape and paragraph (`None` = the whole shape).
    pub shape: ShapeId,
    pub paragraph: Option<usize>,
    pub sequence: Sequence,
    /// Click step within its sequence.
    pub step: usize,
    /// Start within the step (seconds, delay included).
    pub start: f64,
    /// One iteration (seconds).
    pub duration: f64,
    /// Repeat count (fractional allowed); 1 = once.
    pub iterations: f64,
    /// Repeats until the next click.
    pub until_next_click: bool,
    pub auto_reverse: bool,
    pub rewind: bool,
    /// Time from start to end (all iterations, reverse included; one iteration if until-click).
    pub span: f64,
    pub class: AnimClass,
}

/// Playhead of a triggered sequence: `step` clicks on the trigger fully played, the current one
/// playing for `t` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriggerPlay {
    pub trigger: ShapeId,
    pub step: usize,
    pub t: f64,
}

/// When a step of a triggered sequence was started, counted in started steps on the slide. Effects
/// of different triggers apply in this order, so that a shape hidden by one trigger and shown again
/// by another ends up as the later of the two left it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepStamp {
    pub trigger: ShapeId,
    pub step: usize,
    pub at: u64,
}

#[derive(Clone, Debug, Default)]
struct StepInfo {
    auto: bool,
    duration: f64,
}

#[derive(Clone, Debug)]
enum After {
    None,
    Hide,
    HideOnNextClick,
    Dim(Rgba),
}

#[derive(Clone, Debug)]
struct Effect {
    timing: EffectTiming,
    spec: EffectSpec,
    after: After,
}

/// A slide's animations laid out in time.
#[derive(Clone, Debug, Default)]
pub struct Timeline {
    effects: Vec<Effect>,
    main: Vec<StepInfo>,
    triggers: Vec<(ShapeId, Vec<StepInfo>)>,
    /// Base fill colour per shape (for colour emphasis).
    colors: HashMap<ShapeId, Rgba>,
    /// By-paragraph builds animate first-level paragraphs; deeper paragraphs follow their parent:
    /// (shape, member paragraph, owner paragraph).
    groups: Vec<(ShapeId, usize, usize)>,
    /// The owner paragraph of each animated paragraph of a by-paragraph build: a paragraph of a
    /// deeper level plays with the first-level paragraph that holds it.
    para_owner: HashMap<(ShapeId, usize), usize>,
    /// The effects of each animated shape and paragraph, as indices into `effects`, in Animation
    /// Pane order (the `anim` index). The show asks for the state of a shape every frame: the
    /// index keeps that ask from scanning every effect of the slide, and from allocating.
    by_shape: HashMap<(ShapeId, Option<usize>), Vec<u32>>,
    /// The paragraphs of each animated shape that are animated on their own.
    para_targets: HashMap<ShapeId, Vec<usize>>,
    /// The shapes that have any effect, in the order they first appear.
    shapes: Vec<ShapeId>,
}

/// How many effect indices fit in the stack buffer of the order (the rest go to the heap): a
/// shape has a handful of effects, and a frame must not allocate.
const ORDER_ON_STACK: usize = 16;

enum Status {
    NotStarted,
    Active(f64),
    Done,
}

fn secs(ms: u32) -> f64 {
    ms as f64 / 1000.0
}

fn finite_or(v: f64, d: f64) -> f64 {
    if v.is_finite() { v } else { d }
}

fn sanitize_xfrm(x: Xfrm, sw: f64, sh: f64) -> Xfrm {
    Xfrm {
        x: finite_or(x.x, 0.0).clamp(-1.0e7, 1.0e7),
        y: finite_or(x.y, 0.0).clamp(-1.0e7, 1.0e7),
        w: finite_or(x.w, sw / 2.0).clamp(0.0, 1.0e7),
        h: finite_or(x.h, sh / 2.0).clamp(0.0, 1.0e7),
        ..x
    }
}

fn default_scheme() -> Option<ColorScheme> {
    deckcraft_model::theme::builtin_color_schemes().into_iter().next()
}

impl Timeline {
    /// Build the timeline of `slide` (slide size in points). Placeholders without their own box
    /// get a centred default box; colours resolve against the default theme.
    pub fn new(slide: &Slide, slide_w: f64, slide_h: f64) -> Timeline {
        Timeline::with(slide, slide_w, slide_h, None, &|_| None)
    }

    /// Build with a colour scheme for theme colours and a lookup for shape boxes (placeholders
    /// inherit their box from the layout; the lookup wins over `shape.xfrm`).
    pub fn with(slide: &Slide, slide_w: f64, slide_h: f64, scheme: Option<&ColorScheme>, xfrm_of: &dyn Fn(ShapeId) -> Option<Xfrm>) -> Timeline {
        let sw = finite_or(slide_w, 960.0).clamp(1.0, 1.0e7);
        let sh = finite_or(slide_h, 540.0).clamp(1.0, 1.0e7);
        let owned;
        let scheme = match scheme {
            Some(s) => Some(s),
            None => {
                owned = default_scheme();
                owned.as_ref()
            }
        };
        let resolve = |c: &deckcraft_model::ColorRef| -> Rgba {
            match scheme {
                Some(s) => c.resolve(s, None),
                None => match &c.base {
                    deckcraft_model::ColorBase::Rgb { rgb } => *rgb,
                    _ => Rgba::rgb(0x15, 0x60, 0x82),
                },
            }
        };
        let accent = scheme.map(|s| s.get(SchemeSlot::Accent1)).unwrap_or(Rgba::rgb(0x15, 0x60, 0x82));

        let mut tl = Timeline::default();
        // Group the animations by sequence in one pass, sequences in order of first appearance
        // (main first), so many distinct triggers cost linear work, not a scan per trigger.
        let mut seqs: Vec<(Sequence, Vec<usize>)> = vec![(Sequence::Main, Vec::new())];
        let mut seq_index: HashMap<Sequence, usize> = HashMap::from([(Sequence::Main, 0)]);
        for (ai, a) in slide.animations.iter().enumerate() {
            let s = a.trigger.map(Sequence::Trigger).unwrap_or(Sequence::Main);
            let i = *seq_index.entry(s).or_insert_with(|| {
                seqs.push((s, Vec::new()));
                seqs.len() - 1
            });
            if let Some((_, members)) = seqs.get_mut(i) {
                members.push(ai);
            }
        }
        // Paragraph groups already recorded (the list itself keeps their order).
        let mut grouped: HashSet<(ShapeId, usize, usize)> = HashSet::new();
        for (seq, members) in seqs {
            if tl.effects.len() >= MAX_EFFECTS {
                break;
            }
            let mut steps: Vec<StepInfo> = Vec::new();
            let mut group_begin = 0.0f64;
            let mut max_end = 0.0f64;
            for ai in members {
                let Some(a) = slide.animations.get(ai) else { continue };
                if tl.effects.len() >= MAX_EFFECTS {
                    break;
                }
                let shape = slide.shape(a.shape);
                // Paragraph targets of this animation.
                let paras: Vec<Option<usize>> = match (a.paragraph, a.text_build) {
                    (Some(p), _) => vec![Some(p as usize)],
                    (None, TextBuild::ByParagraph) => {
                        let mut v: Vec<Option<usize>> = vec![];
                        if let Some(t) = shape.and_then(|s| s.text.as_ref()) {
                            let filled = |p: &deckcraft_model::text::Paragraph| p.runs.iter().any(|r| !r.text.trim().is_empty());
                            let top = t.paragraphs.iter().filter(|p| filled(p)).map(|p| p.level).min().unwrap_or(0);
                            let mut owner: Option<usize> = None;
                            for (i, p) in t.paragraphs.iter().enumerate() {
                                if p.level <= top && filled(p) && v.len() < MAX_PARAGRAPHS {
                                    owner = Some(i);
                                    v.push(Some(i));
                                } else if let Some(o) = owner
                                    && grouped.insert((a.shape, i, o))
                                {
                                    tl.groups.push((a.shape, i, o));
                                }
                            }
                        }
                        if v.is_empty() { vec![None] } else { v }
                    }
                    _ => vec![None],
                };
                let xf = xfrm_of(a.shape).or_else(|| shape.and_then(|s| s.xfrm)).unwrap_or(Xfrm::new(sw / 4.0, sh / 4.0, sw / 2.0, sh / 2.0));
                let xf = sanitize_xfrm(xf, sw, sh);
                let spec = build_spec(a, xf, sw, sh, &resolve);
                let dur = secs(a.duration_ms);
                let delay = secs(a.delay_ms);
                let until_next_click = a.repeat == u32::MAX;
                let iterations = if until_next_click || a.repeat <= 1 { 1.0 } else { (a.repeat as f64).min(MAX_ITERATIONS) };
                let span = dur * if a.auto_reverse { 2.0 } else { 1.0 } * iterations;
                let after = match a.after.as_deref() {
                    Some("hide") => After::Hide,
                    Some("hideOnNextClick") => After::HideOnNextClick,
                    Some(c) => Rgba::from_hex(c).map(After::Dim).unwrap_or(After::None),
                    None => After::None,
                };
                let mut first_start = 0.0;
                for (k, para) in paras.iter().enumerate() {
                    let mode = if k == 0 {
                        a.start
                    } else if a.start == AnimStart::OnClick {
                        AnimStart::OnClick
                    } else {
                        AnimStart::WithPrevious
                    };
                    let first_of_seq = steps.is_empty();
                    let mode = if first_of_seq && matches!(seq, Sequence::Trigger(_)) { AnimStart::OnClick } else { mode };
                    match mode {
                        AnimStart::OnClick => {
                            steps.push(StepInfo { auto: false, duration: 0.0 });
                            group_begin = 0.0;
                            max_end = 0.0;
                        }
                        AnimStart::WithPrevious => {
                            if first_of_seq {
                                steps.push(StepInfo { auto: true, duration: 0.0 });
                            }
                        }
                        AnimStart::AfterPrevious => {
                            if first_of_seq {
                                steps.push(StepInfo { auto: true, duration: 0.0 });
                            }
                            group_begin = max_end;
                        }
                    }
                    let start = if k > 0 && a.start != AnimStart::OnClick { first_start + k as f64 * PARAGRAPH_STAGGER } else { group_begin + delay };
                    if k == 0 {
                        first_start = start;
                    }
                    let end = start + span;
                    max_end = max_end.max(end);
                    let step = steps.len().saturating_sub(1);
                    if let Some(s) = steps.last_mut() {
                        // Media plays on by itself once started: it holds back an After Previous
                        // effect (`max_end`), but not the next click (#116).
                        s.duration = s.duration.max(if a.class == AnimClass::Media { start } else { end });
                    }
                    tl.effects.push(Effect {
                        timing: EffectTiming {
                            anim: ai,
                            shape: a.shape,
                            paragraph: *para,
                            sequence: seq,
                            step,
                            start,
                            duration: dur,
                            iterations,
                            until_next_click,
                            auto_reverse: a.auto_reverse,
                            rewind: a.rewind,
                            span,
                            class: a.class,
                        },
                        spec: spec.clone(),
                        after: after.clone(),
                    });
                }
            }
            match seq {
                Sequence::Main => tl.main = steps,
                Sequence::Trigger(s) => tl.triggers.push((s, steps)),
            }
        }
        // Base colours of animated shapes.
        for e in &tl.effects {
            let id = e.timing.shape;
            if tl.colors.contains_key(&id) {
                continue;
            }
            let c = match slide.shape(id).and_then(|s| s.fill.as_ref()) {
                Some(Fill::Solid { color }) => resolve(color),
                _ => accent,
            };
            tl.colors.insert(id, c);
        }
        tl.index();
        tl
    }

    /// Index the effects by shape and paragraph, in the order they are played in: the show asks
    /// for the state of a shape every frame, and must not walk the whole timeline for it.
    fn index(&mut self) {
        for (i, e) in self.effects.iter().enumerate() {
            self.by_shape.entry((e.timing.shape, e.timing.paragraph)).or_default().push(i as u32);
        }
        // The Animation Pane order is the order of the animations in the slide's list.
        let anims: Vec<usize> = self.effects.iter().map(|e| e.timing.anim).collect();
        for idxs in self.by_shape.values_mut() {
            idxs.sort_by_key(|&i| anims[i as usize]);
        }
        // The paragraphs of a by-paragraph build: the first-level ones are animated, the deeper
        // ones play with the first-level paragraph that holds them.
        for (shape, member, owner) in &self.groups {
            self.para_owner.entry((*shape, *member)).or_insert(*owner);
        }
        let mut effect_paras: HashMap<ShapeId, Vec<usize>> = HashMap::new();
        for e in &self.effects {
            if let Some(p) = e.timing.paragraph {
                effect_paras.entry(e.timing.shape).or_default().push(p);
            }
        }
        // The members of a by-paragraph build play with the first-level paragraph that holds
        // them: they are animated too, on their own.
        let mut targets = effect_paras.clone();
        for (shape, member, owner) in &self.groups {
            if effect_paras.get(shape).is_some_and(|v| v.contains(owner)) {
                targets.entry(*shape).or_default().push(*member);
            }
        }
        for v in targets.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        self.para_targets = targets;
        let mut seen: HashSet<ShapeId> = HashSet::new();
        self.shapes = self.effects.iter().map(|e| e.timing.shape).filter(|s| seen.insert(*s)).collect();
    }

    /// Number of steps in the main sequence (clicks, plus an automatic first step when the first
    /// effect starts with/after previous).
    pub fn steps(&self) -> usize {
        self.main.len()
    }

    /// Does main step `i` start by itself (no click), as the slide appears?
    pub fn step_is_auto(&self, i: usize) -> bool {
        self.main.get(i).is_some_and(|s| s.auto)
    }

    /// Duration of main step `i` in seconds (0 when out of range).
    pub fn step_duration(&self, i: usize) -> f64 {
        self.main.get(i).map(|s| s.duration).unwrap_or(0.0)
    }

    /// Media effects (Play, Pause, Stop) of main step `step` that start after `from` and by `to`
    /// seconds into it, in order: what a show running the step from `from` to `to` sets off.
    pub fn media_cues(&self, step: usize, from: f64, to: f64) -> Vec<EffectTiming> {
        let mut v: Vec<EffectTiming> = self
            .effects
            .iter()
            .map(|e| &e.timing)
            .filter(|e| e.class == AnimClass::Media && e.sequence == Sequence::Main && e.step == step && e.start > from && e.start <= to)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.start.total_cmp(&b.start));
        v
    }

    /// Total duration of the main sequence played without pauses.
    pub fn total_duration(&self) -> f64 {
        self.main.iter().map(|s| s.duration).sum()
    }

    /// Map a time on the "play everything" clock to `(step, t)` for [`Timeline::state`].
    pub fn locate(&self, time: f64) -> (usize, f64) {
        let mut t = finite_or(time, 0.0).max(0.0);
        for (i, s) in self.main.iter().enumerate() {
            if t < s.duration || (t == 0.0 && s.duration == 0.0 && i == 0) {
                return (i, t);
            }
            t -= s.duration;
        }
        (self.main.len(), 0.0)
    }

    /// Shapes that start an interactive sequence when clicked.
    pub fn trigger_shapes(&self) -> Vec<ShapeId> {
        self.triggers.iter().map(|(s, _)| *s).collect()
    }

    /// Steps (clicks on the trigger) of a triggered sequence.
    pub fn trigger_steps(&self, trigger: ShapeId) -> usize {
        self.triggers.iter().find(|(s, _)| *s == trigger).map(|(_, v)| v.len()).unwrap_or(0)
    }

    pub fn trigger_step_duration(&self, trigger: ShapeId, i: usize) -> f64 {
        self.triggers.iter().find(|(s, _)| *s == trigger).and_then(|(_, v)| v.get(i)).map(|s| s.duration).unwrap_or(0.0)
    }

    /// All effect timings in sequence order.
    pub fn effects(&self) -> Vec<EffectTiming> {
        self.effects.iter().map(|e| e.timing.clone()).collect()
    }

    /// Is anything animated on this slide?
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Paragraph indices of `shape` animated separately (by-paragraph builds).
    pub fn paragraph_targets(&self, shape: ShapeId) -> Vec<usize> {
        self.para_targets.get(&shape).cloned().unwrap_or_default()
    }

    /// Shapes with any effect.
    pub fn animated_shapes(&self) -> Vec<ShapeId> {
        self.shapes.clone()
    }

    /// State of `shape` (or of one of its paragraphs, `para`) when `step` main steps have been
    /// fully played and step `step` has been playing for `t` seconds (`t <= 0`: not started;
    /// `step >= steps()`: everything done). Paragraph states are relative to the shape's own state
    /// (compose them). Triggered sequences are considered not played.
    pub fn state(&self, shape: ShapeId, para: Option<usize>, step: usize, t: f64) -> AnimState {
        self.state_with(shape, para, step, t, &[])
    }

    /// [`Timeline::state`] with the playheads of triggered sequences.
    pub fn state_with(&self, shape: ShapeId, para: Option<usize>, step: usize, t: f64, triggers: &[TriggerPlay]) -> AnimState {
        self.state_ordered(shape, para, step, t, triggers, &[])
    }

    /// [`Timeline::state_with`] applying the effects of triggered sequences in the order their steps
    /// were started (`stamps`), after those of the main sequence. Without stamps they apply in
    /// Animation Pane order.
    pub fn state_ordered(
        &self,
        shape: ShapeId,
        para: Option<usize>,
        step: usize,
        t: f64,
        triggers: &[TriggerPlay],
        stamps: &[StepStamp],
    ) -> AnimState {
        // A paragraph of a deeper level plays with the first-level paragraph that holds it.
        let para = para.and_then(|p| self.para_owner.get(&(shape, p)).copied()).or(para);
        let base_color = self.colors.get(&shape).copied().unwrap_or(Rgba::rgb(0x15, 0x60, 0x82));
        let mut st = AnimState::default();
        let Some(idxs) = self.by_shape.get(&(shape, para)) else { return st.sanitized() };
        // Initial visibility comes from the first effect in the Animation Pane order.
        if let Some(&first) = idxs.first()
            && self.effects[first as usize].timing.class == AnimClass::Entrance
        {
            st.visible = false;
        }
        // Then play in time order: main sequence first, then triggered sequences as they were
        // clicked.
        if stamps.is_empty() {
            // The common case: the Animation Pane order, straight from the timeline.
            return self.play(st, idxs, step, t, triggers, base_color);
        }
        // The show started triggered sequences: the effects play in the order their steps were
        // started. The order is built in a buffer on the stack: a frame must not allocate.
        let mut on_stack = [0u32; ORDER_ON_STACK];
        let mut on_heap: Vec<u32> = Vec::new();
        let buf: &mut [u32] = if idxs.len() <= ORDER_ON_STACK {
            &mut on_stack[..idxs.len()]
        } else {
            on_heap.resize(idxs.len(), 0);
            &mut on_heap
        };
        buf.copy_from_slice(idxs);
        let when = |i: u32| match self.effects[i as usize].timing.sequence {
            Sequence::Main => 0,
            Sequence::Trigger(tr) => {
                let step = self.effects[i as usize].timing.step;
                stamps.iter().find(|s| s.trigger == tr && s.step == step).map_or(u64::MAX, |s| s.at.saturating_add(1))
            }
        };
        buf.sort_by_key(|&i| (when(i), self.effects[i as usize].timing.anim));
        self.play(st, buf, step, t, triggers, base_color)
    }

    /// Play the effects of a shape, in the given order, on top of `st`.
    fn play(&self, mut st: AnimState, order: &[u32], step: usize, t: f64, triggers: &[TriggerPlay], base_color: Rgba) -> AnimState {
        for &i in order {
            let e = &self.effects[i as usize];
            let (pstep, pt) = match e.timing.sequence {
                Sequence::Main => (step, t),
                Sequence::Trigger(tr) => triggers.iter().find(|p| p.trigger == tr).map(|p| (p.step, p.t)).unwrap_or((0, 0.0)),
            };
            let cur = st.tint.unwrap_or(base_color);
            let revert = e.timing.rewind || e.timing.auto_reverse;
            match status(&e.timing, pstep, pt) {
                Status::NotStarted => {}
                Status::Active(p) => {
                    if e.timing.class == AnimClass::Entrance || e.timing.class == AnimClass::Exit {
                        st.visible = true;
                    }
                    st = st.compose(&e.spec.eval(p, cur));
                }
                Status::Done => {
                    match e.timing.class {
                        AnimClass::Entrance => st.visible = !revert,
                        AnimClass::Exit => st.visible = revert,
                        AnimClass::Emphasis | AnimClass::Path if !revert => {
                            let end = e.spec.end_state(cur);
                            st = st.compose(&end);
                        }
                        _ => {}
                    }
                    match e.after {
                        After::Hide => st.visible = false,
                        After::HideOnNextClick if pstep > e.timing.step => st.visible = false,
                        After::Dim(c) if pstep > e.timing.step => st.tint = Some(c),
                        _ => {}
                    }
                }
            }
        }
        st.sanitized()
    }
    /// State on the "play everything" clock (Animation Pane ▸ Play All, preview).
    pub fn state_at(&self, shape: ShapeId, para: Option<usize>, time: f64) -> AnimState {
        let (s, t) = self.locate(time);
        self.state(shape, para, s, t)
    }

    /// The state as it was computed before the timeline grew its index: the effects of the
    /// shape are collected by scanning the whole timeline and sorted, every call. The test
    /// below holds the two to the same answer.
    #[cfg(test)]
    pub(crate) fn state_ordered_reference(
        &self,
        shape: ShapeId,
        para: Option<usize>,
        step: usize,
        t: f64,
        triggers: &[TriggerPlay],
        stamps: &[StepStamp],
    ) -> AnimState {
        let para = para.map(|p| self.groups.iter().find(|g| g.0 == shape && g.1 == p).map(|g| g.2).unwrap_or(p));
        let base_color = self.colors.get(&shape).copied().unwrap_or(Rgba::rgb(0x15, 0x60, 0x82));
        let mut idxs: Vec<u32> =
            self.effects.iter().enumerate().filter(|(_, e)| e.timing.shape == shape && e.timing.paragraph == para).map(|(i, _)| i as u32).collect();
        idxs.sort_by_key(|&i| self.effects[i as usize].timing.anim);
        let mut st = AnimState::default();
        if let Some(&first) = idxs.first()
            && self.effects[first as usize].timing.class == AnimClass::Entrance
        {
            st.visible = false;
        }
        if !stamps.is_empty() {
            let when = |i: u32| match self.effects[i as usize].timing.sequence {
                Sequence::Main => 0,
                Sequence::Trigger(tr) => {
                    let step = self.effects[i as usize].timing.step;
                    stamps.iter().find(|s| s.trigger == tr && s.step == step).map_or(u64::MAX, |s| s.at.saturating_add(1))
                }
            };
            idxs.sort_by_key(|&i| (when(i), self.effects[i as usize].timing.anim));
        }
        self.play(st, &idxs, step, t, triggers, base_color)
    }
}

fn status(e: &EffectTiming, step: usize, t: f64) -> Status {
    if e.step < step {
        return Status::Done;
    }
    if e.step > step {
        return Status::NotStarted;
    }
    let t = finite_or(t, 0.0);
    if t <= 0.0 {
        return Status::NotStarted;
    }
    let local = t - e.start;
    if local < 0.0 {
        return Status::NotStarted;
    }
    let dur = e.duration;
    if dur.is_nan() || dur <= 0.0 || !dur.is_finite() {
        return Status::Done;
    }
    if !e.until_next_click && local >= e.span {
        return Status::Done;
    }
    let period = if e.auto_reverse { 2.0 * dur } else { dur };
    let c = local % period;
    let c = if c.is_finite() { c } else { 0.0 };
    let p = if c > dur { 2.0 - c / dur } else { c / dur };
    Status::Active(p.clamp(0.0, 1.0))
}

fn build_spec(a: &Animation, xf: Xfrm, sw: f64, sh: f64, resolve: &dyn Fn(&deckcraft_model::ColorRef) -> Rgba) -> EffectSpec {
    let effect = if a.class == AnimClass::Exit { exit_base(&a.effect).to_string() } else { a.effect.clone() };
    let path = if a.class == AnimClass::Path { Some(motion_path(&a.effect, &a.option, a.path.as_deref())) } else { None };
    EffectSpec {
        class: a.class,
        effect,
        // Sink Down is Rise Up played backwards, leaving downward.
        option: if a.effect == "sinkDown" { "d".into() } else { a.option.clone() },
        accel: a.smooth_start,
        decel: a.smooth_end,
        bounce: a.bounce_end,
        amount: a.amount,
        color: a.color.as_ref().map(resolve),
        path,
        xf,
        slide_w: sw,
        slide_h: sh,
    }
}
