// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Fiber related components (for developers).
//!
//! Those are mainly exported for developers.
//! So, usual users do not need to be conscious.
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicUsize};
use futures::{self, Async, Future, BoxFuture, IntoFuture, Poll};
use futures::future::Either;
use handy_async::future::FutureExt;

pub use self::schedule::{Scheduler, SchedulerHandle, SchedulerId};
pub use self::schedule::{with_current_context, Context};

use sync::oneshot::{self, Monitor, Link};
use internal::fiber::Task;

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
    fn spawn_boxed(&self, fiber: BoxFuture<(), ()>);

    /// Spawns a fiber which will execute given future.
    fn spawn<F>(&self, fiber: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        self.spawn_boxed(fiber.boxed());
    }

    /// Equivalent to `self.spawn(futures::lazy(|| f()))`.
    fn spawn_fn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.spawn(futures::lazy(|| f()))
    }

    /// Spawns a fiber and returns a future to monitor it's execution result.
    fn spawn_monitor<F, T, E>(&self, f: F) -> Monitor<T, E>
        where F: Future<Item = T, Error = E> + Send + 'static,
              T: Send + 'static,
              E: Send + 'static
    {
        let (monitored, monitor) = oneshot::monitor();
        self.spawn(f.then(move |r| Ok(monitored.exit(r))));
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
    /// # fn main() {
    /// let mut executor = InPlaceExecutor::new().unwrap();
    /// let (tx, rx) = oneshot::channel();
    /// let fiber = empty().and_then(move |()| tx.send(()));
    ///
    /// // Spanws `fiber` and drops `link`.
    /// let link = executor.spawn_link(fiber);
    /// std::mem::drop(link);
    ///
    /// // Channel `rx` is disconnected (e.g., `fiber` exited).
    /// assert!(executor.run_future(rx).unwrap().is_err());
    /// # }
    /// ```
    fn spawn_link<F, T, E>(&self, f: F) -> Link<(), (), T, E>
        where F: Future<Item = T, Error = E> + Send + 'static,
              T: Send + 'static,
              E: Send + 'static
    {
        let (link0, link1) = oneshot::link();
        let future = f.select_either(link1).then(|result| {
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
        where Self: Sized + Send + 'static
    {
        BoxSpawn(Box::new(move |fiber| self.spawn_boxed(fiber)))
    }
}

/// Boxed `Spawn` object.
pub struct BoxSpawn(Box<Fn(BoxFuture<(), ()>) + Send + 'static>);
impl Spawn for BoxSpawn {
    fn spawn_boxed(&self, fiber: BoxFuture<(), ()>) {
        (self.0)(fiber);
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
            fiber_id: fiber_id,
            task: task,
            parks: 0,
            unparks: Arc::new(AtomicUsize::new(0)),
            in_run_queue: false,
        }
    }
    pub fn run_once(&mut self) -> bool {
        if self.parks > 0 {
            if self.unparks.load(atomic::Ordering::SeqCst) > 0 {
                self.parks -= 1;
                self.unparks.fetch_sub(1, atomic::Ordering::SeqCst);
            }
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
    pub fn park(&mut self,
                scheduler_id: schedule::SchedulerId,
                scheduler: schedule::SchedulerHandle)
                -> Unpark {
        self.parks += 1;
        Unpark {
            fiber_id: self.fiber_id,
            unparks: self.unparks.clone(),
            scheduler_id: scheduler_id,
            scheduler: scheduler,
        }
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

/// Cooperatively gives up a poll for the current future (fiber).
///
/// # Examples
///
/// ```
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{fiber, Executor, InPlaceExecutor, Spawn};
/// use futures::{Future, Async, Poll};
///
/// struct HeavyCalculation {
///     polled_count: usize,
///     loops: usize
/// }
/// impl HeavyCalculation {
///     fn new(loop_count: usize) -> Self {
///         HeavyCalculation { polled_count: 0, loops: loop_count }
///     }
/// }
/// impl Future for HeavyCalculation {
///     type Item = usize;
///     type Error = ();
///     fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
///         self.polled_count += 1;
///
///         let mut per_poll_loop_limit = 10;
///         while self.loops > 0 {
///             self.loops -= 1;
///             per_poll_loop_limit -= 1;
///             if per_poll_loop_limit == 0 {
///                 // Suspends calculation and gives execution to other fibers.
///                 return fiber::yield_poll();
///             }
///         }
///         Ok(Async::Ready(self.polled_count))
///     }
/// }
///
/// # fn main() {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let monitor = executor.spawn_monitor(HeavyCalculation::new(100));
/// let result = executor.run_fiber(monitor).unwrap();
/// assert_eq!(result, Ok(11));
/// # }
/// ```
pub fn yield_poll<T, E>() -> Poll<T, E> {
    with_current_context(|mut context| context.park());
    Ok(Async::NotReady)
}
