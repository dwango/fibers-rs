use std::io;
use std::sync::Arc;
use mio;

pub use self::poller::{Poller, PollerHandle, Timeout, EventedHandle};
pub use self::poller::{Register, Interest};
pub use self::pool::{PollerPool, PollerPoolHandle};

use sync::atomic::AtomicCell;

mod poller;
mod pool;

#[derive(Debug)]
pub struct SharableEvented<T>(Arc<AtomicCell<T>>);
impl<T> SharableEvented<T>
    where T: mio::Evented
{
    pub fn new(inner: T) -> Self {
        SharableEvented(Arc::new(AtomicCell::new(inner)))
    }
    pub fn with_inner_mut<F, U>(&self, f: F) -> U
        where F: FnOnce(&mut T) -> U
    {
        loop {
            // TODO: NOTE: We assumes conflictions are very rare so ...
            if let Some(mut inner) = self.0.try_borrow_mut() {
                return f(&mut *inner);
            }
        }
    }
    pub fn with_inner_ref<F, U>(&self, f: F) -> U
        where F: FnOnce(&T) -> U
    {
        loop {
            // TODO: NOTE: We assumes conflictions are very rare so ...
            if let Some(inner) = self.0.try_borrow_mut() {
                return f(&*inner);
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
        self.with_inner_ref(|inner| inner.register(poll, token, interest, opts))
    }
    fn reregister(&self,
                  poll: &mio::Poll,
                  token: mio::Token,
                  interest: mio::Ready,
                  opts: mio::PollOpt)
                  -> io::Result<()> {
        self.with_inner_ref(|inner| inner.reregister(poll, token, interest, opts))
    }
    fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
        self.with_inner_ref(|inner| inner.deregister(poll))
    }
}
