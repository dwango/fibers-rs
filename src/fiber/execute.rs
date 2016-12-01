#![allow(unused_imports, dead_code)]
use std::io;
use std::time;
use std::thread;
use std::sync::mpsc as std_mpsc;
use rand;
use num_cpus;
use futures::{self, Async, Future, IntoFuture};

use internal::io_poll as poll;
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
        panic!()
        // let poller_pool = poll::PollerPool::with_thread_count(self.io_poller_threads)?;
        // let mut links = Vec::new();
        // let mut schedulers = Vec::new();
        // for _ in 0..self.scheduler_threads {
        //     let (link0, mut link1) = oneshot::link();
        //     let mut scheduler = Scheduler::new(poller.handle());
        //     links.push(link0);
        //     schedulers.push(scheduler.handle());
        //     thread::spawn(move || {
        //         while let Ok(Async::NotReady) = link1.poll() {
        //             if !scheduler.run_once() {
        //                 thread::sleep(time::Duration::from_millis(1));
        //             }
        //         }
        //     });
        // }

        // let (tx, rx) = std_mpsc::channel();
        // Ok(Executor {
        //     poller_pool: poller_pool,
        //     links: links,
        //     schedulers: schedulers,
        //     request_tx: tx,
        //     request_rx: rx,
        // })
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
    poller_pool: poll::PollerPool,
    schedulers: Vec<SchedulerHandle>,
    request_tx: RequestSender,
    request_rx: RequestReceiver,
}
impl Executor {
    pub fn new() -> io::Result<Self> {
        Builder::new().build()
    }
    pub fn io_poller_thread_count(&self) -> usize {
        self.poller_pool.thread_count()
    }
    pub fn scheduler_thread_count(&self) -> usize {
        self.schedulers.len()
    }
    pub fn handle(&self) -> ExecutorHandle {
        ExecutorHandle { request_tx: self.request_tx.clone() }
    }
    pub fn spawn<F>(&self, future: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        self.handle().spawn(future);
    }
    pub fn spawn_monitor<F, T, E>(&self, future: F) -> oneshot::Monitor<T, E>
        where F: Future<Item = T, Error = E> + Send + 'static,
              T: Send + 'static,
              E: Send + 'static
    {
        let (monitored, monitor) = oneshot::monitor();
        self.spawn(future.then(|result| {
            monitored.exit(result);
            Ok(())
        }));
        monitor
    }

    pub fn spawn_fn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        self.handle().spawn_fn(f);
    }
    pub fn run(mut self) -> io::Result<()> {
        loop {
            self.run_once(Some(time::Duration::from_millis(1)))?
        }
    }
    pub fn run_once(&mut self, sleep_duration_if_idle: Option<time::Duration>) -> io::Result<()> {
        let mut is_idle = true;
        match self.request_rx.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => unreachable!(),
            Ok(request) => {
                is_idle = false;
                self.handle_request(request)
            }
        }
        if is_idle {
            if let Some(duration) = sleep_duration_if_idle {
                thread::sleep(duration);
            }
        }
        Ok(())
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
    pub fn spawn<F>(&self, future: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        let request = Request::Spawn(fiber::Task(future.boxed()));
        let _ = self.request_tx.send(request);
    }
    pub fn spawn_fn<F, T>(&self, f: F)
        where F: FnOnce() -> T + Send + 'static,
              T: IntoFuture<Item = (), Error = ()> + Send + 'static,
              T::Future: Send
    {
        let request = Request::Spawn(fiber::Task(futures::lazy(f).boxed()));
        let _ = self.request_tx.send(request);
    }
}

// TODO: improve selection logic and scalability
