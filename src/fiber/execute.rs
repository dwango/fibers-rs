use std::io;
use std::time;
use std::thread;
use std::sync::mpsc as std_mpsc;
use rand;
use num_cpus;
use futures::{self, Async, Future, IntoFuture};

use io::poll;
use sync::oneshot::{self, Link};
use fiber;
use super::{Scheduler, SchedulerHandle};

#[derive(Debug)]
pub struct Builder {
    scheduler_threads: usize,
    io_poller_threads: usize,
}
impl Builder {
    pub fn new() -> Self {
        Builder {
            scheduler_threads: num_cpus::get() * 2,
            io_poller_threads: num_cpus::get(),
        }
    }
    pub fn scheduler_threads(&mut self, count: usize) -> &mut Self {
        assert!(count > 0);
        self.scheduler_threads = count;
        self
    }
    pub fn io_poller_threads(&mut self, count: usize) -> &mut Self {
        assert!(count > 0);
        self.io_poller_threads = count;
        self
    }
    pub fn build(&self) -> io::Result<Executor> {
        let poller_pool = poll::PollerPool::with_thread_count(self.io_poller_threads)?;
        let mut links = Vec::new();
        let mut schedulers = Vec::new();
        for _ in 0..self.scheduler_threads {
            let (link0, mut link1) = oneshot::link();
            let mut scheduler = Scheduler::new(poller_pool.handle());
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

        let (tx, rx) = std_mpsc::channel();
        Ok(Executor {
            links: links,
            schedulers: schedulers,
            request_tx: tx,
            request_rx: rx,
        })
    }
}

#[derive(Debug)]
enum Request {
    Spawn(fiber::Task),
}

type RequestSender = std_mpsc::Sender<Request>;
type RequestReceiver = std_mpsc::Receiver<Request>;

#[derive(Debug)]
pub struct Executor {
    links: Vec<Link<io::Error>>,
    schedulers: Vec<SchedulerHandle>,
    request_tx: RequestSender,
    request_rx: RequestReceiver,
}
impl Executor {
    pub fn new() -> io::Result<Self> {
        Builder::new().build()
    }
    pub fn scheduler_thread_count(&self) -> usize {
        self.schedulers.len()
    }
    pub fn handle(&self) -> ExecutorHandle {
        ExecutorHandle { request_tx: self.request_tx.clone() }
    }
    pub fn spawn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.handle().spawn(f);
    }
    pub fn run(mut self) -> io::Result<()> {
        loop {
            if !self.run_once()? {
                thread::sleep(time::Duration::from_millis(1));
            }
        }
    }
    pub fn run_once(&mut self) -> io::Result<bool> {
        let mut did_something = false;
        match self.request_rx.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => unreachable!(),
            Ok(request) => {
                did_something = true;
                self.handle_request(request)
            }
        }
        Ok(did_something)
    }
    fn handle_request(&mut self, request: Request) {
        match request {
            Request::Spawn(task) => {
                self.select_scheduler().spawn_future(task.0);
            }
        }
    }
    fn select_scheduler(&self) -> &SchedulerHandle {
        let i = rand::random::<usize>() % self.schedulers.len();
        &self.schedulers[i]
    }
}

#[derive(Debug, Clone)]
pub struct ExecutorHandle {
    request_tx: RequestSender,
}
impl ExecutorHandle {
    pub fn spawn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        let request = Request::Spawn(fiber::Task(futures::lazy(f).boxed()));
        let _ = self.request_tx.send(request);
    }
}

// TODO: improve selection logic and scalability
