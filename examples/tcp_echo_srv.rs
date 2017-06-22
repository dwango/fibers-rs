// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

extern crate clap;
extern crate futures;
extern crate handy_async;
extern crate fibers;

use std::io;
use clap::{App, Arg};
use fibers::{Spawn, Executor, ThreadPoolExecutor};
use futures::{Future, Stream};
use handy_async::io::{AsyncWrite, ReadFrom};
use handy_async::pattern::AllowPartial;

fn main() {
    let matches = App::new("tcp_echo_srv")
        .arg(
            Arg::with_name("PORT")
                .short("p")
                .takes_value(true)
                .default_value("3000"),
        )
        .get_matches();
    let port = matches.value_of("PORT").unwrap();
    let addr = format!("0.0.0.0:{}", port).parse().expect(
        "Invalid TCP bind address",
    );

    let mut executor = ThreadPoolExecutor::new().expect("Cannot create Executor");
    let handle0 = executor.handle();
    let monitor = executor.spawn_monitor(fibers::net::TcpListener::bind(addr).and_then(
        move |listener| {
            println!("# Start listening: {}: ", addr);
            listener.incoming().for_each(move |(client, addr)| {
                println!("# CONNECTED: {}", addr);
                let handle1 = handle0.clone();
                handle0.spawn(
                    client
                        .and_then(move |client| {
                            let (reader, writer) = (client.clone(), client);
                            let (tx, rx) = fibers::sync::mpsc::channel();

                            // writer
                            handle1.spawn(
                                rx.map_err(|_| -> io::Error { unreachable!() })
                                    .fold(writer, |writer, buf: Vec<u8>| {
                                        println!("# SEND: {} bytes", buf.len());
                                        writer.async_write_all(buf).map(|(w, _)| w).map_err(|e| {
                                            e.into_error()
                                        })
                                    })
                                    .then(|r| {
                                        println!("# Writer finished: {:?}", r);
                                        Ok(())
                                    }),
                            );

                            // reader
                            let stream = vec![0; 1024].allow_partial().into_stream(reader);
                            stream.map_err(|e| e.into_error()).fold(
                                tx,
                                |tx, (mut buf, len)| {
                                    buf.truncate(len);
                                    println!("# RECV: {} bytes", buf.len());
                                    tx.send(buf).expect("Cannot send");
                                    Ok(tx) as io::Result<_>
                                },
                            )
                        })
                        .then(|r| {
                            println!("# Client finished: {:?}", r);
                            Ok(())
                        }),
                );
                Ok(())
            })
        },
    ));
    let result = executor.run_fiber(monitor).expect("Execution failed");
    println!("# Listener finished: {:?}", result);
}
