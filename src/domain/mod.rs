mod event;
mod evidence;
mod fact;
mod node;
mod view;

pub use event::MemoryEvent;
pub use evidence::{Confidence, Evidence};
pub use fact::MemoryFact;
pub use node::MemoryNode;
pub use view::{MemoryView, MemoryViewRef};

use crate::error::{MemoryError, Result};

pub(crate) fn validate_text(field: &'static str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        return Err(MemoryError::InvalidValue {
            field,
            reason: "must be non-empty without surrounding whitespace",
        });
    }
    Ok(())
}

pub(crate) fn validate_optional_text(field: &'static str, value: Option<&str>) -> Result<()> {
    value.map_or(Ok(()), |value| validate_text(field, value))
}
