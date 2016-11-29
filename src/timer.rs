use std::time;

pub use io::poll::Timeout;

use fiber;

// TODO: doc for panic
pub fn timeout(delay_from_now: time::Duration) -> Timeout {
    assert_some!(fiber::with_poller(|poller| poller.set_timeout(delay_from_now)))
}
