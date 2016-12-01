/// Executors
use std::io;
use std::time;
use futures::{self, Future, IntoFuture};

pub use self::in_place::{InPlaceExecutor, InPlaceExecutorHandle};

use sync::oneshot::{self, Monitor};

mod in_place;

// TODO: move
pub trait Spawn {
    fn spawn<F>(&self, future: F) where F: Future<Item = (), Error = ()> + Send + 'static;
    fn spawn_fn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.spawn(futures::lazy(|| f()))
    }
    fn spawn_monitor<F, T, E>(&self, f: F) -> Monitor<T, E>
        where F: Future<Item = T, Error = E> + Send + 'static,
              T: Send + 'static,
              E: Send + 'static
    {
        let (monitored, monitor) = oneshot::monitor();
        self.spawn(f.then(move |r| Ok(monitored.exit(r))));
        monitor
    }
}

pub trait Executor: Sized {
    type Handle: Spawn + Clone + Send + 'static;

    fn handle(&self) -> Self::Handle;

    fn spawn<F>(&self, future: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        self.handle().spawn(future)
    }
    fn spawn_fn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.handle().spawn_fn(f)
    }
    fn spawn_monitor<F, T, E>(&self, f: F) -> Monitor<T, E>
        where F: Future<Item = T, Error = E> + Send + 'static,
              T: Send + 'static,
              E: Send + 'static
    {
        self.handle().spawn_monitor(f)
    }

    fn run_once(&mut self, sleep_duration_if_idle: Option<time::Duration>) -> io::Result<()>;
    fn run(mut self) -> io::Result<()> {
        loop {
            self.run_once(Some(time::Duration::from_millis(1)))?
        }
    }
}
