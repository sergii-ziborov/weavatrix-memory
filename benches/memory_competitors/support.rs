use std::time::Duration;

pub(crate) fn agentic_accepts_dangling_edge() -> bool {
    use agentic_memory::{
        CognitiveEventBuilder, Edge as AgenticEdge, EdgeType, EventType, MemoryGraph,
    };

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

pub(crate) fn record(
    iteration: usize,
    left: &mut Vec<Duration>,
    left_elapsed: Duration,
    right: &mut Vec<Duration>,
    right_elapsed: Duration,
) {
    if iteration >= 2 {
        left.push(left_elapsed);
        right.push(right_elapsed);
    }
}

pub(crate) fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

pub(crate) fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

pub(crate) fn report(name: &str, nodes: usize, edges: usize, median: Duration) {
    println!(
        "{name} nodes={nodes} edges={edges} median_ms={:.3}",
        median.as_secs_f64() * 1_000.0
    );
}
