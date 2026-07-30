use crate::{
    error::{MemoryError, Result},
    store::frame::crc32c,
};
use std::{fs::File, io::Read, path::Path};

const HEADER: &[u8; 8] = b"WMEMSN01";
const HEADER_LEN: usize = 20;

pub(super) enum Payload {
    Buffered(Vec<u8>),
    #[cfg(feature = "mmap")]
    Mapped(mmap_guard::FileData),
}

impl AsRef<[u8]> for Payload {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Buffered(bytes) => bytes,
            #[cfg(feature = "mmap")]
            Self::Mapped(bytes) => &bytes[HEADER_LEN..],
        }
    }
}

pub(super) fn encode(bytes: &[u8]) -> Result<Vec<u8>> {
    let length = u64::try_from(bytes.len()).map_err(|_| MemoryError::CapacityOverflow)?;
    let mut frame = Vec::with_capacity(HEADER_LEN + bytes.len());
    frame.extend_from_slice(HEADER);
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&crc32c(bytes).to_le_bytes());
    frame.extend_from_slice(bytes);
    Ok(frame)
}

#[cfg(not(feature = "mmap"))]
pub(super) fn read(path: &Path, max_bytes: usize) -> Result<Payload> {
    read_buffered(path, max_bytes)
}

#[cfg(feature = "mmap")]
pub(super) fn read(path: &Path, max_bytes: usize, mapped: bool) -> Result<Payload> {
    if mapped {
        read_mapped(path, max_bytes)
    } else {
        read_buffered(path, max_bytes)
    }
}

fn read_buffered(path: &Path, max_bytes: usize) -> Result<Payload> {
    let mut file = File::open(path).map_err(|error| io("open snapshot", error))?;
    let mut header = [0_u8; HEADER_LEN];
    file.read_exact(&mut header)
        .map_err(|error| io("read snapshot header", error))?;
    let (length, expected_crc) = validate_header(&header, max_bytes)?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)
        .map_err(|error| io("read snapshot payload", error))?;
    let actual_len = file
        .metadata()
        .map_err(|error| io("read snapshot metadata", error))?
        .len();
    let framed_len =
        u64::try_from(HEADER_LEN + length).map_err(|_| MemoryError::CapacityOverflow)?;
    if actual_len != framed_len || crc32c(&bytes) != expected_crc {
        return Err(corrupt("snapshot length or checksum mismatch"));
    }
    Ok(Payload::Buffered(bytes))
}

#[cfg(feature = "mmap")]
fn read_mapped(path: &Path, max_bytes: usize) -> Result<Payload> {
    let mapped = mmap_guard::map_file(path).map_err(|error| io("map snapshot", error))?;
    if mapped.len() < HEADER_LEN {
        return Err(corrupt("truncated snapshot header"));
    }
    let header = mapped[..HEADER_LEN]
        .try_into()
        .expect("slice length was checked");
    let (length, expected_crc) = validate_header(header, max_bytes)?;
    let framed_len = HEADER_LEN
        .checked_add(length)
        .ok_or(MemoryError::CapacityOverflow)?;
    if mapped.len() != framed_len {
        return Err(corrupt("snapshot length mismatch"));
    }
    if crc32c(&mapped[HEADER_LEN..]) != expected_crc {
        return Err(corrupt("snapshot checksum mismatch"));
    }
    Ok(Payload::Mapped(mapped))
}

fn validate_header(header: &[u8; HEADER_LEN], max_bytes: usize) -> Result<(usize, u32)> {
    if &header[..8] != HEADER {
        return Err(corrupt("unsupported snapshot header"));
    }
    let length = usize::try_from(u64::from_le_bytes(header[8..16].try_into().unwrap()))
        .map_err(|_| corrupt("snapshot length exceeds platform capacity"))?;
    if length > max_bytes {
        return Err(corrupt("snapshot exceeds configured size limit"));
    }
    Ok((
        length,
        u32::from_le_bytes(header[16..20].try_into().unwrap()),
    ))
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
