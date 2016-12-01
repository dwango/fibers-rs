extern crate fibers;
extern crate futures;
extern crate handy_io;

use fibers::{Spawn, Executor, ThreadPoolExecutor};
use fibers::net::{TcpListener, TcpStream};
use fibers::sync::oneshot;
use futures::{Future, Stream};
use handy_io::io::{AsyncWrite, AsyncRead};

fn main() {
    let mut executor = ThreadPoolExecutor::new().unwrap();
    let handle = executor.handle();
    let (addr_tx, addr_rx) = oneshot::channel();

    // Spawns TCP listener
    executor.spawn(TcpListener::bind("127.0.0.1:0".parse().unwrap())
        .and_then(|listener| {
            let addr = listener.local_addr().unwrap();
            println!("# Start listening: {}", addr);
            addr_tx.send(addr).unwrap();
            listener.incoming()
                .for_each(move |(client, addr)| {
                    println!("# Accepted: {}", addr);
                    handle.spawn(client.map_err(|e| panic!("{:?}", e)).and_then(|client| {
                        client.async_write_all(b"Hello World!")
                            .map_err(|e| panic!("{:?}", e))
                            .map(|_| ())
                    }));
                    Ok(())
                })
        })
        .map_err(|e| panic!("{:?}", e)));

    // Spawns TCP client
    let mut monitor = executor.spawn_monitor(addr_rx.map_err(|e| panic!("{:?}", e))
        .and_then(|server_addr| {
            TcpStream::connect(server_addr).and_then(move |stream| {
                println!("# Connected: {}", server_addr);
                stream.async_read(vec![0; 32])
                    .map(|(_, mut buf, size)| {
                        buf.truncate(size);
                        assert_eq!(buf, b"Hello World!");
                    })
                    .map_err(|e| panic!("{:?}", e))
            })
        }));

    // Runs until the TCP client exits
    while monitor.poll().unwrap().is_not_ready() {
        executor.run_once().unwrap();
    }
    println!("# Succeeded");
}
