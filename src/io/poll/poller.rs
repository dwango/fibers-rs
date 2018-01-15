// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use std::io;
use std::fmt;
use std::time;
use std::collections::HashMap;
use std::sync::mpsc::{RecvError, TryRecvError};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicUsize};
use futures::{self, Future};
use mio;
use nbchan::mpsc as nb_mpsc;

use sync::oneshot;
use collections::HeapMap;
use super::{EventedLock, Interest, SharableEvented};

type RequestSender = nb_mpsc::Sender<Request>;
type RequestReceiver = nb_mpsc::Receiver<Request>;

/// The default capacity of the event buffer of a poller.
pub const DEFAULT_EVENTS_CAPACITY: usize = 128;

struct MioEvents(mio::Events);
impl fmt::Debug for MioEvents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MioEvents(_)")
    }
}

#[derive(Debug)]
struct Registrant {
    is_first: bool,
    evented: BoxEvented,
    read_waitings: Vec<oneshot::Monitored<(), io::Error>>,
    write_waitings: Vec<oneshot::Monitored<(), io::Error>>,
}
impl Registrant {
    pub fn new(evented: BoxEvented) -> Self {
        Registrant {
            is_first: true,
            evented: evented,
            read_waitings: Vec::new(),
            write_waitings: Vec::new(),
        }
    }
    pub fn mio_interest(&self) -> mio::Ready {
        (if self.read_waitings.is_empty() {
            mio::Ready::empty()
        } else {
            mio::Ready::readable()
        }) | (if self.write_waitings.is_empty() {
            mio::Ready::empty()
        } else {
            mio::Ready::writable()
        })
    }
}

/// I/O events poller.
#[derive(Debug)]
pub struct Poller {
    poll: mio::Poll,
    events: MioEvents,
    request_tx: RequestSender,
    request_rx: RequestReceiver,
    next_token: usize,
    next_timeout_id: Arc<AtomicUsize>,
    registrants: HashMap<mio::Token, Registrant>,
    timeout_queue: HeapMap<(time::Instant, usize), oneshot::Sender<()>>,
}
impl Poller {
    /// Creates a new poller.
    ///
    /// This is equivalent to `Poller::with_capacity(DEFAULT_EVENTS_CAPACITY)`.
    pub fn new() -> io::Result<Self> {
        Self::with_capacity(DEFAULT_EVENTS_CAPACITY)
    }

    /// Creates a new poller which has an event buffer of which capacity is `capacity`.
    ///
    /// For the detailed meaning of the `capacity` value,
    /// please see the [mio's documentation]
    /// (https://docs.rs/mio/0.6.1/mio/struct.Events.html#method.with_capacity).
    pub fn with_capacity(capacity: usize) -> io::Result<Self> {
        let poll = mio::Poll::new()?;
        let (tx, rx) = nb_mpsc::channel();
        Ok(Poller {
            poll: poll,
            events: MioEvents(mio::Events::with_capacity(capacity)),
            request_tx: tx,
            request_rx: rx,
            next_token: 0,
            next_timeout_id: Arc::new(AtomicUsize::new(0)),
            registrants: HashMap::new(),
            timeout_queue: HeapMap::new(),
        })
    }

    /// Makes a future to register new evented object to the poller.
    pub fn register<E>(&mut self, evented: E) -> Register<E>
    where
        E: mio::Evented + Send + 'static,
    {
        self.handle().register(evented)
    }

    /// Blocks the current thread and wait until any events happen or `timeout` expires.
    ///
    /// On the former case, the poller notifies the fibers waiting on those events.
    pub fn poll(&mut self, timeout: Option<time::Duration>) -> io::Result<()> {
        let mut did_something = false;

        // Request
        match self.request_rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => unreachable!(),
            Ok(r) => {
                did_something = true;
                self.handle_request(r)?;
            }
        }

        // Timeout
        let now = time::Instant::now();
        while let Some((_, notifier)) = self.timeout_queue.pop_if(|k, _| k.0 <= now) {
            let _ = notifier.send(());
        }

