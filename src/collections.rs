use splay_tree::SplayMap;

#[derive(Debug)]
pub struct HeapMap<K, V> {
    inner: SplayMap<K, V>,
}
impl<K, V> HeapMap<K, V>
    where K: Ord
{
    pub fn new() -> Self {
        HeapMap { inner: SplayMap::new() }
    }
    pub fn push_if_absent(&mut self, key: K, value: V) -> bool {
        if !self.inner.contains_key(&key) {
            self.inner.insert(key, value);
            true
        } else {
            false
        }
    }
    pub fn pop_if<F>(&mut self, f: F) -> Option<(K, V)>
        where F: FnOnce(&K, &V) -> bool
    {
        if self.inner.smallest().map_or(false, |(k, v)| f(k, v)) {
            self.inner.take_smallest()
        } else {
            None
        }
    }
    pub fn peek(&mut self) -> Option<(&K, &V)> {
        self.inner.smallest()
    }
    pub fn remove(&mut self, key: &K) -> bool {
        self.inner.remove(key).is_some()
    }
}
impl<K, V> HeapMap<K, V> {
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}
