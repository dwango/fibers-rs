use std::sync::atomic;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc as std_mpsc;
use std::cell::RefCell;
use futures::{self, Future, IntoFuture};

use fiber;
use timer;
use collections::RemovableHeap;

lazy_static! {
    static ref NEXT_SCHEDULER_ID: atomic::AtomicUsize = {
        atomic::AtomicUsize::new(0)
    };
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Context> = {
        RefCell::new(Context::new())
    };
}

type RequestSender = std_mpsc::Sender<Request>;
type RequestReceiver = std_mpsc::Receiver<Request>;

pub type SchedulerId = usize;

#[derive(Debug)]
pub struct Scheduler {
    scheduler_id: SchedulerId,
    next_fiber_id: fiber::FiberId,
    fibers: HashMap<fiber::FiberId, fiber::FiberState>,
    run_queue: VecDeque<fiber::FiberId>,
    timeout_queue: RemovableHeap<timer::Timeout>,
    request_tx: RequestSender,
    request_rx: RequestReceiver,
}
impl Scheduler {
    pub fn new() -> Self {
        let (request_tx, request_rx) = std_mpsc::channel();
        Scheduler {
            scheduler_id: NEXT_SCHEDULER_ID.fetch_add(1, atomic::Ordering::SeqCst),
            next_fiber_id: 0,
            fibers: HashMap::new(),
            run_queue: VecDeque::new(),
            timeout_queue: RemovableHeap::new(),
            request_tx: request_tx,
            request_rx: request_rx,
        }
    }
    pub fn scheduler_id(&self) -> SchedulerId {
        self.scheduler_id
    }
    pub fn run_queue_len(&self) -> usize {
        self.run_queue.len()
    }
    pub fn timeout_queue_len(&self) -> usize {
        self.timeout_queue.len()
    }
    pub fn fiber_count(&self) -> usize {
        self.fibers.len()
    }
    pub fn handle(&self) -> SchedulerHandle {
        SchedulerHandle { request_tx: self.request_tx.clone() }
    }
    pub fn spawn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.handle().spawn(f);
    }
    pub fn run_once(&mut self, non_blocking: bool) -> bool {
        let mut did_something = false;

        // Request
        let request = if !non_blocking && self.run_queue.len() == 0 &&
                         self.timeout_queue.len() == 0 {
            Some(assert_ok!(self.request_rx.recv()))
        } else {
            match self.request_rx.try_recv() {
                Err(std_mpsc::TryRecvError::Empty) => None,
                Err(std_mpsc::TryRecvError::Disconnected) => unreachable!(),
                Ok(r) => Some(r),
            }
        };
        if let Some(request) = request {
            did_something = true;
            self.handle_request(request);
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
            Request::Spawn(fiber) => self.spawn_fiber(fiber),
            Request::Park(fiber_id) => {
                if let Some(fiber) = self.fibers.get_mut(&fiber_id) {
                    fiber.park();
                }
            }
            Request::Unpark(fiber_id) => {
                let does_exist = self.fibers.get_mut(&fiber_id).map_or(false, |fiber| {
                    fiber.unpark();
                    true
                });
                if does_exist {
                    self.schedule(fiber_id);
                }
            }
        }
    }
    fn spawn_fiber(&mut self, fiber: fiber::FiberState) {
        let fiber_id = self.next_fiber_id();
        self.fibers.insert(fiber_id, fiber);
        self.schedule(fiber_id);
    }
    fn run_fiber(&mut self, fiber_id: fiber::FiberId) {
        let finished;
        let is_runnable = {
            CURRENT_CONTEXT.with(|context| {
                let mut context = context.borrow_mut();
                if context.scheduler_id != Some(self.scheduler_id) {
                    context.switch(self);
                }
                assert!(context.fiber_id.is_none(), "Nested schedulers");
                context.fiber_id = Some(fiber_id);
            });
            let fiber = assert_some!(self.fibers.get_mut(&fiber_id));
            finished = fiber.run_once();
            CURRENT_CONTEXT.with(|context| {
                context.borrow_mut().fiber_id = None;
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
        if fiber.in_run_queue {
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

#[derive(Debug, Clone)]
pub struct SchedulerHandle {
    request_tx: RequestSender,
}
impl SchedulerHandle {
    pub fn spawn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.spawn_future(futures::lazy(f).boxed())
    }
    pub fn spawn_future(&self, f: fiber::FiberFuture) {
        let _ = self.request_tx.send(Request::Spawn(fiber::FiberState::new(f)));
    }
    pub fn park(&self, fiber_id: fiber::FiberId) -> fiber::Unpark {
        let _ = self.request_tx.send(Request::Park(fiber_id));
        fiber::Unpark::new(self.clone(), fiber_id)
    }
    pub fn unpark(&self, fiber_id: fiber::FiberId) {
        let _ = self.request_tx.send(Request::Unpark(fiber_id));
    }
}

#[derive(Debug)]
pub struct Context {
    scheduler_id: Option<SchedulerId>,
    pub scheduler: SchedulerHandle,
    pub fiber_id: Option<fiber::FiberId>,
}
impl Context {
    pub fn new() -> Self {
        let (tx, _) = std_mpsc::channel();
        Context {
            scheduler_id: None,
            scheduler: SchedulerHandle { request_tx: tx }, // dummy
            fiber_id: None,
        }
    }
    pub fn switch(&mut self, scheduler: &Scheduler) {
        self.scheduler_id = Some(scheduler.scheduler_id);
        self.scheduler = scheduler.handle();
    }
    pub fn with_current_ref<F, T>(f: F) -> T
        where F: FnOnce(&Context) -> T
    {
        CURRENT_CONTEXT.with(|context| f(&*context.borrow()))
    }
}

#[derive(Debug)]
pub enum Request {
    Spawn(fiber::FiberState),
    Park(fiber::FiberId),
    Unpark(fiber::FiberId),
}
