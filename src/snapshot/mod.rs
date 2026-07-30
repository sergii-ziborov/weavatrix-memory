mod compact;
mod file;
mod in_memory;

pub use compact::CompactSnapshotCodec;
pub use file::{FileSnapshotStore, SnapshotOptions};
pub use in_memory::InMemorySnapshotStore;

use crate::{error::Result, projection::ProjectionSnapshot};

pub trait SnapshotStore<P> {
    /// Saves an immutable projection snapshot.
    ///
    /// # Errors
    ///
    /// Returns persistence, codec, or conflicting-snapshot errors.
    fn save(&mut self, snapshot: &ProjectionSnapshot<P>) -> Result<()>;

    /// Loads the snapshot with the greatest global event position.
    ///
    /// # Errors
    ///
    /// Returns persistence, codec, or corruption errors.
    fn load_latest(&self) -> Result<Option<ProjectionSnapshot<P>>>;
}
