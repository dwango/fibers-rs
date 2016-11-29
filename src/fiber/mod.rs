use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicUsize};
use futures::{Async, Future, BoxFuture};

pub use self::execute::{Executor, ExecutorHandle, Builder};
pub use self::schedule::{Scheduler, SchedulerHandle};

use io::poll;

mod execute;
mod schedule;

pub type FiberId = usize;

pub type FiberFuture = BoxFuture<(), ()>;

pub struct Task(FiberFuture);
impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Task(_)")
    }
}

#[derive(Debug)]
pub struct FiberState {
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

pub fn park() -> Option<Unpark> {
    schedule::Context::with_current_mut(|context| {
        context.scheduler.as_ref().and_then(|scheduler| {
            context.fiber_mut().map(|fiber| fiber.park(scheduler.id, scheduler.handle.clone()))
        })
    })
}
pub fn context_id() -> Option<(schedule::SchedulerId, FiberId)> {
    schedule::Context::with_current_ref(|context| {
        context.scheduler
            .as_ref()
            .and_then(|scheduler| context.fiber_mut().map(|fiber| (scheduler.id, fiber.fiber_id)))
    })
}
pub fn with_poller<F, T>(f: F) -> Option<T>
    where F: FnOnce(&mut poll::PollerHandle) -> T
{
    schedule::Context::with_current_mut(|context| {
        context.scheduler.as_mut().map(|s| f(&mut s.poller))
    })
}

#[derive(Debug)]
pub struct Unpark {
    fiber_id: FiberId,
    unparks: Arc<AtomicUsize>,
    scheduler_id: schedule::SchedulerId,
    scheduler: schedule::SchedulerHandle,
}
impl Unpark {
    pub fn context_id(&self) -> (schedule::SchedulerId, FiberId) {
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
