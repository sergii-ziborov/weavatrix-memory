use crate::id::{EntityId, FactId};
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::{BuildHasher, BuildHasherDefault, Hasher},
};

pub(crate) trait TextId: Clone + Eq {
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

type FastMap<I> = HashMap<u64, (I, usize), BuildHasherDefault<IdentityHasher>>;
type CollisionMap<I> = HashMap<u64, Vec<(I, usize)>, BuildHasherDefault<IdentityHasher>>;

#[derive(Debug, Clone)]
pub(crate) struct IdIndex<I> {
    seed: u64,
    entries: FastMap<I>,
    collisions: CollisionMap<I>,
}

impl<I: TextId> IdIndex<I> {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let random = RandomState::new();
        let mut hasher = random.build_hasher();
        hasher.write_u64(0x5756_5452_5849_4458);
        Self {
            seed: hasher.finish(),
            entries: FastMap::with_capacity_and_hasher(capacity, BuildHasherDefault::default()),
            collisions: CollisionMap::default(),
        }
    }

    pub(super) fn reserve(&mut self, additional: usize) {
        self.entries.reserve(additional);
    }

    pub(super) fn contains_key(&self, id: &I) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn get(&self, id: &I) -> Option<&usize> {
        let hash = self.hash(id);
        self.get_hashed(id, hash)
    }

    fn get_hashed(&self, id: &I, hash: u64) -> Option<&usize> {
        self.entries
            .get(&hash)
            .filter(|(candidate, _)| candidate == id)
            .map(|(_, index)| index)
            .or_else(|| {
                self.collisions
                    .get(&hash)
                    .and_then(|bucket| bucket.iter().find(|(candidate, _)| candidate == id))
                    .map(|(_, index)| index)
            })
    }

    pub(super) fn insert(&mut self, id: I, index: usize) -> Option<usize> {
        let hash = self.hash(&id);
        self.insert_hashed(id, index, hash)
    }

    pub(super) fn insert_hashed(&mut self, id: I, index: usize, hash: u64) -> Option<usize> {
        if let Some((candidate, prior)) = self.entries.get_mut(&hash) {
            if candidate == &id {
                return Some(core::mem::replace(prior, index));
            }
            let bucket = self.collisions.entry(hash).or_default();
            if let Some((_, prior)) = bucket.iter_mut().find(|(candidate, _)| candidate == &id) {
                return Some(core::mem::replace(prior, index));
            }
            bucket.push((id, index));
            return None;
        }
        if let Some(bucket) = self.collisions.get_mut(&hash)
            && let Some((_, prior)) = bucket.iter_mut().find(|(candidate, _)| candidate == &id)
        {
            return Some(core::mem::replace(prior, index));
        }
        self.entries.insert(hash, (id, index));
        None
    }

    pub(super) fn hash(&self, id: &I) -> u64 {
        fingerprint(self.seed, id.text().as_bytes())
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

#[cfg(test)]
mod tests {
    use super::IdIndex;
    use crate::id::EntityId;

    #[test]
    fn prehashed_index_preserves_colliding_identifiers() {
        let first = EntityId::new("entity:first").unwrap();
        let second = EntityId::new("entity:second").unwrap();
        let mut index = IdIndex::with_capacity(2);

        assert_eq!(index.insert_hashed(first.clone(), 1, 7), None);
        assert_eq!(index.insert_hashed(second.clone(), 2, 7), None);
        assert_eq!(index.get_hashed(&first, 7), Some(&1));
        assert_eq!(index.get_hashed(&second, 7), Some(&2));
        assert_eq!(index.insert_hashed(second.clone(), 3, 7), Some(2));
        assert_eq!(index.get_hashed(&second, 7), Some(&3));
    }
}
