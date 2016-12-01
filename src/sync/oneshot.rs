//! Oneshot communication channels between two peers.
//!
//! # Note
//!
//! Unlike `fibers::net` module, the structures in this module
//! can be used on both inside and outside of a fiber.
//!
//! # Implementation Details
//!
//! The channels provided in this module are specializations of
//! the asynchronous channel in the `fibers::sync::mpsc` module.
//!
//! The former essentially have the same semantics as the latter.
//! But those are useful to clarify the intention of programmers.
use std::fmt;
use std::error;
use std::sync::mpsc::{RecvError, SendError};
use futures::{Poll, Async, Future, Stream};

use sync::mpsc;

/// Creates a new asynchronous oneshot channel, returning the sender/receiver halves.
///
/// # Examples
///
/// ```
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Executor, InPlaceExecutor};
/// use fibers::sync::oneshot;
/// use futures::Future;
///
/// # fn main () {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (tx0, rx0) = oneshot::channel();
/// let (tx1, rx1) = oneshot::channel();
///
/// // Spanws receiver
/// let mut monitor = executor.spawn_monitor(rx0.and_then(move |v| {
///     assert_eq!(v, "first value");
///     rx1
/// })
/// .and_then(|v| {
///     assert_eq!(v, "second value");
///     Ok(())
/// }));
///
/// // Spawns sender for `tx1`
/// executor.spawn_fn(move || {
///     tx1.send("second value").unwrap();
///     Ok(())
/// });
///
/// // It is allowed to send messages from the outside of a fiber.
/// // (The same is true of receiving)
/// tx0.send("first value").unwrap();
///
/// // Runs `executor` until the receiver exits (i.e., channel is disconnected)
/// while monitor.poll().unwrap().is_not_ready() {
///     executor.run_once(None).unwrap();
/// }
/// # }
/// ```
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    (Sender(tx), Receiver(rx))
}

/// The sending-half of an asynchronous oneshot channel.
///
/// This structure can be used on both inside and outside of a fiber.
#[derive(Debug)]
pub struct Sender<T>(mpsc::Sender<T>);
impl<T> Sender<T> {
    /// Sends a value on this asynchronous channel.
    ///
    /// This method will never block the current thread.
    pub fn send(self, t: T) -> Result<(), SendError<T>> {
        self.0.send(t)
    }
}

/// The receiving-half of a oneshot channel.
///
/// This structure can be used on both inside and outside of a fiber.
#[derive(Debug)]
pub struct Receiver<T>(mpsc::Receiver<T>);
impl<T> Future for Receiver<T> {
    type Item = T;
    type Error = RecvError;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match assert_ok!(self.0.poll()) {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(None) => Err(RecvError),
            Async::Ready(Some(t)) => Ok(Async::Ready(t)),
        }
    }
}

/// Creates a oneshot channel for unidirectional monitoring.
///
/// When `Monitored` object is (intentionally or unintentionally) dropped,
/// the corresponding `Monitor` object will detect it and
/// return the resulting value at the next time the `Future::poll` method is
/// called on it.
///
/// # Examples
///
/// An example of monitoring a successful completion:
///
/// ```
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Executor, InPlaceExecutor};
/// use fibers::sync::oneshot;
/// use futures::{Async, Future};
///
/// # fn main () {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (monitored, mut monitor) = oneshot::monitor();
///
/// // Spanws monitored fiber
/// // (In practice, spawning fiber via `spawn_monitor` function is
/// //  more convenient way to archieve the same result)
/// executor.spawn_fn(move || {
///     // Notifies the execution have completed successfully.
///     monitored.exit(Ok("succeeded") as Result<_, ()>);
///     Ok(())
/// });
///
/// // Runs `executor` until above fiber exists
/// loop {
///     let result = monitor.poll().expect("Unexpected failure");
///     if let Async::Ready(value) = result {
///         assert_eq!(value, "succeeded");
///         break;
///     } else {
///         executor.run_once(None).unwrap();
///     }
/// }
/// # }
/// ```
///
/// An example of detecting unintentional termination:
///
/// ```
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Executor, InPlaceExecutor};
/// use fibers::sync::oneshot;
/// use futures::{Async, Future};
///
/// # fn main () {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (monitored, mut monitor) = oneshot::monitor::<(),()>();
///
/// // Spanws monitored fiber
/// // (In practice, spawning fiber via `spawn_monitor` function is
/// //  more convenient way to archieve the same result)
/// executor.spawn_fn(move || {
///     let _ = monitored; // This fiber owns `monitored`
///     Ok(()) // Terminated before calling `Monitored::exit` method
/// });
///
/// // Runs `executor` until above fiber exists
/// loop {
///     match monitor.poll() {
///         Ok(Async::NotReady) => {
///             executor.run_once(None).unwrap();
///         }
///         Ok(Async::Ready(_)) => unreachable!(),
///         Err(e) => {
///             assert_eq!(e, oneshot::MonitorError::Aborted);
///             break;
///         }
///     }
/// }
/// # }
/// ```
///
/// # Implementation Details
///
/// Internally, this channel is almost the same as the one created by `channel` function.
pub fn monitor<T, E>() -> (Monitored<T, E>, Monitor<T, E>) {
    let (tx, rx) = channel();
    (Monitored(tx), Monitor(rx))
}

