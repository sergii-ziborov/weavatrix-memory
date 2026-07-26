use std::time::Duration;

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
