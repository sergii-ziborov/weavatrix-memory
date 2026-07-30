use agentic_memory::{CognitiveEvent, Edge as AgenticEdge, MemoryGraph, QueryEngine};
#[path = "support/memory_competitors_fixtures.rs"]
mod fixtures;
#[path = "support/memory_competitors.rs"]
mod support;
use fixtures::{agentic_fixture, weavatrix_fixture};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use support::{agentic_accepts_dangling_edge, env_usize, median, record, report};
use weavatrix_memory::{
    ContextCompiler, ContextRequest, EntityId, MemoryEvent, MemoryProjection, MemoryView,
    ProjectionClock, StoredEvent, Timestamp, replay,
};

struct BuildSamples {
    replay: Vec<Duration>,
    from_parts: Vec<Duration>,
    competitor: Vec<Duration>,
}

struct ContextSamples {
    ours: Vec<Duration>,
    competitor: Vec<Duration>,
    sizes: (usize, usize, usize, usize),
}

fn main() {
    let node_count = env_usize("WEAVATRIX_BENCH_NODES", 10_000);
    let edges_per_node = env_usize("WEAVATRIX_BENCH_EDGES_PER_NODE", 3);
    let weavatrix_events = weavatrix_fixture(node_count, edges_per_node);
    let (agentic_nodes, agentic_edges) = agentic_fixture(node_count, edges_per_node);
    let replayed = replay::<_, MemoryProjection>(&weavatrix_events).unwrap();
    let clock = ProjectionClock::new(
        Timestamp::from_unix_micros(i64::MAX),
        Timestamp::from_unix_micros(i64::MAX),
    );
    let current = replayed.view(clock);
    let mut builds = benchmark_builds(
        node_count,
        edges_per_node,
        &weavatrix_events,
        &current,
        &agentic_nodes,
        &agentic_edges,
    );

    let projection = replay::<_, MemoryProjection>(&weavatrix_events).unwrap();
    let (mut view_samples, mut view_ref_samples) = benchmark_views(&projection, clock);
    let mut contexts =
        benchmark_contexts(node_count, &projection, agentic_nodes, agentic_edges, clock);

    let edge_count = node_count * edges_per_node;
    report(
        "weavatrix_replay_projection",
        node_count,
        edge_count,
        median(&mut builds.replay),
    );
    report(
        "weavatrix_try_from_parts",
        node_count,
        edge_count,
        median(&mut builds.from_parts),
    );
    report(
        "agentic_memory_from_parts",
        node_count,
        edge_count,
        median(&mut builds.competitor),
    );
    report(
        "weavatrix_full_view",
        node_count,
        edge_count,
        median(&mut view_samples),
    );
    report(
        "weavatrix_full_view_ref",
        node_count,
        edge_count,
        median(&mut view_ref_samples),
    );
    report(
        "weavatrix_context_depth2",
        node_count,
        edge_count,
        median(&mut contexts.ours),
    );
    report(
        "agentic_memory_context_depth2",
        node_count,
        edge_count,
        median(&mut contexts.competitor),
    );
    let (our_nodes, our_edges, their_nodes, their_edges) = contexts.sizes;
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

fn benchmark_builds(
    node_count: usize,
    edges_per_node: usize,
    events: &[StoredEvent<MemoryEvent>],
    current: &MemoryView,
    agentic_nodes: &[CognitiveEvent],
    agentic_edges: &[AgenticEdge],
) -> BuildSamples {
    let mut samples = BuildSamples {
        replay: Vec::new(),
        from_parts: Vec::new(),
        competitor: Vec::new(),
    };
    for iteration in 0..11 {
        let started = Instant::now();
        let projection = replay::<_, MemoryProjection>(events).unwrap();
        let replay_elapsed = started.elapsed();

        let started = Instant::now();
        let parts_projection = MemoryProjection::try_from_parts(
            current.nodes.clone(),
            current.facts.clone(),
            Timestamp::from_unix_micros(i64::MAX),
            Some((node_count * (edges_per_node + 1) - 1) as u64),
        )
        .unwrap();
        let parts_elapsed = started.elapsed();

        let started = Instant::now();
        let graph =
            MemoryGraph::from_parts(agentic_nodes.to_vec(), agentic_edges.to_vec(), 0).unwrap();
        let competitor_elapsed = started.elapsed();
        black_box((projection, parts_projection, graph));
        if iteration >= 2 {
            samples.from_parts.push(parts_elapsed);
        }
        record(
            iteration,
            &mut samples.replay,
            replay_elapsed,
            &mut samples.competitor,
            competitor_elapsed,
        );
    }
    samples
}

fn benchmark_views(
    projection: &MemoryProjection,
    clock: ProjectionClock,
) -> (Vec<Duration>, Vec<Duration>) {
    let mut owned = Vec::new();
    let mut borrowed = Vec::new();
    for iteration in 0..11 {
        let started = Instant::now();
        black_box(projection.view(clock));
        if iteration >= 2 {
            owned.push(started.elapsed());
        }
        let started = Instant::now();
        black_box(projection.view_ref(clock));
        if iteration >= 2 {
            borrowed.push(started.elapsed());
        }
    }
    (owned, borrowed)
}

fn benchmark_contexts(
    node_count: usize,
    projection: &MemoryProjection,
    agentic_nodes: Vec<CognitiveEvent>,
    agentic_edges: Vec<AgenticEdge>,
    clock: ProjectionClock,
) -> ContextSamples {
    let agentic_graph = MemoryGraph::from_parts(agentic_nodes, agentic_edges, 0).unwrap();
    let request = ContextRequest::new(
        vec![EntityId::new(format!("node:{}", node_count / 2)).unwrap()],
        clock.valid_at,
        clock.known_at,
        100_000,
    )
    .unwrap()
    .with_max_depth(2);
    let compiler = ContextCompiler::default();
    let query_engine = QueryEngine::new();
    let mut samples = ContextSamples {
        ours: Vec::new(),
        competitor: Vec::new(),
        sizes: (0, 0, 0, 0),
    };
    for iteration in 0..11 {
        let started = Instant::now();
        let ours = compiler.compile(projection, &request).unwrap();
        let ours_elapsed = started.elapsed();
        let started = Instant::now();
        let theirs = query_engine
            .context(&agentic_graph, (node_count / 2) as u64, 2)
            .unwrap();
        let competitor_elapsed = started.elapsed();
        samples.sizes = (
            ours.graph.node_count(),
            ours.graph.edge_count(),
            theirs.nodes.len(),
            theirs.edges.len(),
        );
        black_box((ours, theirs));
        record(
            iteration,
            &mut samples.ours,
            ours_elapsed,
            &mut samples.competitor,
            competitor_elapsed,
        );
    }
    samples
}
