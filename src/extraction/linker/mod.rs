use super::{
    ExtractedEntity, ExtractionInput, LinkCandidate, LinkDecision, LinkMethod, LinkPolicy,
    key::normalized,
};
use crate::{
    domain::Confidence,
    error::{MemoryError, Result},
    id::EntityId,
};
use std::collections::{BTreeMap, HashMap};

mod catalog;
mod scoring;
use catalog::IndexBucket;
use scoring::{decision, exact_key, generated_id, scope_score};

#[derive(Debug, Clone)]
pub struct EntityLinker {
    nodes: Vec<CatalogEntity>,
    by_id: HashMap<EntityId, usize>,
    by_label: HashMap<(String, String), IndexBucket>,
    by_alias: HashMap<(String, String), IndexBucket>,
    by_external_id: HashMap<(String, String, String), IndexBucket>,
}

#[derive(Debug, Clone)]
struct CatalogEntity {
    id: EntityId,
    kind: String,
    repository: Option<String>,
    branch: Option<String>,
}

impl EntityLinker {
    /// Resolves one extracted mention without silently choosing ambiguity.
    ///
    /// # Errors
    ///
    /// Rejects stable identifiers whose existing entity has a different kind,
    /// generated identifier collisions, or invalid policy scores.
    pub fn link(
        &self,
        mention: &ExtractedEntity,
        input: &ExtractionInput,
        policy: LinkPolicy,
    ) -> Result<LinkDecision> {
        if policy.minimum_score > 10_000 || policy.minimum_margin > 10_000 {
            return Err(MemoryError::InvalidValue {
                field: "link_policy",
                reason: "scores must be between 0 and 10,000 basis points",
            });
        }
        if let Some(stable_id) = &mention.stable_id {
            return self.link_stable_id(mention, stable_id);
        }
        let candidates = self.collect_candidates(mention, input)?;
        self.resolve_candidates(mention, input, policy, candidates)
    }

    fn link_stable_id(
        &self,
        mention: &ExtractedEntity,
        stable_id: &EntityId,
    ) -> Result<LinkDecision> {
        if let Some(index) = self.by_id.get(stable_id) {
            if self.nodes[*index].kind != normalized(&mention.kind) {
                return Err(MemoryError::InvalidValue {
                    field: "extracted_entity.stable_id",
                    reason: "existing entity kind does not match extracted kind",
                });
            }
            let score = mention.confidence;
            return Ok(decision(
                mention,
                Some(stable_id.clone()),
                score,
                LinkMethod::StableId,
                vec![LinkCandidate {
                    entity_id: stable_id.clone(),
                    score,
                    method: LinkMethod::StableId,
                }],
            ));
        }
        Ok(decision(
            mention,
            Some(stable_id.clone()),
            mention.confidence,
            LinkMethod::Created,
            Vec::new(),
        ))
    }

