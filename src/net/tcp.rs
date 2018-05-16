// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use futures::{Async, Future, Poll, Stream};
use mio;
use mio::net::{TcpListener as MioTcpListener, TcpStream as MioTcpStream};
use std::fmt;
use std::io;
use std::mem;
use std::net::SocketAddr;

use super::{into_io_error, Bind};
use fiber::{self, Context};
use io::poll::{EventedHandle, Interest, Register};
use sync::oneshot::Monitor;

/// A structure representing a socket server.
///
/// # Examples
///
/// ```
/// // See also: fibers/examples/tcp_example.rs
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Executor, InPlaceExecutor, Spawn};
/// use fibers::net::{TcpListener, TcpStream};
/// use fibers::sync::oneshot;
/// use futures::{Future, Stream};
///
/// # fn main() {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (addr_tx, addr_rx) = oneshot::channel();
///
/// // Spawns TCP listener
/// executor.spawn(TcpListener::bind("127.0.0.1:0".parse().unwrap())
///     .and_then(|listener| {
///         let addr = listener.local_addr().unwrap();
///         println!("# Start listening: {}", addr);
///         addr_tx.send(addr).unwrap();
///         listener.incoming()
///             .for_each(move |(_client, addr)| {
///                 println!("# Accepted: {}", addr);
///                 Ok(())
///             })
///     })
///     .map_err(|e| panic!("{:?}", e)));
///
/// // Spawns TCP client
/// let mut monitor = executor.spawn_monitor(addr_rx.map_err(|e| panic!("{:?}", e))
///     .and_then(|server_addr| {
///         TcpStream::connect(server_addr).and_then(move |_stream| {
///             println!("# Connected: {}", server_addr);
///             Ok(())
///         })
///     }));
///
/// // Runs until the TCP client exits
/// while monitor.poll().unwrap().is_not_ready() {
///     executor.run_once().unwrap();
/// }
/// println!("# Succeeded");
/// # }
/// ```
pub struct TcpListener {
    handle: EventedHandle<MioTcpListener>,
    monitor: Option<Monitor<(), io::Error>>,
}
impl TcpListener {
    /// Makes a future to create a new `TcpListener` which will be bound to the specified address.
    pub fn bind(addr: SocketAddr) -> TcpListenerBind {
        TcpListenerBind(Bind::Bind(addr, MioTcpListener::bind))
    }

    /// Makes a stream of the connections which will be accepted by this listener.
    pub fn incoming(self) -> Incoming {
        Incoming(self)
    }

    /// Returns the local socket address of this listener.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.handle.inner().local_addr()
    }

    /// Get the value of the `SO_ERROR` option on this socket.
    ///
    /// This will retrieve the stored error in the underlying socket,
    /// clearing the field in the process.
    /// This can be useful for checking errors between calls.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.handle.inner().take_error()
    }

    /// Calls `f` with the reference to the inner socket.
    pub unsafe fn with_inner<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&MioTcpListener) -> T,
    {
        f(&*self.handle.inner())
    }
}
impl fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TcpListener {{ ")?;
        if let Ok(addr) = self.local_addr() {
            write!(f, "local_addr:{:?}, ", addr)?;
        }
        write!(f, ".. }}")?;
        Ok(())
    }
}

/// A future which will create a new `TcpListener` which will be bound to the specified address.
///
/// This is created by calling `TcpListener::bind` function.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the future is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct TcpListenerBind(Bind<fn(&SocketAddr) -> io::Result<MioTcpListener>, MioTcpListener>);
impl Future for TcpListenerBind {
    type Item = TcpListener;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.0.poll()?.map(|handle| TcpListener {
            handle: handle,
            monitor: None,
        }))
    }
}

/// An infinite stream of the connections which will be accepted by the listener.
///
/// This is created by calling `TcpListener::incoming` method.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the stream is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct Incoming(TcpListener);
impl Stream for Incoming {
    type Item = (Connected, SocketAddr);
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            if let Some(mut monitor) = self.0.monitor.take() {
                if let Async::NotReady = monitor.poll().map_err(into_io_error)? {
                    self.0.monitor = Some(monitor);
                    return Ok(Async::NotReady);
                }
            } else {
                match self.0.handle.inner().accept() {
                    Ok((stream, addr)) => {
                        let register = |mut c: Context| c.poller().register(stream);
                        let future = assert_some!(fiber::with_current_context(register));
                        let stream = Connected(Some(future));
                        return Ok(Async::Ready(Some((stream, addr))));
                    }
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            self.0.monitor = Some(self.0.handle.monitor(Interest::Read));
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
        }
    }
}

