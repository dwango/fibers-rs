// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

extern crate clap;
extern crate fibers;
extern crate futures;

use clap::{App, Arg};
use fibers::{Executor, Spawn, ThreadPoolExecutor};
use futures::Future;

fn main() {
    let matches = App::new("fibonacci")
        .arg(Arg::with_name("INPUT_NUMBER").index(1).required(true))
        .get_matches();
    let input_number = matches
        .value_of("INPUT_NUMBER")
        .unwrap()
        .parse()
        .expect("Invalid number");

    let mut executor = ThreadPoolExecutor::new().unwrap();
    let future = fibonacci(input_number, executor.handle());
    let monitor = executor.spawn_monitor(future);
    let answer = executor.run_fiber(monitor).unwrap().unwrap();
    println!("fibonacci({}) = {}", input_number, answer);
}

fn fibonacci<H: Spawn + Clone>(
    n: usize,
    handle: H,
) -> Box<Future<Item = usize, Error = ()> + Send> {
    if n < 2 {
        Box::new(futures::finished(n))
    } else {
        let f0 = handle.spawn_monitor(fibonacci(n - 1, handle.clone()));
        let f1 = handle.spawn_monitor(fibonacci(n - 2, handle.clone()));
        Box::new(f0.join(f1).map(|(a0, a1)| a0 + a1).map_err(|_| ()))
    }
}
