//! Relationships between the media items kept by an opaque object. All part names come from
//! the regular exporter, never from the source package's paths.

use std::collections::{HashMap, HashSet};

use deckcraft_model::{MediaId, OpaqueRel};

use super::Exp;
use crate::opc::RelsOut;

impl Exp<'_> {
    /// Validate the reachable graph without emitting bytes, then write each part's relationships
    /// once. Rejected roots and discarded alternate branches never add hidden package entries.
    pub(super) fn opaque_rels(&mut self, roots: &[MediaId], rels: &[OpaqueRel]) -> bool {
        if rels.len() > 1024 {
            return false;
        }
        let used = crate::read::opaque::graph_ids(roots.iter().copied(), rels);
        let mut planned: HashMap<MediaId, Vec<(String, String, MediaId)>> = HashMap::new();
        for id in &used {
            let Some(media) = self.p.media(*id) else { return false };
            if media.data.is_empty() {
                if !media.link.as_deref().is_some_and(safe_word) {
                    return false;
                }
            } else {
                planned.insert(*id, vec![]);
            }
        }
        let mut ids = HashSet::new();
        // Ignore metadata belonging only to a branch that will not be reconstructed.
        for rel in rels.iter().filter(|r| used.contains(&r.source)) {
            if !safe_word(&rel.id) || !safe_word(&rel.rel_type) || !ids.insert((rel.source, rel.id.as_str())) {
                return false;
            }
            let Some(out) = planned.get_mut(&rel.source) else { return false };
            out.push((rel.id.clone(), rel.rel_type.clone(), rel.target));
        }
        for out in planned.values_mut() {
            out.sort_unstable_by(|a, b| (&a.0, &a.1, a.2.0).cmp(&(&b.0, &b.1, b.2.0)));
        }
        // A shared media part must have one consistent set of relationships across objects.
        // Compare media identities before allocating output paths, including empty sets.
        for (id, out) in &planned {
            if self.media_relationships.get(id).is_some_and(|old| old != out) {
                return false;
            }
        }
        let mut order: Vec<_> = planned.keys().copied().collect();
        order.sort_unstable_by_key(|id| id.0);
        for id in order {
            // Every embedded media id was checked above against the immutable presentation.
            let _ = self.media_part(id);
        }
        for (id, items) in planned {
            if self.media_relationships.contains_key(&id) {
                continue;
            }
            let Some(source) = self.media_parts.get(&id) else { return false };
            let mut out = RelsOut::default();
            for (rid, ty, target) in &items {
                let Some(media) = self.p.media(*target) else { return false };
                let (target, external) = if media.data.is_empty() {
                    let Some(link) = &media.link else { return false };
                    (link.clone(), true)
                } else {
                    let Some(name) = self.media_parts.get(target) else { return false };
                    (crate::opc::relative(source, name), false)
                };
                out.items.push((rid.clone(), ty.clone(), target, external));
            }
            self.pkg.rels(source, &out);
            self.media_relationships.insert(id, items);
        }
        true
    }
}

/// Bound strings retained in native data before they become relationship attributes.
fn safe_word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control)
}
