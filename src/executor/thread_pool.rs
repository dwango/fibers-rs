// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use std::io;
use std::time;
use std::thread;
use std::sync::mpsc::TryRecvError;
use futures::{Async, Future};
use nbchan::mpsc as nb_mpsc;
use num_cpus;

use fiber::{self, Spawn};
use io::poll;
use sync::oneshot::{self, Link};
use fiber::Task;
use super::Executor;

/// An executor that executes spawned fibers on pooled threads.
///
/// # Examples
///
/// An example to calculate fibonacci numbers:
///
/// ```
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Spawn, Executor, ThreadPoolExecutor};
/// use futures::{Async, Future};
///
/// fn fib<H: Spawn + Clone>(n: usize, handle: H) -> Box<Future<Item=usize, Error=()> + Send> {
///     if n < 2 {
///         Box::new(futures::finished(n))
///     } else {
///         let f0 = handle.spawn_monitor(fib(n - 1, handle.clone()));
///         let f1 = handle.spawn_monitor(fib(n - 2, handle.clone()));
///         Box::new(f0.join(f1).map(|(a0, a1)| a0 + a1).map_err(|_| ()))
///     }
/// }
///
/// # fn main() {
/// let mut executor = ThreadPoolExecutor::new().unwrap();
/// let monitor = executor.spawn_monitor(fib(7, executor.handle()));
/// let answer = executor.run_fiber(monitor).unwrap();
/// assert_eq!(answer, Ok(13));
/// # }
/// ```
#[derive(Debug)]
pub struct ThreadPoolExecutor {
    pool: SchedulerPool,
    pollers: PollerPool,
    spawn_rx: nb_mpsc::Receiver<Task>,
    spawn_tx: nb_mpsc::Sender<Task>,
    round: usize,
    steps: usize,
}
impl ThreadPoolExecutor {
    /// Creates a new instance of `ThreadPoolExecutor`.
    ///
    /// This is equivalent to `ThreadPoolExecutor::with_thread_count(num_cpus::get() * 2)`.
    pub fn new() -> io::Result<Self> {
        Self::with_thread_count(num_cpus::get() * 2)
    }

    /// Creates a new instance of `ThreadPoolExecutor` with the specified size of thread pool.
    ///
    /// # Implementation Details
    ///
    /// Note that current implementation is very naive and
    /// should be improved in future releases.
    ///
    /// Internally, `count` threads are assigned to each of
    /// the scheduler (i.e., `fibers::fiber::Scheduler`) and
    /// the I/O poller (i.e., `fibers::io::poll::Poller`).
    ///
    /// When `spawn` function is called, the executor will assign a scheduler (thread)
    /// for the fiber in simple round robin fashion.
    ///
    /// If any of those threads are aborted, the executor will return an error as
    /// a result of `run_once` method call after that.
    pub fn with_thread_count(count: usize) -> io::Result<Self> {
        assert!(count > 0);
        let pollers = PollerPool::new(count)?;
        let schedulers = SchedulerPool::new(&pollers);
        let (tx, rx) = nb_mpsc::channel();
        Ok(ThreadPoolExecutor {
            pool: schedulers,
            pollers: pollers,
            spawn_tx: tx,
            spawn_rx: rx,
            round: 0,
            steps: 0,
        })
    }
}
impl Executor for ThreadPoolExecutor {
    type Handle = ThreadPoolExecutorHandle;
    fn handle(&self) -> Self::Handle {
        ThreadPoolExecutorHandle {
            spawn_tx: self.spawn_tx.clone(),
        }
    }
    fn run_once(&mut self) -> io::Result<()> {
        match self.spawn_rx.try_recv() {
            Err(TryRecvError::Empty) => {
                thread::sleep(time::Duration::from_millis(1));
            }
            Err(TryRecvError::Disconnected) => unreachable!(),
            Ok(task) => {
                let i = self.round % self.pool.schedulers.len();
                self.pool.schedulers[i].spawn_boxed(task.0);
                self.round = self.round.wrapping_add(1);
            }
        }
        self.steps = self.steps.wrapping_add(1);
        let i = self.steps % self.pool.schedulers.len();
        if self.pool.links[i].poll().is_err() {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("The {}-th scheduler thread is aborted", i),
            ))
        } else {
            Ok(())
        }
    }
}
impl Spawn for ThreadPoolExecutor {
    fn spawn_boxed(&self, fiber: Box<Future<Item = (), Error = ()> + Send>) {
        self.handle().spawn_boxed(fiber)
    }
}

/// A handle of a `ThreadPoolExecutor` instance.
#[derive(Debug, Clone)]
pub struct ThreadPoolExecutorHandle {
    spawn_tx: nb_mpsc::Sender<Task>,
}
impl Spawn for ThreadPoolExecutorHandle {
    fn spawn_boxed(&self, fiber: Box<Future<Item = (), Error = ()> + Send>) {
        let _ = self.spawn_tx.send(Task(fiber));
    }
}

#[derive(Debug)]
struct PollerPool {
    pollers: Vec<poll::PollerHandle>,
    links: Vec<Link<(), io::Error>>,
}
impl PollerPool {
    pub fn new(pool_size: usize) -> io::Result<Self> {
        let mut pollers = Vec::new();
        let mut links = Vec::new();
        for _ in 0..pool_size {
            let (link0, mut link1) = oneshot::link();
            let mut poller = poll::Poller::new()?;
            links.push(link0);
            pollers.push(poller.handle());
            thread::spawn(move || {
                while let Ok(Async::NotReady) = link1.poll() {
                    let timeout = time::Duration::from_millis(1);
                    if let Err(e) = poller.poll(Some(timeout)) {
                        link1.exit(Err(e));
                        return;
                    }
                }
            });
        }
        Ok(PollerPool {
            pollers: pollers,
            links: links,
        })
    }
}

#[derive(Debug)]
struct SchedulerPool {
    schedulers: Vec<fiber::SchedulerHandle>,
    links: Vec<Link<(), ()>>,
}
impl SchedulerPool {
    pub fn new(poller_pool: &PollerPool) -> Self {
        let mut schedulers = Vec::new();
        let mut links = Vec::new();
        for poller in &poller_pool.pollers {
            let (link0, mut link1) = oneshot::link();
            let mut scheduler = fiber::Scheduler::new(poller.clone());
            links.push(link0);
            schedulers.push(scheduler.handle());
            thread::spawn(move || {
                while let Ok(Async::NotReady) = link1.poll() {
                    scheduler.run_once(true);
                }
            });
        }
        SchedulerPool {
            schedulers: schedulers,
            links: links,
        }
    }
}
