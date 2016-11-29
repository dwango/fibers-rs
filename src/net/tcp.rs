use std::io;
use std::mem;
use std::error;
use std::net::SocketAddr;
use futures::{Poll, Async, Future, Stream};
use mio;

use fiber;
use io::poll;
use io::poll::EventedHandle;
use io::poll::SharableEvented;
use super::{ReadHalf, WriteHalf};

#[derive(Debug)]
pub struct TcpListener {
    inner: SharableEvented<mio::tcp::TcpListener>,
    handle: EventedHandle,
    monitor: Option<poll::Monitor>,
}
impl TcpListener {
    // TODO: doc for panic condition
    pub fn bind(addr: &SocketAddr) -> Bind {
        mio::tcp::TcpListener::bind(addr)
            .map(|listener| {
                let listener = SharableEvented::new(listener);
                let register =
                    assert_some!(fiber::with_poller(|poller| poller.register(listener.clone())));
                Bind::Registering(listener, register)
            })
            .unwrap_or_else(Bind::Error)
    }
    fn new(inner: SharableEvented<mio::tcp::TcpListener>, handle: EventedHandle) -> Self {
        TcpListener {
            inner: inner,
            handle: handle,
            monitor: None,
        }
    }
}

#[derive(Debug)]
pub struct Incoming(TcpListener);
impl Stream for Incoming {
    type Item = (Connect, SocketAddr); // TODO: name
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Some(mut monitor) = self.0.monitor.take() {
            if let Async::Ready(()) = monitor.poll().map_err(into_io_error)? {
                self.poll()
            } else {
                self.0.monitor = Some(monitor);
                Ok(Async::NotReady)
            }
        } else {
            match self.0.inner.with_inner_mut(|inner| inner.accept()) {
                Ok((stream, addr)) => {
                    let stream = SharableEvented::new(stream);
                    let register =
                        assert_some!(fiber::with_poller(|poller| poller.register(stream.clone())));
                    let stream = Connect::Registering(stream, register);
                    Ok(Async::Ready(Some((stream, addr))))
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        self.0.monitor = Some(self.0.handle.monitor(poll::Interest::Read));
                        Ok(Async::NotReady)
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum Bind {
    Registering(SharableEvented<mio::tcp::TcpListener>, poll::Register),
    Error(io::Error),
    Polled,
}
impl Future for Bind {
    type Item = TcpListener;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(self, Bind::Polled) {
            Bind::Registering(listener, mut future) => {
                if let Async::Ready(handle) = future.poll().map_err(into_io_error)? {
                    Ok(Async::Ready(TcpListener::new(listener, handle)))
                } else {
                    *self = Bind::Registering(listener, future);
                    Ok(Async::NotReady)
                }
            }
            Bind::Error(e) => Err(e),
            Bind::Polled => panic!("Cannot poll Bind twice"),
        }
    }
}

#[derive(Debug)]
pub enum Connect {
    Registering(SharableEvented<mio::tcp::TcpStream>, poll::Register),
    Connecting(TcpStream),
    Error(io::Error),
    Polled,
}
impl Future for Connect {
    type Item = TcpStream;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(self, Connect::Polled) {
            Connect::Registering(stream, mut future) => {
                if let Async::Ready(handle) = future.poll().map_err(into_io_error)? {
                    *self = Connect::Connecting(TcpStream::new(stream, handle));
                    self.poll()
                } else {
                    *self = Connect::Registering(stream, future);
                    Ok(Async::NotReady)
                }
            }
            Connect::Connecting(mut stream) => {
                use std::io::Write;
                match stream.flush() {
                    Ok(()) => Ok(Async::Ready(stream)),
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            Ok(Async::NotReady)
                        } else {
                            Err(e)
                        }
                    }
                }
            }
            Connect::Error(e) => Err(e),
            Connect::Polled => panic!("Cannot poll Connect twice"),
        }
    }
}
#[derive(Debug)]
pub struct TcpStream {
    inner: SharableEvented<mio::tcp::TcpStream>,
    handle: EventedHandle,
    read_monitor: Option<poll::Monitor>,
    write_monitor: Option<poll::Monitor>,
}
impl TcpStream {
    // TODO: implements other tcp related functions
    fn new(inner: SharableEvented<mio::tcp::TcpStream>, handle: EventedHandle) -> Self {
        TcpStream {
            inner: inner,
            handle: handle,
            read_monitor: None,
            write_monitor: None,
        }
    }

    // TODO: doc for panic condition
    pub fn connect(addr: &SocketAddr) -> Connect {
        mio::tcp::TcpStream::connect(addr)
            .map(|stream| {
                let stream = SharableEvented::new(stream);
                let register =
                    assert_some!(fiber::with_poller(|poller| poller.register(stream.clone())));
                Connect::Registering(stream, register)
            })
            .unwrap_or_else(Connect::Error)
    }
    pub fn split(mut self) -> (ReadHalf<Self>, WriteHalf<Self>) {
        let read_half = ReadHalf(TcpStream {
            inner: self.inner.clone(),
            handle: self.handle.clone(),
            read_monitor: self.read_monitor.take(),
            write_monitor: None,
        });
        let write_half = WriteHalf(self);
        (read_half, write_half)
    }
    fn monitor(&mut self, interest: poll::Interest) -> &mut Option<poll::Monitor> {
        if interest.is_read() {
            &mut self.read_monitor
        } else {
            &mut self.write_monitor
        }
    }
    fn operate<F, T>(&mut self, interest: poll::Interest, f: F) -> io::Result<T>
        where F: FnOnce(&mut mio::tcp::TcpStream) -> io::Result<T>
    {
        if let Some(mut monitor) = self.monitor(interest).take() {
            if let Async::Ready(()) =monitor.poll().map_err(into_io_error)? {
                self.operate(interest, f)
            } else {
                *self.monitor(interest) = Some(monitor);
                Err(mio::would_block())
            }
        } else {
            self.inner.with_inner_mut(|mut inner| f(&mut *inner)).map_err(|e| {
                if e.kind() == io::ErrorKind::WouldBlock {
                    *self.monitor(interest) = Some(self.handle.monitor(interest));
                }
                e
            })
        }
    }
}
impl io::Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.operate(poll::Interest::Read, |inner| inner.read(buf))
    }
}
impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.operate(poll::Interest::Write, |inner| inner.write(buf))
    }
    fn flush(&mut self) -> io::Result<()> {
        self.operate(poll::Interest::Write, |inner| inner.flush())
    }
}

fn into_io_error<E: error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, Box::new(error))
}
