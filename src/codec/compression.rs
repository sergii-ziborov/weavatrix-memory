use super::Codec;
use crate::error::{MemoryError, Result};

const HEADER: &[u8; 8] = b"WMEMLZ01";
const HEADER_LEN: usize = 25;
const MODE_RAW: u8 = 0;
const MODE_LZ4: u8 = 1;

/// Size-bounded LZ4 wrapper for any existing codec.
#[derive(Debug, Clone)]
pub struct Lz4Codec<C> {
    inner: C,
    max_decoded_bytes: usize,
}

impl<C> Lz4Codec<C> {
    /// Wraps a codec and caps the allocation accepted during decode.
    ///
    /// # Errors
    ///
    /// Rejects a zero decoded-size limit.
    pub fn new(inner: C, max_decoded_bytes: usize) -> Result<Self> {
        if max_decoded_bytes == 0 {
            return Err(invalid("must be greater than zero"));
        }
        Ok(Self {
            inner,
            max_decoded_bytes,
        })
    }

    #[must_use]
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

impl<T, C> Codec<T> for Lz4Codec<C>
where
    C: Codec<T>,
{
    fn encode(&self, value: &T) -> Result<Vec<u8>> {
        let raw = self.inner.encode(value)?;
        if raw.len() > self.max_decoded_bytes {
            return Err(invalid("encoded value exceeds max_decoded_bytes"));
        }
        let capacity = HEADER_LEN
            .checked_add(lz4_flex::block::get_maximum_output_size(raw.len()))
            .ok_or(MemoryError::CapacityOverflow)?;
        let mut output = vec![0; capacity];
        let compressed_len = lz4_flex::block::compress_into(&raw, &mut output[HEADER_LEN..])
            .map_err(|_| codec("LZ4 output capacity was insufficient"))?;
        let (mode, payload_len) = if compressed_len < raw.len() {
            (MODE_LZ4, compressed_len)
        } else {
            output[HEADER_LEN..HEADER_LEN + raw.len()].copy_from_slice(&raw);
            (MODE_RAW, raw.len())
        };
        let raw_len = u64::try_from(raw.len()).map_err(|_| MemoryError::CapacityOverflow)?;
        let stored_len = u64::try_from(payload_len).map_err(|_| MemoryError::CapacityOverflow)?;
        output[..8].copy_from_slice(HEADER);
        output[8] = mode;
        output[9..17].copy_from_slice(&raw_len.to_le_bytes());
        output[17..25].copy_from_slice(&stored_len.to_le_bytes());
        output.truncate(HEADER_LEN + payload_len);
        Ok(output)
    }

    fn decode(&self, bytes: &[u8]) -> Result<T> {
        if bytes.len() < HEADER_LEN || &bytes[..8] != HEADER {
            return Err(codec("unsupported LZ4 envelope"));
        }
        let raw_len = usize::try_from(u64::from_le_bytes(bytes[9..17].try_into().unwrap()))
            .map_err(|_| codec("decoded length exceeds platform capacity"))?;
        if raw_len > self.max_decoded_bytes {
            return Err(codec("decoded value exceeds configured size limit"));
        }
        let stored_len = usize::try_from(u64::from_le_bytes(bytes[17..25].try_into().unwrap()))
            .map_err(|_| codec("stored length exceeds platform capacity"))?;
        let envelope_len = HEADER_LEN
            .checked_add(stored_len)
            .ok_or(MemoryError::CapacityOverflow)?;
        if bytes.len() != envelope_len {
            return Err(codec("compressed envelope length mismatch"));
        }
        let payload = &bytes[HEADER_LEN..];
        let raw = match bytes[8] {
            MODE_RAW if payload.len() == raw_len => payload.to_vec(),
            MODE_RAW => return Err(codec("raw payload length mismatch")),
            MODE_LZ4 => {
                let mut raw = vec![0; raw_len];
                let written = lz4_flex::block::decompress_into(payload, &mut raw)
                    .map_err(|_| codec("invalid LZ4 payload"))?;
                if written != raw_len {
                    return Err(codec("decoded payload length mismatch"));
                }
                raw
            }
            _ => return Err(codec("unsupported compression mode")),
        };
        if raw.len() != raw_len {
            return Err(codec("decoded payload length mismatch"));
        }
        self.inner.decode(&raw)
    }
}

fn codec(message: &str) -> MemoryError {
    MemoryError::Codec {
        message: message.to_owned(),
    }
}

fn invalid(reason: &'static str) -> MemoryError {
    MemoryError::InvalidValue {
        field: "max_decoded_bytes",
        reason,
    }
}
