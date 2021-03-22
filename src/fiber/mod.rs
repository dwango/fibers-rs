// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Fiber related components (for developers).
//!
//! Those are mainly exported for developers.
//! So, usual users do not need to be conscious.
use futures::future::Either;
use futures::{self, Async, Future, IntoFuture, Poll};
use std::fmt;
use std::sync::atomic::{self, AtomicUsize};
use std::sync::Arc;

use crate::sync::oneshot::{self, Link, Monitor};

/// The identifier of a fiber.
///
/// The value is unique among the live fibers in a scheduler.
pub type FiberId = usize;

/// The `Spawn` trait allows for spawning fibers.
pub trait Spawn {
    /// Spawns a fiber which will execute given boxed future.
    fn spawn_boxed(&self, fiber: Box<dyn Future<Item = (), Error = ()> + Send>);

    /// Spawns a fiber which will execute given future.
    fn spawn<F>(&self, fiber: F)
    where
        F: Future<Item = (), Error = ()> + Send + 'static,
    {
        self.spawn_boxed(Box::new(fiber));
    }

    /// Equivalent to `self.spawn(futures::lazy(|| f()))`.
    fn spawn_fn<F, T>(&self, f: F)
    where
        F: FnOnce() -> T + Send + 'static,
        T: IntoFuture<Item = (), Error = ()> + Send + 'static,
        T::Future: Send,
    {
        self.spawn(futures::lazy(f))
    }

    /// Spawns a fiber and returns a future to monitor its execution result.
    fn spawn_monitor<F, T, E>(&self, f: F) -> Monitor<T, E>
    where
        F: Future<Item = T, Error = E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let (monitored, monitor) = oneshot::monitor();
        self.spawn(f.then(move |r| {
            monitored.exit(r);
            Ok(())
        }));
        monitor
    }

    /// Spawns a linked fiber.
    ///
    /// If the returning `Link` is dropped, the spawned fiber will terminate.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate fibers;
    /// # extern crate futures;
    /// use fibers::sync::oneshot;
    /// use fibers::{Executor, InPlaceExecutor, Spawn};
    /// use futures::{Future, empty};
    ///
    /// let mut executor = InPlaceExecutor::new().unwrap();
    /// let (tx, rx) = oneshot::channel();
    /// let fiber = empty().and_then(move |()| tx.send(()));
    ///
    /// // Spawns `fiber` and drops `link`.
    /// let link = executor.spawn_link(fiber);
    /// std::mem::drop(link);
    ///
    /// // Channel `rx` is disconnected (e.g., `fiber` exited).
    /// assert!(executor.run_future(rx).unwrap().is_err());
    /// ```
    fn spawn_link<F, T, E>(&self, f: F) -> Link<(), (), T, E>
    where
        F: Future<Item = T, Error = E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        unreachable!()
    }

    /// Converts this instance into a boxed object.
    fn boxed(self) -> BoxSpawn
    where
        Self: Sized + Send + 'static,
    {
        BoxSpawn(Box::new(move |fiber| self.spawn_boxed(fiber)))
    }
}

type BoxFn = Box<dyn Fn(Box<dyn Future<Item = (), Error = ()> + Send>) + Send + 'static>;

/// Boxed `Spawn` object.
pub struct BoxSpawn(BoxFn);
impl Spawn for BoxSpawn {
    fn spawn_boxed(&self, fiber: Box<dyn Future<Item = (), Error = ()> + Send>) {
        (self.0)(fiber);
    }
    fn boxed(self) -> BoxSpawn
    where
        Self: Sized + Send + 'static,
    {
        self
    }
}
impl fmt::Debug for BoxSpawn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BoxSpawn(_)")
    }
}
