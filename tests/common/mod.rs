#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use weavatrix_memory::{
    AgentId, EntityId, EventId, EventStore, Evidence, ExpectedVersion, FactId, InMemoryStore,
    MemoryEvent, MemoryFact, MemoryNode, MemoryProjection, NewEvent, SessionId, StreamId,
    Timestamp, replay,
};

pub struct TempLog {
    path: PathBuf,
}

impl TempLog {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("weavatrix-memory-{}-{id}.wmem", std::process::id()));
        let _ = fs::remove_file(&path);
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempLog {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn ts(value: i64) -> Timestamp {
    Timestamp::from_unix_micros(value)
}

pub fn entity(value: &str) -> EntityId {
    EntityId::new(value).unwrap()
}

pub fn node(id: &str, kind: &str, label: &str) -> MemoryNode {
    MemoryNode::new(entity(id), kind, label).unwrap()
}

pub fn fact(
    id: &str,
    source: &str,
    relation: &str,
    target: &str,
    valid_from: i64,
    recorded_at: i64,
) -> MemoryFact {
    MemoryFact::new(
        FactId::new(id).unwrap(),
        entity(source),
        relation,
        entity(target),
        ts(valid_from),
        ts(recorded_at),
        agent(),
        session(),
        Evidence::new("test", "integration-suite").unwrap(),
    )
    .unwrap()
}

pub fn event(id: &str, recorded_at: i64, payload: MemoryEvent) -> NewEvent<MemoryEvent> {
    let event_type = match &payload {
        MemoryEvent::NodeUpserted { .. } => "node_upserted",
        MemoryEvent::FactRecorded { .. } => "fact_recorded",
        MemoryEvent::FactRetracted { .. } => "fact_retracted",
    };
    NewEvent::new(
        EventId::new(id).unwrap(),
        event_type,
        ts(recorded_at),
        ts(recorded_at),
        agent(),
        session(),
        payload,
    )
    .unwrap()
}

pub fn agent() -> AgentId {
    AgentId::new("codex-test").unwrap()
}

pub fn session() -> SessionId {
    SessionId::new("session-test").unwrap()
}

pub fn simple_projection() -> MemoryProjection {
    let nodes = [
        node("task:1", "task", "Task")
            .in_repository("example")
            .on_branch("main"),
        node("file:1", "file", "File")
            .in_repository("example")
            .on_branch("main"),
    ];
    let events = vec![
        event(
            "event:task",
            1,
            MemoryEvent::NodeUpserted {
                node: nodes[0].clone(),
            },
        ),
        event(
            "event:file",
            1,
            MemoryEvent::NodeUpserted {
                node: nodes[1].clone(),
            },
        ),
        event(
            "event:fact",
            2,
            MemoryEvent::FactRecorded {
                fact: fact("fact:1", "task:1", "affects", "file:1", 2, 2),
            },
        ),
    ];
    let stream = StreamId::new("stream:simple").unwrap();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();
    replay(&store.load_all(None, usize::MAX)).unwrap()
}
