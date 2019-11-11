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

pub use self::schedule::{with_current_context, yield_poll, Context};
pub use self::schedule::{Scheduler, SchedulerHandle, SchedulerId};

use sync::oneshot::{self, Link, Monitor};

mod schedule;

/// The identifier of a fiber.
///
/// The value is unique among the live fibers in a scheduler.
pub type FiberId = usize;

/// The identifier of an execution context.
pub type ContextId = (SchedulerId, FiberId);

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
        let (link0, link1) = oneshot::link();
        let future = SelectEither::new(f, link1).then(|result| {
            match result {
                Err(Either::A((result, link1))) => {
                    link1.exit(Err(result));
                }
                Ok(Either::A((result, link1))) => {
                    link1.exit(Ok(result));
                }
                _ => {
                    // Disconnected by `link0`
                }
            }
            Ok(())
        });
        self.spawn(future);
        link0
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

#[derive(Debug)]
struct FiberState {
    pub fiber_id: FiberId,
    task: Task,
    parks: usize,
    unparks: Arc<AtomicUsize>,
    pub in_run_queue: bool,
}
impl FiberState {
    pub fn new(fiber_id: FiberId, task: Task) -> Self {
        FiberState {
            fiber_id,
            task,
            parks: 0,
            unparks: Arc::new(AtomicUsize::new(0)),
            in_run_queue: false,
        }
    }
    pub fn run_once(&mut self) -> bool {
        if self.parks > 0 && self.unparks.load(atomic::Ordering::SeqCst) > 0 {
            self.parks -= 1;
            self.unparks.fetch_sub(1, atomic::Ordering::SeqCst);
        }
        if let Ok(Async::NotReady) = self.task.0.poll() {
            false
        } else {
            true
        }
    }
    pub fn is_runnable(&self) -> bool {
        self.parks == 0 || self.unparks.load(atomic::Ordering::SeqCst) > 0
    }
    pub fn park(
        &mut self,
        scheduler_id: schedule::SchedulerId,
        scheduler: schedule::SchedulerHandle,
    ) -> Unpark {
        self.parks += 1;
        Unpark {
            fiber_id: self.fiber_id,
            unparks: Arc::clone(&self.unparks),
            scheduler_id,
            scheduler,
        }
    }
    pub fn yield_once(&mut self) {
        self.parks += 1;
        self.unparks.fetch_add(1, atomic::Ordering::SeqCst);
    }
}

/// Unpark object.
///
/// When this object is dropped, it unparks the associated fiber.
///
/// This is created by calling `Context::park` method.
#[derive(Debug)]
pub struct Unpark {
    fiber_id: FiberId,
    unparks: Arc<AtomicUsize>,
    scheduler_id: schedule::SchedulerId,
    scheduler: schedule::SchedulerHandle,
}
impl Unpark {
    /// Returns the identifier of the context on which this object was created.
    pub fn context_id(&self) -> ContextId {
        (self.scheduler_id, self.fiber_id)
    }
}
impl Drop for Unpark {
    fn drop(&mut self) {
        let old = self.unparks.fetch_add(1, atomic::Ordering::SeqCst);
        if old == 0 {
            self.scheduler.wakeup(self.fiber_id);
        }
    }
}

pub(crate) type FiberFuture = Box<dyn Future<Item = (), Error = ()> + Send>;

pub(crate) struct Task(pub FiberFuture);
impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Task(_)")
    }
}

struct SelectEither<A, B>(Option<(A, B)>);
impl<A: Future, B: Future> SelectEither<A, B> {
    fn new(a: A, b: B) -> Self {
        SelectEither(Some((a, b)))
    }
}
impl<A: Future, B: Future> Future for SelectEither<A, B> {
    type Item = Either<(A::Item, B), (A, B::Item)>;
    type Error = Either<(A::Error, B), (A, B::Error)>;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (mut a, mut b) = self.0.take().expect("Cannot poll SelectEither twice");
        match a.poll() {
            Err(e) => return Err(Either::A((e, b))),
            Ok(Async::Ready(v)) => return Ok(Async::Ready(Either::A((v, b)))),
            Ok(Async::NotReady) => {}
        }
        match b.poll() {
            Err(e) => return Err(Either::B((a, e))),
            Ok(Async::Ready(v)) => return Ok(Async::Ready(Either::B((a, v)))),
            Ok(Async::NotReady) => {}
        }
        self.0 = Some((a, b));
        Ok(Async::NotReady)
    }
}
