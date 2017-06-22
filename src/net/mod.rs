// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Networking primitives for TCP/UDP communication.
//!
//! # Implementation Details
//!
//! Basically, the structures in this module are simple wrapper of
//! the [mio](https://github.com/carllerche/mio)'s counterparts.
//! The former implement [futures](https://github.com/alexcrichton/futures-rs) interface
//! to facilitate writing code for asynchronous I/O.
//!
//! If a socket is not available (i.e., may block) at the time the `Future::poll` method
//! for the corresponding future is called, it will suspend (deschedule) current fiber by invoking
//! the `fibers::fiber::park` function.
//! Then it sends a request to a `mio::Poll` to monitor an event that
//! indicates the socket becomes available.
//! After that, when the event happens, the fiber will be resumed and
//! rescheduled for next execution.
use std::io;
use std::mem;
use std::fmt;
use std::error;
use std::net::SocketAddr;
use mio;
use futures::{Poll, Async, Future};

pub use self::udp::UdpSocket;
pub use self::tcp::{TcpListener, TcpStream};

use fiber;
use internal::io_poll::{EventedHandle, Register};

pub mod futures {
    //! Implementations of `futures::Future` trait.
    pub use super::udp::{UdpSocketBind, SendTo, RecvFrom};
    pub use super::tcp::{TcpListenerBind, Connect, Connected};
}
pub mod streams {
    //! Implementations of `futures::Stream` trait.
    pub use super::tcp::Incoming;
}

mod udp;
mod tcp;

enum Bind<F, T> {
    Bind(SocketAddr, F),
    Registering(Register<T>),
    Polled,
}
impl<F, T> Future for Bind<F, T>
where
    F: FnOnce(&SocketAddr) -> io::Result<T>,
    T: mio::Evented + Send + 'static,
{
    type Item = EventedHandle<T>;
    type Error = io::Error;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(self, Bind::Polled) {
            Bind::Bind(addr, bind) => {
                let socket = bind(&addr)?;
                let register = assert_some!(fiber::with_current_context(
                    |mut c| c.poller().register(socket),
                ));
                *self = Bind::Registering(register);
                self.poll()
            }
            Bind::Registering(mut future) => {
                if let Async::Ready(handle) = future.poll().map_err(into_io_error)? {
                    Ok(Async::Ready(handle))
                } else {
                    *self = Bind::Registering(future);
                    Ok(Async::NotReady)
                }
            }
            Bind::Polled => panic!("Cannot poll Bind twice"),
        }
    }
}
impl<F, T> fmt::Debug for Bind<F, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Bind::Bind(addr, _) => write!(f, "Bind::Bind({:?}, _)", addr),
            Bind::Registering(_) => write!(f, "Bind::Registering(_)"),
            Bind::Polled => write!(f, "Bind::Polled"),
        }
    }
}

fn into_io_error<E: error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, Box::new(error))
}
