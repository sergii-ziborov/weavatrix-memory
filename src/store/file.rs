use super::{
    EventStore, ExpectedVersion, InMemoryStore,
    frame::{self, ScanOutcome},
};
use crate::{Codec, MemoryError, NewEvent, Result, StoredEvent, StreamId};
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPolicy {
    Strict,
    TruncatePartialTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    Flush,
    SyncData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStoreOptions {
    pub recovery: RecoveryPolicy,
    pub durability: Durability,
    pub max_frame_bytes: usize,
}

impl Default for FileStoreOptions {
    fn default() -> Self {
        Self {
            recovery: RecoveryPolicy::Strict,
            durability: Durability::SyncData,
            max_frame_bytes: 256 * 1024 * 1024,
        }
    }
}

pub struct FileEventStore<E, C> {
    path: PathBuf,
    file: File,
    codec: C,
    options: FileStoreOptions,
    inner: InMemoryStore<E>,
    durable_len: u64,
}

impl<E, C> FileEventStore<E, C>
where
    E: Clone,
    C: Codec<StoredEvent<E>>,
{
    /// Opens or creates a framed append-only event journal.
    ///
    /// # Errors
    ///
    /// Rejects invalid headers, checksum corruption, invalid restored event
    /// sequences, and partial tails in strict mode.
    pub fn open(path: impl AsRef<Path>, codec: C, options: FileStoreOptions) -> Result<Self> {
        if options.max_frame_bytes < 4 {
            return Err(MemoryError::InvalidValue {
                field: "max_frame_bytes",
                reason: "must fit at least the event-count field",
            });
        }
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| io("open event log", error))?;
        if file
            .metadata()
            .map_err(|error| io("read event log metadata", error))?
            .len()
            == 0
        {
            file.write_all(frame::FILE_HEADER)
                .map_err(|error| io("write event log header", error))?;
            sync(&file, options.durability)?;
        }
        let outcome = frame::scan(&mut file, &codec, options.max_frame_bytes)?;
        let (events, durable_len, partial) = match outcome {
            ScanOutcome::Complete {
                events,
                durable_len,
            } => (events, durable_len, false),
            ScanOutcome::PartialTail {
                events,
                durable_len,
            } => (events, durable_len, true),
        };
        if partial && options.recovery == RecoveryPolicy::Strict {
            return Err(MemoryError::CorruptLog {
                offset: durable_len,
                reason: "partial trailing batch".to_owned(),
            });
        }
        if partial {
            file.set_len(durable_len)
                .map_err(|error| io("truncate partial event batch", error))?;
            sync(&file, options.durability)?;
        }
        file.seek(SeekFrom::End(0))
            .map_err(|error| io("seek event log end", error))?;
        Ok(Self {
            path,
            file,
            codec,
            options,
            inner: InMemoryStore::restore(events)?,
            durable_len,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn persist(&mut self, events: &[StoredEvent<E>]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let actual_len = self
            .file
            .metadata()
            .map_err(|error| io("read event log metadata", error))?
            .len();
        if actual_len != self.durable_len {
            return Err(MemoryError::ExternalModification);
        }
        let frame = frame::encode_batch(events, &self.codec, self.options.max_frame_bytes)?;
        self.file
            .seek(SeekFrom::Start(self.durable_len))
            .map_err(|error| io("seek append position", error))?;
        if let Err(error) = self.file.write_all(&frame) {
            self.rollback_tail()?;
            return Err(io("append event batch", error));
        }
        if let Err(error) = sync(&self.file, self.options.durability) {
            self.rollback_tail()?;
            return Err(error);
        }
        self.durable_len = self
            .durable_len
            .checked_add(u64::try_from(frame.len()).map_err(|_| MemoryError::CapacityOverflow)?)
            .ok_or(MemoryError::CapacityOverflow)?;
        Ok(())
    }

    fn rollback_tail(&mut self) -> Result<()> {
        self.file
            .set_len(self.durable_len)
            .map_err(|error| io("rollback partial event batch", error))?;
        self.file
            .seek(SeekFrom::Start(self.durable_len))
            .map_err(|error| io("seek after rollback", error))?;
        sync(&self.file, self.options.durability)
    }
}

impl<E, C> EventStore<E> for FileEventStore<E, C>
where
    E: Clone,
    C: Codec<StoredEvent<E>>,
{
    fn append(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: &[NewEvent<E>],
    ) -> Result<Vec<StoredEvent<E>>> {
        let committed = self.inner.prepare_append(stream, expected, events)?;
        self.persist(&committed)?;
        self.inner.commit_prepared(&committed);
        Ok(committed)
    }

    fn append_owned(
        &mut self,
        stream: &StreamId,
        expected: ExpectedVersion,
        events: Vec<NewEvent<E>>,
    ) -> Result<Vec<StoredEvent<E>>> {
        let committed = self.inner.prepare_append_owned(stream, expected, events)?;
        self.persist(&committed)?;
        self.inner.commit_prepared(&committed);
        Ok(committed)
    }

    fn load_stream(&self, stream: &StreamId, after: Option<u64>) -> Vec<StoredEvent<E>> {
        self.inner.load_stream(stream, after)
    }

    fn load_all(&self, after: Option<u64>, limit: usize) -> Vec<StoredEvent<E>> {
        self.inner.load_all(after, limit)
    }

    fn stream_version(&self, stream: &StreamId) -> Option<u64> {
        self.inner.stream_version(stream)
    }

    fn len(&self) -> usize {
        self.inner.len()
    }
}

fn sync(file: &File, durability: Durability) -> Result<()> {
    match durability {
        Durability::Flush => Ok(()),
        Durability::SyncData => file
            .sync_data()
            .map_err(|error| io("sync event log", error)),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn io(operation: &'static str, error: std::io::Error) -> MemoryError {
    MemoryError::Io {
        operation,
        message: error.to_string(),
    }
}
