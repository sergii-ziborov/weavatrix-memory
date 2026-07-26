use crate::{EntityId, FactId};
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::{BuildHasher, BuildHasherDefault, Hash, Hasher},
};

pub(super) trait TextId: Clone + Eq {
    fn text(&self) -> &str;
}

impl TextId for EntityId {
    fn text(&self) -> &str {
        self.as_str()
    }
}

impl TextId for FactId {
    fn text(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone)]
struct IndexedId<I> {
    id: I,
    hash: u64,
}

impl<I: Eq> PartialEq for IndexedId<I> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<I: Eq> Eq for IndexedId<I> {}

impl<I> Hash for IndexedId<I> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

#[derive(Default)]
struct IdentityHasher(u64);

impl Hasher for IdentityHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = fingerprint(0, bytes);
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

type FastMap<I> = HashMap<IndexedId<I>, usize, BuildHasherDefault<IdentityHasher>>;

#[derive(Debug, Clone)]
pub(super) struct IdIndex<I> {
    seed: u64,
    entries: FastMap<I>,
}

impl<I: TextId> IdIndex<I> {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        let random = RandomState::new();
        let mut hasher = random.build_hasher();
        hasher.write_u64(0x5756_5452_5849_4458);
        Self {
            seed: hasher.finish(),
            entries: FastMap::with_capacity_and_hasher(capacity, BuildHasherDefault::default()),
        }
    }

    pub(super) fn reserve(&mut self, additional: usize) {
        self.entries.reserve(additional);
    }

    pub(super) fn contains_key(&self, id: &I) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn get(&self, id: &I) -> Option<&usize> {
        self.entries.get(&self.key(id))
    }

    pub(super) fn insert(&mut self, id: I, index: usize) -> Option<usize> {
        let hash = self.hash(&id);
        self.insert_hashed(id, index, hash)
    }

    pub(super) fn insert_hashed(&mut self, id: I, index: usize, hash: u64) -> Option<usize> {
        self.entries.insert(IndexedId { id, hash }, index)
    }

    pub(super) fn hash(&self, id: &I) -> u64 {
        fingerprint(self.seed, id.text().as_bytes())
    }

    fn key(&self, id: &I) -> IndexedId<I> {
        IndexedId {
            id: id.clone(),
            hash: fingerprint(self.seed, id.text().as_bytes()),
        }
    }
}

fn fingerprint(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed ^ 0xa076_1d64_78bd_642f;
    for chunk in bytes.chunks(8) {
        let mut word = [0_u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        hash ^= u64::from_le_bytes(word).wrapping_mul(0xe703_7ed1_a0b4_28db);
        hash = hash.rotate_left(23).wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    }
    hash ^= bytes.len() as u64;
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(0x1656_6791_9e37_79f9);
    hash ^ (hash >> 32)
}
