// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! I/O events polling functionalities (for developers).
//!
//! This module is mainly exported for developers.
//! So, usual users do not need to be conscious.
//!
//! # Implementation Details
//!
//! This module is a wrapper of the [mio](https://github.com/carllerche/mio) crate.
use mio;
use std::io;
use std::ops;
use std::sync::Arc;

pub use self::poller::{EventedHandle, Poller, PollerHandle};
pub use self::poller::{Register, DEFAULT_EVENTS_CAPACITY};

use sync_atomic::{AtomicBorrowMut, AtomicCell};

pub(crate) mod poller;

#[derive(Debug)]
pub(crate) struct SharableEvented<T>(Arc<AtomicCell<T>>);
impl<T> SharableEvented<T>
where
    T: mio::Evented,
{
    pub fn new(inner: T) -> Self {
        SharableEvented(Arc::new(AtomicCell::new(inner)))
    }
    pub fn lock(&self) -> EventedLock<T> {
        loop {
            // NOTE: We assumes conflictions are very rare.
            // (But should be refined in future releases)
            if let Some(inner) = self.0.try_borrow_mut() {
                return EventedLock(inner);
            }
        }
    }
}
impl<T> Clone for SharableEvented<T> {
    fn clone(&self) -> Self {
        SharableEvented(Arc::clone(&self.0))
    }
}
impl<T> mio::Evented for SharableEvented<T>
where
    T: mio::Evented,
{
    fn register(
        &self,
        poll: &mio::Poll,
        token: mio::Token,
        interest: mio::Ready,
        opts: mio::PollOpt,
    ) -> io::Result<()> {
        self.lock().register(poll, token, interest, opts)
    }
    fn reregister(
        &self,
        poll: &mio::Poll,
        token: mio::Token,
        interest: mio::Ready,
        opts: mio::PollOpt,
    ) -> io::Result<()> {
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