/// A future which represents a `TcpStream` connected to a `TcpListener`.
///
/// This is produced by `Incoming` stream.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the future is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct Connected(Option<Register<MioTcpStream>>);
impl Future for Connected {
    type Item = TcpStream;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut future = self.0.take().expect("Cannot poll Connected twice");
        if let Async::Ready(handle) = future.poll().map_err(into_io_error)? {
            Ok(Async::Ready(TcpStream::new(handle)))
        } else {
            self.0 = Some(future);
            Ok(Async::NotReady)
        }
    }
}

/// A structure which represents a TCP stream between a local socket and a remote socket.
///
/// The socket will be closed when the value is dropped.
///
/// # Note
///
/// Non blocking mode is always enabled on this socket.
/// Roughly speaking, if an operation (read or write) for a socket would block,
/// it returns the `std::io::ErrorKind::WouldBlock` error and
/// current fiber is suspended until the socket becomes available.
/// If the fiber has multiple sockets (or other objects which may block),
/// it will be suspended only the case that all of them are unavailable.
///
/// To handle read/write operations over TCP streams in
/// [futures](https://github.com/alexcrichton/futures-rs) style,
/// it is preferred to use external crate like [`handy_async`](https://github.com/sile/handy_async).
///
/// # Examples
///
/// ```
/// // See also: fibers/examples/tcp_example.rs
/// # extern crate fibers;
/// # extern crate futures;
/// use fibers::{Executor, InPlaceExecutor, Spawn};
/// use fibers::net::{TcpListener, TcpStream};
/// use fibers::sync::oneshot;
/// use futures::{Future, Stream};
///
/// # fn main() {
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (addr_tx, addr_rx) = oneshot::channel();
///
/// // Spawns TCP listener
/// executor.spawn(TcpListener::bind("127.0.0.1:0".parse().unwrap())
///     .and_then(|listener| {
///         let addr = listener.local_addr().unwrap();
///         println!("# Start listening: {}", addr);
///         addr_tx.send(addr).unwrap();
///         listener.incoming()
///             .for_each(move |(_client, addr)| {
///                 println!("# Accepted: {}", addr);
///                 Ok(())
///             })
///     })
///     .map_err(|e| panic!("{:?}", e)));
///
/// // Spawns TCP client
/// let mut monitor = executor.spawn_monitor(addr_rx.map_err(|e| panic!("{:?}", e))
///     .and_then(|server_addr| {
///         TcpStream::connect(server_addr).and_then(move |mut stream| {
///             use std::io::Write;
///             println!("# Connected: {}", server_addr);
///             stream.write(b"Hello World!"); // This may return `WouldBlock` error
///             Ok(())
///         })
///     }));
///
/// // Runs until the TCP client exits
/// while monitor.poll().unwrap().is_not_ready() {
///     executor.run_once().unwrap();
/// }
/// println!("# Succeeded");
/// # }
/// ```
pub struct TcpStream {
    handle: EventedHandle<MioTcpStream>,
    read_monitor: Option<Monitor<(), io::Error>>,
    write_monitor: Option<Monitor<(), io::Error>>,
}
impl Clone for TcpStream {
    fn clone(&self) -> Self {
        TcpStream {
            handle: self.handle.clone(),
            read_monitor: None,
            write_monitor: None,
        }
    }
}
impl TcpStream {
    fn new(handle: EventedHandle<MioTcpStream>) -> Self {
        TcpStream {
            handle: handle,
            read_monitor: None,
            write_monitor: None,
        }
    }

    /// Makes a future to open a TCP connection to a remote host.
    pub fn connect(addr: SocketAddr) -> Connect {
        Connect(ConnectInner::Connect(addr))
    }

