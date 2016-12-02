// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Non-blocking variants of standard I/O streams.
use std::io::{self, Read};
use std::error;
use std::thread;
use std::sync::mpsc as std_mpsc;
use futures::{Async, Stream};

use sync::mpsc as fibers_mpsc;

macro_rules! break_if_err {
    ($e:expr) => {
        match $e {
            Err(_) => break,
            Ok(v) => v
        }
    }
}

/// Returns a non-blocking variant of the standard input stream (i.e., `std::io::Stdin`).
///
/// This stream returns the `ErrorKind::WouldBlock` error, if an operation on it would block.
pub fn stdin() -> Stdin {
    let (req_tx, req_rx) = std_mpsc::channel();
    let (res_tx, res_rx) = fibers_mpsc::channel();
    thread::spawn(move || {
        // # (1) Lock Phase
        while let Ok(x) = req_rx.recv() {
            assert_eq!(x, 0);
            let stdin = io::stdin();
            let mut locked_stdin = stdin.lock();

            // # (2) Readability Check Phase
            break_if_err!(locked_stdin.read(&mut []));
            break_if_err!(res_tx.send(Ok(Vec::new())));

            // # (3) Read Phase
            let required_size = break_if_err!(req_rx.recv());
            let mut buf = vec![0; required_size];
            let result = locked_stdin.read(&mut buf).map(|read_size| {
                buf.truncate(read_size);
                buf
            });
            break_if_err!(res_tx.send(result));
        }
    });
    Stdin {
        lock_requested: false,
        req_tx: req_tx,
        res_rx: res_rx,
    }
}

/// A non-blocking variant of the standard input stream (i.e., `std::io::Stdin`).
///
/// This is created by calling `fibers::io::stdin` function.
#[derive(Debug)]
pub struct Stdin {
    lock_requested: bool,
    req_tx: std_mpsc::Sender<usize>,
    res_rx: fibers_mpsc::Receiver<io::Result<Vec<u8>>>,
}
impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.lock_requested {
            self.lock_requested = true;
            self.req_tx.send(0).map_err(into_io_error)?;
            return Err(would_block());
        }
        match self.res_rx.poll().expect("unreachable") {
            Async::NotReady => Err(would_block()),
            Async::Ready(None) => Err(unexpected_eof()),
            Async::Ready(Some(Err(e))) => Err(e),
            Async::Ready(Some(Ok(_))) => {
                self.lock_requested = false;
                self.req_tx.send(buf.len()).map_err(into_io_error)?;
                loop {
                    if let Async::Ready(result) = self.res_rx.poll().expect("unreachable") {
                        let result = if let Some(result) = result {
                            result
                        } else {
                            return Err(unexpected_eof());
                        };
                        return match result {
                            Err(e) => Err(e),
                            Ok(data) => {
                                let read_size = data.len();
                                buf[..read_size].copy_from_slice(&data[..]);
                                Ok(read_size)
                            }
                        };
                    }
                }
            }
        }
    }
}

fn would_block() -> io::Error {
    io::Error::new(io::ErrorKind::WouldBlock, "I/O operation would block")
}

fn unexpected_eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof,
                   "I/O thread unexpectedly terminated")
}

fn into_io_error<E: error::Error + Send + Sync + 'static>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, Box::new(error))
}
