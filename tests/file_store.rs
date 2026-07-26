#![cfg(feature = "json")]

mod common;

use common::{event, node};
use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use weavatrix_memory::{
    Codec, Durability, EventStore, ExpectedVersion, FileEventStore, FileStoreOptions,
    InMemoryStore, JsonCodec, MemoryError, MemoryEvent, RecoveryPolicy, StreamId,
};

#[test]
fn memory_and_file_stores_share_append_contract() {
    let mut memory = InMemoryStore::default();
    let file = TempLog::new();
    let mut durable = FileEventStore::open(file.path(), JsonCodec, fast_options()).unwrap();

    assert_store_contract(&mut memory);
    assert_store_contract(&mut durable);
}

#[test]
fn reopen_restores_global_and_stream_cursors() {
    let file = TempLog::new();
    let stream = StreamId::new("task:durable").unwrap();
    {
        let mut store = FileEventStore::open(file.path(), JsonCodec, fast_options()).unwrap();
        store
            .append(
                &stream,
                ExpectedVersion::NoStream,
                &[node_event("event:1", "node:1")],
            )
            .unwrap();
        store
            .append(
                &stream,
                ExpectedVersion::Exact(0),
                &[node_event("event:2", "node:2")],
            )
            .unwrap();
    }

    let reopened =
        FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, fast_options()).unwrap();
    assert_eq!(reopened.len(), 2);
    assert_eq!(reopened.stream_version(&stream), Some(1));
    assert_eq!(
        reopened.load_all(Some(0), 10)[0].metadata.global_position,
        1
    );
}

#[test]
fn owned_append_is_durable_after_reopen() {
    let file = TempLog::new();
    let stream = StreamId::new("task:owned").unwrap();
    {
        let mut store = FileEventStore::open(file.path(), JsonCodec, fast_options()).unwrap();
        store
            .append_owned(
                &stream,
                ExpectedVersion::NoStream,
                vec![node_event("event:owned", "node:owned")],
            )
            .unwrap();
    }

    let reopened =
        FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, fast_options()).unwrap();
    assert_eq!(reopened.load_stream(&stream, None).len(), 1);
}

#[test]
fn recovery_truncates_only_an_incomplete_tail() {
    let file = TempLog::new();
    let stream = StreamId::new("task:recovery").unwrap();
    {
        let mut store = FileEventStore::open(file.path(), JsonCodec, fast_options()).unwrap();
        store
            .append(
                &stream,
                ExpectedVersion::NoStream,
                &[node_event("event:1", "node:1")],
            )
            .unwrap();
    }
    let durable_len = fs::metadata(file.path()).unwrap().len();
    OpenOptions::new()
        .append(true)
        .open(file.path())
        .unwrap()
        .write_all(b"partial")
        .unwrap();

    let strict = FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, fast_options())
        .err()
        .unwrap();
    assert!(matches!(strict, MemoryError::CorruptLog { .. }));

    let mut options = fast_options();
    options.recovery = RecoveryPolicy::TruncatePartialTail;
    let mut recovered = FileEventStore::open(file.path(), JsonCodec, options).unwrap();
    assert_eq!(fs::metadata(file.path()).unwrap().len(), durable_len);
    recovered
        .append(
            &stream,
            ExpectedVersion::Exact(0),
            &[node_event("event:2", "node:2")],
        )
        .unwrap();
    drop(recovered);
    assert_eq!(
        FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, fast_options())
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn checksum_corruption_is_never_silently_recovered() {
    let file = TempLog::new();
    {
        let mut store = FileEventStore::open(file.path(), JsonCodec, fast_options()).unwrap();
        store
            .append(
                &StreamId::new("task:checksum").unwrap(),
                ExpectedVersion::NoStream,
                &[node_event("event:1", "node:1")],
            )
            .unwrap();
    }
    let mut raw = OpenOptions::new()
        .read(true)
        .write(true)
        .open(file.path())
        .unwrap();
    raw.seek(SeekFrom::End(-1)).unwrap();
    raw.write_all(&[0xff]).unwrap();
    drop(raw);

    for recovery in [RecoveryPolicy::Strict, RecoveryPolicy::TruncatePartialTail] {
        let mut options = fast_options();
        options.recovery = recovery;
        let error = FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, options)
            .err()
            .unwrap();
        assert!(matches!(error, MemoryError::CorruptLog { .. }));
    }
}

#[test]
fn active_writer_detects_external_file_changes() {
    let file = TempLog::new();
    let mut store = FileEventStore::open(file.path(), JsonCodec, fast_options()).unwrap();
    OpenOptions::new()
        .append(true)
        .open(file.path())
        .unwrap()
        .write_all(b"x")
        .unwrap();

    let error = store
        .append(
            &StreamId::new("task:external").unwrap(),
            ExpectedVersion::NoStream,
            &[node_event("event:1", "node:1")],
        )
        .unwrap_err();
    assert_eq!(error, MemoryError::ExternalModification);
}

#[test]
fn invalid_headers_limits_and_codec_payloads_are_rejected() {
    let file = TempLog::new();
    fs::write(file.path(), b"NOT-A-LOG").unwrap();
    let error = FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, fast_options())
        .err()
        .unwrap();
    assert!(matches!(error, MemoryError::CorruptLog { .. }));

    fs::remove_file(file.path()).unwrap();
    let mut options = fast_options();
    options.max_frame_bytes = 3;
    assert!(FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, options).is_err());

    options.max_frame_bytes = 16;
    let mut store =
        FileEventStore::<MemoryEvent, _>::open(file.path(), JsonCodec, options).unwrap();
    let original_len = fs::metadata(file.path()).unwrap().len();
    assert!(
        store
            .append(
                &StreamId::new("task:limit").unwrap(),
                ExpectedVersion::NoStream,
                &[node_event("event:large", "node:large")],
            )
            .is_err()
    );
    assert_eq!(store.len(), 0);
    assert_eq!(store.path(), file.path());
    store
        .append(
            &StreamId::new("task:empty").unwrap(),
            ExpectedVersion::NoStream,
            &[],
        )
        .unwrap();
    assert_eq!(fs::metadata(file.path()).unwrap().len(), original_len);
    assert!(Codec::<MemoryEvent>::decode(&JsonCodec, b"not-json").is_err());
}

fn assert_store_contract(store: &mut impl EventStore<MemoryEvent>) {
    let stream = StreamId::new("task:contract").unwrap();
    let committed = store
        .append(
            &stream,
            ExpectedVersion::NoStream,
            &[node_event("event:1", "node:1")],
        )
        .unwrap();
    assert_eq!(committed[0].metadata.global_position, 0);
    assert_eq!(committed[0].metadata.stream_version, 0);
    assert_eq!(store.load_stream(&stream, None), committed);
    assert!(
        store
            .append(
                &stream,
                ExpectedVersion::NoStream,
                &[node_event("event:2", "node:2")],
            )
            .is_err()
    );
    assert_eq!(store.len(), 1);
}

fn node_event(id: &str, node_id: &str) -> weavatrix_memory::NewEvent<MemoryEvent> {
    event(
        id,
        1,
        MemoryEvent::NodeUpserted {
            node: node(node_id, "observation", "Observed"),
        },
    )
}

fn fast_options() -> FileStoreOptions {
    FileStoreOptions {
        durability: Durability::Flush,
        ..FileStoreOptions::default()
    }
}

struct TempLog {
    path: PathBuf,
}

impl TempLog {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("weavatrix-memory-{}-{id}.wmem", std::process::id()));
        let _ = fs::remove_file(&path);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempLog {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
