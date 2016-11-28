#![allow(dead_code)]
use std::io;
use std::fmt;
use std::time;
use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use mio;

use fiber;
use collections::RemovableHeap;

pub type RequestSender = std_mpsc::Sender<Request>;
pub type RequestReceiver = std_mpsc::Receiver<Request>;

pub const DEFAULT_EVENTS_CAPACITY: usize = 128;

struct MioEvents(mio::Events);
impl fmt::Debug for MioEvents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MioEvents(_)")
    }
}

#[derive(Debug)]
pub struct Registrant;

#[derive(Debug)]
pub struct Poller {
    poll: mio::Poll,
    events: MioEvents,
    request_tx: RequestSender,
    request_rx: RequestReceiver,
    next_token: usize,
    registrants: HashMap<mio::Token, Registrant>,
    timeout_queue: RemovableHeap<()>,
}
impl Poller {
    pub fn new() -> io::Result<Self> {
        Self::with_capacity(DEFAULT_EVENTS_CAPACITY)
    }
    pub fn with_capacity(capacity: usize) -> io::Result<Self> {
        let poll = mio::Poll::new()?;
        let (tx, rx) = std_mpsc::channel();
        Ok(Poller {
            poll: poll,
            events: MioEvents(mio::Events::with_capacity(capacity)),
            request_tx: tx,
            request_rx: rx,
            next_token: 0,
            registrants: HashMap::new(),
            timeout_queue: RemovableHeap::new(),
        })
    }
    pub fn registrant_count(&self) -> usize {
        self.registrants.len()
    }
    pub fn handle(&self) -> PollerHandle {
        PollerHandle {
            request_tx: self.request_tx.clone(),
            is_alive: true,
        }
    }
    pub fn poll(&mut self, timeout: Option<time::Duration>) -> io::Result<()> {
        let mut did_something = false;

        // Request
        match self.request_rx.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {}
            Err(std_mpsc::TryRecvError::Disconnected) => unreachable!(),
            Ok(r) => {
                did_something = true;
                self.handle_request(r)?;
            }
        }

        // Timeout
        // TODO

        // I/O event
        let timeout = if !did_something && self.timeout_queue.len() == 0 {
            // TODO: min(timeout, timeout_queue.front())
            timeout
        } else {
            Some(time::Duration::from_millis(0))
        };
        let _ = self.poll.poll(&mut self.events.0, timeout)?;
        for _e in self.events.0.iter() {
        }

        Ok(())
    }
    fn handle_request(&mut self, request: Request) -> io::Result<()> {
        match request {
            _ => unimplemented!(),
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PollerHandle {
    request_tx: RequestSender,
    is_alive: bool,
}
impl PollerHandle {
    pub fn is_alive(&self) -> bool {
        self.is_alive
    }
}

#[derive(Debug)]
pub enum Interest {
    Read,
    Write,
}

pub struct BoxEvented(Box<mio::Evented + Send + 'static>);
impl fmt::Debug for BoxEvented {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BoxEvented(_)")
    }
}

#[derive(Debug)]
pub enum Request {
    Register(mio::Token, BoxEvented),
    Deregister(mio::Token),
    Monitor(mio::Token, Interest, fiber::Unpark),
    SetTimeout,
    CancelTimeout,
}
