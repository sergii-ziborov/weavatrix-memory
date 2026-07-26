#![cfg(feature = "json")]

mod common;

use common::{event, node, ts};
use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use weavatrix_memory::{
    CatchUpSubscription, Codec, Durability, EventStore, ExpectedVersion, FileSnapshotStore,
    InMemorySnapshotStore, InMemoryStore, JsonCodec, MemoryEvent, MemoryProjection,
    ProjectionClock, ProjectionSnapshot, ReplayCursor, SnapshotOptions, SnapshotStore, StreamId,
    SubscriptionCheckpoint, replay, replay_tracked, resume,
};

#[test]
fn snapshot_resume_matches_complete_replay() {
    let store = event_store(6);
    let all = store.load_all(None, usize::MAX);
    let (projection, cursor) = replay_tracked::<_, MemoryProjection>(&all[..3]).unwrap();
    let snapshot = ProjectionSnapshot { cursor, projection };
    let (resumed, cursor) = resume(snapshot, &all[3..]).unwrap();
    let complete: MemoryProjection = replay(&all).unwrap();
    let clock = ProjectionClock::new(ts(100), ts(100));

    assert_eq!(resumed.view(clock), complete.view(clock));
    assert_eq!(cursor.global_position, Some(5));
}

#[test]
fn resume_rejects_a_gap_after_snapshot() {
    let store = event_store(4);
    let all = store.load_all(None, usize::MAX);
    let (projection, cursor) = replay_tracked::<_, MemoryProjection>(&all[..2]).unwrap();
    let snapshot = ProjectionSnapshot { cursor, projection };

    assert!(resume(snapshot, &all[3..]).is_err());
}

#[test]
fn in_memory_snapshots_are_immutable_and_latest_wins() {
    let store = event_store(4);
    let all = store.load_all(None, usize::MAX);
    let (first, first_cursor) = replay_tracked::<_, MemoryProjection>(&all[..2]).unwrap();
    let (second, second_cursor) = replay_tracked::<_, MemoryProjection>(&all).unwrap();
    let first = ProjectionSnapshot {
        cursor: first_cursor,
        projection: first,
    };
    let second = ProjectionSnapshot {
        cursor: second_cursor,
        projection: second,
    };
    let mut snapshots = InMemorySnapshotStore::default();

    snapshots.save(&first).unwrap();
    snapshots.save(&first).unwrap();
    snapshots.save(&second).unwrap();
    assert_eq!(snapshots.load_latest().unwrap(), Some(second));

    let conflict = ProjectionSnapshot {
        cursor: first.cursor.clone(),
        projection: MemoryProjection::default(),
    };
    assert!(snapshots.save(&conflict).is_err());
}

#[test]
fn file_snapshots_round_trip_and_detect_corruption() {
    let directory = TempDirectory::new();
    let store = event_store(4);
    let all = store.load_all(None, usize::MAX);
    let (first_projection, first_cursor) =
        replay_tracked::<_, MemoryProjection>(&all[..2]).unwrap();
    let first = ProjectionSnapshot {
        cursor: first_cursor,
        projection: first_projection,
    };
    let (projection, cursor) = replay_tracked::<_, MemoryProjection>(&all).unwrap();
    let snapshot = ProjectionSnapshot { cursor, projection };
    let mut snapshots = FileSnapshotStore::open(
        directory.path(),
        "context",
        JsonCodec,
        SnapshotOptions {
            durability: Durability::Flush,
            ..SnapshotOptions::default()
        },
    )
    .unwrap();

    snapshots.save(&first).unwrap();
    snapshots.save(&snapshot).unwrap();
    snapshots.save(&snapshot).unwrap();
    assert_eq!(snapshots.load_latest().unwrap(), Some(snapshot));

    let path = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|value| value == "wmsnap"))
        .max()
        .unwrap();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0xff]).unwrap();
    assert!(snapshots.load_latest().is_err());
}

#[test]
fn file_snapshot_configuration_and_conflicts_are_checked() {
    let directory = TempDirectory::new();
    let invalid_prefix = FileSnapshotStore::<MemoryProjection, _>::open(
        directory.path(),
        " bad/name ",
        JsonCodec,
        SnapshotOptions::default(),
    )
    .err()
    .unwrap();
    assert!(matches!(
        invalid_prefix,
        weavatrix_memory::MemoryError::InvalidValue { .. }
    ));

    let options = SnapshotOptions {
        max_snapshot_bytes: 0,
        ..SnapshotOptions::default()
    };
    assert!(
        FileSnapshotStore::<MemoryProjection, _>::open(
            directory.path(),
            "context",
            JsonCodec,
            options,
        )
        .is_err()
    );

    let mut snapshots = FileSnapshotStore::open(
        directory.path(),
        "context",
        JsonCodec,
        SnapshotOptions {
            durability: Durability::Flush,
            ..SnapshotOptions::default()
        },
    )
    .unwrap();
    let empty = ProjectionSnapshot {
        cursor: ReplayCursor::default(),
        projection: MemoryProjection::default(),
    };
    assert!(snapshots.save(&empty).is_err());
    assert!(Codec::<ProjectionSnapshot<MemoryProjection>>::decode(&JsonCodec, b"{").is_err());
}

#[test]
fn catch_up_subscription_redelivers_until_acknowledged() {
    let store = event_store(5);
    let mut subscription = CatchUpSubscription::new(SubscriptionCheckpoint::default(), 2).unwrap();

    let first = subscription.poll(&store);
    assert_eq!(first.len(), 2);
    assert_eq!(subscription.poll(&store), first);
    assert!(subscription.acknowledge(9).is_err());
    subscription.acknowledge(0).unwrap();
    assert_eq!(subscription.checkpoint().global_position, Some(0));

    let next = subscription.poll(&store);
    assert_eq!(next[0].metadata.global_position, 1);
    subscription.acknowledge(2).unwrap();
    assert_eq!(subscription.poll(&store)[0].metadata.global_position, 3);
    assert!(CatchUpSubscription::new(SubscriptionCheckpoint::default(), 0).is_err());
}

fn event_store(count: usize) -> InMemoryStore<MemoryEvent> {
    let stream = StreamId::new("stream:snapshot").unwrap();
    let events = (0..count)
        .map(|index| {
            event(
                &format!("event:{index}"),
                i64::try_from(index).unwrap(),
                MemoryEvent::NodeUpserted {
                    node: node(&format!("observation:{index}"), "observation", "Observed"),
                },
            )
        })
        .collect::<Vec<_>>();
    let mut store = InMemoryStore::default();
    store
        .append(&stream, ExpectedVersion::NoStream, &events)
        .unwrap();
    store
}

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weavatrix-memory-snapshots-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
