//! The `Executor` trait and its implementations.
use std::io;
use futures::{Async, Future, IntoFuture};

pub use self::in_place::{InPlaceExecutor, InPlaceExecutorHandle};
pub use self::thread_pool::{ThreadPoolExecutor, ThreadPoolExecutorHandle};

use fiber::Spawn;
use sync::oneshot::{Monitor, MonitorError};

mod in_place;
mod thread_pool;

/// The `Executor` trait allows for spawning and executing fibers.
pub trait Executor: Sized {
    /// The handle type of the executor.
    type Handle: Spawn + Clone + Send + 'static;

    /// Returns the handle of the executor.
    fn handle(&self) -> Self::Handle;

    /// Spawns a fiber which will execute given future.
    fn spawn<F>(&self, future: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        self.handle().spawn(future)
    }

    /// Equivalent to `self.spawn(futures::lazy(|| f()))`.
    fn spawn_fn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.handle().spawn_fn(f)
    }

    /// Spawns a fiber and returns a future to monitor it's execution result.
    fn spawn_monitor<F, T, E>(&self, f: F) -> Monitor<T, E>
        where F: Future<Item = T, Error = E> + Send + 'static,
              T: Send + 'static,
              E: Send + 'static
    {
        self.handle().spawn_monitor(f)
    }

    /// Runs one one unit of works.
    fn run_once(&mut self) -> io::Result<()>;

    /// Runs until the monitored fiber exits.
    fn run_fiber<T, E>(&mut self,
                       mut monitor: Monitor<T, E>)
                       -> io::Result<Result<T, MonitorError<E>>> {
        loop {
            match monitor.poll() {
                Err(e) => return Ok(Err(e)),
                Ok(Async::Ready(v)) => return Ok(Ok(v)),
                Ok(Async::NotReady) => {}
            }
            self.run_once()?;
        }
    }

    /// Runs infinitely until an error happens.
    fn run(mut self) -> io::Result<()> {
        loop {
            self.run_once()?
        }
    }
}
