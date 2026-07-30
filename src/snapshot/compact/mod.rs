mod domain;
mod io;

use crate::{
    codec::Codec,
    error::Result,
    id::{FactId, StreamId},
    projection::{
        ProjectionSnapshot, ReplayCursor,
        memory::{
            index::IdIndex,
            state::{MemoryProjection, NodeHistory, NodeRevision, Retraction, Supersession},
        },
    },
    time::Timestamp,
};
use domain::{read_fact, read_node, write_fact, write_node};
use io::{Reader, Writer, codec};
use std::collections::{BTreeMap, HashMap};

const HEADER: &[u8; 8] = b"WMEMCB01";

#[derive(Debug, Clone, Copy, Default)]
pub struct CompactSnapshotCodec;

impl Codec<ProjectionSnapshot<MemoryProjection>> for CompactSnapshotCodec {
    fn encode(&self, value: &ProjectionSnapshot<MemoryProjection>) -> Result<Vec<u8>> {
        let mut writer = Writer::new(HEADER);
        write_cursor(&mut writer, &value.cursor)?;
        write_projection(&mut writer, &value.projection)?;
        Ok(writer.finish())
    }

    fn decode(&self, bytes: &[u8]) -> Result<ProjectionSnapshot<MemoryProjection>> {
        let mut reader = Reader::new(bytes, HEADER)?;
        let cursor = read_cursor(&mut reader)?;
        let projection = read_projection(&mut reader)?;
        reader.finish()?;
        if cursor.global_position != projection.last_global_position {
            return Err(codec("cursor and projection positions disagree"));
        }
        Ok(ProjectionSnapshot { cursor, projection })
    }
}

fn write_cursor(writer: &mut Writer, cursor: &ReplayCursor) -> Result<()> {
    writer.optional_u64(cursor.global_position);
    writer.usize(cursor.stream_versions.len())?;
    for (stream, version) in &cursor.stream_versions {
        writer.string(stream.as_str())?;
        writer.varint(*version);
    }
    Ok(())
}

fn read_cursor(reader: &mut Reader<'_>) -> Result<ReplayCursor> {
    let global_position = reader.optional_u64()?;
    let count = reader.count()?;
    let mut stream_versions = BTreeMap::new();
    for _ in 0..count {
        let stream = StreamId::new(reader.string()?)?;
        let version = reader.varint()?;
        if stream_versions.insert(stream, version).is_some() {
            return Err(codec("duplicate stream in replay cursor"));
        }
    }
    Ok(ReplayCursor {
        global_position,
        stream_versions,
    })
}

fn write_projection(writer: &mut Writer, projection: &MemoryProjection) -> Result<()> {
    writer.usize(projection.nodes.len())?;
    for history in &projection.nodes {
        writer.usize(history.later.len() + 1)?;
        write_revision(writer, &history.first)?;
        for revision in &history.later {
            write_revision(writer, revision)?;
        }
    }
    writer.usize(projection.facts.len())?;
    for fact in &projection.facts {
        write_fact(writer, fact)?;
    }
    writer.usize(projection.supersessions.len())?;
    for (prior, change) in &projection.supersessions {
        writer.string(prior.as_str())?;
        writer.string(change.replacement.as_str())?;
        writer.signed(change.valid_from.as_unix_micros());
        writer.signed(change.recorded_at.as_unix_micros());
    }
    writer.usize(projection.retractions.len())?;
    for (fact, change) in &projection.retractions {
        writer.string(fact.as_str())?;
        writer.signed(change.valid_until.as_unix_micros());
        writer.signed(change.recorded_at.as_unix_micros());
    }
    writer.optional_u64(projection.last_global_position);
    Ok(())
}

fn read_projection(reader: &mut Reader<'_>) -> Result<MemoryProjection> {
    let node_count = reader.count()?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(read_history(reader)?);
    }
    let fact_count = reader.count()?;
    let mut facts = Vec::with_capacity(fact_count);
    for _ in 0..fact_count {
        facts.push(read_fact(reader)?);
    }
    let supersessions = read_supersessions(reader)?;
    let retractions = read_retractions(reader)?;
    let last_global_position = reader.optional_u64()?;
    let mut projection = MemoryProjection {
        nodes,
        facts,
        node_lookup: IdIndex::with_capacity(node_count),
        fact_lookup: IdIndex::with_capacity(fact_count),
        incident_offsets: Vec::new(),
        incident_facts: Vec::new(),
        incident_delta: HashMap::new(),
        supersessions,
        retractions,
        last_global_position,
    };
    projection.rebuild_indexes()?;
    Ok(projection)
}

fn write_revision(writer: &mut Writer, revision: &NodeRevision) -> Result<()> {
    write_node(writer, &revision.node)?;
    writer.signed(revision.recorded_at.as_unix_micros());
    writer.varint(revision.position);
    Ok(())
}

fn read_history(reader: &mut Reader<'_>) -> Result<NodeHistory> {
    let count = reader.count()?;
    if count == 0 {
        return Err(codec("node revision history must not be empty"));
    }
    let first = read_revision(reader)?;
    let mut later = Vec::with_capacity(count - 1);
    for _ in 1..count {
        later.push(read_revision(reader)?);
    }
    Ok(NodeHistory { first, later })
}

fn read_revision(reader: &mut Reader<'_>) -> Result<NodeRevision> {
    Ok(NodeRevision {
        node: read_node(reader)?,
        recorded_at: Timestamp::from_unix_micros(reader.signed()?),
        position: reader.varint()?,
    })
}

fn read_supersessions(reader: &mut Reader<'_>) -> Result<BTreeMap<FactId, Supersession>> {
    let count = reader.count()?;
    let mut changes = BTreeMap::new();
    for _ in 0..count {
        let prior = FactId::new(reader.string()?)?;
        let change = Supersession {
            replacement: FactId::new(reader.string()?)?,
            valid_from: Timestamp::from_unix_micros(reader.signed()?),
            recorded_at: Timestamp::from_unix_micros(reader.signed()?),
        };
        if changes.insert(prior, change).is_some() {
            return Err(codec("duplicate supersession"));
        }
    }
    Ok(changes)
}

fn read_retractions(reader: &mut Reader<'_>) -> Result<BTreeMap<FactId, Retraction>> {
    let count = reader.count()?;
    let mut changes = BTreeMap::new();
    for _ in 0..count {
        let fact = FactId::new(reader.string()?)?;
        let change = Retraction {
            valid_until: Timestamp::from_unix_micros(reader.signed()?),
            recorded_at: Timestamp::from_unix_micros(reader.signed()?),
        };
        if changes.insert(fact, change).is_some() {
            return Err(codec("duplicate retraction"));
        }
    }
    Ok(changes)
}