    fn collect_candidates(
        &self,
        mention: &ExtractedEntity,
        input: &ExtractionInput,
    ) -> Result<Vec<LinkCandidate>> {
        let kind = normalized(&mention.kind);
        let mut candidates = BTreeMap::<EntityId, LinkCandidate>::new();
        self.add_index_matches(
            self.by_label
                .get(&(kind.clone(), normalized(&mention.label))),
            mention,
            input,
            9_000,
            LinkMethod::Label,
            &mut candidates,
        )?;
        self.add_index_matches(
            self.by_alias
                .get(&(kind.clone(), normalized(&mention.label))),
            mention,
            input,
            8_500,
            LinkMethod::Alias,
            &mut candidates,
        )?;
        for alias in &mention.aliases {
            self.add_index_matches(
                self.by_alias.get(&(kind.clone(), normalized(alias))),
                mention,
                input,
                8_500,
                LinkMethod::Alias,
                &mut candidates,
            )?;
        }
        for (key, value) in &mention.attributes {
            if key.starts_with("external_id.") {
                self.add_index_matches(
                    self.by_external_id
                        .get(&(kind.clone(), key.clone(), exact_key(value))),
                    mention,
                    input,
                    9_700,
                    LinkMethod::ExternalId,
                    &mut candidates,
                )?;
            }
        }
        for hint in &mention.hints {
            if let Some(index) = self.by_id.get(&hint.entity_id)
                && self.nodes[*index].kind == kind
            {
                self.add_candidate(
                    *index,
                    mention,
                    input,
                    hint.confidence.basis_points(),
                    LinkMethod::ProviderHint,
                    &mut candidates,
                )?;
            }
        }
        let mut candidates = candidates.into_values().collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.entity_id.cmp(&right.entity_id))
        });
        Ok(candidates)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_index_matches(
        &self,
        indexes: Option<&IndexBucket>,
        mention: &ExtractedEntity,
        input: &ExtractionInput,
        base_score: u16,
        method: LinkMethod,
        candidates: &mut BTreeMap<EntityId, LinkCandidate>,
    ) -> Result<()> {
        if let Some(indexes) = indexes {
            match indexes {
                IndexBucket::One(index) => {
                    self.add_candidate(*index, mention, input, base_score, method, candidates)?;
                }
                IndexBucket::Many(indexes) => {
                    for index in indexes {
                        self.add_candidate(*index, mention, input, base_score, method, candidates)?;
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_candidate(
        &self,
        index: usize,
        mention: &ExtractedEntity,
        input: &ExtractionInput,
        base_score: u16,
        method: LinkMethod,
        candidates: &mut BTreeMap<EntityId, LinkCandidate>,
    ) -> Result<()> {
        let node = &self.nodes[index];
        let (score, scoped) = scope_score(base_score, node, input);
        let score = score.min(mention.confidence.basis_points());
        let method = if method == LinkMethod::Label && scoped {
            LinkMethod::ScopedLabel
        } else {
            method
        };
        let candidate = LinkCandidate {
            entity_id: node.id.clone(),
            score: Confidence::from_basis_points(score)?,
            method,
        };
        match candidates.get(&node.id) {
            Some(existing) if existing.score >= candidate.score => {}
            _ => {
                candidates.insert(node.id.clone(), candidate);
            }
        }
        Ok(())
    }

    fn resolve_candidates(
        &self,
        mention: &ExtractedEntity,
        input: &ExtractionInput,
        policy: LinkPolicy,
        candidates: Vec<LinkCandidate>,
    ) -> Result<LinkDecision> {
        let Some(best) = candidates.first() else {
            return self.unmatched(mention, input, policy, candidates);
        };
        if best.score.basis_points() < policy.minimum_score {
            return self.unmatched(mention, input, policy, candidates);
        }
        if candidates.get(1).is_some_and(|second| {
            best.score
                .basis_points()
                .saturating_sub(second.score.basis_points())
                < policy.minimum_margin
        }) {
            return Ok(decision(
                mention,
                None,
                best.score,
                LinkMethod::Ambiguous,
                candidates,
            ));
        }
        Ok(decision(
            mention,
            Some(best.entity_id.clone()),
            best.score,
            best.method,
            candidates,
        ))
    }

    fn unmatched(
        &self,
        mention: &ExtractedEntity,
        input: &ExtractionInput,
        policy: LinkPolicy,
        candidates: Vec<LinkCandidate>,
    ) -> Result<LinkDecision> {
        if !policy.create_unmatched {
            return Ok(decision(
                mention,
                None,
                Confidence::from_basis_points(0)?,
                LinkMethod::Unresolved,
                candidates,
            ));
        }
        let id = generated_id(mention, input)?;
        if self.by_id.contains_key(&id) {
            return Err(MemoryError::Extraction {
                provider: "entity-linker".to_owned(),
                message: format!("generated entity identifier collides with {id}"),
            });
        }
        Ok(decision(
            mention,
            Some(id),
            mention.confidence,
            LinkMethod::Created,
            candidates,
        ))
    }
}
