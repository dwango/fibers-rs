// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! The `Executor` trait and its implementations.
use std::io;
use futures::{Async, Future};

pub use self::in_place::{InPlaceExecutor, InPlaceExecutorHandle};
pub use self::thread_pool::{ThreadPoolExecutor, ThreadPoolExecutorHandle};

use fiber::Spawn;
use sync::oneshot::{Monitor, MonitorError};

mod in_place;
mod thread_pool;

mod tokio;

/// The `Executor` trait allows for spawning and executing fibers.
pub trait Executor: Sized {
    /// The handle type of the executor.
    type Handle: Spawn + Clone + Send + 'static;

    /// Returns the handle of the executor.
    fn handle(&self) -> Self::Handle;

    /// Runs one one unit of works.
    fn run_once(&mut self) -> io::Result<()>;

    /// Runs until the monitored fiber exits.
    fn run_fiber<T, E>(&mut self,
                       monitor: Monitor<T, E>)
                       -> io::Result<Result<T, MonitorError<E>>> {
        self.run_future(monitor)
    }

    /// Runs until the future is ready.
    fn run_future<F: Future>(&mut self, mut future: F) -> io::Result<Result<F::Item, F::Error>> {
        loop {
            match future.poll() {
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
