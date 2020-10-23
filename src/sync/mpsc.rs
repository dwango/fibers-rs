// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Multi-producer, single-consumer FIFO queue communication primitives.
//!
//! Basically, the structures in this module are thin wrapper of
//! the standard [mpsc](https://doc.rust-lang.org/stable/std/sync/mpsc/) module's counterparts.
//! The former implement [futures](https://github.com/alexcrichton/futures-rs) interface
//! due to facilitate writing asynchronous processings and can wait efficiently on fibers
//! until an event of interest happens.
//!
//! # Examples
//!
//! ```
//! # extern crate futures;
//! # extern crate fibers;
//! use fibers::{Executor, InPlaceExecutor, Spawn};
//! use fibers::sync::mpsc;
//! use futures::{Future, Stream};
//!
//! let mut executor = InPlaceExecutor::new().unwrap();
//! let (tx0, rx) = mpsc::channel();
//!
//! // Spawns receiver
//! let mut monitor = executor.spawn_monitor(rx.for_each(|m| {
//!     println!("# Recv: {:?}", m);
//!     Ok(())
//! }));
//!
//! // Spawns sender
//! let tx1 = tx0.clone();
//! executor.spawn_fn(move || {
//!     tx1.send(1).unwrap();
//!     tx1.send(2).unwrap();
//!     Ok(())
//! });
//!
//! // It is allowed to send messages from the outside of a fiber.
//! // (The same is true of receiving)
//! tx0.send(0).unwrap();
//! std::mem::drop(tx0);
//!
//! // Runs `executor` until the receiver exits (i.e., channel is disconnected)
//! while monitor.poll().unwrap().is_not_ready() {
//!     executor.run_once().unwrap();
//! }
//! ```
//!
//! # Note
//!
//! Unlike `fibers::net` module, the structures in this module
//! can be used on both inside and outside of a fiber.
//!
//! # Implementation Details
//!
//! If a receiver tries to receive a message from an empty channel,
//! it will suspend (deschedule) current fiber by invoking the function.
//! Then it writes data which means "I'm waiting on this fiber" to
//! an object shared with the senders.
//! If a corresponding sender finds there is a waiting receiver,
//! it will resume (reschedule) the fiber, after sending a message.
use futures::{Poll, Stream};
use std::fmt;
use std::sync::mpsc::SendError;

/// Creates a new asynchronous channel, returning the sender/receiver halves.
///
/// All data sent on the sender will become available on the receiver,
/// and no send will block the calling thread (this channel has an "infinite buffer").
///
/// # Examples
///
/// ```
/// # extern crate futures;
/// # extern crate fibers;
/// use fibers::{Executor, InPlaceExecutor, Spawn};
/// use fibers::sync::mpsc;
/// use futures::{Future, Stream};
///
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (tx0, rx) = mpsc::channel();
///
/// // Spawns receiver
/// let mut monitor = executor.spawn_monitor(rx.for_each(|m| {
///     println!("# Recv: {:?}", m);
///     Ok(())
/// }));
///
/// // Spawns sender
/// let tx1 = tx0.clone();
/// executor.spawn_fn(move || {
///     tx1.send(1).unwrap();
///     tx1.send(2).unwrap();
///     Ok(())
/// });
///
/// // It is allowed to send messages from the outside of a fiber.
/// // (The same is true of receiving)
/// tx0.send(0).unwrap();
/// std::mem::drop(tx0);
///
/// // Runs `executor` until the receiver exits (i.e., channel is disconnected)
/// while monitor.poll().unwrap().is_not_ready() {
///     executor.run_once().unwrap();
/// }
/// ```
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = futures::sync::mpsc::unbounded();
    (Sender { inner: tx }, Receiver { inner: rx })
}

/// The receiving-half of a mpsc channel.
///
/// This receving stream will never fail.
///
/// This structure can be used on both inside and outside of a fiber.
pub struct Receiver<T> {
    inner: futures::sync::mpsc::UnboundedReceiver<T>,
}
impl<T> Stream for Receiver<T> {
    /// # Note
    ///
    /// This stream will never result in an error.
    type Error = ();
    type Item = T;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}
impl<T> fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Receiver {{ .. }}")
    }
}

/// The sending-half of a asynchronous channel.
///
/// This structure can be used on both inside and outside of a fiber.
pub struct Sender<T> {
    inner: futures::sync::mpsc::UnboundedSender<T>,
}
impl<T> Sender<T> {
    /// Sends a value on this asynchronous channel.
    ///
    /// This method will never block the current thread.
    pub fn send(&self, t: T) -> Result<(), SendError<T>> {
        self.inner
            .unbounded_send(t)
            .map_err(|e| SendError(e.into_inner()))
    }

    /// Returns `true` if the receiver has dropped, otherwise `false`.
    pub fn is_disconnected(&self) -> bool {
        self.inner.is_closed()
    }
}
unsafe impl<T: Send> Sync for Sender<T> {}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            inner: self.inner.clone(),
        }
    }
}
impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Sender {{ .. }}")
    }
}
