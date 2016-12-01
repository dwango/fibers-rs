use std::io;
use std::ops;
use std::sync::Arc;
use mio;

pub use self::poller::{Poller, PollerHandle, Timeout, EventedHandle};
pub use self::poller::Register;

use internal::sync_atomic::{AtomicCell, AtomicBorrowMut};

pub mod poller;

#[derive(Debug)]
pub struct SharableEvented<T>(Arc<AtomicCell<T>>);
impl<T> SharableEvented<T>
    where T: mio::Evented
{
    pub fn new(inner: T) -> Self {
        SharableEvented(Arc::new(AtomicCell::new(inner)))
    }
    pub fn lock(&self) -> EventedLock<T> {
        loop {
            // TODO: NOTE: We assumes conflictions are very rare so ...
            if let Some(inner) = self.0.try_borrow_mut() {
                return EventedLock(inner);
            }
        }
    }
}
impl<T> Clone for SharableEvented<T> {
    fn clone(&self) -> Self {
        SharableEvented(self.0.clone())
    }
}
impl<T> mio::Evented for SharableEvented<T>
    where T: mio::Evented
{
    fn register(&self,
                poll: &mio::Poll,
                token: mio::Token,
                interest: mio::Ready,
                opts: mio::PollOpt)
                -> io::Result<()> {
        self.lock().register(poll, token, interest, opts)
    }
    fn reregister(&self,
                  poll: &mio::Poll,
                  token: mio::Token,
                  interest: mio::Ready,
                  opts: mio::PollOpt)
                  -> io::Result<()> {
        self.lock().reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
        self.lock().deregister(poll)
    }
}

/// The locked reference to an evented object.
pub struct EventedLock<'a, T: 'a>(AtomicBorrowMut<'a, T>);
impl<'a, T: 'a> ops::Deref for EventedLock<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &*self.0
    }
}
impl<'a, T: 'a> ops::DerefMut for EventedLock<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.0
    }
}

/// The list of the monitorable event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interest {
    /// Read readiness event
    Read,

    /// Write readiness event
    Write,
}
