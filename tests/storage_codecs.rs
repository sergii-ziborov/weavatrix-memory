mod common;

#[cfg(any(feature = "compression", feature = "encryption"))]
use weavatrix_memory::{Codec, MemoryError, Result};

#[cfg(any(feature = "compression", feature = "encryption"))]
#[derive(Debug, Clone, Copy)]
struct BytesCodec;

#[cfg(any(feature = "compression", feature = "encryption"))]
impl Codec<Vec<u8>> for BytesCodec {
    fn encode(&self, value: &Vec<u8>) -> Result<Vec<u8>> {
        Ok(value.clone())
    }

    fn decode(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        Ok(bytes.to_vec())
    }
}

#[cfg(feature = "mmap")]
struct TempDirectory {
    path: std::path::PathBuf,
}

#[cfg(feature = "mmap")]
impl TempDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("weavatrix-memory-mmap-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self { path }
    }
}

#[cfg(feature = "mmap")]
impl Drop for TempDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}

#[cfg(feature = "compression")]
mod compression {
    use super::*;
    use weavatrix_memory::Lz4Codec;

    #[test]
    fn lz4_round_trips_and_avoids_expanding_raw_data() {
        let codec = Lz4Codec::new(BytesCodec, 64 * 1024).unwrap();
        let repeated = vec![b'a'; 32 * 1024];
        let encoded = codec.encode(&repeated).unwrap();
        assert!(encoded.len() < repeated.len() / 10);
        assert_eq!(codec.decode(&encoded).unwrap(), repeated);

        let tiny = vec![1, 2, 3];
        let encoded = codec.encode(&tiny).unwrap();
        assert_eq!(codec.decode(&encoded).unwrap(), tiny);
    }

    #[test]
    fn lz4_rejects_bad_envelopes_and_allocation_bombs() {
        assert!(Lz4Codec::new(BytesCodec, 0).is_err());
        let codec = Lz4Codec::new(BytesCodec, 64).unwrap();
        assert!(codec.encode(&vec![0; 65]).is_err());
        assert!(codec.decode(b"not-an-envelope").is_err());

        let mut encoded = codec.encode(&vec![0; 32]).unwrap();
        encoded[9..17].copy_from_slice(&65_u64.to_le_bytes());
        assert!(codec.decode(&encoded).is_err());
        encoded[8] = 99;
        encoded[9..17].copy_from_slice(&32_u64.to_le_bytes());
        assert!(codec.decode(&encoded).is_err());
        let mut encoded = codec.encode(&vec![0; 32]).unwrap();
        encoded.push(0);
        assert!(codec.decode(&encoded).is_err());
    }
}

#[cfg(feature = "encryption")]
mod encryption {
    use super::*;
    use weavatrix_memory::{NonceSource, StaticKey, XChaCha20Codec};

    #[derive(Clone, Copy)]
    struct FixedNonce(u8);

    impl NonceSource for FixedNonce {
        fn fill(&self, nonce: &mut [u8; 24]) -> Result<()> {
            nonce.fill(self.0);
            Ok(())
        }
    }

    fn fixed_codec(
        key: [u8; 32],
        context: &[u8],
    ) -> XChaCha20Codec<BytesCodec, StaticKey, FixedNonce> {
        XChaCha20Codec::with_nonce_source(
            BytesCodec,
            StaticKey::new("primary", key).unwrap(),
            context,
            4096,
            FixedNonce(7),
        )
        .unwrap()
    }

    #[test]
    fn encryption_round_trips_and_authenticates_every_byte() {
        let codec = fixed_codec([3; 32], b"snapshot");
        let plaintext = b"memory with provenance".to_vec();
        let encoded = codec.encode(&plaintext).unwrap();
        assert_ne!(encoded, plaintext);
        assert_eq!(codec.decode(&encoded).unwrap(), plaintext);

        let mut tampered = encoded.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(codec.decode(&tampered).is_err());
        assert!(fixed_codec([4; 32], b"snapshot").decode(&encoded).is_err());
        assert!(fixed_codec([3; 32], b"journal").decode(&encoded).is_err());
    }

    #[test]
    fn encryption_validates_keys_limits_and_envelope() {
        assert!(StaticKey::new("", [0; 32]).is_err());
        assert!(StaticKey::new("é", [0; 32]).is_err());
        assert!(
            XChaCha20Codec::new(
                BytesCodec,
                StaticKey::new("key", [0; 32]).unwrap(),
                Vec::new(),
                100,
            )
            .is_err()
        );
        let codec = fixed_codec([9; 32], b"snapshot");
        assert!(codec.encode(&vec![0; 4097]).is_err());
        assert!(matches!(
            codec.decode(b"bad"),
            Err(MemoryError::Codec { .. })
        ));
    }

    #[test]
    fn os_nonces_make_repeated_encryptions_distinct() {
        let codec = XChaCha20Codec::new(
            BytesCodec,
            StaticKey::new("primary", [5; 32]).unwrap(),
            b"journal",
            4096,
        )
        .unwrap();
        let value = b"same plaintext".to_vec();
        assert_ne!(codec.encode(&value).unwrap(), codec.encode(&value).unwrap());
    }
}

#[cfg(all(feature = "compression", feature = "encryption"))]
#[test]
fn compression_and_encryption_compose_in_encode_order() {
    use weavatrix_memory::{Lz4Codec, StaticKey, XChaCha20Codec};

    let compressed = Lz4Codec::new(BytesCodec, 64 * 1024).unwrap();
    let codec = XChaCha20Codec::new(
        compressed,
        StaticKey::new("primary", [11; 32]).unwrap(),
        b"projection-snapshot",
        64 * 1024,
    )
    .unwrap();
    let value = vec![b'z'; 32 * 1024];
    let encoded = codec.encode(&value).unwrap();
    assert!(encoded.len() < value.len() / 10);
    assert_eq!(codec.decode(&encoded).unwrap(), value);
}

#[cfg(feature = "mmap")]
#[test]
fn immutable_snapshot_round_trips_through_guarded_mmap() {
    use common::simple_projection;
    use std::{
        collections::BTreeMap,
        io::{Seek, SeekFrom, Write},
    };
    use weavatrix_memory::{
        CompactSnapshotCodec, Durability, FileSnapshotStore, ProjectionSnapshot, ReplayCursor,
        SnapshotOptions, SnapshotStore, StreamId,
    };

    let directory = TempDirectory::new();
    let snapshot = ProjectionSnapshot {
        cursor: ReplayCursor {
            global_position: Some(2),
            stream_versions: BTreeMap::from([(StreamId::new("stream:simple").unwrap(), 2)]),
        },
        projection: simple_projection(),
    };
    let mut store = FileSnapshotStore::open(
        &directory.path,
        "context",
        CompactSnapshotCodec,
        SnapshotOptions {
            durability: Durability::Flush,
            ..SnapshotOptions::default()
        },
    )
    .unwrap()
    .with_memory_mapped_reads();
    store.save(&snapshot).unwrap();
    assert_eq!(store.load_latest().unwrap(), Some(snapshot));

    let path = std::fs::read_dir(&directory.path)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0xff]).unwrap();
    assert!(store.load_latest().is_err());
}
