#[cfg(feature = "json")]
use crate::error::MemoryError;
use crate::error::Result;

#[cfg(feature = "compression")]
mod compression;
#[cfg(feature = "encryption")]
mod encryption;

#[cfg(feature = "compression")]
pub use compression::Lz4Codec;
#[cfg(feature = "encryption")]
pub use encryption::{
    EncryptionKey, EncryptionKeys, NonceSource, OsNonce, StaticKey, XChaCha20Codec,
};

/// Deterministic serialization boundary used by durable stores.
pub trait Codec<T> {
    /// Encodes one value.
    ///
    /// # Errors
    ///
    /// Returns a codec-specific serialization error.
    fn encode(&self, value: &T) -> Result<Vec<u8>>;

    /// Decodes one complete value.
    ///
    /// # Errors
    ///
    /// Returns a codec-specific deserialization error.
    fn decode(&self, bytes: &[u8]) -> Result<T>;
}

#[cfg(feature = "json")]
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonCodec;

#[cfg(feature = "json")]
impl<T> Codec<T> for JsonCodec
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn encode(&self, value: &T) -> Result<Vec<u8>> {
        blazingly_json::to_vec(value).map_err(|error| MemoryError::Codec {
            message: error.to_string(),
        })
    }

    fn decode(&self, bytes: &[u8]) -> Result<T> {
        blazingly_json::from_slice(bytes).map_err(|error| MemoryError::Codec {
            message: error.to_string(),
        })
    }
}
