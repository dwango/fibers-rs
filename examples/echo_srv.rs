extern crate clap;
extern crate futures;
extern crate handy_io;
extern crate fibers;

use std::io;
use clap::{App, Arg};
use fibers::fiber::Executor;
use futures::{Future, Stream};
use handy_io::io::{AsyncWrite, AsyncRead};
use handy_io::pattern::{Pattern, AllowPartial};

fn main() {
    let matches = App::new("echo_srv")
        .arg(Arg::with_name("PORT")
            .short("p")
            .takes_value(true)
            .default_value("3000"))
        .get_matches();
    let port = matches.value_of("PORT").unwrap();
    let addr = format!("0.0.0.0:{}", port).parse().expect("Invalid TCP bind address");

    let executor = Executor::new().expect("Cannot create Executor");
    let handle0 = executor.handle();
    executor.spawn(fibers::net::TcpListener::bind(addr)
        .and_then(move |listener| {
            println!("# Start listening: {}: ", addr);
            listener.incoming().for_each(move |(client, addr)| {
                println!("# CONNECTED: {}", addr);
                let handle1 = handle0.clone();
                handle0.spawn(client.and_then(move |client| {
                        let (r, w) = (client.clone(), client);
                        let (tx, rx) = fibers::sync::mpsc::channel();

                        // writer
                        handle1.spawn(rx.map_err(|_| -> io::Error { unreachable!() })
                            .fold(w, |w, buf: Vec<u8>| {
                                println!("# SEND: {} bytes", buf.len());
                                w.async_write_all(buf).map(|(w, _)| w).map_err(|(_, _, e)| e)
                            })
                            .then(|r| {
                                println!("# Writer finished: {:?}", r);
                                Ok(())
                            }));

                        // reader
                        let stream = vec![0;1024].allow_partial().repeat();
                        r.async_read_stream(stream)
                            .map_err(|(_, e)| e)
                            .fold(tx, |tx, (mut buf, len)| {
                                buf.truncate(len);
                                println!("# RECV: {} bytes", buf.len());
                                tx.send(buf).expect("Cannot send");
                                Ok(tx) as io::Result<_>
                            })
                    })
                    .then(|r| {
                        println!("# Client finished: {:?}", r);
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
