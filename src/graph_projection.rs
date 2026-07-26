use crate::{Confidence as MemoryConfidence, MemoryView, Result};
use std::{collections::BTreeMap, str::FromStr};
use weavatrix_graph::{
    AttributeValue, Confidence, Edge, EdgeKind, EvidenceKind, Graph, Node, NodeId, NodeKind,
    Provenance,
};

/// Converts a temporal memory view into a canonical immutable graph.
///
/// # Errors
///
/// Returns graph validation errors for invalid custom kinds or endpoints.
pub fn project_graph(view: &MemoryView) -> Result<Graph> {
    let mut ids = BTreeMap::new();
    let mut nodes = Vec::with_capacity(view.nodes.len());
    for memory_node in &view.nodes {
        let mut node = Node::new(
            memory_node.id.as_str(),
            memory_node.label.clone(),
            NodeKind::from_str(&memory_node.kind)?,
        )?;
        if let Some(repository) = &memory_node.repository {
            node = node.with_attribute("memory.repository", repository.clone());
        }
        if let Some(branch) = &memory_node.branch {
            node = node.with_attribute("memory.branch", branch.clone());
        }
        for (key, value) in &memory_node.attributes {
            node = node.with_attribute(format!("memory.{key}"), value.clone());
        }
        ids.insert(memory_node.id.clone(), node.id.clone());
        nodes.push(node);
    }

    let mut edges = Vec::with_capacity(view.facts.len());
    for fact in &view.facts {
        let source = endpoint(&ids, &fact.source)?;
        let target = endpoint(&ids, &fact.target)?;
        let primary = &fact.evidence[0];
        let provenance = Provenance::new(
            primary.source.clone(),
            EvidenceKind::from_str(&primary.kind)?,
            graph_confidence(fact.confidence),
        )?
        .with_detail(
            primary
                .locator
                .clone()
                .unwrap_or_else(|| fact.id.to_string()),
        );
        let evidence = fact
            .evidence
            .iter()
            .map(|item| {
                AttributeValue::String(format!(
                    "{}:{}:{}",
                    item.kind,
                    item.source,
                    item.locator.as_deref().unwrap_or("")
                ))
            })
            .collect::<Vec<_>>();
        let mut edge = Edge::new(
            source,
            target,
            EdgeKind::from_str(&fact.relation)?,
            provenance,
        )
        .with_attribute("memory.fact_id", fact.id.to_string())
        .with_attribute("memory.valid_from", fact.valid_from.as_unix_micros())
        .with_attribute("memory.recorded_at", fact.recorded_at.as_unix_micros())
        .with_attribute(
            "memory.confidence_bps",
            u64::from(fact.confidence.basis_points()),
        )
        .with_attribute("memory.evidence", AttributeValue::List(evidence));
        if let Some(valid_until) = fact.valid_until {
            edge = edge.with_attribute("memory.valid_until", valid_until.as_unix_micros());
        }
        if let Some(supersedes) = &fact.supersedes {
            edge = edge.with_attribute("memory.supersedes", supersedes.to_string());
        }
        edges.push(edge);
    }
    Ok(Graph::try_from_sorted_nodes(nodes, edges)?)
}

fn endpoint(ids: &BTreeMap<crate::EntityId, NodeId>, id: &crate::EntityId) -> Result<NodeId> {
    ids.get(id)
        .cloned()
        .ok_or_else(|| crate::MemoryError::MissingEntity { id: id.to_string() })
}

const fn graph_confidence(confidence: MemoryConfidence) -> Confidence {
    match confidence.basis_points() {
        9_500..=10_000 => Confidence::Exact,
        8_000..=9_499 => Confidence::High,
        5_000..=7_999 => Confidence::Medium,
        _ => Confidence::Low,
    }
}
