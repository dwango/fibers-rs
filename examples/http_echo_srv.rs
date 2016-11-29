extern crate clap;
extern crate httparse;
extern crate futures;
extern crate handy_io;
extern crate fibers;

use clap::{App, Arg};
use handy_io::io::{AsyncWrite, AsyncRead};
use futures::{Future, Stream};
use fibers::fiber::Executor;

fn main() {
    let matches = App::new("http_echo_srv")
        .arg(Arg::with_name("PORT")
            .short("p")
            .takes_value(true)
            .default_value("3000"))
        .arg(Arg::with_name("VERBOSE").short("v"))
        .get_matches();
    let port = matches.value_of("PORT").unwrap();
    let addr = format!("0.0.0.0:{}", port).parse().expect("Invalid TCP bind address");
    let verbose = matches.is_present("VERBOSE");

    let executor = Executor::new().expect("Cannot create Executor");
    let handle0 = executor.handle();
    executor.spawn(fibers::net::TcpListener::bind(addr)
        .and_then(move |listener| {
            println!("# Start listening: {}: ", addr);
            listener.incoming().for_each(move |(client, addr)| {
                if verbose {
                    println!("# CONNECTED: {}", addr);
                }
                handle0.spawn(client.and_then(|client| {
                        client.async_read_fold(vec![0; 1024], None, |_, mut buf, read_size| {
                                // TODO: Use Window
                                buf.truncate(read_size);
                                let response = {
                                    let mut headers = [httparse::EMPTY_HEADER; 16];
                                    let mut req = httparse::Request::new(&mut headers);
                                    let status = req.parse(&buf).expect("TODO");
                                    if status.is_partial() {
                                        None
                                    } else {
                                        let body_offset = status.unwrap();
                                        let response = handle_request(req, &buf[body_offset..]);
                                        Some(response)
                                    }
                                };
                                if let Some(response) = response {
                                    Err((buf, Ok(Some(response))))
                                } else {
                                    let new_len = buf.len() + read_size;
                                    buf.resize(new_len, 0);
                                    Ok((buf, None))
                                }
                            })
                            .map_err(|(_, _, e)| e)
                    })
                    .and_then(|(client, _, response)| {
                        client.async_write_all(response.unwrap()).map_err(|(_, _, e)| e)
                    })
                    .then(move |r| {
                        if verbose {
                            println!("# Client finished: {:?}", r);
                        }
                        Ok(())
                    }));
                Ok(())
            })
        })
        .then(|r| {
            println!("# Listener finished: {:?}", r);
            Ok(())
        }));
    executor.run().expect("Execution failed");
}

fn handle_request(request: httparse::Request, body: &[u8]) -> Vec<u8> {
    let mut v: Vec<_> = format!("HTTP/1.{} 200 OK\r\nContent-Length: {}\r\nConnection: \
                                 close\r\n\r\n",
                                request.version.unwrap(),
                                body.len())
        .into();
    v.extend(body);
    v
}
