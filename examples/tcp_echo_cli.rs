// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

extern crate clap;
extern crate futures;
extern crate handy_io;
extern crate fibers;

use clap::{App, Arg};
use fibers::{Spawn, Executor, ThreadPoolExecutor};
use futures::{Future, Stream};
use handy_io::io::{AsyncWrite, AsyncRead};
use handy_io::pattern::{Pattern, AllowPartial};

fn main() {
    let matches = App::new("tcp_echo_cli")
        .arg(Arg::with_name("SERVER_HOST")
            .short("h")
            .takes_value(true)
            .default_value("127.0.0.1"))
        .arg(Arg::with_name("SERVER_PORT")
            .short("p")
            .takes_value(true)
            .default_value("3000"))
        .get_matches();
    let host = matches.value_of("SERVER_HOST").unwrap();
    let port = matches.value_of("SERVER_PORT").unwrap();
    let addr = format!("{}:{}", host, port).parse().expect("Invalid TCP address");

    let mut executor = ThreadPoolExecutor::new().expect("Cannot create Executor");
    let handle = executor.handle();
    let monitor =
        executor.spawn_monitor(fibers::net::TcpStream::connect(addr).and_then(move |stream| {
            println!("# CONNECTED: {}", addr);
            let (reader, writer) = (stream.clone(), stream);

            // writer
            let stdin_stream = fibers::io::stdin()
                .async_read_stream(vec![0; 1024].allow_partial().repeat());
            handle.spawn(stdin_stream.map_err(|(_, e)| e)
                .fold(writer, |writer, (mut buf, size)| {
                    buf.truncate(size);
                    writer.async_write_all(buf).map(|(w, _)| w).map_err(|(_, _, e)| e)
                })
                .then(|r| {
                    println!("# Writer finished: {:?}", r);
                    Ok(())
                }));

            // reader
            let stream = vec![0; 1024].allow_partial().repeat();
            reader.async_read_stream(stream)
                .map_err(|(_, e)| e)
                .for_each(|(mut buf, len)| {
                    buf.truncate(len);
                    println!("{}", String::from_utf8(buf).expect("Invalid UTF-8"));
                    Ok(())
                })
        }));
    let result = executor.run_fiber(monitor).expect("Execution failed");
    println!("# Disconnected: {:?}", result);
}
