use super::SnapshotStore;
use crate::{
    error::{MemoryError, Result},
    projection::ProjectionSnapshot,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct InMemorySnapshotStore<P> {
    snapshots: BTreeMap<u64, ProjectionSnapshot<P>>,
}

impl<P> SnapshotStore<P> for InMemorySnapshotStore<P>
where
    P: Clone + PartialEq,
{
    fn save(&mut self, snapshot: &ProjectionSnapshot<P>) -> Result<()> {
        let position = snapshot
            .cursor
            .global_position
            .ok_or(MemoryError::InvalidValue {
                field: "snapshot.cursor",
                reason: "cannot persist an empty replay cursor",
            })?;
        if let Some(existing) = self.snapshots.get(&position) {
            if existing == snapshot {
                return Ok(());
            }
            return Err(MemoryError::InvalidValue {
                field: "snapshot",
                reason: "different snapshot already exists at this position",
            });
        }
        self.snapshots.insert(position, snapshot.clone());
        Ok(())
    }

    fn load_latest(&self) -> Result<Option<ProjectionSnapshot<P>>> {
        Ok(self
            .snapshots
            .last_key_value()
            .map(|(_, snapshot)| snapshot.clone()))
    }
}
