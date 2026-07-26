use super::SnapshotStore;
use crate::{Codec, Durability, MemoryError, ProjectionSnapshot, Result, store::frame::crc32c};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const SNAPSHOT_HEADER: &[u8; 8] = b"WMEMSN01";
const FRAME_HEADER_LEN: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotOptions {
    pub durability: Durability,
    pub max_snapshot_bytes: usize,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            durability: Durability::SyncData,
            max_snapshot_bytes: 512 * 1024 * 1024,
        }
    }
}

pub struct FileSnapshotStore<P, C> {
    directory: PathBuf,
    prefix: String,
    codec: C,
    options: SnapshotOptions,
    marker: PhantomData<fn() -> P>,
}

impl<P, C> FileSnapshotStore<P, C>
where
    C: Codec<ProjectionSnapshot<P>>,
{
    /// Creates an immutable, generation-named snapshot store.
    ///
    /// # Errors
    ///
    /// Rejects an invalid prefix or inaccessible directory.
    pub fn open(
        directory: impl AsRef<Path>,
        prefix: impl Into<String>,
        codec: C,
        options: SnapshotOptions,
    ) -> Result<Self> {
        let prefix = prefix.into();
        if prefix.is_empty() || prefix.trim() != prefix || prefix.contains(['/', '\\']) {
            return Err(MemoryError::InvalidValue {
                field: "snapshot.prefix",
                reason: "must be a simple non-empty file prefix",
            });
        }
        if options.max_snapshot_bytes == 0 {
            return Err(MemoryError::InvalidValue {
                field: "max_snapshot_bytes",
                reason: "must be greater than zero",
            });
        }
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory).map_err(|error| io("create snapshot directory", error))?;
        Ok(Self {
            directory,
            prefix,
            codec,
            options,
            marker: PhantomData,
        })
    }

    fn final_path(&self, position: u64) -> PathBuf {
        self.directory
            .join(format!("{}-{position:020}.wmsnap", self.prefix))
    }

    fn latest_path(&self) -> Result<Option<(u64, PathBuf)>> {
        let start = format!("{}-", self.prefix);
        let mut latest = None;
        for entry in
            fs::read_dir(&self.directory).map_err(|error| io("read snapshot directory", error))?
        {
            let entry = entry.map_err(|error| io("read snapshot entry", error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(raw) = name
                .strip_prefix(&start)
                .and_then(|value| value.strip_suffix(".wmsnap"))
            else {
                continue;
            };
            let Ok(position) = raw.parse::<u64>() else {
                continue;
            };
            if latest
                .as_ref()
                .is_none_or(|(current, _)| position > *current)
            {
                latest = Some((position, entry.path()));
            }
        }
        Ok(latest)
    }

    fn read_path(&self, path: &Path, expected_position: u64) -> Result<ProjectionSnapshot<P>> {
        let mut file = File::open(path).map_err(|error| io("open snapshot", error))?;
        let mut header = [0_u8; FRAME_HEADER_LEN];
        file.read_exact(&mut header)
            .map_err(|error| io("read snapshot header", error))?;
        if &header[..8] != SNAPSHOT_HEADER {
            return Err(corrupt("unsupported snapshot header"));
        }
        let length = usize::try_from(u64::from_le_bytes(header[8..16].try_into().unwrap()))
            .map_err(|_| corrupt("snapshot length exceeds platform capacity"))?;
        if length > self.options.max_snapshot_bytes {
            return Err(corrupt("snapshot exceeds configured size limit"));
        }
        let expected_crc = u32::from_le_bytes(header[16..20].try_into().unwrap());
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)
            .map_err(|error| io("read snapshot payload", error))?;
        let actual_len = file
            .metadata()
            .map_err(|error| io("read snapshot metadata", error))?
            .len();
        let framed_len =
            u64::try_from(FRAME_HEADER_LEN + length).map_err(|_| MemoryError::CapacityOverflow)?;
        if actual_len != framed_len || crc32c(&bytes) != expected_crc {
            return Err(corrupt("snapshot length or checksum mismatch"));
        }
        let snapshot = self.codec.decode(&bytes)?;
        if snapshot.cursor.global_position != Some(expected_position) {
            return Err(corrupt("snapshot filename and cursor disagree"));
        }
        Ok(snapshot)
    }

    fn encode_frame(&self, snapshot: &ProjectionSnapshot<P>) -> Result<Vec<u8>> {
        let bytes = self.codec.encode(snapshot)?;
        if bytes.len() > self.options.max_snapshot_bytes {
            return Err(MemoryError::InvalidValue {
                field: "snapshot",
                reason: "encoded snapshot exceeds max_snapshot_bytes",
            });
        }
        let length = u64::try_from(bytes.len()).map_err(|_| MemoryError::CapacityOverflow)?;
        let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + bytes.len());
        frame.extend_from_slice(SNAPSHOT_HEADER);
        frame.extend_from_slice(&length.to_le_bytes());
        frame.extend_from_slice(&crc32c(&bytes).to_le_bytes());
        frame.extend_from_slice(&bytes);
        Ok(frame)
    }
}

impl<P, C> SnapshotStore<P> for FileSnapshotStore<P, C>
where
    C: Codec<ProjectionSnapshot<P>>,
{
    fn save(&mut self, snapshot: &ProjectionSnapshot<P>) -> Result<()> {
        let position = snapshot
            .cursor
            .global_position
            .ok_or(MemoryError::InvalidValue {
                field: "snapshot.cursor",
                reason: "cannot persist an empty replay cursor",
            })?;
        let final_path = self.final_path(position);
        let frame = self.encode_frame(snapshot)?;
        if final_path.exists() {
            let existing =
                fs::read(&final_path).map_err(|error| io("read existing snapshot", error))?;
            if existing == frame {
                return Ok(());
            }
            return Err(MemoryError::InvalidValue {
                field: "snapshot",
                reason: "different snapshot already exists at this position",
            });
        }
        let temporary = temporary_path(&final_path);
        let mut guard = TempGuard::new(temporary.clone());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| io("create temporary snapshot", error))?;
        file.write_all(&frame)
            .map_err(|error| io("write snapshot", error))?;
        match self.options.durability {
            Durability::Flush => file.flush().map_err(|error| io("flush snapshot", error))?,
            Durability::SyncData => file
                .sync_data()
                .map_err(|error| io("sync snapshot", error))?,
        }
        drop(file);
        fs::rename(&temporary, &final_path).map_err(|error| io("commit snapshot", error))?;
        guard.committed = true;
        Ok(())
    }

    fn load_latest(&self) -> Result<Option<ProjectionSnapshot<P>>> {
        self.latest_path()?
            .map(|(position, path)| self.read_path(&path, position))
            .transpose()
    }
}

fn temporary_path(final_path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("snapshot");
    final_path.with_file_name(format!(".{name}.tmp-{}-{id}", std::process::id()))
}

struct TempGuard {
    path: PathBuf,
    committed: bool,
}

impl TempGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn corrupt(reason: &str) -> MemoryError {
    MemoryError::CorruptLog {
        offset: 0,
        reason: reason.to_owned(),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn io(operation: &'static str, error: std::io::Error) -> MemoryError {
    MemoryError::Io {
        operation,
        message: error.to_string(),
    }
}