/// The monitored-half of a monitor channel.
///
/// This is created by calling `monitor` function.
#[derive(Debug)]
pub struct Monitored<T, E>(Sender<Result<T, E>>);
impl<T, E> Monitored<T, E> {
    /// Notifies the monitoring peer that the monitored target has exited intentionally.
    pub fn exit(self, result: Result<T, E>) {
        let _ = self.0.send(result);
    }
}

/// The monitoring-half of a monitor channel.
///
/// This is created by calling `monitor` function.
#[derive(Debug)]
pub struct Monitor<T, E>(Receiver<Result<T, E>>);
impl<T, E> Future for Monitor<T, E> {
    type Item = T;
    type Error = MonitorError<E>;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Async::Ready(r) = self.0.poll().or(Err(MonitorError::Aborted))? {
            match r {
                Err(e) => Err(MonitorError::Failed(e)),
                Ok(v) => Ok(Async::Ready(v)),
            }
        } else {
            Ok(Async::NotReady)
        }
    }
}

/// The reason that a monitored peer has not completed successfully.
#[derive(Debug, PartialEq, Eq)]
pub enum MonitorError<E> {
    /// The monitor channel is disconnected.
    Aborted,

    /// The monitored peer has exited with an error `E`.
    ///
    /// i.e., `Monitored::exit(self, Err(E))` was called
    Failed(E),
}
impl<E: error::Error> error::Error for MonitorError<E> {
    fn description(&self) -> &str {
        match *self {
            MonitorError::Aborted => "Monitor target aborted",
            MonitorError::Failed(_) => "Monitor target failed: {}",
        }
    }
    fn cause(&self) -> Option<&error::Error> {
        match *self {
            MonitorError::Aborted => None,
            MonitorError::Failed(ref e) => Some(e),
        }
    }
}
impl<E: fmt::Display> fmt::Display for MonitorError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            MonitorError::Aborted => write!(f, "Monitor target aborted"),
            MonitorError::Failed(ref e) => write!(f, "Monitor target failed: {}", e),
        }
    }
}

/// Creates a oneshot channel for bidirectional monitoring.
pub fn link<E0, E1>() -> (Link<E0, E1>, Link<E1, E0>) {
    let (tx0, rx0) = monitor();
    let (tx1, rx1) = monitor();
    (Link { tx: tx0, rx: rx1 }, Link { tx: tx1, rx: rx0 })
}

/// The half of a link channel.
///
/// This is created by calling `link` function.
#[derive(Debug)]
pub struct Link<E0, E1 = E0> {
    tx: Monitored<(), E0>,
    rx: Monitor<(), E1>,
}
impl<E0, E1> Link<E0, E1> {
    /// Notifies the linked peer that this peer has exited intentionally.
    pub fn exit(self, result: Result<(), E0>) {
        self.tx.exit(result);
    }
}
impl<E0, E1> Future for Link<E0, E1> {
    type Item = ();
    type Error = MonitorError<E1>;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.rx.poll()
    }
}
