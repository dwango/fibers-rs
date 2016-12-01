use std::io;
use std::time;
use futures::Future;

use fiber::{self, Spawn};
use io::poll;
use super::Executor;

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
/// use futures::{Async, Future, BoxFuture};
///
/// fn fib<H: Spawn + Clone>(n: usize, handle: H) -> BoxFuture<usize, ()> {
///     if n < 2 {
///         futures::finished(n).boxed()
///     } else {
///         let f0 = handle.spawn_monitor(fib(n - 1, handle.clone()));
///         let f1 = handle.spawn_monitor(fib(n - 2, handle.clone()));
///         f0.join(f1).map(|(a0, a1)| a0 + a1).map_err(|_| ()).boxed()
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
            poller: poller,
        })
    }
}
impl Executor for InPlaceExecutor {
    type Handle = InPlaceExecutorHandle;
    fn handle(&self) -> Self::Handle {
        InPlaceExecutorHandle { scheduler: self.scheduler.handle() }
    }
    fn run_once(&mut self) -> io::Result<()> {
        self.scheduler.run_once();
        self.poller.poll(Some(time::Duration::from_millis(1)))?;
        Ok(())
    }
}

/// A handle of an `InPlaceExecutor`.
#[derive(Debug, Clone)]
pub struct InPlaceExecutorHandle {
    scheduler: fiber::SchedulerHandle,
}
impl Spawn for InPlaceExecutorHandle {
    fn spawn<F>(&self, future: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        self.scheduler.spawn_future(future.boxed())
    }
}
