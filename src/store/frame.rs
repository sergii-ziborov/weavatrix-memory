use crate::{Codec, MemoryError, Result, StoredEvent};
use std::io::{Read, Seek, SeekFrom};

pub(super) const FILE_HEADER: &[u8; 8] = b"WMEMLOG1";
const BATCH_MAGIC: &[u8; 4] = b"BAT1";
const BATCH_HEADER_LEN: usize = 16;

pub(super) enum ScanOutcome<E> {
    Complete {
        events: Vec<StoredEvent<E>>,
        durable_len: u64,
    },
    PartialTail {
        events: Vec<StoredEvent<E>>,
        durable_len: u64,
    },
}

pub(super) fn encode_batch<E>(
    events: &[StoredEvent<E>],
    codec: &impl Codec<StoredEvent<E>>,
    max_frame_bytes: usize,
) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    let count = u32::try_from(events.len()).map_err(|_| MemoryError::CapacityOverflow)?;
    payload.extend_from_slice(&count.to_le_bytes());
    for event in events {
        let encoded = codec.encode(event)?;
        let length = u64::try_from(encoded.len()).map_err(|_| MemoryError::CapacityOverflow)?;
        payload.extend_from_slice(&length.to_le_bytes());
        payload.extend_from_slice(&encoded);
    }
    if payload.len() > max_frame_bytes {
        return Err(MemoryError::InvalidValue {
            field: "event_batch",
            reason: "encoded batch exceeds max_frame_bytes",
        });
    }
    let length = u64::try_from(payload.len()).map_err(|_| MemoryError::CapacityOverflow)?;
    let mut frame = Vec::with_capacity(BATCH_HEADER_LEN + payload.len());
    frame.extend_from_slice(BATCH_MAGIC);
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&crc32c(&payload).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub(super) fn scan<E>(
    reader: &mut (impl Read + Seek),
    codec: &impl Codec<StoredEvent<E>>,
    max_frame_bytes: usize,
) -> Result<ScanOutcome<E>> {
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io("seek", error))?;
    let mut file_header = [0_u8; 8];
    reader
        .read_exact(&mut file_header)
        .map_err(|error| io("read file header", error))?;
    if &file_header != FILE_HEADER {
        return Err(MemoryError::CorruptLog {
            offset: 0,
            reason: "unsupported file header".to_owned(),
        });
    }
    let mut events = Vec::new();
    let mut offset = u64::try_from(FILE_HEADER.len()).expect("header length fits u64");
    loop {
        let mut header = [0_u8; BATCH_HEADER_LEN];
        match read_or_partial(reader, &mut header)
            .map_err(|error| io("read batch header", error))?
        {
            ReadState::Eof => {
                return Ok(ScanOutcome::Complete {
                    events,
                    durable_len: offset,
                });
            }
            ReadState::Partial => {
                return Ok(ScanOutcome::PartialTail {
                    events,
                    durable_len: offset,
                });
            }
            ReadState::Complete => {}
        }
        if &header[..4] != BATCH_MAGIC {
            return Err(corrupt(offset, "invalid batch marker"));
        }
        let payload_len = usize::try_from(u64::from_le_bytes(header[4..12].try_into().unwrap()))
            .map_err(|_| corrupt(offset, "batch length exceeds platform capacity"))?;
        if payload_len > max_frame_bytes {
            return Err(corrupt(offset, "batch exceeds configured frame limit"));
        }
        let expected_crc = u32::from_le_bytes(header[12..16].try_into().unwrap());
        let mut payload = vec![0; payload_len];
        if !matches!(
            read_or_partial(reader, &mut payload).map_err(|error| io("read batch", error))?,
            ReadState::Complete
        ) {
            return Ok(ScanOutcome::PartialTail {
                events,
                durable_len: offset,
            });
        }
        if crc32c(&payload) != expected_crc {
            return Err(corrupt(offset, "batch checksum mismatch"));
        }
        events.extend(decode_payload(&payload, codec, offset)?);
        offset = offset
            .checked_add(
                u64::try_from(BATCH_HEADER_LEN + payload_len)
                    .map_err(|_| MemoryError::CapacityOverflow)?,
            )
            .ok_or(MemoryError::CapacityOverflow)?;
    }
}

fn decode_payload<E>(
    payload: &[u8],
    codec: &impl Codec<StoredEvent<E>>,
    offset: u64,
) -> Result<Vec<StoredEvent<E>>> {
    let mut cursor = 0;
    let count =
        read_u32(payload, &mut cursor).ok_or_else(|| corrupt(offset, "missing event count"))?;
    let mut events =
        Vec::with_capacity(usize::try_from(count).map_err(|_| MemoryError::CapacityOverflow)?);
    for _ in 0..count {
        let length = read_u64(payload, &mut cursor)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| corrupt(offset, "invalid event length"))?;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= payload.len())
            .ok_or_else(|| corrupt(offset, "truncated event payload"))?;
        events.push(codec.decode(&payload[cursor..end])?);
        cursor = end;
    }
    if cursor != payload.len() {
        return Err(corrupt(offset, "trailing bytes in batch"));
    }
    Ok(events)
}

enum ReadState {
    Complete,
    Partial,
    Eof,
}

fn read_or_partial(reader: &mut impl Read, output: &mut [u8]) -> std::io::Result<ReadState> {
    let mut read = 0;
    while read < output.len() {
        let count = reader.read(&mut output[read..])?;
        if count == 0 {
            return Ok(if read == 0 {
                ReadState::Eof
            } else {
                ReadState::Partial
            });
        }
        read += count;
    }
    Ok(ReadState::Complete)
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    let value = u32::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let end = cursor.checked_add(8)?;
    let value = u64::from_le_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
    *cursor = end;
    Some(value)
}

const CRC32C_TABLE: [u32; 256] = crc32c_table();

pub(crate) fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        let index = usize::from(crc.to_le_bytes()[0] ^ byte);
        crc = (crc >> 8) ^ CRC32C_TABLE[index];
    }
    !crc
}

#[allow(clippy::cast_possible_truncation)]
const fn crc32c_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < table.len() {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0x82f6_3b78 & mask);
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

fn corrupt(offset: u64, reason: &str) -> MemoryError {
    MemoryError::CorruptLog {
        offset,
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

#[cfg(test)]
mod tests {
    fn reference_crc32c(bytes: &[u8]) -> u32 {
        let mut crc = !0_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = 0_u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0x82f6_3b78 & mask);
            }
        }
        !crc
    }

    #[test]
    fn crc32c_matches_the_standard_check_value() {
        assert_eq!(super::crc32c(b"123456789"), 0xe306_9283);
    }

    #[test]
    fn table_crc32c_matches_the_reference_for_varied_inputs() {
        for length in 0_usize..=1_024 {
            let bytes = (0..length)
                .map(|index| index.to_le_bytes()[0].wrapping_mul(31).wrapping_add(17))
                .collect::<Vec<_>>();
            assert_eq!(super::crc32c(&bytes), reference_crc32c(&bytes));
        }
    }
}
