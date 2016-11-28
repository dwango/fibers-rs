use std::fmt;
use futures::{Async, Future, BoxFuture};

pub use self::schedule::Scheduler;

mod schedule;

pub type FiberId = usize;

pub type FiberFuture = BoxFuture<(), ()>;

struct Task(FiberFuture);
impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Task(_)")
    }
}

#[derive(Debug)]
pub struct FiberState {
    task: Task,
    parks: usize,
    unparks: usize,
    pub in_run_queue: bool,
}
impl FiberState {
    pub fn new(task: FiberFuture) -> Self {
        FiberState {
            task: Task(task),
            parks: 0,
            unparks: 0,
            in_run_queue: false,
        }
    }
    pub fn run_once(&mut self) -> bool {
        if self.unparks > 0 {
            self.parks -= 1;
            self.unparks -= 1;
        }
        if let Ok(Async::NotReady) = self.task.0.poll() {
            false
        } else {
            true
        }
    }
    pub fn is_runnable(&self) -> bool {
        self.parks == 0 || self.unparks > 0
    }
    pub fn park(&mut self) {
        self.parks += 1;
    }
    pub fn unpark(&mut self) {
        assert!(self.unparks < self.parks);
        self.unparks += 1;
    }
}

pub fn park() -> Option<Unpark> {
    schedule::Context::with_current_ref(|context| {
        context.fiber_id.map(|fiber_id| context.scheduler.park(fiber_id))
    })
}

#[derive(Debug)]
pub struct Unpark {
    scheduler: schedule::SchedulerHandle,
    fiber_id: FiberId,
}
impl Unpark {
    pub fn new(scheduler: schedule::SchedulerHandle, fiber_id: FiberId) -> Self {
        Unpark {
            scheduler: scheduler,
            fiber_id: fiber_id,
        }
    }
}
impl Drop for Unpark {
    fn drop(&mut self) {
        self.scheduler.unpark(self.fiber_id);
    }
}
