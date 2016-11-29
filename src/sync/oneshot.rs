use std::fmt;
use std::error;
use std::sync::mpsc::{RecvError, SendError};
use futures::{Poll, Async, Future, Stream};

use sync::mpsc;

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    (Sender(tx), Receiver(rx))
}

#[derive(Debug)]
pub struct Sender<T>(mpsc::Sender<T>);
impl<T> Sender<T> {
    pub fn send(self, t: T) -> Result<(), SendError<T>> {
        self.0.send(t)
    }
}

#[derive(Debug)]
pub struct Receiver<T>(mpsc::Receiver<T>);
impl<T> Future for Receiver<T> {
    type Item = T;
    type Error = RecvError;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match assert_ok!(self.0.poll()) {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(None) => Err(RecvError),
            Async::Ready(Some(t)) => Ok(Async::Ready(t)),
        }
    }
}

pub fn monitor<E>() -> (Monitored<E>, Monitor<E>) {
    let (tx, rx) = channel();
    (Monitored(Some(tx)), Monitor(rx))
}

#[derive(Debug)]
pub struct Monitored<E>(Option<Sender<Result<(), E>>>);
impl<E> Monitored<E> {
    pub fn exit(mut self, result: Result<(), E>) {
        let tx = assert_some!(self.0.take());
        let _ = tx.send(result);
    }
    pub fn succeed(self) {
        self.exit(Ok(()));
    }
    pub fn fail(self, error: E) {
        self.exit(Err(error));
    }
}

#[derive(Debug)]
pub struct Monitor<E>(Receiver<Result<(), E>>);
impl<E> Future for Monitor<E> {
    type Item = ();
    type Error = MonitorError<E>;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Async::Ready(r) = self.0.poll().or(Err(MonitorError::Aborted))? {
            match r {
                Err(e) => Err(MonitorError::Failed(e)),
                Ok(v) => Ok(Async::Ready(v)),
            }
        } else {
            Ok(Async::NotReady)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MonitorError<E> {
    Aborted,
    Failed(E),
}
impl<E: error::Error> error::Error for MonitorError<E> {
    fn description(&self) -> &str {
        match *self {
            MonitorError::Aborted => "Monitor target aborted",
            MonitorError::Failed(_) => "Monitor target failed: {}",
        }
    }
    fn cause(&self) -> Option<&error::Error> {
        match *self {
            MonitorError::Aborted => None,
            MonitorError::Failed(ref e) => Some(e),
        }
    }
}
impl<E: fmt::Display> fmt::Display for MonitorError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            MonitorError::Aborted => write!(f, "Monitor target aborted"),
            MonitorError::Failed(ref e) => write!(f, "Monitor target failed: {}", e),
        }
    }
}

pub fn link<E>() -> (Link<E>, Link<E>) {
    let (tx0, rx0) = monitor();
    let (tx1, rx1) = monitor();
    (Link { tx: tx0, rx: rx1 }, Link { tx: tx1, rx: rx0 })
}

#[derive(Debug)]
pub struct Link<E> {
    tx: Monitored<E>,
    rx: Monitor<E>,
}
impl<E> Link<E> {
    pub fn exit(self, result: Result<(), E>) {
        self.tx.exit(result);
    }
    pub fn succeed(self) {
        self.tx.succeed();
    }
    pub fn fail(self, error: E) {
        self.tx.fail(error);
    }
}
impl<E> Future for Link<E> {
    type Item = ();
    type Error = MonitorError<E>;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.rx.poll()
    }
}
