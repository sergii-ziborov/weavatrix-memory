use super::Codec;
use crate::error::{MemoryError, Result};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadInOut, KeyInit, Payload},
};
use zeroize::Zeroizing;

const HEADER: &[u8; 8] = b"WMEMXE01";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const MAX_KEY_ID_BYTES: usize = 64;
const FIXED_HEADER_LEN: usize = 9 + NONCE_LEN;

/// Borrowed key material returned by an [`EncryptionKeys`] provider.
pub struct EncryptionKey<'a> {
    pub id: &'a str,
    pub bytes: &'a [u8; 32],
}

/// Supplies the active encryption key and historical decryption keys.
pub trait EncryptionKeys {
    /// Returns the key used for new envelopes.
    ///
    /// # Errors
    ///
    /// Returns an error when the active key is unavailable.
    fn active_key(&self) -> Result<EncryptionKey<'_>>;

    /// Returns key material for an identifier stored in an envelope.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested key is unavailable.
    fn decryption_key(&self, id: &str) -> Result<&[u8; 32]>;
}

/// One zeroizing key suitable for applications without key rotation.
pub struct StaticKey {
    id: String,
    bytes: Zeroizing<[u8; 32]>,
}

impl StaticKey {
    /// Creates a key with a stable, envelope-visible identifier.
    ///
    /// # Errors
    ///
    /// Rejects empty, non-ASCII, or overlong identifiers.
    pub fn new(id: impl Into<String>, bytes: [u8; 32]) -> Result<Self> {
        let id = id.into();
        validate_key_id(&id)?;
        Ok(Self {
            id,
            bytes: Zeroizing::new(bytes),
        })
    }
}

impl EncryptionKeys for StaticKey {
    fn active_key(&self) -> Result<EncryptionKey<'_>> {
        Ok(EncryptionKey {
            id: &self.id,
            bytes: &self.bytes,
        })
    }

    fn decryption_key(&self, id: &str) -> Result<&[u8; 32]> {
        if id == self.id {
            Ok(&self.bytes)
        } else {
            Err(codec("encryption key is unavailable"))
        }
    }
}

/// Produces a unique `XChaCha20` nonce for each encoded value.
pub trait NonceSource {
    /// Fills one 192-bit nonce.
    ///
    /// # Errors
    ///
    /// Returns an error when secure randomness is unavailable.
    fn fill(&self, nonce: &mut [u8; NONCE_LEN]) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OsNonce;

impl NonceSource for OsNonce {
    fn fill(&self, nonce: &mut [u8; NONCE_LEN]) -> Result<()> {
        getrandom::fill(nonce).map_err(|_| codec("operating-system randomness is unavailable"))
    }
}

/// Authenticated XChaCha20-Poly1305 wrapper for any existing codec.
pub struct XChaCha20Codec<C, K, N = OsNonce> {
    inner: C,
    keys: K,
    nonce_source: N,
    context: Vec<u8>,
    max_plaintext_bytes: usize,
}

impl<C, K> XChaCha20Codec<C, K, OsNonce> {
    /// Uses OS randomness for nonces and binds ciphertext to `context`.
    ///
    /// # Errors
    ///
    /// Rejects an empty context or zero plaintext limit.
    pub fn new(
        inner: C,
        keys: K,
        context: impl Into<Vec<u8>>,
        max_plaintext_bytes: usize,
    ) -> Result<Self> {
        Self::with_nonce_source(inner, keys, context, max_plaintext_bytes, OsNonce)
    }
}

impl<C, K, N> XChaCha20Codec<C, K, N> {
    /// Injects a nonce source, primarily for deterministic testing.
    ///
    /// # Errors
    ///
    /// Rejects an empty context or zero plaintext limit.
    pub fn with_nonce_source(
        inner: C,
        keys: K,
        context: impl Into<Vec<u8>>,
        max_plaintext_bytes: usize,
        nonce_source: N,
    ) -> Result<Self> {
        let context = context.into();
        if context.is_empty() {
            return Err(invalid("encryption.context", "must not be empty"));
        }
        if max_plaintext_bytes == 0 {
            return Err(invalid("max_plaintext_bytes", "must be greater than zero"));
        }
        Ok(Self {
            inner,
            keys,
            nonce_source,
            context,
            max_plaintext_bytes,
        })
    }
}

impl<T, C, K, N> Codec<T> for XChaCha20Codec<C, K, N>
where
    C: Codec<T>,
    K: EncryptionKeys,
    N: NonceSource,
{
    fn encode(&self, value: &T) -> Result<Vec<u8>> {
        let plaintext = Zeroizing::new(self.inner.encode(value)?);
        if plaintext.len() > self.max_plaintext_bytes {
            return Err(invalid(
                "plaintext",
                "encoded value exceeds max_plaintext_bytes",
            ));
        }
        let key = self.keys.active_key()?;
        validate_key_id(key.id)?;
        let mut nonce = [0_u8; NONCE_LEN];
        self.nonce_source.fill(&mut nonce)?;
        let header = envelope_header(key.id, &nonce)?;
        let mut aad = header.clone();
        aad.extend_from_slice(&self.context);
        let cipher = XChaCha20Poly1305::new_from_slice(key.bytes)
            .map_err(|_| codec("invalid encryption key"))?;
        let nonce = XNonce::from(nonce);
        let mut output = Vec::with_capacity(header.len() + plaintext.len() + TAG_LEN);
        output.extend_from_slice(&header);
        output.extend_from_slice(&plaintext);
        let tag = cipher
            .encrypt_inout_detached(&nonce, &aad, (&mut output[header.len()..]).into())
            .map_err(|_| codec("encryption failed"))?;
        output.extend_from_slice(&tag);
        Ok(output)
    }

    fn decode(&self, bytes: &[u8]) -> Result<T> {
        let envelope = parse_envelope(bytes)?;
        if envelope.ciphertext.len() < TAG_LEN
            || envelope.ciphertext.len() - TAG_LEN > self.max_plaintext_bytes
        {
            return Err(codec("ciphertext exceeds configured size limit"));
        }
        let mut aad = envelope.header.to_vec();
        aad.extend_from_slice(&self.context);
        let cipher = XChaCha20Poly1305::new_from_slice(self.keys.decryption_key(envelope.key_id)?)
            .map_err(|_| codec("invalid encryption key"))?;
        let nonce = XNonce::from(*envelope.nonce);
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: envelope.ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| codec("authentication failed"))?,
        );
        self.inner.decode(&plaintext)
    }
}

