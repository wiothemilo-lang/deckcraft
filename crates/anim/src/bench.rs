//! A micro-benchmark of the per-frame cost of a show's animation timeline.
//!
//! Every frame of a show asks the timeline for the state of every shape (and of each of its
//! paragraphs), so this is the cost that decides whether a busy slide animates smoothly. The
//! old way of finding the effects of a shape (scan the whole timeline, sort it, allocate) is
//! measured next to the new one, in the same binary.
//!
//! Run it with:
//!
//! ```text
//! cargo test -p deckcraft-anim --release -- --nocapture bench_
//! ```
//!
//! The numbers are only meaningful on the machine that ran them: they compare the shapes of the
//! work (per frame, per shape), not absolute speed.

use std::time::Instant;

use deckcraft_model::{AnimClass, AnimStart, Animation, Shape, ShapeId, Slide, Xfrm};

use crate::{StepStamp, Timeline, TriggerPlay};

const W: f64 = 960.0;
const H: f64 = 540.0;

/// A slide of `shapes` shapes, each with `per_shape` effects (an entrance, an emphasis and an
/// exit): the shape of a real animated slide.
fn busy_slide(shapes: u32, per_shape: u32) -> Slide {
    let mut anims: Vec<Animation> = Vec::new();
    for i in 0..shapes {
        for k in 0..per_shape {
            let (class, effect) = match k % 3 {
                0 => (AnimClass::Entrance, "fade"),
                1 => (AnimClass::Emphasis, "pulse"),
                _ => (AnimClass::Exit, "fadeOut"),
            };
            let mut a = Animation {
                shape: ShapeId(i + 1),
                class,
                effect: effect.into(),
                start: if k == 0 { AnimStart::OnClick } else { AnimStart::WithPrevious },
                duration_ms: 800,
                ..Default::default()
            };
            a.delay_ms = k * 120;
            anims.push(a);
        }
    }
    Slide {
        shapes: (0..shapes)
            .map(|i| Shape { id: ShapeId(i + 1), name: format!("S{i}"), xfrm: Some(Xfrm::new(50.0, 50.0, 200.0, 100.0)), ..Default::default() })
            .collect(),
        animations: anims,
        ..Default::default()
    }
}

/// What the show does for one frame: the state of every shape, then of each of its paragraphs
/// (the same calls the show's canvas makes). `old` asks the timeline the way it did before it
/// grew its index.
fn frame(tl: &Timeline, ids: &[ShapeId], step: usize, t: f64, plays: &[TriggerPlay], stamps: &[StepStamp], old: bool) -> f64 {
    let mut sum = 0.0;
    for id in ids {
        let a = if old { tl.state_ordered_reference(*id, None, step, t, plays, stamps) } else { tl.state_ordered(*id, None, step, t, plays, stamps) };
        sum += a.opacity;
        let paras = if old { tl.paragraph_targets_reference(*id) } else { tl.paragraph_targets(*id) };
        for p in paras {
            let a = tl.state(*id, Some(p), step, t);
            sum += a.opacity;
        }
    }
    sum
}

fn run(name: &str, shapes: u32, per_shape: u32, frames: u32, stamps: bool) {
    let slide = busy_slide(shapes, per_shape);
    let tl = Timeline::new(&slide, W, H);
    let ids: Vec<ShapeId> = (0..shapes).map(|i| ShapeId(i + 1)).collect();
    let plays: Vec<TriggerPlay> = vec![TriggerPlay { trigger: ShapeId(900), step: 0, t: 0.3 }];
    let st: Vec<StepStamp> = if stamps { vec![StepStamp { trigger: ShapeId(900), step: 0, at: 1 }] } else { Vec::new() };
    let mut times = [0.0f64; 2];
    for (i, old) in [false, true].into_iter().enumerate() {
        // Warm up (the first frames pay for the caches of the allocator).
        for f in 0..8 {
            frame(&tl, &ids, 0, f as f64 * 0.016, &plays, &st, old);
        }
        let t0 = Instant::now();
        let mut sum = 0.0;
        for f in 0..frames {
            sum += frame(&tl, &ids, 0, f as f64 * 0.016, &plays, &st, old);
        }
        times[i] = t0.elapsed().as_secs_f64();
        assert!(sum.is_finite());
    }
    let per_frame = |s: f64| s * 1000.0 / f64::from(frames);
    let now_ms = per_frame(times[0]);
    let before_ms = per_frame(times[1]);
    let factor = times[1] / times[0].max(1e-12);
    println!("{name:<28} {shapes:>3} shapes x {per_shape} effects: {now_ms:>7.3} ms/frame now, {before_ms:>7.3} ms/frame before ({factor:.1}x)");
}

#[test]
fn bench_timeline_frame_cost() {
    run("20 shapes, no triggers", 20, 3, 400, false);
    run("20 shapes, with triggers", 20, 3, 400, true);
    run("100 shapes, no triggers", 100, 3, 100, false);
}
