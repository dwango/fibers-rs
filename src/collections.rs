use splay_tree::SplaySet;

#[derive(Debug)]
pub struct RemovableHeap<T> {
    inner: SplaySet<T>,
}
impl<T> RemovableHeap<T>
    where T: Ord
{
    pub fn new() -> Self {
        RemovableHeap { inner: SplaySet::new() }
    }
    pub fn push_if_absent(&mut self, item: T) -> bool {
        if !self.inner.contains(&item) {
            self.inner.insert(item);
            true
        } else {
            false
        }
    }
    pub fn pop_if<F>(&mut self, f: F) -> Option<T>
        where F: FnOnce(&T) -> bool
    {
        if self.inner.smallest().map_or(false, |item| f(item)) {
            self.inner.take_smallest()
        } else {
            None
        }
    }
    pub fn remove(&mut self, item: &T) -> bool {
        self.inner.remove(item)
    }
}
impl<T> RemovableHeap<T> {
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}
