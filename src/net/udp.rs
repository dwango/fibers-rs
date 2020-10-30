// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use futures::{Async, Future, Poll};
use mio::net::UdpSocket as MioUdpSocket;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use super::{into_io_error, Bind};
use crate::io::poll::{EventedHandle, Interest};
use crate::sync::oneshot::Monitor;

/// A User Datagram Protocol socket.
///
/// # Examples
///
/// ```
/// # extern crate futures;
/// # extern crate fibers;
/// // See also: fibers/examples/udp_example.rs
/// use fibers::{Executor, InPlaceExecutor, Spawn};
/// use fibers::net::UdpSocket;
/// use fibers::sync::oneshot;
/// use futures::Future;
///
/// let mut executor = InPlaceExecutor::new().unwrap();
/// let (addr_tx, addr_rx) = oneshot::channel();
///
/// // Spawns receiver
/// let mut monitor = executor.spawn_monitor(UdpSocket::bind("127.0.0.1:0".parse().unwrap())
///     .and_then(|socket| {
///         addr_tx.send(socket.local_addr().unwrap()).unwrap();
///         socket.recv_from(vec![0; 32]).map_err(|e| panic!("{:?}", e))
///     })
///     .and_then(|(_, mut buf, len, _)| {
///         buf.truncate(len);
///         assert_eq!(buf, b"hello world");
///         Ok(())
///     }));
///
/// // Spawns sender
/// executor.spawn(addr_rx.map_err(|e| panic!("{:?}", e))
///     .and_then(|receiver_addr| {
///         UdpSocket::bind("127.0.0.1:0".parse().unwrap())
///             .and_then(move |socket| {
///                 socket.send_to(b"hello world", receiver_addr).map_err(|e| panic!("{:?}", e))
///             })
///             .then(|r| Ok(assert!(r.is_ok())))
///     }));
///
/// // Runs until the monitored fiber (i.e., receiver) exits.
/// while monitor.poll().unwrap().is_not_ready() {
///     executor.run_once().unwrap();
/// }
/// ```
#[derive(Clone)]
pub struct UdpSocket {
    handle: Arc<EventedHandle<MioUdpSocket>>,
}
impl UdpSocket {
    /// Makes a future to create a UDP socket binded to the given address.
    pub fn bind(addr: SocketAddr) -> UdpSocketBind {
        UdpSocketBind(Bind::Bind(addr, MioUdpSocket::bind))
    }

    /// Makes a future to send data on the socket to the given address.
    pub fn send_to<B: AsRef<[u8]>>(self, buf: B, target: SocketAddr) -> SendTo<B> {
        SendTo(Some(SendToInner {
            socket: self,
            buf,
            target,
            monitor: None,
        }))
    }

    /// Makes a future to receive data from the socket.
    pub fn recv_from<B: AsMut<[u8]>>(self, buf: B) -> RecvFrom<B> {
        RecvFrom(Some(RecvFromInner {
            socket: self,
            buf,
            monitor: None,
        }))
    }

    /// Returns the socket address that this socket was created from.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.handle.inner().local_addr()
    }
}
impl fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UdpSocket {{ ")?;
        if let Ok(addr) = self.local_addr() {
            write!(f, "local_addr:{:?}, ", addr)?;
        }
        write!(f, ".. }}")?;
        Ok(())
    }
}

/// A future which will create a UDP socket binded to the given address.
///
/// This is created by calling `UdpSocket::bind` function.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the future is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct UdpSocketBind(Bind<fn(&SocketAddr) -> io::Result<MioUdpSocket>, MioUdpSocket>);
impl Future for UdpSocketBind {
    type Item = UdpSocket;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.0.poll()?.map(|handle| UdpSocket { handle }))
    }
}

/// A future which will send data `B` on the socket to the given address.
///
/// This is created by calling `UdpSocket::send_to` method.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the future is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct SendTo<B>(Option<SendToInner<B>>);
impl<B: AsRef<[u8]>> Future for SendTo<B> {
    type Item = (UdpSocket, B, usize);
    type Error = (UdpSocket, B, io::Error);
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut state = self.0.take().expect("Cannot poll SendTo twice");
        loop {
            if let Some(mut monitor) = state.monitor.take() {
                match monitor.poll() {
                    Err(e) => return Err((state.socket, state.buf, into_io_error(e))),
                    Ok(Async::NotReady) => {
                        state.monitor = Some(monitor);
                        self.0 = Some(state);
                        return Ok(Async::NotReady);
                    }
                    Ok(Async::Ready(())) => {}
                }
            } else {
                let result = state
                    .socket
                    .handle
                    .inner()
                    .send_to(state.buf.as_ref(), &state.target);
                match result {
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            state.monitor = Some(state.socket.handle.monitor(Interest::Write));
                        } else {
                            return Err((state.socket, state.buf, e));
                        }
                    }
                    Ok(size) => return Ok(Async::Ready((state.socket, state.buf, size))),
                }
            }
        }
    }
}

#[derive(Debug)]
struct SendToInner<B> {
    socket: UdpSocket,
    buf: B,
    target: SocketAddr,
    monitor: Option<Monitor<(), io::Error>>,
}

/// A future which will receive data from the socket.
///
/// This is created by calling `UdpSocket::recv_from` method.
/// It is permitted to move the future across fibers.
///
/// # Panics
///
/// If the future is polled on the outside of a fiber, it may crash.
#[derive(Debug)]
pub struct RecvFrom<B>(Option<RecvFromInner<B>>);
impl<B: AsMut<[u8]>> Future for RecvFrom<B> {
    type Item = (UdpSocket, B, usize, SocketAddr);
    type Error = (UdpSocket, B, io::Error);
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut state = self.0.take().expect("Cannot poll RecvFrom twice");
        loop {
            if let Some(mut monitor) = state.monitor.take() {
                match monitor.poll() {
                    Err(e) => return Err((state.socket, state.buf, into_io_error(e))),
                    Ok(Async::NotReady) => {
                        state.monitor = Some(monitor);
                        self.0 = Some(state);
                        return Ok(Async::NotReady);
                    }
                    Ok(Async::Ready(())) => {}
                }
            } else {
                let mut buf = state.buf;
                let result = state.socket.handle.inner().recv_from(buf.as_mut());
                state.buf = buf;
                match result {
                    Err(e) => {
                        if e.kind() == io::ErrorKind::WouldBlock {
                            state.monitor = Some(state.socket.handle.monitor(Interest::Read));
                        } else {
                            return Err((state.socket, state.buf, e));
                        }
                    }
                    Ok((size, addr)) => {
                        return Ok(Async::Ready((state.socket, state.buf, size, addr)))
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct RecvFromInner<B> {
    socket: UdpSocket,
    buf: B,
    monitor: Option<Monitor<(), io::Error>>,
}
