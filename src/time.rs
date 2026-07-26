use serde::{Deserialize, Serialize};

/// Unix timestamp in microseconds.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    #[must_use]
    pub const fn from_unix_micros(value: i64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_unix_micros(self) -> i64 {
        self.0
    }
}
