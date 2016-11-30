//! Crate internal data structures.
use splay_tree::SplayMap;

/// A heap (a.k.a, priority queue) which has map like properties.
///
/// This heap can remove an entry by it's key.
///
/// # Notice
///
/// Unlink standard `BinaryHeap`, this heap
/// pops the entry which has the smallest key in all entries.
#[derive(Debug)]
pub struct HeapMap<K, V> {
    inner: SplayMap<K, V>,
}
impl<K, V> HeapMap<K, V>
    where K: Ord
{
    /// Makes new heap instance.
    pub fn new() -> Self {
        HeapMap { inner: SplayMap::new() }
    }

    /// Pushes the entry in the heap, if an entry which has the `key`
    /// would not exist.
    ///
    /// If the entry is inserted, this will return `true`, otherwise `false`.
    pub fn push_if_absent(&mut self, key: K, value: V) -> bool {
        if !self.inner.contains_key(&key) {
            self.inner.insert(key, value);
            true
        } else {
            false
        }
    }

    /// Pops the entry which has the smallest key if the predicate `f` would be satisfied.
    pub fn pop_if<F>(&mut self, f: F) -> Option<(K, V)>
        where F: FnOnce(&K, &V) -> bool
    {
        if self.inner.smallest().map_or(false, |(k, v)| f(k, v)) {
            self.inner.take_smallest()
        } else {
            None
        }
    }

    /// Peeks the entry which has the smallest key.
    ///
    /// If the heap is empty, this will return `None`.
    pub fn peek(&mut self) -> Option<(&K, &V)> {
        self.inner.smallest()
    }

    /// Removes the entry which has `key` from the heap.
    ///
    /// If such entry exists, this will return `true`, otherwise `false`.
    pub fn remove(&mut self, key: &K) -> bool {
        self.inner.remove(key).is_some()
    }
}
impl<K, V> HeapMap<K, V> {
    /// Returns the entry count of the heap.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn it_works() {
        let mut heap = HeapMap::new();

        // TEST: push_if_absent()
        assert_eq!(heap.len(), 0);
        assert!(heap.push_if_absent(1, "value-a"));
        assert!(!heap.push_if_absent(1, "value-b"));
        assert_eq!(heap.len(), 1);

        assert!(heap.push_if_absent(0, "value-b"));
        assert!(heap.push_if_absent(2, "value-c"));

        // TEST: peek
        assert_eq!(heap.peek(), Some((&0, &"value-b")));

        // TEST: remove
        assert_eq!(heap.len(), 3);

        assert!(heap.remove(&1));
        assert_eq!(heap.len(), 2);

        assert!(!heap.remove(&1));
        assert_eq!(heap.len(), 2);

        // TEST: pop_if
        assert_eq!(heap.pop_if(|_, _| false), None);
        assert_eq!(heap.pop_if(|key, _| *key == 0), Some((0, "value-b")));
        assert_eq!(heap.pop_if(|key, _| *key == 2), Some((2, "value-c")));
        assert_eq!(heap.pop_if(|_, _| true), None);
    }
}
