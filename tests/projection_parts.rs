mod common;

use common::{fact, node, ts};
use weavatrix_memory::{FactId, MemoryProjection, ProjectionClock};

#[test]
fn trusted_projection_parts_keep_validation() {
    let nodes = vec![
        node("task:parts", "task", "Task"),
        node("file:parts", "file", "File"),
    ];
    let facts = vec![fact(
        "fact:parts",
        "task:parts",
        "affects",
        "file:parts",
        1,
        2,
    )];
    let projection =
        MemoryProjection::try_from_parts(nodes.clone(), facts, ts(2), Some(10)).unwrap();
    assert_eq!(projection.last_global_position(), Some(10));
    assert_eq!(
        projection
            .view(ProjectionClock::new(ts(2), ts(2)))
            .facts
            .len(),
        1
    );

    let duplicate = vec![nodes[0].clone(), nodes[0].clone()];
    assert!(MemoryProjection::try_from_parts(duplicate, Vec::new(), ts(2), None).is_err());
    let missing = vec![fact(
        "fact:missing-parts",
        "task:parts",
        "affects",
        "missing:parts",
        1,
        2,
    )];
    assert!(MemoryProjection::try_from_parts(nodes, missing, ts(2), None).is_err());
}

#[test]
fn trusted_projection_parts_accept_unsorted_supersession_chain() {
    let nodes = vec![
        node("task:parts", "task", "Task"),
        node("file:parts", "file", "File"),
    ];
    let replacement = fact(
        "fact:replacement",
        "task:parts",
        "affects",
        "file:parts",
        2,
        2,
    )
    .supersedes(FactId::new("fact:prior").unwrap())
    .unwrap();
    let prior = fact("fact:prior", "task:parts", "affects", "file:parts", 1, 1);

    let projection =
        MemoryProjection::try_from_parts(nodes, vec![replacement, prior], ts(2), None).unwrap();

    assert_eq!(
        projection
            .view(ProjectionClock::new(ts(2), ts(2)))
            .facts
            .len(),
        1
    );
}
