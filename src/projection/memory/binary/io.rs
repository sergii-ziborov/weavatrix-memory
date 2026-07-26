use crate::{MemoryError, Result};

pub(super) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub(super) fn new(header: &[u8]) -> Self {
        Self {
            bytes: header.to_vec(),
        }
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub(super) fn varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.bytes.push((value.to_le_bytes()[0] & 0x7f) | 0x80);
            value >>= 7;
        }
        self.bytes.push(value.to_le_bytes()[0]);
    }

    pub(super) fn usize(&mut self, value: usize) -> Result<()> {
        self.varint(u64::try_from(value).map_err(|_| MemoryError::CapacityOverflow)?);
        Ok(())
    }

    pub(super) fn signed(&mut self, value: i64) {
        self.varint(((value << 1) ^ (value >> 63)).cast_unsigned());
    }

    pub(super) fn bool(&mut self, value: bool) {
        self.bytes.push(u8::from(value));
    }

    pub(super) fn optional_u64(&mut self, value: Option<u64>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.varint(value);
        }
    }

    pub(super) fn optional_signed(&mut self, value: Option<i64>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.signed(value);
        }
    }

    pub(super) fn string(&mut self, value: &str) -> Result<()> {
        self.usize(value.len())?;
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    pub(super) fn optional_string(&mut self, value: Option<&str>) -> Result<()> {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.string(value)?;
        }
        Ok(())
    }
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8], header: &[u8]) -> Result<Self> {
        if !bytes.starts_with(header) {
            return Err(codec("unsupported compact snapshot header"));
        }
        Ok(Self {
            bytes,
            position: header.len(),
        })
    }

    pub(super) fn finish(self) -> Result<()> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(codec("trailing bytes after compact snapshot"))
        }
    }

    pub(super) fn varint(&mut self) -> Result<u64> {
        let mut value = 0_u64;
        for shift in (0..=63).step_by(7) {
            let byte = self.byte()?;
            if shift == 63 && byte > 1 {
                return Err(codec("varint overflows u64"));
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(codec("unterminated varint"))
    }

    pub(super) fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.varint()?).map_err(|_| codec("length exceeds platform capacity"))
    }

    pub(super) fn count(&mut self) -> Result<usize> {
        let count = self.usize()?;
        if count > self.remaining() {
            return Err(codec("item count exceeds remaining snapshot bytes"));
        }
        Ok(count)
    }

    pub(super) fn signed(&mut self) -> Result<i64> {
        let value = self.varint()?;
        Ok((value >> 1).cast_signed() ^ -(value & 1).cast_signed())
    }

    pub(super) fn bool(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(codec("invalid boolean tag")),
        }
    }

    pub(super) fn optional_u64(&mut self) -> Result<Option<u64>> {
        self.bool()?.then(|| self.varint()).transpose()
    }

    pub(super) fn optional_signed(&mut self) -> Result<Option<i64>> {
        self.bool()?.then(|| self.signed()).transpose()
    }

    pub(super) fn string(&mut self) -> Result<String> {
        let length = self.usize()?;
        let end = self
            .position
            .checked_add(length)
            .ok_or(MemoryError::CapacityOverflow)?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| codec("string extends past snapshot"))?;
        self.position = end;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| codec("snapshot string is not UTF-8"))
    }

    pub(super) fn optional_string(&mut self) -> Result<Option<String>> {
        self.bool()?.then(|| self.string()).transpose()
    }

    fn byte(&mut self) -> Result<u8> {
        let byte = self
            .bytes
            .get(self.position)
            .copied()
            .ok_or_else(|| codec("unexpected end of compact snapshot"))?;
        self.position += 1;
        Ok(byte)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

pub(super) fn codec(message: impl Into<String>) -> MemoryError {
    MemoryError::Codec {
        message: message.into(),
    }
}
