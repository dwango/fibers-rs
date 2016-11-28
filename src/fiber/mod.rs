use std::fmt;
use futures::{Async, Future, BoxFuture};

pub use self::schedule::Scheduler;

mod schedule;

pub type FiberId = usize;

pub type FiberFuture = BoxFuture<(), ()>;

pub struct FiberState {
    task: FiberFuture,
    pub in_run_queue: bool,
}
impl FiberState {
    pub fn new(task: FiberFuture) -> Self {
        FiberState {
            task: task,
            in_run_queue: false,
        }
    }
    pub fn run_once(&mut self) -> bool {
        if let Ok(Async::NotReady) = self.task.poll() {
            false
        } else {
            true
        }
    }
    pub fn is_runnable(&self) -> bool {
        true
    }
}
impl fmt::Debug for FiberState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FiberState {{ task: .. }}")
    }
}