        // I/O event
        let timeout = if did_something {
            Some(time::Duration::from_millis(0))
        } else if let Some((k, _)) = self.timeout_queue.peek() {
            let duration_until_next_expiry_time = k.0 - now;
            if let Some(timeout) = timeout {
                use std::cmp;
                Some(cmp::min(timeout, duration_until_next_expiry_time))
            } else {
                Some(duration_until_next_expiry_time)
            }
        } else {
            timeout
        };
        let _ = self.poll.poll(&mut self.events.0, timeout)?;
        for e in self.events.0.iter() {
            let r = assert_some!(self.registrants.get_mut(&e.token()));
            if e.readiness().is_readable() {
                for _ in r.read_waitings.drain(..).map(|tx| tx.exit(Ok(()))) {}
            }
            if e.readiness().is_writable() {
                for _ in r.write_waitings.drain(..).map(|tx| tx.exit(Ok(()))) {}
            }
            Self::mio_register(&self.poll, e.token(), r)?;
        }

        Ok(())
    }

    /// Makes a handle of the poller.
    pub fn handle(&self) -> PollerHandle {
        PollerHandle {
            request_tx: self.request_tx.clone(),
            next_timeout_id: Arc::clone(&self.next_timeout_id),
            is_alive: true,
        }
    }

    fn handle_request(&mut self, request: Request) -> io::Result<()> {
        match request {
            Request::Register(evented, mut reply) => {
                let token = self.next_token();
                self.registrants.insert(token, Registrant::new(evented));
                (reply.0)(token);
            }
            Request::Deregister(token) => {
                let r = assert_some!(self.registrants.remove(&token));
                if !r.is_first {
                    self.poll.deregister(&*r.evented.0)?;
                }
            }
            Request::Monitor(token, interest, notifier) => {
                let r = assert_some!(self.registrants.get_mut(&token));
                match interest {
                    Interest::Read => r.read_waitings.push(notifier),
                    Interest::Write => r.write_waitings.push(notifier),
                }
                if r.read_waitings.len() == 1 || r.write_waitings.len() == 1 {
                    Self::mio_register(&self.poll, token, r)?;
                }
            }
            Request::SetTimeout(timeout_id, expiry_time, reply) => {
                assert!(
                    self.timeout_queue
                        .push_if_absent((expiry_time, timeout_id), reply,)
                );
            }
            Request::CancelTimeout(timeout_id, expiry_time) => {
                self.timeout_queue.remove(&(expiry_time, timeout_id));
            }
        }
        Ok(())
    }
    fn mio_register(poll: &mio::Poll, token: mio::Token, r: &mut Registrant) -> io::Result<()> {
        let interest = r.mio_interest();
        if interest != mio::Ready::empty() {
            let options = mio::PollOpt::edge() | mio::PollOpt::oneshot();
            if r.is_first {
                r.is_first = false;
                poll.register(&*r.evented.0, token, interest, options)?;
            } else {
                poll.reregister(&*r.evented.0, token, interest, options)?;
            }
        }
        Ok(())
    }
    fn next_token(&mut self) -> mio::Token {
        loop {
            let token = self.next_token;
            self.next_token = token.wrapping_add(1);
            if self.registrants.contains_key(&mio::Token(token)) {
                continue;
            }
            return mio::Token(token);
        }
    }
}

/// A handle of a poller.
#[derive(Debug, Clone)]
pub struct PollerHandle {
    request_tx: RequestSender,
    next_timeout_id: Arc<AtomicUsize>,
    is_alive: bool,
}
impl PollerHandle {
    /// Returns `true` if the original poller maybe alive, otherwise `false`.
    pub fn is_alive(&self) -> bool {
        self.is_alive
    }

    /// Makes a future to register new evented object to the poller.
    pub fn register<E>(&mut self, evented: E) -> Register<E>
    where
        E: mio::Evented + Send + 'static,
    {
        let evented = SharableEvented::new(evented);
        let box_evented = BoxEvented(Box::new(evented.clone()));
        let request_tx = self.request_tx.clone();
        let (tx, rx) = oneshot::channel();
        let mut reply = Some(move |token| {
            let handle = EventedHandle::new(evented, request_tx, token);
            let _ = tx.send(handle);
        });
        let reply = RegisterReplyFn(Box::new(move |token| {
            let reply = reply.take().unwrap();
            reply(token)
        }));
        if self.request_tx
            .send(Request::Register(box_evented, reply))
            .is_err()
        {
            self.is_alive = false;
        }
        Register { rx: rx }
    }

