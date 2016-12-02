fibers
======

[![fibers](http://meritbadge.herokuapp.com/fibers)](https://crates.io/crates/fibers)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`fibers` is a library to execute ... .

[Documentation](https://docs.rs/fibers/)

A fiber is a future which executed by ... .
(expect users familliar with the futures)
(mio is hidden and no prerequirement)

futures is gread ... . but a remaiing problems is
"how exeuted the futures efficiently (and especially) with asynchronous I/O ?"

fiber is an answer about it.

erlang like, green thread, ....

intuitive, can use like ordinay threads(?).

and main concern of this library is "how execute and schedule the fibers(futures)".
and it is recommened to use other crate like handy_io.

asynchronous i/o and other asynchronous primitives.

multithread support ... . thread transparent
(for example future and i/o primitive free to moves between fibers) .

intuitive and easy to use.


Installation
------------

First, add following lines to your `Cargo.toml`:

```toml
[dependencies]
fibers = "0.1"
futures = "0.1"
```

Next, add this to your crate:

```rust
extern crate fibers;
extern crate futures;
```

See following examples for ... .
And examples directory in ... also has some ... .


Examples
--------

### Calculation of fibonacci numbers

```rust
// NOTE: An running example is found in examples/...
extern crate fibers;
extern crate futures;

use fibers::{Spawn, Executor, ThreadPoolExecutor};
use futures::{Future, BoxFuture};

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

fn fibonacci<H: Spawn + Clone>(n: usize, handle: H) -> BoxFuture<usize, ()> {
    if n < 2 {
        futures::finished(n).boxed()
    } else {
        /// Spawns a new fiber per recursive call.
        let f0 = handle.spawn_monitor(fibonacci(n - 1, handle.clone()));
        let f1 = handle.spawn_monitor(fibonacci(n - 2, handle.clone()));
        f0.join(f1).map(|(a0, a1)| a0 + a1).map_err(|_| ()).boxed()
    }
}
```

### TCP Echo Server and Client

server:

```rust
```

client(read from standard input ...):

```rust
```

---

Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
