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
