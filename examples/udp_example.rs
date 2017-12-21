// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

extern crate fibers;
extern crate futures;

use fibers::{Executor, InPlaceExecutor, Spawn};
use fibers::net::UdpSocket;
use fibers::sync::oneshot;
use futures::Future;

fn main() {
    let mut executor = InPlaceExecutor::new().unwrap();
    let (addr_tx, addr_rx) = oneshot::channel();

    // Spawns receiver
    let mut monitor = executor.spawn_monitor(
        UdpSocket::bind("127.0.0.1:0".parse().unwrap())
            .and_then(|socket| {
                addr_tx.send(socket.local_addr().unwrap()).unwrap();
                socket.recv_from(vec![0; 32]).map_err(|(_, _, e)| e)
            })
            .and_then(|(_, mut buf, len, addr)| {
                println!("# Recv from: {}", addr);
                buf.truncate(len);
                assert_eq!(buf, b"hello world");
                Ok(())
            }),
    );

    // Spawns sender
    executor.spawn(
        addr_rx
            .map_err(|e| panic!("{:?}", e))
            .and_then(|receiver_addr| {
                UdpSocket::bind("127.0.0.1:0".parse().unwrap())
                    .and_then(move |socket| {
                        println!("# Send to: {}", receiver_addr);
                        socket
                            .send_to(b"hello world", receiver_addr)
                            .map_err(|e| panic!("{:?}", e))
                    })
                    .map_err(|e| panic!("{:?}", e))
                    .map(|_| ())
            }),
    );

    // Runs until the receiver fiber exits.
    while monitor.poll().unwrap().is_not_ready() {
        executor.run_once().unwrap();
    }
    println!("# Succeeded");
}
