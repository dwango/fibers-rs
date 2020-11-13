// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Time related functionalities.
pub mod timer {
    //! Timer
    use futures::{empty as empty01, Async, Empty as Empty01, Future, Poll};
    use futures03::compat::Future01CompatExt;
    use futures03::Future as Future03;
    use futures03::{FutureExt, TryFutureExt};
    use pin_project::pin_project;
    use std::pin::Pin;
    use std::sync::mpsc::RecvError;
    use std::time;

    use crate::fiber::{self, Context};
    use crate::io::poll;

    /// A timer related extension of the `Future` trait.
    pub trait TimerExt: Sized + Future {
        /// Adds the specified timeout to this future.
        fn timeout_after(self, duration: time::Duration) -> TimeoutAfter<Self> {
            let fut03 = self.compat().timeout_after(duration).map(|x| match x {
                None => Err(None),
                Some(Ok(x)) => Ok(x),
                Some(Err(x)) => Err(Some(x)),
            });
            //let fut03 = self.map_err(Some).compat();
            TimeoutAfter {
                inner: Box::new(fut03.compat()),
            }
        }
    }
    impl<T: Future> TimerExt for T {}

    pub trait TimerExt03: Sized + Future03 {
        fn timeout_after(self, duration: time::Duration) -> TimeoutAfter03<Self> {
            let inner = tokio::time::timeout(duration, self);
            TimeoutAfter03 { inner }
        }
    }
    impl<T: Future03> TimerExt03 for T {}

    /// A future which will try executing `T` within the specified time duration.
    ///
    /// If the timeout duration passes, it will return `Err(None)`.
    /// If an error occurres before the expiration time, this will result in `Err(Some(T::Error))`.
    pub struct TimeoutAfter<T: Future + 'static> {
        inner: Box<dyn Future<Item = T::Item, Error = Option<T::Error>>>,
    }
    impl<T: Future> Future for TimeoutAfter<T> {
        type Item = T::Item;
        type Error = Option<T::Error>;
        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.inner.poll()
        }
    }

    #[pin_project]
    pub struct TimeoutAfter03<T> {
        #[pin]
        inner: tokio::time::Timeout<T>,
    }

    impl<T: Future03> Future03 for TimeoutAfter03<T> {
        type Output = Option<T::Output>;
        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            let this = self.project();
            let inner: Pin<&mut _> = this.inner;
            inner.poll(cx).map(|x| x.ok())
        }
    }

    /// A future which will expire at the specified time instant.
    ///
    /// If this object is dropped before expiration, the timer will be cancelled.
    /// Thus, for example, the repetation of setting and canceling of
    /// a timer only consumpts constant memory region.
    pub struct Timeout {
        inner: TimeoutAfter<Empty01<(), ()>>,
    }

    /// Makes a future which will expire after `delay_from_now`.
    pub fn timeout(delay_from_now: time::Duration) -> Timeout {
        Timeout {
            inner: empty01().timeout_after(delay_from_now),
        }
    }
    impl Future for Timeout {
        type Item = ();
        type Error = RecvError;
        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.inner.poll().map_err(|_| RecvError)
        }
    }

    impl std::fmt::Debug for Timeout {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Timeout").finish()
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;
        use futures::{self, Async, Future};
        use std::time::Duration;

        #[test]
        fn it_works() {
            let mut timeout = timeout(Duration::from_secs(0));
            assert_eq!(timeout.poll(), Ok(Async::Ready(())));
        }

        #[test]
        fn timeout_after_works() {
            let mut future = futures::empty::<(), ()>().timeout_after(Duration::from_secs(0));
            assert_eq!(future.poll(), Err(None));

            let mut future = futures::finished::<(), ()>(()).timeout_after(Duration::from_secs(1));
            assert_eq!(future.poll(), Ok(Async::Ready(())));

            let mut future = futures::failed::<(), ()>(()).timeout_after(Duration::from_secs(1));
            assert_eq!(future.poll(), Err(Some(())));
        }
    }
}
