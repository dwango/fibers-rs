// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use std::sync::atomic;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc as std_mpsc;
use std::cell::RefCell;
use futures::BoxFuture;

use fiber;
use internal::fiber::Task;
use internal::io_poll as poll;
use super::{Spawn, FiberState};

lazy_static! {
    static ref NEXT_SCHEDULER_ID: atomic::AtomicUsize = {
        atomic::AtomicUsize::new(0)
    };
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<InnerContext> = {
        RefCell::new(InnerContext::new())
    };
}

type RequestSender = std_mpsc::Sender<Request>;
type RequestReceiver = std_mpsc::Receiver<Request>;

/// The identifier of a scheduler.
pub type SchedulerId = usize;

/// Scheduler of spawned fibers.
///
/// Scheduler manages spawned fibers state.
/// If a fiber is in runnable state (e.g., not waiting for I/O events),
/// the scheduler will push the fiber in it's run queue.
/// When `run_once` method is called, the first fiber (i.e., future) in the queue
/// will be poped and executed (i.e., `Future::poll` method is called).
/// If the future of a fiber moves to readied state,
/// it will be removed from the scheduler.

/// For efficiency reasons, it is recommended to run a scheduler on a dedicated thread.
#[derive(Debug)]
pub struct Scheduler {
    scheduler_id: SchedulerId,
    next_fiber_id: fiber::FiberId,
    fibers: HashMap<fiber::FiberId, fiber::FiberState>,
    run_queue: VecDeque<fiber::FiberId>,
    request_tx: RequestSender,
    request_rx: RequestReceiver,
    poller: poll::PollerHandle,
}
impl Scheduler {
    /// Creates a new scheduler instance.
    pub fn new(poller: poll::PollerHandle) -> Self {
        let (request_tx, request_rx) = std_mpsc::channel();
        Scheduler {
            scheduler_id: NEXT_SCHEDULER_ID.fetch_add(1, atomic::Ordering::SeqCst),
            next_fiber_id: 0,
            fibers: HashMap::new(),
            run_queue: VecDeque::new(),
            request_tx: request_tx,
            request_rx: request_rx,
            poller: poller,
        }
    }

    /// Returns the identifier of this scheduler.
    pub fn scheduler_id(&self) -> SchedulerId {
        self.scheduler_id
    }

    /// Returns the length of the run queue of this scheduler.
    pub fn run_queue_len(&self) -> usize {
        self.run_queue.len()
    }

    /// Returns the count of alive fibers (i.e., not readied futures) in this scheduler.
    pub fn fiber_count(&self) -> usize {
        self.fibers.len()
    }

    /// Returns a handle of this scheduler.
    pub fn handle(&self) -> SchedulerHandle {
        SchedulerHandle { request_tx: self.request_tx.clone() }
    }

    /// Runs one unit of works.
    ///
    /// If the scheduler does something, it will return `true` otherwise `false`.
    pub fn run_once(&mut self) -> bool {
        let mut did_something = false;

        // Request
        match self.request_rx.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => unreachable!(),
            Ok(request) => {
                did_something = true;
                self.handle_request(request);
            }
        }

        // Task
        if let Some(fiber_id) = self.next_runnable() {
            did_something = true;
            self.run_fiber(fiber_id);
        }

