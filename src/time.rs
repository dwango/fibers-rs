// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Time related functionalities.
pub mod timer {
    //! Timer
    use std::time;
    pub use internal::io_poll::Timeout;
    use internal::io_poll;
    use fiber;

    /// Makes a future which will expire after `delay_from_now`.
    ///
    /// # Panics
    ///
    /// If this function is called on the outside of a fiber, it may crash.
    pub fn timeout(delay_from_now: time::Duration) -> Timeout {
        assert_some!(fiber::with_current_context(|mut c| {
            io_poll::poller::set_timeout(c.poller(), delay_from_now)
        }))
    }
}
