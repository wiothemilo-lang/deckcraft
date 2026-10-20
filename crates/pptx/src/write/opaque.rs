//! Relationships between the media items kept by an opaque object. All part names come from
//! the regular exporter, never from the source package's paths.

use std::collections::{HashMap, HashSet};

use deckcraft_model::{MediaId, OpaqueRel};

use super::Exp;
use crate::opc::RelsOut;

impl Exp<'_> {
    /// Write each kept part's relationships once, preserving ids used inside its XML. Validate
    /// the complete set before writing any relationship part; malformed native data falls back.
    pub(super) fn opaque_rels(&mut self, rels: &[OpaqueRel]) -> bool {
        if rels.len() > 1024 {
            return false;
        }
        let mut planned: HashMap<MediaId, (String, RelsOut)> = HashMap::new();
        let mut ids = HashSet::new();
        for rel in rels {
            if [&rel.id, &rel.rel_type].iter().any(|s| s.is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
                || !ids.insert((rel.source, rel.id.as_str()))
            {
                return false;
            }
            let Some(source) = self.media_part(rel.source) else { return false };
            let Some(target) = self.p.media(rel.target) else { return false };
            let (target, external) = if target.data.is_empty() {
                let Some(link) = target.link.as_ref().filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control)) else {
                    return false;
                };
                (link.clone(), true)
            } else {
                let Some(name) = self.media_part(rel.target) else { return false };
                (crate::opc::relative(&source, &name), false)
            };
            let (_, out) = planned.entry(rel.source).or_insert_with(|| (source, RelsOut::default()));
            out.items.push((rel.id.clone(), rel.rel_type.clone(), target, external));
        }
        for (_, out) in planned.values_mut() {
            out.items.sort_unstable();
        }
        // A shared media part must have one consistent set of relationships across objects.
        for (id, (_, out)) in &planned {
            if self.media_relationships.get(id).is_some_and(|old| old != &out.items) {
                return false;
            }
        }
        for (id, (source, out)) in planned {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.media_relationships.entry(id) {
                self.pkg.rels(&source, &out);
                entry.insert(out.items);
            }
        }
        true
    }
}