        did_something
    }

    fn handle_request(&mut self, request: Request) {
        match request {
            Request::Spawn(task) => self.spawn_fiber(task),
            Request::WakeUp(fiber_id) => {
                if self.fibers.contains_key(&fiber_id) {
                    self.schedule(fiber_id);
                }
            }
        }
    }
    fn spawn_fiber(&mut self, task: Task) {
        let fiber_id = self.next_fiber_id();
        self.fibers.insert(fiber_id, fiber::FiberState::new(fiber_id, task));
        self.schedule(fiber_id);
    }
    fn run_fiber(&mut self, fiber_id: fiber::FiberId) {
        let finished;
        let is_runnable = {
            CURRENT_CONTEXT.with(|context| {
                let mut context = context.borrow_mut();
                if context.scheduler.as_ref().map_or(true, |s| s.id != self.scheduler_id) {
                    context.switch(self);
                }
                {
                    let scheduler = assert_some!(context.scheduler.as_mut());
                    if !scheduler.poller.is_alive() {
                        // TODO: Return `Err(io::Error)` to caller and
                        // handle the error in upper layers
                        panic!("Poller is down");
                    }
                }
                assert!(context.fiber.is_none(), "Nested schedulers");
                let fiber = assert_some!(self.fibers.get_mut(&fiber_id));
                context.fiber = Some(fiber as _);
            });
            let fiber = assert_some!(self.fibers.get_mut(&fiber_id));
            finished = fiber.run_once();
            CURRENT_CONTEXT.with(|context| {
                context.borrow_mut().fiber = None;
            });
            fiber.is_runnable()
        };
        if finished {
            self.fibers.remove(&fiber_id);
        } else if is_runnable {
            self.schedule(fiber_id);
        }
    }
    fn next_fiber_id(&mut self) -> fiber::FiberId {
        loop {
            let id = self.next_fiber_id;
            self.next_fiber_id = id.wrapping_add(1);
            if !self.fibers.contains_key(&id) {
                return id;
            }
        }
    }
    fn schedule(&mut self, fiber_id: fiber::FiberId) {
        let fiber = assert_some!(self.fibers.get_mut(&fiber_id));
        if !fiber.in_run_queue {
            self.run_queue.push_back(fiber_id);
            fiber.in_run_queue = true;
        }
    }
    fn next_runnable(&mut self) -> Option<fiber::FiberId> {
        while let Some(fiber_id) = self.run_queue.pop_front() {
            if let Some(fiber) = self.fibers.get_mut(&fiber_id) {
                fiber.in_run_queue = false;
                return Some(fiber_id);
            }
        }
        None
    }
}

/// A handle of a scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerHandle {
    request_tx: RequestSender,
}
impl SchedulerHandle {
    /// Wakes up a specified fiber in the scheduler.
    ///
    /// This forces the fiber to be pushed to the run queue of the scheduler.
    pub fn wakeup(&self, fiber_id: fiber::FiberId) {
        let _ = self.request_tx.send(Request::WakeUp(fiber_id));
    }
}
impl Spawn for SchedulerHandle {
    fn spawn_boxed(&self, fiber: BoxFuture<(), ()>) {
        let _ = self.request_tx.send(Request::Spawn(Task(fiber)));
    }
}

#[derive(Debug)]
pub struct CurrentScheduler {
    pub id: SchedulerId,
    pub handle: SchedulerHandle,
    pub poller: poll::PollerHandle,
}


/// Calls `f` with the current execution context.
///
/// If this function is called on the outside of a fiber, it will ignores `f` and returns `None`.
pub fn with_current_context<F, T>(f: F) -> Option<T>
    where F: FnOnce(Context) -> T
{
    CURRENT_CONTEXT.with(|inner_context| inner_context.borrow_mut().as_context().map(f))
}

/// The execution context of the currently running fiber.
#[derive(Debug)]
pub struct Context<'a> {
    scheduler: &'a mut CurrentScheduler,
    fiber: &'a mut FiberState,
}
impl<'a> Context<'a> {
    /// Returns the identifier of the current exeuction context.
    pub fn context_id(&self) -> super::ContextId {
        (self.scheduler.id, self.fiber.fiber_id)
    }

    /// Parks the current fiber.
    pub fn park(&mut self) -> super::Unpark {
        self.fiber.park(self.scheduler.id, self.scheduler.handle.clone())
    }

    /// Returns the I/O event poller for this context.
    pub fn poller(&mut self) -> &mut poll::PollerHandle {
        &mut self.scheduler.poller
    }
}

// TODO: rename
#[derive(Debug)]
struct InnerContext {
    pub scheduler: Option<CurrentScheduler>,
    fiber: Option<*mut FiberState>,
}
impl InnerContext {
    fn new() -> Self {
        InnerContext {
            scheduler: None,
            fiber: None,
        }
    }
    pub fn switch(&mut self, scheduler: &Scheduler) {
        self.scheduler = Some(CurrentScheduler {
            id: scheduler.scheduler_id,
            handle: scheduler.handle(),
            poller: scheduler.poller.clone(),
        })
    }
    pub fn as_context(&mut self) -> Option<Context> {
        if let Some(scheduler) = self.scheduler.as_mut() {
            if let Some(fiber) = self.fiber {
                let fiber = unsafe { &mut *fiber };
                return Some(Context {
                    scheduler: scheduler,
                    fiber: fiber,
                });
            }
        }
        None
    }
}

#[derive(Debug)]
enum Request {
    Spawn(Task),
    WakeUp(fiber::FiberId),
}
