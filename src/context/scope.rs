use super::ContextRequest;
use crate::MemoryNode;

pub(super) fn node_allowed(request: &ContextRequest, node: &MemoryNode) -> bool {
    (request.repositories.is_empty()
        || node
            .repository
            .as_ref()
            .is_some_and(|value| request.repositories.contains(value)))
        && (request.branches.is_empty()
            || node
                .branch
                .as_ref()
                .is_some_and(|value| request.branches.contains(value)))
}

pub(super) fn relation_allowed(request: &ContextRequest, relation: &str) -> bool {
    request.relations.is_empty() || request.relations.contains(relation)
}
