use cqrs_es::{
    Aggregate, DomainEvent, EventStore as CqrsEventStore, event_sink::EventSink,
    mem_store::MemStore,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    convert::Infallible,
    hint::black_box,
    time::{Duration, Instant},
};
use weavatrix_memory::{
    AgentId, EventId, EventStore, ExpectedVersion, InMemoryStore, NewEvent, SessionId, StreamId,
    Timestamp,
};

fn main() {
    let count = std::env::var("WEAVATRIX_BENCH_EVENTS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let weavatrix_events = weavatrix_fixture(count);
    let cqrs_events = cqrs_fixture(count);
    let mut weavatrix_samples = Vec::new();
    let mut owned_append_samples = Vec::new();
    let mut receipt_append_samples = Vec::new();
    let mut cqrs_samples = Vec::new();
    for iteration in 0..11 {
        let mut store = InMemoryStore::default();
        let stream = StreamId::new("benchmark").unwrap();
        let events = weavatrix_events.clone();
        let started = Instant::now();
        store
            .append_owned(&stream, ExpectedVersion::NoStream, events)
            .unwrap();
        black_box(store.load_stream(&stream, None));
        let weavatrix_elapsed = started.elapsed();

        let mut store = InMemoryStore::default();
        let events = weavatrix_events.clone();
        let started = Instant::now();
        black_box(
            store
                .append_owned(&stream, ExpectedVersion::NoStream, events)
                .unwrap(),
        );
        let owned_append_elapsed = started.elapsed();

        let mut store = InMemoryStore::default();
        let events = weavatrix_events.clone();
        let started = Instant::now();
        black_box(
            store
                .append_owned_receipt(&stream, ExpectedVersion::NoStream, events)
                .unwrap(),
        );
        let receipt_append_elapsed = started.elapsed();

        let store = MemStore::<BenchAggregate>::default();
        let events = cqrs_events.clone();
        let started = Instant::now();
        runtime.block_on(async {
            let context = store.load_aggregate("benchmark").await.unwrap();
            store.commit(events, context, HashMap::new()).await.unwrap();
            black_box(store.load_events("benchmark").await.unwrap());
        });
        let cqrs_elapsed = started.elapsed();
        if iteration >= 2 {
            weavatrix_samples.push(weavatrix_elapsed);
            owned_append_samples.push(owned_append_elapsed);
            receipt_append_samples.push(receipt_append_elapsed);
            cqrs_samples.push(cqrs_elapsed);
        }
    }
    report(
        "weavatrix_evidence_append_load",
        count,
        median(&mut weavatrix_samples),
    );
    report(
        "weavatrix_owned_append",
        count,
        median(&mut owned_append_samples),
    );
    report(
        "weavatrix_receipt_append",
        count,
        median(&mut receipt_append_samples),
    );
    report(
        "cqrs_es_evidence_append_load",
        count,
        median(&mut cqrs_samples),
    );
}

fn weavatrix_fixture(count: usize) -> Vec<NewEvent<BenchPayload>> {
    let agent = AgentId::new("bench-agent").unwrap();
    let session = SessionId::new("bench-session").unwrap();
    (0..count)
        .map(|index| {
            let timestamp = i64::try_from(index).expect("benchmark count fits i64");
            NewEvent::new(
                EventId::new(format!("event:{index}")).unwrap(),
                "node_upserted",
                Timestamp::from_unix_micros(timestamp),
                Timestamp::from_unix_micros(timestamp),
                agent.clone(),
                session.clone(),
                BenchPayload::Observed,
            )
            .unwrap()
        })
        .collect()
}

fn cqrs_fixture(count: usize) -> Vec<BenchEvent> {
    (0..count)
        .map(|index| BenchEvent {
            id: format!("event:{index}"),
            event_type: "node_upserted".to_owned(),
            occurred_at: i64::try_from(index).expect("benchmark count fits i64"),
            recorded_at: i64::try_from(index).expect("benchmark count fits i64"),
            agent_id: "bench-agent".to_owned(),
            session_id: "bench-session".to_owned(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
enum BenchPayload {
    Observed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BenchAggregate {
    observed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BenchEvent {
    id: String,
    event_type: String,
    occurred_at: i64,
    recorded_at: i64,
    agent_id: String,
    session_id: String,
}

impl DomainEvent for BenchEvent {
    fn event_type(&self) -> String {
        self.event_type.clone()
    }

    fn event_version(&self) -> String {
        "1".to_owned()
    }
}

impl Aggregate for BenchAggregate {
    const TYPE: &'static str = "benchmark";
    type Command = ();
    type Event = BenchEvent;
    type Error = Infallible;
    type Services = ();

    async fn handle(
        &mut self,
        _command: Self::Command,
        _service: &Self::Services,
        _sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        black_box(event);
        self.observed += 1;
    }
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn report(name: &str, count: usize, median: Duration) {
    let rate =
        u128::try_from(count).expect("usize fits u128") * 1_000_000_000 / median.as_nanos().max(1);
    println!(
        "{name} events={count} median_ms={:.3} events_per_second={rate}",
        median.as_secs_f64() * 1_000.0
    );
}
