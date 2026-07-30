use super::io::{Reader, Writer};
use crate::{
    domain::{Confidence, Evidence, MemoryFact, MemoryNode},
    error::Result,
    id::{AgentId, EntityId, FactId, SessionId},
    time::Timestamp,
};
use std::collections::BTreeMap;

pub(super) fn write_node(writer: &mut Writer, node: &MemoryNode) -> Result<()> {
    writer.string(node.id.as_str())?;
    writer.string(&node.kind)?;
    writer.string(&node.label)?;
    writer.optional_string(node.repository.as_deref())?;
    writer.optional_string(node.branch.as_deref())?;
    writer.usize(node.attributes.len())?;
    for (key, value) in &node.attributes {
        writer.string(key)?;
        writer.string(value)?;
    }
    Ok(())
}

pub(super) fn read_node(reader: &mut Reader<'_>) -> Result<MemoryNode> {
    let id = EntityId::new(reader.string()?)?;
    let kind = reader.string()?;
    let label = reader.string()?;
    let repository = reader.optional_string()?;
    let branch = reader.optional_string()?;
    let count = reader.count()?;
    let mut attributes = BTreeMap::new();
    for _ in 0..count {
        let key = reader.string()?;
        let value = reader.string()?;
        if attributes.insert(key, value).is_some() {
            return Err(super::io::codec("duplicate node attribute"));
        }
    }
    Ok(MemoryNode {
        id,
        kind,
        label,
        repository,
        branch,
        attributes,
    })
}

pub(super) fn write_fact(writer: &mut Writer, fact: &MemoryFact) -> Result<()> {
    writer.string(fact.id.as_str())?;
    writer.string(fact.source.as_str())?;
    writer.string(&fact.relation)?;
    writer.string(fact.target.as_str())?;
    writer.signed(fact.valid_from.as_unix_micros());
    writer.optional_signed(fact.valid_until.map(Timestamp::as_unix_micros));
    writer.signed(fact.observed_at.as_unix_micros());
    writer.signed(fact.recorded_at.as_unix_micros());
    writer.string(fact.agent_id.as_str())?;
    writer.string(fact.session_id.as_str())?;
    writer.varint(u64::from(fact.confidence.basis_points()));
    writer.usize(fact.evidence.len())?;
    for evidence in &fact.evidence {
        write_evidence(writer, evidence)?;
    }
    writer.bool(fact.supersedes.is_some());
    if let Some(id) = &fact.supersedes {
        writer.string(id.as_str())?;
    }
    Ok(())
}

pub(super) fn read_fact(reader: &mut Reader<'_>) -> Result<MemoryFact> {
    let id = FactId::new(reader.string()?)?;
    let source = EntityId::new(reader.string()?)?;
    let relation = reader.string()?;
    let target = EntityId::new(reader.string()?)?;
    let valid_from = Timestamp::from_unix_micros(reader.signed()?);
    let valid_until = reader.optional_signed()?.map(Timestamp::from_unix_micros);
    let observed_at = Timestamp::from_unix_micros(reader.signed()?);
    let recorded_at = Timestamp::from_unix_micros(reader.signed()?);
    let agent_id = AgentId::new(reader.string()?)?;
    let session_id = SessionId::new(reader.string()?)?;
    let confidence =
        u16::try_from(reader.varint()?).map_err(|_| super::io::codec("confidence exceeds u16"))?;
    let confidence = Confidence::from_basis_points(confidence)?;
    let count = reader.count()?;
    let mut evidence = Vec::with_capacity(count);
    for _ in 0..count {
        evidence.push(read_evidence(reader)?);
    }
    let supersedes = reader
        .bool()?
        .then(|| reader.string().and_then(FactId::new))
        .transpose()?;
    Ok(MemoryFact {
        id,
        source,
        relation,
        target,
        valid_from,
        valid_until,
        observed_at,
        recorded_at,
        agent_id,
        session_id,
        confidence,
        evidence,
        supersedes,
    })
}

fn write_evidence(writer: &mut Writer, evidence: &Evidence) -> Result<()> {
    writer.string(&evidence.kind)?;
    writer.string(&evidence.source)?;
    writer.optional_string(evidence.locator.as_deref())?;
    writer.optional_string(evidence.digest.as_deref())
}

fn read_evidence(reader: &mut Reader<'_>) -> Result<Evidence> {
    Ok(Evidence {
        kind: reader.string()?,
        source: reader.string()?,
        locator: reader.optional_string()?,
        digest: reader.optional_string()?,
    })
}
