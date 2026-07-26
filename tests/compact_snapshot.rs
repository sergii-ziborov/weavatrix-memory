mod common;

use common::simple_projection;
use std::collections::BTreeMap;
use weavatrix_memory::{Codec, CompactSnapshotCodec, ProjectionSnapshot, ReplayCursor, StreamId};

fn snapshot() -> ProjectionSnapshot<weavatrix_memory::MemoryProjection> {
    ProjectionSnapshot {
        cursor: ReplayCursor {
            global_position: Some(2),
            stream_versions: BTreeMap::from([(StreamId::new("stream:simple").unwrap(), 2)]),
        },
        projection: simple_projection(),
    }
}

#[test]
fn compact_snapshot_is_deterministic_and_rebuilds_indexes() {
    let snapshot = snapshot();
    let first = CompactSnapshotCodec.encode(&snapshot).unwrap();
    let second = CompactSnapshotCodec.encode(&snapshot).unwrap();
    let decoded = CompactSnapshotCodec.decode(&first).unwrap();
    let rebuilt = CompactSnapshotCodec.encode(&decoded).unwrap();

    assert_eq!(first, second);
    assert_eq!(first, rebuilt);
    assert_eq!(decoded, snapshot);
    assert_eq!(
        decoded
            .projection
            .view(weavatrix_memory::ProjectionClock::new(
                common::ts(10),
                common::ts(10)
            ))
            .facts
            .len(),
        1
    );
}

#[test]
fn compact_snapshot_rejects_wrong_bounds_and_positions() {
    let snapshot = snapshot();
    let bytes = CompactSnapshotCodec.encode(&snapshot).unwrap();

    let mut wrong_header = bytes.clone();
    wrong_header[0] ^= 1;
    assert!(CompactSnapshotCodec.decode(&wrong_header).is_err());

    let mut truncated = bytes.clone();
    truncated.pop();
    assert!(CompactSnapshotCodec.decode(&truncated).is_err());

    let mut trailing = bytes;
    trailing.push(0);
    assert!(CompactSnapshotCodec.decode(&trailing).is_err());

    let mut mismatch = snapshot;
    mismatch.cursor.global_position = Some(99);
    let mismatch = CompactSnapshotCodec.encode(&mismatch).unwrap();
    assert!(CompactSnapshotCodec.decode(&mismatch).is_err());
}

#[cfg(feature = "json")]
#[test]
fn compact_snapshot_is_smaller_than_json_for_the_same_state() {
    let snapshot = snapshot();
    let compact = CompactSnapshotCodec.encode(&snapshot).unwrap();
    let json = weavatrix_memory::JsonCodec.encode(&snapshot).unwrap();

    assert!(
        compact.len() < json.len(),
        "compact={} json={}",
        compact.len(),
        json.len()
    );
}