    fn set_timeout(&self, delay_from_now: time::Duration) -> Timeout {
        let (tx, rx) = oneshot::channel();
        let expiry_time = time::Instant::now() + delay_from_now;
        let timeout_id = self.next_timeout_id.fetch_add(1, atomic::Ordering::SeqCst);
        let request = Request::SetTimeout(timeout_id, expiry_time, tx);
        let _ = self.request_tx.send(request);
        Timeout {
            cancel: Some(CancelTimeout {
                timeout_id: timeout_id,
                expiry_time: expiry_time,
                request_tx: self.request_tx.clone(),
            }),
            rx: rx,
        }
    }
}

pub fn set_timeout(poller: &PollerHandle, delay_from_now: time::Duration) -> Timeout {
    poller.set_timeout(delay_from_now)
}

#[derive(Debug)]
struct CancelTimeout {
    timeout_id: usize,
    expiry_time: time::Instant,
    request_tx: RequestSender,
}
impl CancelTimeout {
    pub fn cancel(self) {
        let _ = self.request_tx
            .send(Request::CancelTimeout(self.timeout_id, self.expiry_time));
    }
}

/// A future which will expire at the specified time instant.
///
/// If this object is dropped before expiration, the timer will be cancelled.
/// Thus, for example, the repetation of setting and canceling of
/// a timer only consumpts constant memory region.
#[derive(Debug)]
pub struct Timeout {
    cancel: Option<CancelTimeout>,
    rx: oneshot::Receiver<()>,
}
impl Future for Timeout {
    type Item = ();
    type Error = RecvError;
    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        let result = self.rx.poll();
        if result != Ok(futures::Async::NotReady) {
            self.cancel = None;
        }
        result
    }
}
impl Drop for Timeout {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}

/// A future which will register a new evented object to a poller.
#[derive(Debug)]
pub struct Register<T> {
    rx: oneshot::Receiver<EventedHandle<T>>,
}
impl<T> Future for Register<T> {
    type Item = EventedHandle<T>;
    type Error = RecvError;
    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        self.rx.poll()
    }
}

/// The handle of an evented object which has been registered in a poller.
///
/// When all copy of this handle are dropped,
/// the corresponding entry in the poller is deregistered.
#[derive(Debug)]
pub struct EventedHandle<T> {
    token: mio::Token,
    request_tx: RequestSender,
    shared_count: Arc<AtomicUsize>,
    inner: SharableEvented<T>,
}
impl<T: mio::Evented> EventedHandle<T> {
    fn new(inner: SharableEvented<T>, request_tx: RequestSender, token: mio::Token) -> Self {
        EventedHandle {
            token: token,
            request_tx: request_tx,
            shared_count: Arc::new(AtomicUsize::new(1)),
            inner: inner,
        }
    }

    /// Monitors occurrence of an event specified by `interest`.
    pub fn monitor(&self, interest: Interest) -> oneshot::Monitor<(), io::Error> {
        let (monitored, monitor) = oneshot::monitor();
        let _ = self.request_tx
            .send(Request::Monitor(self.token, interest, monitored));
        monitor
    }

    /// Returns the locked reference to the inner evented object.
    pub fn inner(&self) -> EventedLock<T> {
        self.inner.lock()
    }
}
impl<T> Clone for EventedHandle<T> {
    fn clone(&self) -> Self {
        self.shared_count.fetch_add(1, atomic::Ordering::SeqCst);
        EventedHandle {
            token: self.token,
            request_tx: self.request_tx.clone(),
            shared_count: Arc::clone(&self.shared_count),
            inner: self.inner.clone(),
        }
    }
}
impl<T> Drop for EventedHandle<T> {
    fn drop(&mut self) {
        if 1 == self.shared_count.fetch_sub(1, atomic::Ordering::SeqCst) {
            let _ = self.request_tx.send(Request::Deregister(self.token));
        }
    }
}

struct BoxEvented(Box<mio::Evented + Send + 'static>);
impl fmt::Debug for BoxEvented {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BoxEvented(_)")
    }
}

struct RegisterReplyFn(Box<FnMut(mio::Token) + Send + 'static>);
impl fmt::Debug for RegisterReplyFn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RegisterReplyFn(_)")
    }
}

#[derive(Debug)]
enum Request {
    Register(BoxEvented, RegisterReplyFn),
    Deregister(mio::Token),
    Monitor(mio::Token, Interest, oneshot::Monitored<(), io::Error>),
    SetTimeout(usize, time::Instant, oneshot::Sender<()>),
    CancelTimeout(usize, time::Instant),
}
