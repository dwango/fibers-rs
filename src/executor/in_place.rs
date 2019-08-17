// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use futures::Future;
use std::io;
use std::time;

use super::Executor;
use fiber::{self, Spawn};
use io::poll;

/// An executor that executes spawned fibers and I/O event polling on current thread.
///
/// # Examples
///
/// An example to calculate fibonacci numbers:
///
/// ```
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Spawn, Executor, InPlaceExecutor};
/// use futures::{Async, Future};
///
/// fn fib<H: Spawn + Clone>(n: usize, handle: H) -> Box<dyn Future<Item=usize, Error=()> + Send> {
///     if n < 2 {
///         Box::new(futures::finished(n))
///     } else {
///         let f0 = handle.spawn_monitor(fib(n - 1, handle.clone()));
///         let f1 = handle.spawn_monitor(fib(n - 2, handle.clone()));
///         Box::new(f0.join(f1).map(|(a0, a1)| a0 + a1).map_err(|_| ()))
///     }
/// }
///
/// # fn main() {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let mut monitor = executor.spawn_monitor(fib(7, executor.handle()));
/// loop {
///     if let Async::Ready(answer) = monitor.poll().unwrap() {
///         assert_eq!(answer, 13);
///         return;
///     } else {
///         executor.run_once().unwrap();
///     }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct InPlaceExecutor {
    scheduler: fiber::Scheduler,
    poller: poll::Poller,
}
impl InPlaceExecutor {
    /// Creates a new instance of `InPlaceExecutor`.
    pub fn new() -> io::Result<Self> {
        let poller = poll::Poller::new()?;
        Ok(InPlaceExecutor {
            scheduler: fiber::Scheduler::new(poller.handle()),
            poller,
        })
    }
}
impl Executor for InPlaceExecutor {
    type Handle = InPlaceExecutorHandle;
    fn handle(&self) -> Self::Handle {
        InPlaceExecutorHandle {
            scheduler: self.scheduler.handle(),
        }
    }
    fn run_once(&mut self) -> io::Result<()> {
        self.scheduler.run_once(false);
        self.poller.poll(Some(time::Duration::from_millis(1)))?;
        Ok(())
    }
}
impl Spawn for InPlaceExecutor {
    fn spawn_boxed(&self, fiber: Box<dyn Future<Item = (), Error = ()> + Send>) {
        self.handle().spawn_boxed(fiber)
    }
}

/// A handle of an `InPlaceExecutor` instance.
#[derive(Debug, Clone)]
pub struct InPlaceExecutorHandle {
    scheduler: fiber::SchedulerHandle,
}
impl Spawn for InPlaceExecutorHandle {
    fn spawn_boxed(&self, fiber: Box<dyn Future<Item = (), Error = ()> + Send>) {
        self.scheduler.spawn_boxed(fiber)
    }
}
