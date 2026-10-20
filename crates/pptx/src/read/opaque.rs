//! Bounded relationships of parts carried by an unmodelled object. Bytes live in the ordinary
//! media store so native saves and cross-document copies can keep them too.

use std::collections::{HashMap, HashSet};

use deckcraft_model::{MediaId, OpaqueRel};

use super::{Imp, Part};
use crate::xml::El;

const MAX_PARTS: usize = 256;
const MAX_RELS: usize = 1024;
const MAX_TOTAL_RELS: usize = 16_384;
const MAX_DEPTH: usize = 8;
const MAX_BYTES: usize = 64 << 20;

type Parts = Vec<(String, String, MediaId)>;

#[derive(Default)]
struct Budget {
    parts: usize,
    rels: usize,
    bytes: usize,
}

#[derive(Default)]
struct Closure {
    seen: HashMap<String, MediaId>,
    rels: Vec<OpaqueRel>,
}

/// The parts named by an object's XML, with the complete relationships of each kept part.
/// A missing or over-budget target rejects that root part instead of emitting dangling links.
pub(super) fn capture(imp: &mut Imp, part: &Part, el: &El) -> (Parts, Vec<OpaqueRel>) {
    let mut parts = vec![];
    let mut rels = vec![];
    let mut seen_rels = HashSet::new();
    let mut budget = Budget::default();
    for id in super::rel_refs(el).into_iter().take(MAX_RELS) {
        let Some(rel) = part.rels.get(&id).filter(|r| !r.external) else { continue };
        let mut closure = Closure::default();
        if let Some(media) = copy_part(imp, &rel.target, rel.kind == "diagramData", &mut closure, &mut budget, 0) {
            parts.push((id, rel.ty.clone(), media));
            for r in closure.rels {
                if seen_rels.insert((r.source, r.id.clone())) {
                    rels.push(r);
                }
            }
        } else {
            log::warn!("pptx: kept object part {} has missing, unsupported or over-budget relationships", rel.target);
        }
    }
    (parts, rels)
}

/// Relationship strings are retained in the model, so cap their contribution to the closure.
fn safe_word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control)
}

/// Follow data/media sidecars, not the presentation's slide/master graph.
fn child_part(part: &str) -> bool {
    if crate::opc::sidecar_part(part) {
        return true;
    }
    let n = crate::opc::norm(part);
    n.starts_with("ppt/diagrams/") && n.split('/').all(|s| !s.is_empty() && s != "." && s != "..") && !n.ends_with(".rels")
}

/// Register parts before descending to bound cycles. Failed branches still spend their budget,
/// so repeated references to a damaged graph cannot restart unbounded work.
fn copy_part(imp: &mut Imp, part: &str, diagram: bool, closure: &mut Closure, budget: &mut Budget, depth: usize) -> Option<MediaId> {
    let key = crate::opc::norm(part);
    if let Some(id) = closure.seen.get(&key) {
        return Some(*id);
    }
    if depth > MAX_DEPTH || budget.parts >= MAX_PARTS || (depth > 0 && !child_part(part)) {
        return None;
    }
    let size = imp.pkg.get(part)?.len();
    if size > MAX_BYTES.saturating_sub(budget.bytes) {
        return None;
    }
    budget.parts += 1;
    budget.bytes += size;
    let media = if diagram { diagram_data(imp, part)? } else { imp.media_for(part)? };
    // Register before descending: a cycle reuses this id and never recurses forever.
    closure.seen.insert(key, media);
    let mut ids = HashSet::new();
    for rel in imp.pkg.rels(part).items {
        if budget.rels >= MAX_RELS
            || imp.opaque_rels_read >= MAX_TOTAL_RELS
            || !safe_word(&rel.id)
            || !safe_word(&rel.ty)
            || !safe_word(&rel.target_raw)
            || !ids.insert(rel.id.clone())
        {
            return None;
        }
        budget.rels += 1;
        imp.opaque_rels_read += 1;
        let target = if rel.external {
            // Preserve the address as data; nothing here opens a URL or a local file.
            imp.media_link(&rel.target_raw)
        } else {
            copy_part(imp, &rel.target, rel.kind == "diagramData", closure, budget, depth + 1)?
        };
        closure.rels.push(OpaqueRel { source: media, id: rel.id, rel_type: rel.ty, target });
    }
    Some(media)
}

/// A diagram with no readable cached drawing keeps its data, but not a stale slide-scoped
/// `dataModelExt relId`. Its ordinary part-scoped relationships are retained separately.
fn diagram_data(imp: &mut Imp, target: &str) -> Option<MediaId> {
    let key = crate::opc::norm(target);
    if let Some(id) = imp.media_by_part.get(&key) {
        return Some(*id);
    }
    fn strip(e: &mut El, depth: usize) {
        if depth > crate::xml::MAX_DEPTH {
            return;
        }
        e.children.retain(|n| !matches!(n, crate::xml::Node::El(c) if c.is("ext") && c.find("dataModelExt").is_some()));
        for c in e.elements_mut() {
            strip(c, depth + 1);
        }
    }
    let mut doc = imp.pkg.xml(target)?;
    strip(&mut doc.root, 0);
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}", doc.root.to_xml());
    let id = MediaId(imp.alloc());
    let content_type = imp.pkg.content_type(target)?;
    imp.media.push(deckcraft_model::MediaItem {
        id,
        name: crate::opc::file_of(target).to_string(),
        content_type,
        data: std::sync::Arc::new(xml.into_bytes()),
        link: None,
    });
    imp.media_by_part.insert(key, id);
    Some(id)
}
