use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use weavatrix_memory::{
    AgentId, AutoExtractionEngine, Confidence, EntityLinker, ExtractedEntity, ExtractionError,
    ExtractionInput, ExtractionOutput, ExtractionProvider, LinkPolicy, MemoryNode, MemoryView,
    SessionId, Timestamp,
};

#[derive(Clone)]
struct FixtureProvider {
    output: ExtractionOutput,
}

impl ExtractionProvider for FixtureProvider {
    fn name(&self) -> &'static str {
        "benchmark"
    }

    fn extract(&self, _input: &ExtractionInput) -> Result<ExtractionOutput, ExtractionError> {
        Ok(self.output.clone())
    }
}

fn main() {
    let node_count = env_usize("WEAVATRIX_BENCH_NODES", 100_000).max(1);
    let mention_count = env_usize("WEAVATRIX_BENCH_MENTIONS", 10_000).min(node_count);
    let view = fixture_view(node_count);
    let mentions = fixture_mentions(node_count, mention_count);
    let provider = FixtureProvider {
        output: ExtractionOutput {
            entities: mentions.clone(),
            relations: Vec::new(),
        },
    };
    let input = ExtractionInput::new(
        "benchmark",
        "entity linking benchmark",
        Timestamp::from_unix_micros(1),
        Timestamp::from_unix_micros(1),
        AgentId::new("benchmark").unwrap(),
        SessionId::new("benchmark").unwrap(),
    )
    .unwrap();

    let catalog = samples(|| EntityLinker::from_view(black_box(&view)).unwrap());
    let linker = EntityLinker::from_view(&view).unwrap();
    let linking = samples(|| {
        for mention in &mentions {
            black_box(
                linker
                    .link(black_box(mention), &input, LinkPolicy::default())
                    .unwrap(),
            );
        }
    });
    let engine = AutoExtractionEngine::default();
    let planning = samples(|| {
        let plan = engine.plan_with_linker(&provider, &input, &linker).unwrap();
        assert_eq!(plan.links.len(), mention_count);
        assert!(plan.events.is_empty());
        black_box(plan);
    });

    report("catalog_build", node_count, median(catalog));
    report("indexed_link", mention_count, median(linking));
    report("validated_plan", mention_count, median(planning));
}

fn fixture_view(node_count: usize) -> MemoryView {
    let nodes = (0..node_count)
        .map(|index| {
            MemoryNode::new(
                weavatrix_memory::EntityId::new(format!("symbol:{index}")).unwrap(),
                "function",
                format!("Symbol {index}"),
            )
            .unwrap()
            .with_attribute("alias.0", format!("fn_{index}"))
            .with_attribute("external_id.symbol", format!("ext-{index}"))
        })
        .collect();
    MemoryView {
        nodes,
        facts: Vec::new(),
    }
}

fn fixture_mentions(node_count: usize, mention_count: usize) -> Vec<ExtractedEntity> {
    let stride = (node_count / mention_count.max(1)).max(1);
    (0..mention_count)
        .map(|mention| {
            let index = (mention * stride).min(node_count - 1);
            ExtractedEntity::new(
                format!("mention:{mention}"),
                "function",
                format!("symbol-{index}"),
                Confidence::CERTAIN,
            )
            .unwrap()
        })
        .collect()
}

fn samples<T>(mut operation: impl FnMut() -> T) -> Vec<Duration> {
    let mut samples = Vec::with_capacity(9);
    for iteration in 0..11 {
        let started = Instant::now();
        black_box(operation());
        if iteration >= 2 {
            samples.push(started.elapsed());
        }
    }
    samples
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn report(contract: &str, items: usize, elapsed: Duration) {
    let throughput =
        u128::try_from(items).expect("usize fits u128") * 1_000_000_000 / elapsed.as_nanos();
    println!(
        "{contract} items={items} median_ms={:.3} throughput_per_sec={throughput}",
        elapsed.as_secs_f64() * 1_000.0,
    );
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
