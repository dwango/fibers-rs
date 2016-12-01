use std::io;
use std::net::SocketAddr;
use futures::{Poll, Async, Future};
use mio;

use io::poll;
use io::poll::{SharableEvented, EventedHandle};
use sync::oneshot::Monitor;
use super::{into_io_error, Bind};

/// A User Datagram Protocol socket.
///
/// # Examples
///
/// ```
/// # extern crate futures;
/// # extern crate fibers;
/// // See also: fibers/examples/udp_example.rs
/// use fibers::fiber::Executor;
/// use fibers::net::UdpSocket;
/// use fibers::sync::oneshot;
/// use futures::Future;
///
/// # fn main() {
/// let mut executor = Executor::new().unwrap();
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
///     executor.run_once(None).unwrap();
/// }
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct UdpSocket {
    inner: SharableEvented<mio::udp::UdpSocket>,
    handle: EventedHandle,
}
impl UdpSocket {
    /// Makes a future to create a UDP socket binded to the given address.
    pub fn bind(addr: SocketAddr) -> UdpSocketBind {
        UdpSocketBind(Bind::Bind(addr, mio::udp::UdpSocket::bind))
    }

    /// Makes a future to send data on the socket to the given address.
    pub fn send_to<B: AsRef<[u8]>>(self, buf: B, target: SocketAddr) -> SendTo<B> {
        SendTo(Some(SendToInner {
            socket: self,
            buf: buf,
            target: target,
            monitor: None,
        }))
    }

    /// Makes a future to receive data from the socket.
    pub fn recv_from<B: AsMut<[u8]>>(self, buf: B) -> RecvFrom<B> {
        RecvFrom(Some(RecvFromInner {
            socket: self,
            buf: buf,
            monitor: None,
        }))
    }

    /// Returns the socket address that this socket was created from.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.with_inner_ref(|inner| inner.local_addr())
    }

    /// Calls `f` with the reference to the inner socket.
    pub unsafe fn with_inner<F, T>(&self, f: F) -> T
        where F: FnOnce(&mio::udp::UdpSocket) -> T
    {
        self.inner.with_inner_ref(f)
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
pub struct UdpSocketBind(Bind<fn(&SocketAddr) -> io::Result<mio::udp::UdpSocket>,
                              mio::udp::UdpSocket>);
impl Future for UdpSocketBind {
    type Item = UdpSocket;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.0.poll()?.map(|(listener, handle)| {
            UdpSocket {
                inner: listener,
                handle: handle,
            }
        }))
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
        if let Some(mut monitor) = state.monitor.take() {
            match monitor.poll() {
                Err(e) => Err((state.socket, state.buf, into_io_error(e))),
                Ok(Async::NotReady) => {
                    state.monitor = Some(monitor);
                    self.0 = Some(state);
                    Ok(Async::NotReady)
                }
                Ok(Async::Ready(())) => {
                    self.0 = Some(state);
                    self.poll()
                }
            }
        } else {
            let result = state.socket
                .inner
                .with_inner_mut(|inner| inner.send_to(state.buf.as_ref(), &state.target));
            match result {
                Err(e) => Err((state.socket, state.buf, e)),
                Ok(None) => {
                    state.monitor = Some(state.socket.handle.monitor(poll::Interest::Write));
                    self.0 = Some(state);
                    Ok(Async::NotReady)
                }
                Ok(Some(size)) => Ok(Async::Ready((state.socket, state.buf, size))),
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
        if let Some(mut monitor) = state.monitor.take() {
            match monitor.poll() {
                Err(e) => Err((state.socket, state.buf, into_io_error(e))),
                Ok(Async::NotReady) => {
                    state.monitor = Some(monitor);
                    self.0 = Some(state);
                    Ok(Async::NotReady)
                }
                Ok(Async::Ready(())) => {
                    self.0 = Some(state);
                    self.poll()
                }
            }
        } else {
            let mut buf = state.buf;
            let result = state.socket
                .inner
                .with_inner_mut(|inner| inner.recv_from(buf.as_mut()));
            state.buf = buf;
            match result {
                Err(e) => Err((state.socket, state.buf, e)),
                Ok(None) => {
                    state.monitor = Some(state.socket.handle.monitor(poll::Interest::Read));
                    self.0 = Some(state);
                    Ok(Async::NotReady)
                }
                Ok(Some((size, addr))) => Ok(Async::Ready((state.socket, state.buf, size, addr))),
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
