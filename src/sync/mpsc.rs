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
//! # fn main() {
//! let mut executor = InPlaceExecutor::new().unwrap();
//! let (tx0, rx) = mpsc::channel();
//!
//! // Spanws receiver
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
//! # }
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
use std::fmt;
use std::sync::mpsc::{SendError, TryRecvError, TrySendError};
use futures::{Async, AsyncSink, Poll, Sink, StartSend, Stream};
use nbchan::mpsc as nb_mpsc;

use super::Notifier;

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
/// # fn main() {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (tx0, rx) = mpsc::channel();
///
/// // Spanws receiver
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
/// # }
/// ```
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let notifier = Notifier::new();
    let (tx, rx) = nb_mpsc::channel();
    (
        Sender {
            inner: tx,
            notifier: notifier.clone(),
        },
        Receiver {
            inner: rx,
            notifier: notifier,
        },
    )
}

/// Creates a new synchronous, bounded channel.
pub fn sync_channel<T>(bound: usize) -> (SyncSender<T>, Receiver<T>) {
    let notifier = Notifier::new();
    let (tx, rx) = nb_mpsc::sync_channel(bound);
    (
        SyncSender {
            inner: tx,
            notifier: notifier.clone(),
        },
        Receiver {
            inner: rx,
            notifier: notifier,
        },
    )
}

/// The receiving-half of a mpsc channel.
///
/// This receving stream will never fail.
///
/// This structure can be used on both inside and outside of a fiber.
pub struct Receiver<T> {
    inner: nb_mpsc::Receiver<T>,
    notifier: Notifier,
}
impl<T> Stream for Receiver<T> {
    /// # Note
    ///
    /// This stream will never result in an error.
    type Error = ();
    type Item = T;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut result = self.inner.try_recv();
        if let Err(TryRecvError::Empty) = result {
            self.notifier.await();
            result = self.inner.try_recv();
        }
        match result {
            Err(TryRecvError::Empty) => Ok(Async::NotReady),
            Err(TryRecvError::Disconnected) => Ok(Async::Ready(None)),
            Ok(t) => Ok(Async::Ready(Some(t))),
        }
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.notifier.notify();
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
    inner: nb_mpsc::Sender<T>,
    notifier: Notifier,
}
impl<T> Sender<T> {
    /// Sends a value on this asynchronous channel.
    ///
    /// This method will never block the current thread.
    pub fn send(&self, t: T) -> Result<(), SendError<T>> {
        self.inner.send(t)?;
        self.notifier.notify();
        Ok(())
    }
}
unsafe impl<T: Send> Sync for Sender<T> {}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            inner: self.inner.clone(),
            notifier: self.notifier.clone(),
        }
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.notifier.notify();
    }
}
impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Sender {{ .. }}")
    }
}

/// The sending-half of a synchronous channel.
///
/// This structure can be used on both inside and outside of a fiber.
pub struct SyncSender<T> {
    inner: nb_mpsc::SyncSender<T>,
    notifier: Notifier,
}
impl<T> Sink for SyncSender<T> {
    type SinkItem = T;
    type SinkError = SendError<T>;
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.inner.try_send(item) {
            Err(TrySendError::Full(item)) => Ok(AsyncSink::NotReady(item)),
            Err(TrySendError::Disconnected(item)) => Err(SendError(item)),
            Ok(()) => {
                self.notifier.notify();
                Ok(AsyncSink::Ready)
            }
        }
    }
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}
unsafe impl<T: Send> Sync for SyncSender<T> {}
impl<T> Clone for SyncSender<T> {
    fn clone(&self) -> Self {
        SyncSender {
            inner: self.inner.clone(),
            notifier: self.notifier.clone(),
        }
    }
}
impl<T> Drop for SyncSender<T> {
    fn drop(&mut self) {
        self.notifier.notify();
    }
}
impl<T> fmt::Debug for SyncSender<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SyncSender {{ .. }}")
    }
}
