// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use futures::Future;
use futures03::compat::Future01CompatExt;
use futures03::executor::{LocalPool as LocalPool03, LocalSpawner as LocalSpawner03};
use futures03::task::{FutureObj as FutureObj03, Spawn as _};
use futures03::FutureExt;
use std::io;

use super::Executor;
use fiber::Spawn;

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
/// ```
#[derive(Debug)]
pub struct InPlaceExecutor {
    pool: LocalPool03,
}
impl InPlaceExecutor {
    /// Creates a new instance of `InPlaceExecutor`.
    pub fn new() -> io::Result<Self> {
        let pool = LocalPool03::new();
        Ok(InPlaceExecutor { pool })
    }
}
impl Executor for InPlaceExecutor {
    type Handle = InPlaceExecutorHandle;
    fn handle(&self) -> Self::Handle {
        InPlaceExecutorHandle {
            spawner: self.pool.spawner(),
        }
    }
    fn run_once(&mut self) -> io::Result<()> {
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
    spawner: LocalSpawner03,
}

// TODO: don't rely on this
unsafe impl Send for InPlaceExecutorHandle {}
impl Spawn for InPlaceExecutorHandle {
    fn spawn_boxed(&self, fiber: Box<dyn Future<Item = (), Error = ()> + Send>) {
        let future03 = fiber.compat().map(|_result| ());
        let futureobj03: FutureObj03<()> = Box::new(future03).into();
        // TODO: proper error handlings
        self.spawner.spawn_obj(futureobj03).unwrap();
    }
}
