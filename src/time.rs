//! Time related functionalities.
pub mod timer {
    //! Timer
    use std::time;
    pub use io::poll::Timeout;
    use fiber;

    /// Makes a future which will expire after `delay_from_now`.
    ///
    /// # Panics
    ///
    /// If this function is called on the outside of a fiber, it may crash.
    pub fn timeout(delay_from_now: time::Duration) -> Timeout {
        assert_some!(fiber::with_poller(|poller| poller.set_timeout(delay_from_now)))
    }
}
