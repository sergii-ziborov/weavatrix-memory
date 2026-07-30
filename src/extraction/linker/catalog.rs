use super::{EntityLinker, scoring::exact_key};
use crate::{
    domain::MemoryView,
    error::{MemoryError, Result},
    extraction::key::normalized,
};
use std::{collections::HashMap, hash::Hash};

#[derive(Debug, Clone)]
pub(super) enum IndexBucket {
    One(usize),
    Many(Vec<usize>),
}

impl EntityLinker {
    /// Builds reusable exact-match indexes from one temporal memory view.
    ///
    /// # Errors
    ///
    /// Rejects invalid catalog nodes, duplicate identifiers, and malformed
    /// aliases or external identifiers.
    pub fn from_view(view: &MemoryView) -> Result<Self> {
        let mut linker = Self {
            nodes: Vec::with_capacity(view.nodes.len()),
            by_id: HashMap::with_capacity(view.nodes.len()),
            by_label: HashMap::with_capacity(view.nodes.len()),
            by_alias: HashMap::with_capacity(view.nodes.len()),
            by_external_id: HashMap::with_capacity(view.nodes.len()),
        };
        for (index, node) in view.nodes.iter().enumerate() {
            node.validate()?;
            if linker.by_id.insert(node.id.clone(), index).is_some() {
                return Err(MemoryError::InvalidValue {
                    field: "entity_linker.catalog",
                    reason: "entity identifiers must be unique",
                });
            }
            let kind = normalized(&node.kind);
            linker.nodes.push(super::CatalogEntity {
                id: node.id.clone(),
                kind: kind.clone(),
                repository: node.repository.clone(),
                branch: node.branch.clone(),
            });
            insert_index(
                &mut linker.by_label,
                (kind.clone(), normalized(&node.label)),
                index,
            );
            for (key, value) in &node.attributes {
                if key == "alias" || key.starts_with("alias.") {
                    crate::domain::validate_text("entity_linker.alias", value)?;
                    insert_index(
                        &mut linker.by_alias,
                        (kind.clone(), normalized(value)),
                        index,
                    );
                } else if key.starts_with("external_id.") {
                    crate::domain::validate_text("entity_linker.external_id", value)?;
                    insert_index(
                        &mut linker.by_external_id,
                        (kind.clone(), key.clone(), exact_key(value)),
                        index,
                    );
                }
            }
        }
        Ok(linker)
    }
}

fn insert_index<K: Eq + Hash>(index: &mut HashMap<K, IndexBucket>, key: K, value: usize) {
    match index.entry(key) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(IndexBucket::One(value));
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => match entry.get_mut() {
            IndexBucket::One(first) => {
                let first = *first;
                entry.insert(IndexBucket::Many(vec![first, value]));
            }
            IndexBucket::Many(values) => values.push(value),
        },
    }
}
