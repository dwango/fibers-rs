pub use self::schedule::Scheduler;

mod schedule;

pub type FiberId = usize;

#[derive(Debug)]
pub struct FiberState;
