//! I/O events polling functionalities.
//!
//! # Implementation Details
//!
//! This module is a wrapper of the [mio](https://github.com/carllerche/mio) crate.
pub use internal::io_poll::{Poller, PollerHandle};
pub use internal::io_poll::{Interest, Register, EventedHandle, EventedLock};
pub use internal::io_poll::poller::DEFAULT_EVENTS_CAPACITY;
