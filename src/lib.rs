extern crate mio;
extern crate rand;
extern crate futures;
extern crate splay_tree;
extern crate num_cpus;
#[macro_use]
extern crate lazy_static;

macro_rules! assert_some {
    ($e:expr) => {
        $e.expect(&format!("[{}:{}] {:?} must be a Some(..)",
                           file!(), line!(), stringify!($e)))
    }
}

macro_rules! assert_ok {
    ($e:expr) => {
        $e.expect(&format!("[{}:{}] {:?} must be a Ok(..)",
                           file!(), line!(), stringify!($e)))
    }
}

// Add executor module

pub mod io;
pub mod net;
pub mod sync;
pub mod fiber;

// TODO(?): s/timer/time/
pub mod timer;

mod internal;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
