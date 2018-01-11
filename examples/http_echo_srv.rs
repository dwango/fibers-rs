// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

extern crate clap;
extern crate fibers;
extern crate futures;
extern crate handy_async;
extern crate httparse;

use std::io;
use clap::{App, Arg};
use handy_async::io::{ReadFrom, WriteInto};
use handy_async::pattern::{self, Branch, Pattern, Window};
use futures::{Future, Stream};
use fibers::{Executor, Spawn, ThreadPoolExecutor};

fn main() {
    let matches = App::new("http_echo_srv")
        .arg(
            Arg::with_name("PORT")
                .short("p")
                .takes_value(true)
                .default_value("3000"),
        )
        .arg(Arg::with_name("VERBOSE").short("v"))
        .get_matches();
    let port = matches.value_of("PORT").unwrap();
    let addr = format!("0.0.0.0:{}", port)
        .parse()
        .expect("Invalid TCP bind address");
    let verbose = matches.is_present("VERBOSE");

    let executor = ThreadPoolExecutor::new().expect("Cannot create Executor");
    let handle = executor.handle();
    executor.spawn(
        fibers::net::TcpListener::bind(addr)
            .and_then(move |listener| {
                println!("# Start listening: {}: ", addr);
                listener.incoming().for_each(move |(client, addr)| {
                    if verbose {
                        println!("# CONNECTED: {}", addr);
                    }
                    handle.spawn(
                        client
                            .and_then(|client| {
                                let read_request_pattern = pattern::read::until(|buf, _is_eos| {
                                    // Read header
                                    let mut headers = [httparse::EMPTY_HEADER; 16];
                                    let mut req = httparse::Request::new(&mut headers);
                                    let status = req.parse(buf).map_err(into_io_error)?;
                                    if status.is_partial() {
                                        Ok(None)
                                    } else {
                                        let content_len = get_content_length(req.headers)?;
                                        let content_offset = status.unwrap();
                                        Ok(Some((content_offset, content_len)))
                                    }
                                }).and_then(
                                    |(mut buf, (content_offset, content_len))| {
                                        // Read content
                                        let read_size = buf.len();
                                        let request_len = content_offset + content_len;
                                        buf.resize(request_len, 0);
                                        let buf = Window::new(buf).skip(read_size);
                                        let pattern = if read_size == request_len {
                                            Branch::A(Ok(buf)) as Branch<_, _>
                                        } else {
                                            Branch::B(buf)
                                        };
                                        pattern.map(move |buf: Window<_>| {
                                            buf.set_start(content_offset)
                                        })
                                    },
                                );
                                read_request_pattern.read_from(client).then(|result| {
                                    // Write response
                                    let (client, result) = match result {
                                        Ok((client, content)) => (client, Ok(content)),
                                        Err(error) => {
                                            let (client, io_error) = error.unwrap();
                                            (client, Err(io_error))
                                        }
                                    };
                                    let pattern = Pattern::and_then(result, |content| {
                                        format!(
                                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                            content.as_ref().len()
                                        ).chain(content)
                                            .map(|_| ())
                                    }).or_else(
                                        |error: io::Error| {
                                            let message = error.to_string();
                                            format!(
                                                "HTTP/1.1 500 OK\r\nContent-Length: {}\r\n\r\n",
                                                message.len()
                                            ).chain(message)
                                                .map(|_| ())
                                        },
                                    );
                                    pattern.write_into(client).map_err(|e| e.into_error())
                                })
                            })
                            .then(move |r| {
                                if verbose {
                                    println!("# Client finished: {:?}", r);
                                }
                                Ok(())
                            }),
                    );
                    Ok(())
                })
            })
            .then(|r| {
                println!("# Listener finished: {:?}", r);
                Ok(())
            }),
    );
    executor.run().expect("Execution failed");
}

fn get_content_length(headers: &[httparse::Header]) -> io::Result<usize> {
    headers
        .iter()
        .find(|h| h.name.to_lowercase() == "content-length")
        .map(|h| {
            std::str::from_utf8(h.value)
                .map_err(into_io_error)
                .and_then(|s| s.parse::<usize>().map_err(into_io_error))
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "No content-length header",
            ))
        })
}

fn into_io_error<E: std::error::Error + Send + Sync + 'static>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, Box::new(e))
}
