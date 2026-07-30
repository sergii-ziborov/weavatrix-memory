use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use weavatrix_memory::Codec;

#[derive(Default)]
pub(crate) struct Samples {
    pub(crate) encode: Vec<Duration>,
    pub(crate) decode: Vec<Duration>,
}

impl Samples {
    fn report(&mut self, name: &str) {
        report(&format!("{name}_encode"), median(&mut self.encode));
        report(&format!("{name}_decode"), median(&mut self.decode));
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BytesCodec;

impl Codec<Vec<u8>> for BytesCodec {
    fn encode(&self, value: &Vec<u8>) -> weavatrix_memory::Result<Vec<u8>> {
        Ok(value.clone())
    }

    fn decode(&self, bytes: &[u8]) -> weavatrix_memory::Result<Vec<u8>> {
        Ok(bytes.to_vec())
    }
}

pub(crate) fn report_codecs(
    node_count: usize,
    edge_count: usize,
    bytes: [&[u8]; 4],
    samples: [(&mut Samples, &str); 4],
) {
    for (sample, name) in samples {
        sample.report(name);
    }
    println!(
        "secure_storage_sizes nodes={node_count} edges={edge_count} compact_bytes={} lz4_bytes={} encrypted_bytes={} secure_bytes={}",
        bytes[0].len(),
        bytes[1].len(),
        bytes[2].len(),
        bytes[3].len()
    );
}

pub(crate) fn sample_codec<C>(
    iteration: usize,
    codec: &C,
    value: &Vec<u8>,
    bytes: &[u8],
    samples: &mut Samples,
) where
    C: Codec<Vec<u8>>,
{
    record(
        iteration,
        &mut samples.encode,
        elapsed(|| black_box(codec.encode(value).unwrap())),
    );
    record(
        iteration,
        &mut samples.decode,
        elapsed(|| black_box(codec.decode(bytes).unwrap())),
    );
}

pub(crate) fn elapsed<T>(operation: impl FnOnce() -> T) -> Duration {
    let started = Instant::now();
    black_box(operation());
    started.elapsed()
}

pub(crate) fn record(iteration: usize, samples: &mut Vec<Duration>, value: Duration) {
    if iteration >= 2 {
        samples.push(value);
    }
}

pub(crate) fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

pub(crate) fn report(name: &str, median: Duration) {
    println!("{name} median_ms={:.3}", median.as_secs_f64() * 1_000.0);
}

pub(crate) fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
