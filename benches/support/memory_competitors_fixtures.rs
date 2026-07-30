use agentic_memory::{
    CognitiveEvent, CognitiveEventBuilder, Edge as AgenticEdge, EdgeType, EventType,
};
use weavatrix_memory::{
    AgentId, EntityId, EventId, EventStore, Evidence, ExpectedVersion, FactId, InMemoryStore,
    MemoryEvent, MemoryFact, MemoryNode, NewEvent, SessionId, StoredEvent, StreamId, Timestamp,
};

pub(crate) fn weavatrix_fixture(
    node_count: usize,
    edges_per_node: usize,
) -> Vec<StoredEvent<MemoryEvent>> {
    let agent = AgentId::new("bench-agent").unwrap();
    let session = SessionId::new("bench-session").unwrap();
    let evidence = Evidence::new("benchmark", "memory-competitors").unwrap();
    let mut pending = Vec::with_capacity(node_count * (edges_per_node + 1));
    for index in 0..node_count {
        pending.push(new_event(
            pending.len(),
            &agent,
            &session,
            MemoryEvent::NodeUpserted {
                node: MemoryNode::new(
                    EntityId::new(format!("node:{index}")).unwrap(),
                    "observation",
                    format!("benchmark node {index}"),
                )
                .unwrap(),
            },
        ));
    }
    for source in 0..node_count {
        for offset in 1..=edges_per_node {
            let target = (source + offset) % node_count;
            let fact = MemoryFact::new(
                FactId::new(format!("fact:{source}:{target}")).unwrap(),
                EntityId::new(format!("node:{source}")).unwrap(),
                "supports",
                EntityId::new(format!("node:{target}")).unwrap(),
                Timestamp::from_unix_micros(1),
                Timestamp::from_unix_micros(1),
                agent.clone(),
                session.clone(),
                evidence.clone(),
            )
            .unwrap();
            pending.push(new_event(
                pending.len(),
                &agent,
                &session,
                MemoryEvent::FactRecorded { fact },
            ));
        }
    }
    let mut store = InMemoryStore::default();
    store
        .append_owned(
            &StreamId::new("benchmark").unwrap(),
            ExpectedVersion::NoStream,
            pending,
        )
        .unwrap();
    store.load_all(None, usize::MAX)
}

fn new_event(
    index: usize,
    agent: &AgentId,
    session: &SessionId,
    payload: MemoryEvent,
) -> NewEvent<MemoryEvent> {
    NewEvent::new(
        EventId::new(format!("event:{index}")).unwrap(),
        payload.event_type(),
        Timestamp::from_unix_micros(1),
        Timestamp::from_unix_micros(1),
        agent.clone(),
        session.clone(),
        payload,
    )
    .unwrap()
}

pub(crate) fn agentic_fixture(
    node_count: usize,
    edges_per_node: usize,
) -> (Vec<CognitiveEvent>, Vec<AgenticEdge>) {
    let nodes = (0..node_count)
        .map(|index| {
            let mut event =
                CognitiveEventBuilder::new(EventType::Fact, format!("benchmark node {index}"))
                    .session_id(1)
                    .created_at(1)
                    .build();
            event.id = index as u64;
            event.feature_vec.clear();
            event
        })
        .collect();
    let mut edges = Vec::with_capacity(node_count * edges_per_node);
    for source in 0..node_count {
        for offset in 1..=edges_per_node {
            let target = (source + offset) % node_count;
            edges.push(AgenticEdge::with_timestamp(
                source as u64,
                target as u64,
                EdgeType::Supports,
                1.0,
                1,
            ));
        }
    }
    (nodes, edges)
}
