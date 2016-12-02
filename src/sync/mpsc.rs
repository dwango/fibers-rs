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
//! use fibers::{Executor, InPlaceExecutor};
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
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use futures::{Poll, Async, Stream, Sink, StartSend, AsyncSink};

use fiber;
use internal::sync_atomic::AtomicCell;

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
/// use fibers::{Executor, InPlaceExecutor};
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
    let (tx, rx) = std_mpsc::channel();
    (Sender {
        inner: tx,
        notifier: notifier.clone(),
    },
     Receiver {
        inner: rx,
        notifier: notifier,
    })
}

/// Creates a new synchronous, bounded channel.
pub fn sync_channel<T>(bound: usize) -> (SyncSender<T>, Receiver<T>) {
    let notifier = Notifier::new();
    let (tx, rx) = std_mpsc::sync_channel(bound);
    (SyncSender {
        inner: tx,
        notifier: notifier.clone(),
    },
     Receiver {
        inner: rx,
        notifier: notifier,
    })
}

/// The receiving-half of a mpsc channel.
///
/// This receving stream will never fail.
///
/// This structure can be used on both inside and outside of a fiber.
#[derive(Debug)]
pub struct Receiver<T> {
    inner: std_mpsc::Receiver<T>,
    notifier: Notifier,
}
impl<T> Stream for Receiver<T> {
    /// # Note
    ///
    /// This stream will never result in an error.
    type Error = ();
    type Item = T;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {
                self.notifier.await();
                Ok(Async::NotReady)
            }
            Err(std_mpsc::TryRecvError::Disconnected) => Ok(Async::Ready(None)),
            Ok(t) => Ok(Async::Ready(Some(t))),
        }
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.notifier.notify();
    }
}

/// The sending-half of a asynchronous channel.
///
/// This structure can be used on both inside and outside of a fiber.
#[derive(Debug)]
pub struct Sender<T> {
    inner: std_mpsc::Sender<T>,
    notifier: Notifier,
}
impl<T> Sender<T> {
    /// Sends a value on this asynchronous channel.
    ///
    /// This method will never block the current thread.
    pub fn send(&self, t: T) -> Result<(), std_mpsc::SendError<T>> {
        self.inner.send(t)?;
        self.notifier.notify();
        Ok(())
    }
}
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

/// The sending-half of a synchronous channel.
///
/// This structure can be used on both inside and outside of a fiber.
#[derive(Debug)]
pub struct SyncSender<T> {
    inner: std_mpsc::SyncSender<T>,
    notifier: Notifier,
}
impl<T> Sink for SyncSender<T> {
    type SinkItem = T;
    type SinkError = std_mpsc::SendError<T>;
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.inner.try_send(item) {
            Err(std_mpsc::TrySendError::Full(item)) => Ok(AsyncSink::NotReady(item)),
            Err(std_mpsc::TrySendError::Disconnected(item)) => Err(std_mpsc::SendError(item)),
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

#[derive(Debug, Clone)]
struct Notifier {
    unpark: Arc<AtomicCell<Option<fiber::Unpark>>>,
}
impl Notifier {
    pub fn new() -> Self {
        Notifier { unpark: Arc::new(AtomicCell::new(None)) }
    }
    pub fn await(&mut self) {
        loop {
            if let Some(mut unpark) = self.unpark.try_borrow_mut() {
                let context_id = fiber::with_current_context(|c| c.context_id());
                if unpark.as_ref().map(|u| u.context_id()) != context_id {
                    *unpark = fiber::with_current_context(|mut c| c.park());
                }
                return;
            }
        }
    }
    pub fn notify(&self) {
        loop {
            if let Some(mut unpark) = self.unpark.try_borrow_mut() {
                *unpark = None;
                return;
            }
        }
    }
}
