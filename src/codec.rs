use crate::Result;

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
        serde_json::to_vec(value).map_err(|error| crate::MemoryError::Codec {
            message: error.to_string(),
        })
    }

    fn decode(&self, bytes: &[u8]) -> Result<T> {
        serde_json::from_slice(bytes).map_err(|error| crate::MemoryError::Codec {
            message: error.to_string(),
        })
    }
}
