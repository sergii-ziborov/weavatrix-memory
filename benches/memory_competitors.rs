use agentic_memory::{
    CognitiveEvent, CognitiveEventBuilder, Edge as AgenticEdge, EdgeType, EventType, MemoryGraph,
    QueryEngine,
};
#[path = "memory_competitors/support.rs"]
mod support;
use std::{hint::black_box, time::Instant};
use support::{env_usize, median, record, report};
use weavatrix_memory::{
    AgentId, ContextCompiler, ContextRequest, EntityId, EventId, EventStore, Evidence,
    ExpectedVersion, FactId, InMemoryStore, MemoryEvent, MemoryFact, MemoryNode, MemoryProjection,
    NewEvent, SessionId, StoredEvent, StreamId, Timestamp, replay,
};

#[allow(clippy::too_many_lines)]
fn main() {
    let node_count = env_usize("WEAVATRIX_BENCH_NODES", 10_000);
    let edges_per_node = env_usize("WEAVATRIX_BENCH_EDGES_PER_NODE", 3);
    let weavatrix_events = weavatrix_fixture(node_count, edges_per_node);
    let (agentic_nodes, agentic_edges) = agentic_fixture(node_count, edges_per_node);
    let replayed = replay::<_, MemoryProjection>(&weavatrix_events).unwrap();
    let current = replayed.view(weavatrix_memory::ProjectionClock::new(
        Timestamp::from_unix_micros(i64::MAX),
        Timestamp::from_unix_micros(i64::MAX),
    ));

    let mut weavatrix_build = Vec::new();
    let mut weavatrix_parts_build = Vec::new();
    let mut agentic_build = Vec::new();
    for iteration in 0..11 {
        let started = Instant::now();
        let projection = replay::<_, MemoryProjection>(&weavatrix_events).unwrap();
        let weavatrix_elapsed = started.elapsed();

        let nodes = current.nodes.clone();
        let facts = current.facts.clone();
        let started = Instant::now();
        let parts_projection = MemoryProjection::try_from_parts(
            nodes,
            facts,
            Timestamp::from_unix_micros(i64::MAX),
            Some((node_count * (edges_per_node + 1) - 1) as u64),
        )
        .unwrap();
        let weavatrix_parts_elapsed = started.elapsed();

        let nodes = agentic_nodes.clone();
        let edges = agentic_edges.clone();
        let started = Instant::now();
        let graph = MemoryGraph::from_parts(nodes, edges, 0).unwrap();
        let agentic_elapsed = started.elapsed();
        black_box((projection, parts_projection, graph));
        if iteration >= 2 {
            weavatrix_parts_build.push(weavatrix_parts_elapsed);
        }
        record(
            iteration,
            &mut weavatrix_build,
            weavatrix_elapsed,
            &mut agentic_build,
            agentic_elapsed,
        );
    }

    let projection = replay::<_, MemoryProjection>(&weavatrix_events).unwrap();
    let agentic_graph = MemoryGraph::from_parts(agentic_nodes, agentic_edges, 0).unwrap();
    let request = ContextRequest::new(
        vec![EntityId::new(format!("node:{}", node_count / 2)).unwrap()],
        Timestamp::from_unix_micros(i64::MAX),
        Timestamp::from_unix_micros(i64::MAX),
        100_000,
    )
    .unwrap()
    .with_max_depth(2);
    let compiler = ContextCompiler::default();
    let query_engine = QueryEngine::new();
    let mut weavatrix_context = Vec::new();
    let mut agentic_context = Vec::new();
    let mut result_sizes = None;
    for iteration in 0..11 {
        let started = Instant::now();
        let ours = compiler.compile(&projection, &request).unwrap();
        let weavatrix_elapsed = started.elapsed();

        let started = Instant::now();
        let theirs = query_engine
            .context(&agentic_graph, (node_count / 2) as u64, 2)
            .unwrap();
        let agentic_elapsed = started.elapsed();
        result_sizes = Some((
            ours.graph.node_count(),
            ours.graph.edge_count(),
            theirs.nodes.len(),
            theirs.edges.len(),
        ));
        black_box((ours, theirs));
        record(
            iteration,
            &mut weavatrix_context,
            weavatrix_elapsed,
            &mut agentic_context,
            agentic_elapsed,
        );
    }

    let edge_count = node_count * edges_per_node;
    report(
        "weavatrix_replay_projection",
        node_count,
        edge_count,
        median(&mut weavatrix_build),
    );
    report(
        "weavatrix_try_from_parts",
        node_count,
        edge_count,
        median(&mut weavatrix_parts_build),
    );
    report(
        "agentic_memory_from_parts",
        node_count,
        edge_count,
        median(&mut agentic_build),
    );
    report(
        "weavatrix_context_depth2",
        node_count,
        edge_count,
        median(&mut weavatrix_context),
    );
    report(
        "agentic_memory_context_depth2",
        node_count,
        edge_count,
        median(&mut agentic_context),
    );
    let (our_nodes, our_edges, their_nodes, their_edges) = result_sizes.unwrap();
    assert_eq!(
        (our_nodes, our_edges),
        (their_nodes, their_edges),
        "context outputs must contain the same topology"
    );
    println!(
        "context_output weavatrix={our_nodes}/{our_edges} agentic_memory={their_nodes}/{their_edges}"
    );
    println!(
        "contract_probe agentic_memory_accepts_dangling_edge={}",
        agentic_accepts_dangling_edge()
    );
}

fn weavatrix_fixture(node_count: usize, edges_per_node: usize) -> Vec<StoredEvent<MemoryEvent>> {
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

fn agentic_fixture(
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

fn agentic_accepts_dangling_edge() -> bool {
    let mut node = CognitiveEventBuilder::new(EventType::Fact, "only node")
        .created_at(1)
        .build();
    node.feature_vec.clear();
    MemoryGraph::from_parts(
        vec![node],
        vec![AgenticEdge::with_timestamp(
            0,
            1,
            EdgeType::Supports,
            1.0,
            1,
        )],
        0,
    )
    .is_ok()
}
