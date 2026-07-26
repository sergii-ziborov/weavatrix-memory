use core::fmt;

pub type Result<T> = core::result::Result<T, MemoryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemoryError {
    InvalidId {
        kind: &'static str,
        value: String,
    },
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    VersionConflict {
        stream: String,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    DuplicateEvent {
        id: String,
    },
    InvalidReplay {
        reason: String,
    },
    MissingEntity {
        id: String,
    },
    MissingFact {
        id: String,
    },
    ConflictingFact {
        id: String,
    },
    BudgetTooSmall {
        required: usize,
        available: usize,
    },
    CapacityOverflow,
    Graph(String),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { kind, value } => {
                write!(formatter, "invalid {kind} identifier: {value:?}")
            }
            Self::InvalidValue { field, reason } => {
                write!(formatter, "invalid {field}: {reason}")
            }
            Self::VersionConflict {
                stream,
                expected,
                actual,
            } => write!(
                formatter,
                "version conflict for {stream}: expected {expected:?}, actual {actual:?}"
            ),
            Self::DuplicateEvent { id } => write!(formatter, "duplicate event identifier: {id}"),
            Self::InvalidReplay { reason } => write!(formatter, "invalid replay: {reason}"),
            Self::MissingEntity { id } => write!(formatter, "missing memory entity: {id}"),
            Self::MissingFact { id } => write!(formatter, "missing memory fact: {id}"),
            Self::ConflictingFact { id } => write!(formatter, "conflicting memory fact: {id}"),
            Self::BudgetTooSmall {
                required,
                available,
            } => write!(
                formatter,
                "context budget needs {required} tokens but only {available} are available"
            ),
            Self::CapacityOverflow => formatter.write_str("event store capacity exceeded"),
            Self::Graph(reason) => write!(formatter, "graph projection failed: {reason}"),
        }
    }
}

impl std::error::Error for MemoryError {}

impl From<weavatrix_graph::GraphError> for MemoryError {
    fn from(value: weavatrix_graph::GraphError) -> Self {
        Self::Graph(value.to_string())
    }
}
