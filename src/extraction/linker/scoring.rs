use super::super::{
    ExtractedEntity, ExtractionInput, LinkCandidate, LinkDecision, LinkMethod,
    key::{normalized, slug, stable_hash},
};
use crate::{Confidence, EntityId, Result};

pub(super) fn decision(
    mention: &ExtractedEntity,
    entity_id: Option<EntityId>,
    score: Confidence,
    method: LinkMethod,
    candidates: Vec<LinkCandidate>,
) -> LinkDecision {
    LinkDecision {
        mention_id: mention.local_id.clone(),
        entity_id,
        score,
        method,
        candidates,
    }
}

pub(super) fn generated_id(mention: &ExtractedEntity, input: &ExtractionInput) -> Result<EntityId> {
    let label = normalized(&mention.label);
    let hash = stable_hash(&[
        input.repository.as_deref().unwrap_or(""),
        input.branch.as_deref().unwrap_or(""),
        &normalized(&mention.kind),
        &label,
    ]);
    EntityId::new(format!("auto:{}:{hash:016x}", slug(&mention.kind)))
}

pub(super) fn scope_score(
    base: u16,
    node: &super::CatalogEntity,
    input: &ExtractionInput,
) -> (u16, bool) {
    let mut score = i32::from(base);
    let mut scoped = false;
    if let Some(repository) = &input.repository {
        match node.repository.as_ref() {
            Some(candidate) if candidate == repository => {
                score += 400;
                scoped = true;
            }
            Some(_) => score -= 2_000,
            None => score -= 300,
        }
    }
    if let Some(branch) = &input.branch {
        match node.branch.as_ref() {
            Some(candidate) if candidate == branch => {
                score += 200;
                scoped = true;
            }
            Some(_) => score -= 700,
            None => score -= 100,
        }
    }
    (
        u16::try_from(score.clamp(0, 10_000)).expect("clamped score fits u16"),
        scoped,
    )
}

pub(super) fn exact_key(value: &str) -> String {
    value.trim().to_lowercase()
}
