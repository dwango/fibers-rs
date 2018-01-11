fibers
======

[![fibers](http://meritbadge.herokuapp.com/fibers)](https://crates.io/crates/fibers)
[![Documentation](https://docs.rs/fibers/badge.svg)](https://docs.rs/fibers)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

This is a library to execute a number of lightweight asynchronous tasks (a.k.a, fibers).

[Documentation](https://docs.rs/fibers/)

Note that `fibers` heavily uses [futures](https://github.com/alexcrichton/futures-rs) to
represent asynchronous task. If you are not familiar with it,
we recommend that you refer the `README.md` and `TUTORIAL.md` of [futures](https://github.com/alexcrichton/futures-rs)
before reading the following.

This library also uses [mio](https://github.com/carllerche/mio) to achieve
efficient asynchronous I/O handling (mainly for networking primitives).
However, its existence is hidden from the user, so you do not usually have to worry about it.

---

`Future` is an excellent way to represent asynchronous task.
It is intuitive, easily composed with other futures to represent a complicated task,
without runtime overhead.
But, there is a remaining problem that
"How to efficiently execute (possibility a very large amount of) concurrent tasks?".
`fibers` is an answer to the problem.

Conceptually, the responsibility of `fibers` is very simple.
It represents an asynchronous task (a.k.a., fiber) as a future instance.
And there is an executor that takes futures and executes them like following.

```rust
// Creates an executor.
let mut executor = ThreadPoolExecutor::new().unwrap();

// Spawns fibers (i.e., passes futures to the executor).
executor.spawn(futures::lazy(|| { println!("Hello"); Ok(())} ));
executor.spawn(futures::lazy(|| { println!("World!"); Ok(())} ));

// Executes them.
executor.run().unwrap();
```

Fibers may be run on different background threads, but the user does not need to notice it.
If it runs on machines with a large number of processors, performance will improve naturally.

Roughly speaking, if a future returns `Async::NotReady` response to a call of `Future::poll` method,
the fiber associated with the future will move into the "waiting" state.
Then, it is suspended (descheduled) until any event in which the future is interested happens
(e.g., waits until data is arrived on a target TCP socket).
Finally, if a future returns `Async::Ready` response, the fiber will be regarded as completed and
the executor will drop the fiber.

This library provides primitives for writing programs in an efficient
asynchronous fashion (See documentations of [net](https://docs.rs/fibers/0.1/fibers/net/index.html),
[sync](https://docs.rs/fibers/0.1/fibers/sync/index.html), [io](https://docs.rs/fibers/0.1/fibers/io/index.html),
[time](https://docs.rs/fibers/0.1/fibers/time/index.html) modules for more details).

The main concern of this library is "how to execute fibers".
So it is preferred to use external crates (e.g., [handy_async](https://github.com/sile/handy_async))
to describe "how to represent asynchronous tasks".


Installation
------------

First, add following lines to your `Cargo.toml`:

```toml
[dependencies]
fibers = "0.1"
futures = "0.1"  # In practical, `futures` is mandatory to use `fibers`.
```

Next, add this to your crate:

```rust
extern crate fibers;
extern crate futures;
```

Several runnable examples are given in the next section.


Examples
--------

The following are examples of writing code to perform asynchronous tasks.

Other examples are found in "fibers/examples" directory.
And you can run an example by executing the following command.

```bash
$ cargo run --example ${EXAMPLE_NAME}
```

### Calculation of fibonacci numbers

```rust
// See also: "fibers/examples/fibonacci.rs"
extern crate fibers;
extern crate futures;

use fibers::{Spawn, Executor, ThreadPoolExecutor};
use futures::Future;

fn main() {
    // Creates an executor instance.
    let mut executor = ThreadPoolExecutor::new().unwrap();

    // Creates a future which will calculate the fibonacchi number of `10`.
    let input_number = 10;
    let future = fibonacci(input_number, executor.handle());

    // Spawns and executes the future (fiber).
    let monitor = executor.spawn_monitor(future);
    let answer = executor.run_fiber(monitor).unwrap();

    // Checkes the answer.
    assert_eq!(answer, Ok(55));
}

fn fibonacci<H: Spawn + Clone>(n: usize, handle: H) -> Box<Future<Item=usize, Error=()> + Send> {
    if n < 2 {
        Box::new(futures::finished(n))
    } else {
        /// Spawns a new fiber per recursive call.
        let f0 = handle.spawn_monitor(fibonacci(n - 1, handle.clone()));
        let f1 = handle.spawn_monitor(fibonacci(n - 2, handle.clone()));
        Box::new(f0.join(f1).map(|(a0, a1)| a0 + a1).map_err(|_| ()))
    }
}
```

### TCP Echo Server and Client

An example of TCP echo server listening at the address "127.0.0.1:3000":

```rust
// See also: "fibers/examples/tcp_echo_srv.rs"
extern crate fibers;
extern crate futures;
extern crate handy_async;

use std::io;
use fibers::{Spawn, Executor, ThreadPoolExecutor};
use fibers::net::TcpListener;
use futures::{Future, Stream};
use handy_async::io::{AsyncWrite, ReadFrom};
use handy_async::pattern::AllowPartial;

fn main() {
    let server_addr = "127.0.0.1:3000".parse().expect("Invalid TCP bind address");

    let mut executor = ThreadPoolExecutor::new().expect("Cannot create Executor");
    let handle0 = executor.handle();
    let monitor = executor.spawn_monitor(TcpListener::bind(server_addr)
        .and_then(move |listener| {
            println!("# Start listening: {}: ", server_addr);

            // Creates a stream of incoming TCP client sockets
            listener.incoming().for_each(move |(client, addr)| {
                // New client is connected.
                println!("# CONNECTED: {}", addr);
                let handle1 = handle0.clone();

                // Spawns a fiber to handle the client.
                handle0.spawn(client.and_then(move |client| {
                        // For simplicity, splits reading process and
                        // writing process into differrent fibers.
                        let (reader, writer) = (client.clone(), client);
                        let (tx, rx) = fibers::sync::mpsc::channel();

                        // Spawns a fiber for the writer side.
                        // When a message is arrived in `rx`,
                        // this fiber sends it back to the client.
                        handle1.spawn(rx.map_err(|_| -> io::Error { unreachable!() })
                            .fold(writer, |writer, buf: Vec<u8>| {
                                println!("# SEND: {} bytes", buf.len());
                                writer.async_write_all(buf).map(|(w, _)| w).map_err(|e| e.into_error())
                            })
                            .then(|r| {
                                println!("# Writer finished: {:?}", r);
                                Ok(())
                            }));

                        // The reader side is executed in the current fiber.
                        let stream = vec![0;1024].allow_partial().into_stream(reader);
                        stream.map_err(|e| e.into_error())
                            .fold(tx, |tx, (mut buf, len)| {
                                buf.truncate(len);
                                println!("# RECV: {} bytes", buf.len());

                                // Sends received  to the writer half.
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
        }));
    let result = executor.run_fiber(monitor).expect("Execution failed");
    println!("# Listener finished: {:?}", result);
}
```

And the code of the client side:

```rust
// See also: "fibers/examples/tcp_echo_cli.rs"
extern crate fibers;
extern crate futures;
extern crate handy_async;

use fibers::{Spawn, Executor, InPlaceExecutor};
use fibers::net::TcpStream;
use futures::{Future, Stream};
use handy_async::io::{AsyncWrite, ReadFrom};
use handy_async::pattern::AllowPartial;

fn main() {
    let server_addr = "127.0.0.1:3000".parse().unwrap();

    // `InPlaceExecutor` is suitable to execute a few fibers.
    // It does not create any background threads,
    // so the overhead to manage fibers is lower than `ThreadPoolExecutor`.
    let mut executor = InPlaceExecutor::new().expect("Cannot create Executor");
    let handle = executor.handle();

    // Spawns a fiber for echo client.
    let monitor = executor.spawn_monitor(TcpStream::connect(server_addr).and_then(move |stream| {
        println!("# CONNECTED: {}", server_addr);
        let (reader, writer) = (stream.clone(), stream);

        // Writer: It sends data read from the standard input stream to the connected server.
        let stdin_stream = vec![0; 256].allow_partial().into_stream(fibers::io::stdin());
        handle.spawn(stdin_stream.map_err(|e| e.into_error())
            .fold(writer, |writer, (mut buf, size)| {
                buf.truncate(size);
                writer.async_write_all(buf).map(|(w, _)| w).map_err(|e| e.into_error())
            })
            .then(|r| {
                println!("# Writer finished: {:?}", r);
                Ok(())
            }));

        // Reader: It outputs data received from the server to the standard output stream.
        let stream = vec![0; 256].allow_partial().into_stream(reader);
        stream.map_err(|e| e.into_error())
            .for_each(|(mut buf, len)| {
                buf.truncate(len);
                println!("{}", String::from_utf8(buf).expect("Invalid UTF-8"));
                Ok(())
            })
    }));

    // Runs until the above fiber is terminated (i.e., The TCP stream is disconnected).
    let result = executor.run_fiber(monitor).expect("Execution failed");
    println!("# Disconnected: {:?}", result);
}
```

Real examples using `fibers`
----------------------------

Here is a list of known projects using `fibers`:

- [erl_dist](https://github.com/sile/erl_dist): Erlang Distribution Protocol Implementation
- [miasht](https://github.com/sile/miasht): Minimum Asynchronous HTTP server/client
- [rustun](https://github.com/sile/rustun): STUN(RFC5389) server/client Implementation
- [rusturn](https://github.com/sile/rusturn): TURN(RFC5766) server/client Implementation

License
-------

This library is released under the MIT License.

See the [LICENSE](LICENSE) file for full license information.

Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
