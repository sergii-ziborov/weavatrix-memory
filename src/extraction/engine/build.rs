use super::super::{
    EntityLinker, ExtractedEntity, ExtractedRelation, ExtractionInput, ExtractionOutput,
    ExtractionPlan, LinkDecision, LinkMethod, LinkPolicy, RejectedRelation, TextSpan,
    key::stable_hash,
};
use crate::{
    Confidence, EntityId, EventId, Evidence, FactId, MemoryError, MemoryEvent, MemoryFact,
    MemoryNode, NewEvent, Result,
};
use std::collections::BTreeMap;

pub(super) fn build_plan(
    policy: LinkPolicy,
    provider: &str,
    input: &ExtractionInput,
    linker: &EntityLinker,
    output: &ExtractionOutput,
) -> Result<ExtractionPlan> {
    let mut links = Vec::with_capacity(output.entities.len());
    let mut by_mention = BTreeMap::new();
    for entity in &output.entities {
        let link = linker.link(entity, input, policy)?;
        by_mention.insert(entity.local_id.clone(), link.clone());
        links.push(link);
    }

    let fingerprint = format!("{:016x}", stable_hash(&[&input.content]));
    let mut events = Vec::with_capacity(output.entities.len() + output.relations.len());
    let mut created_nodes = BTreeMap::<EntityId, MemoryNode>::new();
    for (entity, link) in output.entities.iter().zip(&links) {
        if link.method != LinkMethod::Created {
            continue;
        }
        let id = link
            .entity_id
            .clone()
            .ok_or_else(|| extraction_error(provider, "created link has no entity identifier"))?;
        let node = build_node(entity, id.clone(), input, provider)?;
        if let Some(existing) = created_nodes.get_mut(&id) {
            merge_node(existing, node, provider)?;
        } else {
            created_nodes.insert(id, node);
        }
    }
    for node in created_nodes.into_values() {
        events.push(node_event(provider, input, &fingerprint, node)?);
    }

    let mut rejected_relations = Vec::new();
    for relation in &output.relations {
        let source = &by_mention[&relation.source];
        let target = &by_mention[&relation.target];
        let (Some(source_id), Some(target_id)) = (&source.entity_id, &target.entity_id) else {
            rejected_relations.push(rejected_relation(relation, source, target));
            continue;
        };
        let fact = build_fact(
            provider,
            input,
            &fingerprint,
            relation,
            source,
            target,
            source_id,
            target_id,
        )?;
        events.push(fact_event(provider, input, &fingerprint, fact)?);
    }
    Ok(ExtractionPlan {
        provider: provider.to_owned(),
        source: input.source.clone(),
        events,
        links,
        rejected_relations,
    })
}

fn merge_node(existing: &mut MemoryNode, incoming: MemoryNode, provider: &str) -> Result<()> {
    if existing.kind != incoming.kind
        || existing.label != incoming.label
        || existing.repository != incoming.repository
        || existing.branch != incoming.branch
    {
        return Err(extraction_error(
            provider,
            "two mentions generated conflicting entity payloads",
        ));
    }
    for (key, value) in incoming.attributes {
        if key.starts_with("alias.") {
            if existing
                .attributes
                .iter()
                .any(|(candidate, alias)| candidate.starts_with("alias.") && alias == &value)
            {
                continue;
            }
            let mut index = existing.attributes.len();
            while existing.attributes.contains_key(&format!("alias.{index}")) {
                index += 1;
            }
            existing.attributes.insert(format!("alias.{index}"), value);
        } else if existing
            .attributes
            .insert(key, value.clone())
            .is_some_and(|previous| previous != value)
        {
            return Err(extraction_error(
                provider,
                "two mentions generated conflicting entity attributes",
            ));
        }
    }
    Ok(())
}

fn build_node(
    entity: &ExtractedEntity,
    id: EntityId,
    input: &ExtractionInput,
    provider: &str,
) -> Result<MemoryNode> {
    let mut node = MemoryNode::new(id, entity.kind.clone(), entity.label.clone())?
        .with_attribute("extracted_by", provider)
        .with_attribute("extraction_source", &input.source);
    if let Some(repository) = &input.repository {
        node = node.in_repository(repository);
    }
    if let Some(branch) = &input.branch {
        node = node.on_branch(branch);
    }
    for (key, value) in &entity.attributes {
        node = node.with_attribute(key, value);
    }
    for (index, alias) in entity.aliases.iter().enumerate() {
        node = node.with_attribute(format!("alias.{index}"), alias);
    }
    Ok(node)
}

