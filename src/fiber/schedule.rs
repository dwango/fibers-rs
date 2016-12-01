use std::sync::atomic;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc as std_mpsc;
use std::cell::RefCell;
use futures::{self, Future, IntoFuture};

use internal::io_poll as poll;
use fiber;

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
    request_tx: RequestSender,
    request_rx: RequestReceiver,
    poller_pool: poll::PollerPoolHandle,
}
impl Scheduler {
    pub fn new(poller_pool: poll::PollerPoolHandle) -> Self {
        let (request_tx, request_rx) = std_mpsc::channel();
        Scheduler {
            scheduler_id: NEXT_SCHEDULER_ID.fetch_add(1, atomic::Ordering::SeqCst),
            next_fiber_id: 0,
            fibers: HashMap::new(),
            run_queue: VecDeque::new(),
            request_tx: request_tx,
            request_rx: request_rx,
            poller_pool: poller_pool,
        }
    }
    pub fn scheduler_id(&self) -> SchedulerId {
        self.scheduler_id
    }
    pub fn run_queue_len(&self) -> usize {
        self.run_queue.len()
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
    fn spawn_fiber(&mut self, task: fiber::Task) {
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
                        scheduler.poller = self.poller_pool.allocate_poller();
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
        let _ = self.request_tx.send(Request::Spawn(fiber::Task(f)));
    }
    pub fn wakeup(&self, fiber_id: fiber::FiberId) {
        let _ = self.request_tx.send(Request::WakeUp(fiber_id));
    }
}

#[derive(Debug)]
pub struct CurrentScheduler {
    pub id: SchedulerId,
    pub handle: SchedulerHandle,
    pub poller: poll::PollerHandle,
}

#[derive(Debug)]
pub struct Context {
    pub scheduler: Option<CurrentScheduler>,
    pub fiber: Option<*mut fiber::FiberState>,
}
impl Context {
    pub fn new() -> Self {
        Context {
            scheduler: None,
            fiber: None,
        }
    }
    pub fn switch(&mut self, scheduler: &Scheduler) {
        self.scheduler = Some(CurrentScheduler {
            id: scheduler.scheduler_id,
            handle: scheduler.handle(),
            poller: scheduler.poller_pool.allocate_poller(),
        })
    }
    pub fn with_current_ref<F, T>(f: F) -> T
        where F: FnOnce(&Context) -> T
    {
        CURRENT_CONTEXT.with(|context| f(&*context.borrow()))
    }
    pub fn with_current_mut<F, T>(f: F) -> T
        where F: FnOnce(&mut Context) -> T
    {
        CURRENT_CONTEXT.with(|context| f(&mut *context.borrow_mut()))
    }
    pub fn fiber_mut(&self) -> Option<&mut fiber::FiberState> {
        // TODO: &mut self
        self.fiber.map(|fiber| unsafe { &mut *fiber })
    }
}

#[derive(Debug)]
pub enum Request {
    Spawn(fiber::Task),
    WakeUp(fiber::FiberId),
}
