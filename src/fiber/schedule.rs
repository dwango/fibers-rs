use std::sync::atomic;
use std::collections::{HashMap, VecDeque};

use fiber;
use timer;
use collections::RemovableHeap;

const SENTINEL_SCHEDULER_ID: SchedulerId = 0;

lazy_static! {
    static ref NEXT_SCHEDULER_ID: atomic::AtomicUsize = {
        atomic::AtomicUsize::new(SENTINEL_SCHEDULER_ID + 1)
    };
}

pub type SchedulerId = usize;

#[derive(Debug)]
pub struct Scheduler {
    scheduler_id: SchedulerId,
    next_fiber_id: fiber::FiberId,
    fibers: HashMap<fiber::FiberId, fiber::FiberState>,
    run_queue: VecDeque<fiber::FiberId>,
    timeout_queue: RemovableHeap<timer::Timeout>,
}
impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            scheduler_id: NEXT_SCHEDULER_ID.fetch_add(1, atomic::Ordering::SeqCst),
            next_fiber_id: 0,
            fibers: HashMap::new(),
            run_queue: VecDeque::new(),
            timeout_queue: RemovableHeap::new(),
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
}