    /// Returns the local socket address of this listener.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.handle.inner().local_addr()
    }

    /// Returns the socket address of the remote peer of this TCP connection.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.handle.inner().peer_addr()
    }

    /// Get the value of the `SO_ERROR` option on this socket.
    ///
    /// This will retrieve the stored error in the underlying socket,
    /// clearing the field in the process.
    /// This can be useful for checking errors between calls.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.handle.inner().take_error()
    }

    /// Gets the value of the `TCP_NODELAY` option on this socket.
    pub fn nodelay(&self) -> io::Result<bool> {
        self.handle.inner().nodelay()
    }

    /// Sets the value of the `TCP_NODELAY `option on this socket.
    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.handle.inner().set_nodelay(nodelay)
    }

    /// Calls `f` with the reference to the inner socket.
    pub unsafe fn with_inner<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&MioTcpStream) -> T,
    {
        f(&*self.handle.inner())
    }

    fn monitor(&mut self, interest: Interest) -> &mut Option<Monitor<(), io::Error>> {
        if interest == Interest::Read {
            &mut self.read_monitor
        } else {
            &mut self.write_monitor
        }
    }
    fn start_monitor_if_needed(&mut self, interest: Interest) -> Result<bool, io::Error> {
        if self.monitor(interest).is_none() {
            *self.monitor(interest) = Some(self.handle.monitor(interest));
            if let Err(e) = self.monitor(interest).poll() {
                return Err(e.unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::Other, "Monitor channel disconnected")
                }));
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn operate<F, T>(&mut self, interest: Interest, mut f: F) -> io::Result<T>
    where
        F: FnMut(&mut MioTcpStream) -> io::Result<T>,
    {
        loop {
            if let Some(mut monitor) = self.monitor(interest).take() {
                if let Async::NotReady = monitor.poll().map_err(into_io_error)? {
                    *self.monitor(interest) = Some(monitor);
                    return Err(mio::would_block());
                }
            } else {
                let result = f(&mut *self.handle.inner());
                match result {
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            *self.monitor(interest) = Some(self.handle.monitor(interest));
                        } else {
                            return Err(e);
                        }
                    }
                    Ok(v) => return Ok(v),
                }
            }
        }
    }
}
impl io::Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.operate(Interest::Read, |inner| inner.read(buf))
    }
}
impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.operate(Interest::Write, |inner| inner.write(buf))
    }
    fn flush(&mut self) -> io::Result<()> {
        self.operate(Interest::Write, |inner| inner.flush())
    }
}
impl fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "TcpStream {{ ")?;
        if let Ok(addr) = self.local_addr() {
            write!(f, "local_addr:{:?}, ", addr)?;
        }
        if let Ok(addr) = self.peer_addr() {
            write!(f, "peer_addr:{:?}, ", addr)?;
        }
        write!(f, ".. }}")?;
        Ok(())
    }
}

/// A future which will open a TCP connection to a remote host.
///
/// This is created by calling `TcpStream::connect` function.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the future is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct Connect(ConnectInner);
impl Future for Connect {
    type Item = TcpStream;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll()
    }
}

#[derive(Debug)]
enum ConnectInner {
    Connect(SocketAddr),
    Registering(Register<MioTcpStream>),
    Connecting(TcpStream),
    Polled,
}
impl Future for ConnectInner {
    type Item = TcpStream;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(self, ConnectInner::Polled) {
            ConnectInner::Connect(addr) => {
                let stream = MioTcpStream::connect(&addr)?;
                let register =
                    assert_some!(fiber::with_current_context(|mut c| c.poller().register(stream),));
                *self = ConnectInner::Registering(register);
                self.poll()
            }
            ConnectInner::Registering(mut future) => {
                if let Async::Ready(handle) = future.poll().map_err(into_io_error)? {
                    *self = ConnectInner::Connecting(TcpStream::new(handle));
                    self.poll()
                } else {
                    *self = ConnectInner::Registering(future);
                    Ok(Async::NotReady)
                }
            }
            ConnectInner::Connecting(mut stream) => match stream.peer_addr() {
                Ok(_) => Ok(Async::Ready(stream)),
                Err(e) => {
                    if let Some(e) = stream.take_error()? {
                        Err(e)?;
                    }
                    if e.kind() == io::ErrorKind::NotConnected {
                        let retry = stream.start_monitor_if_needed(Interest::Write)?;
                        *self = ConnectInner::Connecting(stream);
                        if retry {
                            self.poll()
                        } else {
                            Ok(Async::NotReady)
                        }
                    } else {
                        Err(e)
                    }
                }
            },
            ConnectInner::Polled => panic!("Cannot poll ConnectInner twice"),
        }
    }
}
