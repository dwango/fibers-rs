use std::fmt;
use std::error;
use std::sync::mpsc::SendError;
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
    type Error = Disconnected;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match assert_ok!(self.0.poll()) {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(None) => Err(Disconnected),
            Async::Ready(Some(t)) => Ok(Async::Ready(t)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disconnected;
impl error::Error for Disconnected {
    fn description(&self) -> &str {
        "The sender of the oneshot channel was disconnected"
    }
    fn cause(&self) -> Option<&error::Error> {
        None
    }
}
impl fmt::Display for Disconnected {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "The sender of the oneshot channel was disconnected")
    }
}
