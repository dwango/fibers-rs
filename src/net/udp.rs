use std::io;
use std::mem;
use std::error;
use std::net::SocketAddr;
use futures::{Poll, Async, Future};
use mio;

use fiber;
use io::poll;
use io::poll::{SharableEvented, EventedHandle};
use sync::oneshot::Monitor;

#[derive(Debug)]
pub struct UdpSocket {
    inner: SharableEvented<mio::udp::UdpSocket>,
    handle: EventedHandle,
}
impl UdpSocket {
    pub fn bind(addr: &SocketAddr) -> UdpBind {
        mio::udp::UdpSocket::bind(addr)
            .map(|socket| {
                let socket = SharableEvented::new(socket);
                let register =
                    assert_some!(fiber::with_poller(|poller| poller.register(socket.clone())));
                UdpBind::Registering(socket, register)
            })
            .unwrap_or_else(UdpBind::Error)
    }
    pub fn send_to<B>(self, buf: B, target: &SocketAddr) -> SendTo<B>
        where B: AsRef<[u8]>
    {
        SendTo(Some(SendToInner {
            socket: self,
            buf: buf,
            target: target.clone(),
            monitor: None,
        }))
    }
    pub fn recv_from<B>(self, buf: B) -> RecvFrom<B>
        where B: AsMut<[u8]>
    {
        RecvFrom(Some(RecvFromInner {
            socket: self,
            buf: buf,
            monitor: None,
        }))
    }
    fn new(inner: SharableEvented<mio::udp::UdpSocket>, handle: EventedHandle) -> Self {
        UdpSocket {
            inner: inner,
            handle: handle,
        }
    }
}

#[derive(Debug)]
struct SendToInner<B> {
    socket: UdpSocket,
    buf: B,
    target: SocketAddr,
    monitor: Option<Monitor<io::Error>>,
}

#[derive(Debug)]
pub struct SendTo<B>(Option<SendToInner<B>>);
impl<B> Future for SendTo<B>
    where B: AsRef<[u8]>
{
    type Item = (UdpSocket, B, usize);
    type Error = (UdpSocket, B, io::Error);
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(mut state) = self.0.take() {
            if let Some(mut monitor) = state.monitor.take() {
                match monitor.poll() {
                    Err(e) => Err((state.socket, state.buf, into_io_error(e))),
                    Ok(Async::NotReady) => {
                        state.monitor = Some(monitor);
                        self.0 = Some(state);
                        Ok(Async::NotReady)
                    }
                    Ok(Async::Ready(())) => {
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
        } else {
            panic!("Cannot poll SendTo twice")
        }
    }
}

#[derive(Debug)]
struct RecvFromInner<B> {
    socket: UdpSocket,
    buf: B,
    monitor: Option<Monitor<io::Error>>,
}

#[derive(Debug)]
pub struct RecvFrom<B>(Option<RecvFromInner<B>>);
impl<B> Future for RecvFrom<B>
    where B: AsMut<[u8]>
{
    type Item = (UdpSocket, B, usize, SocketAddr);
    type Error = (UdpSocket, B, io::Error);
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(mut state) = self.0.take() {
            if let Some(mut monitor) = state.monitor.take() {
                match monitor.poll() {
                    Err(e) => Err((state.socket, state.buf, into_io_error(e))),
                    Ok(Async::NotReady) => {
                        state.monitor = Some(monitor);
                        self.0 = Some(state);
                        Ok(Async::NotReady)
                    }
                    Ok(Async::Ready(())) => {
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
                    Ok(Some((size, addr))) => {
                        Ok(Async::Ready((state.socket, state.buf, size, addr)))
                    }
                }
            }
        } else {
            panic!("Cannot poll RecvFrom twice")
        }
    }
}

// TODO: name
#[derive(Debug)]
pub enum UdpBind {
    Registering(SharableEvented<mio::udp::UdpSocket>, poll::Register),
    Error(io::Error),
    Polled,
}
impl Future for UdpBind {
    type Item = UdpSocket;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(self, UdpBind::Polled) {
            UdpBind::Registering(socket, mut future) => {
                if let Async::Ready(handle) = future.poll().map_err(into_io_error)? {
                    Ok(Async::Ready(UdpSocket::new(socket, handle)))
                } else {
                    *self = UdpBind::Registering(socket, future);
                    Ok(Async::NotReady)
                }
            }
            UdpBind::Error(e) => Err(e),
            UdpBind::Polled => panic!("Cannot poll UdpBind twice"),
        }
    }
}

// TODO: 共通化
fn into_io_error<E: error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, Box::new(error))
}
