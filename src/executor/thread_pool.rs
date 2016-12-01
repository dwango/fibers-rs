use std::io;
use std::time;
use std::thread;
use std::sync::mpsc as std_mpsc;
use num_cpus;
use futures::{Async, Future};

use fiber::{self, Spawn};
use io::poll;
use sync::oneshot::{self, Link};
use super::Executor;

/// Current implmentation is very naive ...
#[derive(Debug)]
pub struct ThreadPoolExecutor {
    pool: SchedulerPool,
    pollers: PollerPool,
    spawn_rx: std_mpsc::Receiver<fiber::Task>,
    spawn_tx: std_mpsc::Sender<fiber::Task>,
    round: usize,
    steps: usize,
}
impl ThreadPoolExecutor {
    pub fn new() -> io::Result<Self> {
        Self::with_thread_count(num_cpus::get() * 2)
    }
    pub fn with_thread_count(count: usize) -> io::Result<Self> {
        assert!(count > 0);
        let pollers = PollerPool::new(count)?;
        let schedulers = SchedulerPool::new(&pollers);
        let (tx, rx) = std_mpsc::channel();
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
        ThreadPoolExecutorHandle { spawn_tx: self.spawn_tx.clone() }
    }
    fn run_once(&mut self) -> io::Result<()> {
        match self.spawn_rx.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {
                thread::sleep(time::Duration::from_millis(1));
            }
            Err(std_mpsc::TryRecvError::Disconnected) => unreachable!(),
            Ok(task) => {
                let i = self.round % self.pool.schedulers.len();
                self.pool.schedulers[i].spawn_future(task.0);
                self.round = self.round.wrapping_add(1);
            }
        }
        self.steps = self.steps.wrapping_add(1);
        let i = self.steps % self.pool.schedulers.len();
        if let Err(_) = self.pool.links[i].poll() {
            Err(io::Error::new(io::ErrorKind::Other,
                               format!("The {}-th scheduler thread is aborted", i)))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThreadPoolExecutorHandle {
    spawn_tx: std_mpsc::Sender<fiber::Task>,
}
impl Spawn for ThreadPoolExecutorHandle {
    fn spawn<F>(&self, future: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        let _ = self.spawn_tx.send(fiber::Task(future.boxed()));
    }
}

#[derive(Debug)]
struct PollerPool {
    pollers: Vec<poll::PollerHandle>,
    links: Vec<Link<io::Error>>,
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
    links: Vec<Link<()>>,
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
                    if !scheduler.run_once() {
                        thread::sleep(time::Duration::from_millis(1));
                    }
                }
            });
        }
        SchedulerPool {
            schedulers: schedulers,
            links: links,
        }
    }
}
