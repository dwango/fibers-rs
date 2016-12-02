// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! I/O events polling functionalities (for developers).
//!
//! This module is mainly exported for developers.
//! So, usual users do not need to be conscious.
//!
//! # Implementation Details
//!
//! This module is a wrapper of the [mio](https://github.com/carllerche/mio) crate.
pub use internal::io_poll::{Poller, PollerHandle};
pub use internal::io_poll::{Interest, Register, EventedHandle, EventedLock};
pub use internal::io_poll::poller::DEFAULT_EVENTS_CAPACITY;
