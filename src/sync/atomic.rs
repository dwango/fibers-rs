use std::ops;
use std::ptr;
use std::sync::atomic::{self, AtomicPtr};

#[derive(Debug)]
pub struct AtomicCell<T> {
    inner: AtomicPtr<T>,
}
impl<T> AtomicCell<T> {
    pub fn new(inner: T) -> Self {
        let boxed = Box::new(inner);
        AtomicCell { inner: AtomicPtr::new(Box::into_raw(boxed)) }
    }
    pub fn try_borrow_mut(&self) -> Option<AtomicBorrowMut<T>> {
        let old = self.inner.swap(ptr::null_mut(), atomic::Ordering::SeqCst);
        if old == ptr::null_mut() {
            None
        } else {
            let inner = unsafe { Box::from_raw(old) };
            Some(AtomicBorrowMut::new(self, inner))
        }
    }
    pub fn try_borrow(&self) -> Option<AtomicBorrowRef<T>> {
        self.try_borrow_mut().map(AtomicBorrowRef)
    }
}
impl<T> Drop for AtomicCell<T> {
    fn drop(&mut self) {
        let inner = self.inner.load(atomic::Ordering::SeqCst);
        assert_ne!(inner, ptr::null_mut());
        let _ = unsafe { Box::from_raw(inner) };
    }
}

pub struct AtomicBorrowMut<'a, T: 'a> {
    owner: &'a AtomicCell<T>,
    inner: Option<Box<T>>,
}
impl<'a, T> AtomicBorrowMut<'a, T> {
    fn new(owner: &'a AtomicCell<T>, inner: Box<T>) -> Self {
        AtomicBorrowMut {
            owner: owner,
            inner: Some(inner),
        }
    }
}
impl<'a, T> ops::Deref for AtomicBorrowMut<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*assert_some!(self.inner.as_ref())
    }
}
impl<'a, T> ops::DerefMut for AtomicBorrowMut<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *assert_some!(self.inner.as_mut())
    }
}
impl<'a, T> Drop for AtomicBorrowMut<'a, T> {
    fn drop(&mut self) {
        let inner = Box::into_raw(assert_some!(self.inner.take()));
        let old = self.owner.inner.swap(inner, atomic::Ordering::SeqCst);
        assert_eq!(old, ptr::null_mut());
    }
}

pub struct AtomicBorrowRef<'a, T: 'a>(AtomicBorrowMut<'a, T>);
impl<'a, T> ops::Deref for AtomicBorrowRef<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn it_works() {
        let x = AtomicCell::new(10);
        {
            let y = x.try_borrow_mut();
            assert!(y.is_some());
            let mut y = y.unwrap();
            assert_eq!(*y, 10);

            let z = x.try_borrow_mut();
            assert!(z.is_none());

            *y = 20;
        }
        let z = x.try_borrow_mut();
        assert!(z.is_some());
        assert_eq!(*z.unwrap(), 20);
    }
}
