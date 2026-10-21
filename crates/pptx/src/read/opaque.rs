//! Bounded relationships of parts carried by an unmodelled object. Bytes live in the ordinary
//! media store so native saves and cross-document copies can keep them too.

use std::collections::{HashMap, HashSet};

use deckcraft_model::{MediaId, OpaqueRel};

use super::{Imp, Part};
use crate::xml::El;

const MAX_PARTS: usize = 256;
const MAX_RELS: usize = 1024;
const MAX_TOTAL_RELS: usize = 16_384;
const MAX_RELS_XML_BYTES: usize = 1 << 20;
const MAX_TOTAL_RELS_XML_BYTES: usize = 16 << 20;
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
    let start = imp.media.len();
    let mut parts = vec![];
    let mut rels = vec![];
    let mut seen_rels = HashSet::new();
    let mut budget = Budget::default();
    for id in super::rel_refs(el).into_iter().take(MAX_RELS) {
        let Some(rel) = part.rels.get(&id) else { continue };
        if !spend_visit(imp, &mut budget) || !safe_word(&rel.id) || !safe_word(&rel.ty) || !safe_word(&rel.target_raw) {
            continue;
        }
        if rel.external {
            parts.push((id, rel.ty.clone(), imp.media_link(&rel.target_raw)));
            continue;
        }
        let checkpoint = imp.media.len();
        let mut closure = Closure::default();
        if let Some(media) = copy_part(imp, &rel.target, rel.kind == "diagramData", &mut closure, &mut budget, 0) {
            parts.push((id, rel.ty.clone(), media));
            for r in closure.rels {
                if seen_rels.insert((r.source, r.id.clone())) {
                    rels.push(r);
                }
            }
        } else {
            discard_new(imp, checkpoint, &HashSet::new());
            log::warn!("pptx: kept object part {} has missing, unsupported or over-budget relationships", rel.target);
        }
    }
    let available = parts.iter().map(|(id, ..)| id.as_str()).collect();
    let kept = retained_refs(el, &available).unwrap_or_default();
    parts.retain(|(id, ..)| kept.contains(id));
    let used = graph_ids(parts.iter().map(|(_, _, id)| *id), &rels);
    rels.retain(|r| used.contains(&r.source));
    discard_new(imp, start, &used);
    (parts, rels)
}

/// Discard only allocations made by this attempt; earlier previews and shared parts stay owned.
/// Ids and traversal budgets remain spent so rejection cannot restart unbounded work.
fn discard_new(imp: &mut Imp, start: usize, keep: &HashSet<MediaId>) {
    let mut removed = HashSet::new();
    let added = imp.media.split_off(start);
    for item in added {
        if keep.contains(&item.id) {
            imp.media.push(item);
        } else {
            removed.insert(item.id);
        }
    }
    imp.media_by_part.retain(|_, id| !removed.contains(id));
}

/// The references of complete branches only. A normal frame needs every root; an alternate
/// block can keep valid choices, or its final valid fallback when no choice survives.
pub(crate) fn retained_refs(el: &El, available: &HashSet<&str>) -> Option<HashSet<String>> {
    let complete = |e: &El| {
        let refs = super::rel_refs(e);
        refs.iter().all(|r| available.contains(r.as_str())).then_some(refs)
    };
    if !el.is("AlternateContent") {
        return complete(el).map(|r| r.into_iter().collect());
    }
    let mut choices = 0;
    let mut used = HashSet::new();
    let mut fallback = None;
    for branch in el.elements().filter(|e| e.is("Choice") || e.is("Fallback")) {
        if let Some(refs) = complete(branch) {
            if branch.is("Choice") {
                choices += 1;
            } else {
                fallback = Some(refs.clone());
            }
            used.extend(refs);
        }
    }
    if choices > 0 { Some(used) } else { fallback.map(|r| r.into_iter().collect()) }
}

/// Reachable media ids, following each part's outgoing relations once even in cyclic graphs.
pub(crate) fn graph_ids(roots: impl Iterator<Item = MediaId>, rels: &[OpaqueRel]) -> HashSet<MediaId> {
    let mut used = HashSet::new();
    let mut pending: Vec<_> = roots.collect();
    while let Some(id) = pending.pop() {
        if used.insert(id) {
            pending.extend(rels.iter().filter(|r| r.source == id).map(|r| r.target));
        }
    }
    used
}

/// Relationship strings are retained in the model, so cap their contribution to the closure.
fn safe_word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control)
}

/// Charge attempted work before parsing or validation, so failures cannot renew the budget.
fn spend_visit(imp: &mut Imp, budget: &mut Budget) -> bool {
    if budget.rels >= MAX_RELS || imp.opaque_rels_read >= MAX_TOTAL_RELS {
        return false;
    }
    budget.rels += 1;
    imp.opaque_rels_read += 1;
    true
}

/// Bound relationship XML before Package::rels can build a tree or materialize its entries.
/// Repeated failed decodes spend import-wide bytes as well as visits; no media is allocated yet.
fn read_rels(imp: &mut Imp, part: &str, budget: &mut Budget) -> Option<crate::opc::Rels> {
    let path = crate::opc::rels_path(part);
    let Some(bytes) = imp.pkg.get(&path) else { return Some(crate::opc::Rels::default()) };
    let size = bytes.len();
    if !spend_visit(imp, budget) || size > MAX_RELS_XML_BYTES || size > MAX_BYTES.saturating_sub(budget.bytes) {
        return None;
    }
    if size > MAX_TOTAL_RELS_XML_BYTES.saturating_sub(imp.opaque_rels_xml_read) {
        imp.opaque_rels_xml_read = MAX_TOTAL_RELS_XML_BYTES;
        return None;
    }
    imp.opaque_rels_xml_read += size;
    budget.bytes += size;
    Some(imp.pkg.rels(part))
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
    let nested = read_rels(imp, part, budget)?;
    let media = if diagram { diagram_data(imp, part)? } else { imp.media_for(part)? };
    // Register before descending: a cycle reuses this id and never recurses forever.
    closure.seen.insert(key, media);
    let mut ids = HashSet::new();
    for rel in nested.items {
        if !spend_visit(imp, budget) || !safe_word(&rel.id) || !safe_word(&rel.ty) || !safe_word(&rel.target_raw) || !ids.insert(rel.id.clone()) {
            return None;
        }
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
    /// Remove stale slide-scoped drawing ids without touching the part's own relationships.
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
