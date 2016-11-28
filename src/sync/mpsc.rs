use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use futures::{Poll, Async, Stream, Sink, StartSend, AsyncSink};

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

pub fn sync_channel<T>(bound: usize) -> (SyncSender<T>, Receiver<T>) {
    let notifier = Notifier::new();
    let (tx, rx) = std_mpsc::sync_channel(bound);
    (SyncSender {
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

#[derive(Debug)]
pub struct SyncSender<T> {
    inner: std_mpsc::SyncSender<T>,
    notifier: Notifier,
}
impl<T> Sink for SyncSender<T> {
    type SinkItem = T;
    type SinkError = std_mpsc::SendError<T>;
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.inner.try_send(item) {
            Err(std_mpsc::TrySendError::Full(item)) => Ok(AsyncSink::NotReady(item)),
            Err(std_mpsc::TrySendError::Disconnected(item)) => Err(std_mpsc::SendError(item)),
            Ok(()) => {
                self.notifier.notify();
                Ok(AsyncSink::Ready)
            }
        }
    }
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}
impl<T> Clone for SyncSender<T> {
    fn clone(&self) -> Self {
        SyncSender {
            inner: self.inner.clone(),
            notifier: self.notifier.clone(),
        }
    }
}
impl<T> Drop for SyncSender<T> {
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