#[allow(clippy::too_many_arguments)]
fn build_fact(
    provider: &str,
    input: &ExtractionInput,
    fingerprint: &str,
    relation: &ExtractedRelation,
    source_link: &LinkDecision,
    target_link: &LinkDecision,
    source: &EntityId,
    target: &EntityId,
) -> Result<MemoryFact> {
    let id = FactId::new(format!(
        "fact:auto:{:016x}",
        stable_hash(&[
            provider,
            &input.source,
            fingerprint,
            &relation.local_id,
            source.as_str(),
            &relation.relation,
            target.as_str(),
        ])
    ))?;
    let mut fact = MemoryFact::new(
        id,
        source.clone(),
        relation.relation.clone(),
        target.clone(),
        relation.valid_from.unwrap_or(input.occurred_at),
        input.recorded_at,
        input.agent_id.clone(),
        input.session_id.clone(),
        source_evidence(input)?,
    )?
    .observed_at(input.occurred_at)
    .with_confidence(min_confidence([
        relation.confidence,
        source_link.score,
        target_link.score,
    ]))
    .with_evidence(provider_evidence(provider, relation.span)?);
    if let Some(valid_until) = relation.valid_until {
        fact = fact.valid_until(valid_until)?;
    }
    Ok(fact)
}

fn source_evidence(input: &ExtractionInput) -> Result<Evidence> {
    let mut evidence = Evidence::new("source", &input.source)?;
    if let Some(locator) = &input.locator {
        evidence = evidence.with_locator(locator);
    }
    if let Some(digest) = &input.digest {
        evidence = evidence.with_digest(digest);
    }
    Ok(evidence)
}

fn provider_evidence(provider: &str, span: Option<TextSpan>) -> Result<Evidence> {
    let mut evidence = Evidence::new("extraction_provider", provider)?;
    if let Some(span) = span {
        evidence = evidence.with_locator(format!("bytes:{}-{}", span.start, span.end));
    }
    Ok(evidence)
}

fn min_confidence(values: [Confidence; 3]) -> Confidence {
    values.into_iter().min().unwrap_or(Confidence::CERTAIN)
}

fn node_event(
    provider: &str,
    input: &ExtractionInput,
    fingerprint: &str,
    node: MemoryNode,
) -> Result<NewEvent<MemoryEvent>> {
    let id = EventId::new(format!(
        "event:auto:node:{:016x}",
        stable_hash(&[provider, &input.source, fingerprint, node.id.as_str()])
    ))?;
    NewEvent::new(
        id,
        "node_upserted",
        input.occurred_at,
        input.recorded_at,
        input.agent_id.clone(),
        input.session_id.clone(),
        MemoryEvent::NodeUpserted { node },
    )
}

fn fact_event(
    provider: &str,
    input: &ExtractionInput,
    fingerprint: &str,
    fact: MemoryFact,
) -> Result<NewEvent<MemoryEvent>> {
    let id = EventId::new(format!(
        "event:auto:fact:{:016x}",
        stable_hash(&[provider, &input.source, fingerprint, fact.id.as_str()])
    ))?;
    NewEvent::new(
        id,
        "fact_recorded",
        input.occurred_at,
        input.recorded_at,
        input.agent_id.clone(),
        input.session_id.clone(),
        MemoryEvent::FactRecorded { fact },
    )
}

fn rejected_relation(
    relation: &ExtractedRelation,
    source: &LinkDecision,
    target: &LinkDecision,
) -> RejectedRelation {
    RejectedRelation {
        relation_id: relation.local_id.clone(),
        source_mention: relation.source.clone(),
        target_mention: relation.target.clone(),
        reason: format!(
            "unresolved endpoints: source={:?}, target={:?}",
            source.method, target.method
        ),
    }
}

fn extraction_error(provider: &str, message: &str) -> MemoryError {
    MemoryError::Extraction {
        provider: provider.to_owned(),
        message: message.to_owned(),
    }
}
