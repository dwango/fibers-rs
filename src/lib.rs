extern crate mio;
extern crate futures;

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

pub mod sync;

#[cfg(test)]
mod tests {

    #[test]
    fn it_works() {}
}