fn envelope_header(key_id: &str, nonce: &[u8; NONCE_LEN]) -> Result<Vec<u8>> {
    let key_len = u8::try_from(key_id.len()).map_err(|_| codec("key identifier is too long"))?;
    let mut header = Vec::with_capacity(FIXED_HEADER_LEN + key_id.len());
    header.extend_from_slice(HEADER);
    header.push(key_len);
    header.extend_from_slice(nonce);
    header.extend_from_slice(key_id.as_bytes());
    Ok(header)
}

struct Envelope<'a> {
    header: &'a [u8],
    key_id: &'a str,
    nonce: &'a [u8; NONCE_LEN],
    ciphertext: &'a [u8],
}

fn parse_envelope(bytes: &[u8]) -> Result<Envelope<'_>> {
    if bytes.len() < FIXED_HEADER_LEN || &bytes[..8] != HEADER {
        return Err(codec("unsupported encrypted envelope"));
    }
    let header_len = FIXED_HEADER_LEN
        .checked_add(usize::from(bytes[8]))
        .ok_or(MemoryError::CapacityOverflow)?;
    if bytes.len() < header_len + TAG_LEN {
        return Err(codec("truncated encrypted envelope"));
    }
    let nonce = bytes[9..FIXED_HEADER_LEN]
        .try_into()
        .map_err(|_| codec("invalid encryption nonce"))?;
    let key_id = core::str::from_utf8(&bytes[FIXED_HEADER_LEN..header_len])
        .map_err(|_| codec("invalid key identifier"))?;
    validate_key_id(key_id)?;
    Ok(Envelope {
        header: &bytes[..header_len],
        key_id,
        nonce,
        ciphertext: &bytes[header_len..],
    })
}

fn validate_key_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > MAX_KEY_ID_BYTES || !id.is_ascii() {
        return Err(invalid("encryption.key_id", "must be 1..=64 ASCII bytes"));
    }
    Ok(())
}

fn codec(message: &str) -> MemoryError {
    MemoryError::Codec {
        message: message.to_owned(),
    }
}

fn invalid(field: &'static str, reason: &'static str) -> MemoryError {
    MemoryError::InvalidValue { field, reason }
}
