extern crate clap;
extern crate futures;
extern crate handy_io;
extern crate fibers;

use std::io;
use clap::{App, Arg};
use fibers::fiber::Executor;
use futures::{Future, Stream, Async};
use handy_io::io::{AsyncWrite, AsyncRead};
use handy_io::pattern::{Pattern, AllowPartial};

fn main() {
    let matches = App::new("echo_cli")
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

    // TODO: s/Executor/System/ (?)
    let mut executor = Executor::new().expect("Cannot create Executor");
    let (stdin_tx, stdin_rx) = fibers::sync::mpsc::channel();
    let handle = executor.handle();
    let (monitored, mut monitor) = fibers::sync::oneshot::monitor();
    executor.spawn(fibers::net::TcpStream::connect(addr)
        .and_then(move |stream| {
            println!("# CONNECTED: {}", addr);
            let (r, w) = stream.split();

            // writer
            handle.spawn(stdin_rx.map_err(|_| -> io::Error { unreachable!() })
                .fold(w, |w, buf: Vec<u8>| {
                    println!("# SEND: {} bytes", buf.len());
                    w.async_write_all(buf).map(|(w, _)| w).map_err(|(_, _, e)| e)
                })
                .then(|r| {
                    println!("# Writer finished: {:?}", r);
                    Ok(())
                }));

            // reader
            let stream = vec![0; 1024].allow_partial().repeat();
            r.async_read_stream(stream)
                .map_err(|(_, e)| e)
                .for_each(|(mut buf, len)| {
                    println!("# RECV: {} bytes", len);
                    buf.truncate(len);
                    println!("{}", String::from_utf8(buf).expect("Invalid UTF-8"));
                    Ok(())
                })
        })
        .then(|r| {
            println!("# Disconnected: {:?}", r);
            monitored.exit(r);
            Ok(())
        }));

    std::thread::spawn(move || {
        let stream = vec![0; 1024].allow_partial().repeat();
        let mut future = io::stdin().async_read_stream(stream).for_each(move |(mut buf, len)| {
            buf.truncate(len);
            stdin_tx.send(buf).expect("Cannot send input buffer");
            Ok(())
        });
        loop {
            future.poll().map_err(|(_, e)| e).expect("Standard input error");
        }
    });

    while let Ok(Async::NotReady) = monitor.poll() {
        executor.run_once().expect("Execution failed");
    }
}
