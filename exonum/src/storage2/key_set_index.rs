use std::marker::PhantomData;

use super::{BaseIndex, BaseIndexIter, Snapshot, Fork, StorageKey};

pub struct KeySetIndex<T, K> {
    base: BaseIndex<T>,
    _k: PhantomData<K>,
}

pub struct KeySetIndexIter<'a, K> {
    base_iter: BaseIndexIter<'a, K, ()>
}

impl<T, K> KeySetIndex<T, K> {
    pub fn new(prefix: Vec<u8>, base: T) -> Self {
        KeySetIndex {
            base: BaseIndex::new(prefix, base),
            _k: PhantomData,
        }
    }
}

impl<T, K> KeySetIndex<T, K> where T: AsRef<Snapshot>,
                                   K: StorageKey {
    pub fn contains(&self, item: &K) -> bool {
        self.base.contains(item)
    }

    pub fn iter(&self) -> KeySetIndexIter<K> {
        KeySetIndexIter { base_iter: self.base.iter() }
    }

    pub fn iter_from(&self, from: &K) -> KeySetIndexIter<K> {
        KeySetIndexIter { base_iter: self.base.iter_from(from) }
    }
}

impl<T, K> KeySetIndex<T, K> where T: AsMut<Fork>,
                                   K: StorageKey {
    pub fn insert(&mut self, item: K) {
        self.base.put(&item, ())
    }

    pub fn delete(&mut self, item: &K) {
        self.base.delete(item)
    }

    pub fn clear(&mut self) {
        self.base.clear()
    }
}

impl<'a, K> Iterator for KeySetIndexIter<'a, K> where K: StorageKey {
    type Item = K;

    fn next(&mut self) -> Option<Self::Item> {
        self.base_iter.next().map(|(k, ..)| k)
    }
}
