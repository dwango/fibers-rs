use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use futures::{Poll, Async, Stream};

use fiber;
use sync::atomic::AtomicCell;

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let notifier = Notifier::new();
    let (tx, rx) = std_mpsc::channel();
    (Sender {
        inner: tx,
        notifier: notifier.clone(),
    },
     Receiver {
        inner: rx,
        notifier: notifier,
    })
}

#[derive(Debug)]
pub struct Receiver<T> {
    inner: std_mpsc::Receiver<T>,
    notifier: Notifier,
}
impl<T> Stream for Receiver<T> {
    type Item = T;
    type Error = ();
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {
                self.notifier.await();
                Ok(Async::NotReady)
            }
            Err(std_mpsc::TryRecvError::Disconnected) => Ok(Async::Ready(None)),
            Ok(t) => Ok(Async::Ready(Some(t))),
        }
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.notifier.notify();
    }
}

#[derive(Debug)]
pub struct Sender<T> {
    inner: std_mpsc::Sender<T>,
    notifier: Notifier,
}
impl<T> Sender<T> {
    pub fn send(&self, t: T) -> Result<(), std_mpsc::SendError<T>> {
        self.inner.send(t)?;
        self.notifier.notify();
        Ok(())
    }
}
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            inner: self.inner.clone(),
            notifier: self.notifier.clone(),
        }
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.notifier.notify();
    }
}

#[derive(Debug, Clone)]
pub struct Notifier {
    unpark: Arc<AtomicCell<Option<fiber::Unpark>>>,
}
impl Notifier {
    pub fn new() -> Self {
        Notifier { unpark: Arc::new(AtomicCell::new(None)) }
    }
    pub fn await(&mut self) {
        loop {
            if let Some(mut unpark) = self.unpark.try_borrow_mut() {
                if unpark.as_ref().map(|u| u.context_id()) != fiber::context_id() {
                    *unpark = fiber::park();
                }
                return;
            }
        }
    }
    pub fn notify(&self) {
        loop {
            if let Some(mut unpark) = self.unpark.try_borrow_mut() {
                *unpark = None;
                return;
            }
        }
    }
}

// TODO: sync_channel
