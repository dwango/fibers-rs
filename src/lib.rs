extern crate mio;
extern crate futures;
extern crate splay_tree;
#[macro_use]
extern crate lazy_static;

#[cfg(test)]
extern crate handy_io;

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

pub mod io;
pub mod net;
pub mod sync;
pub mod fiber;
pub mod timer;
pub mod collections;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
