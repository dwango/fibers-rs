// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

//! Synchronization primitives.
use std::sync::Arc;

use crate::fiber;
use crate::sync_atomic::AtomicCell;

pub mod mpsc;
pub mod oneshot;

#[derive(Debug, Clone)]
struct Notifier {
    unpark: Arc<AtomicCell<Option<fiber::Unpark>>>,
}
impl Notifier {
    pub fn new() -> Self {
        Notifier {
            unpark: Arc::new(AtomicCell::new(None)),
        }
    }
    pub fn r#await(&mut self) {
        loop {
            if let Some(mut unpark) = self.unpark.try_borrow_mut() {
                let context_id = fiber::with_current_context(|c| c.context_id());
                if unpark.as_ref().map(|u| u.context_id()) != context_id {
                    *unpark = fiber::with_current_context(|mut c| c.park());
                }
                return;
            }
        }
    }
    pub fn notify(&self) {
        loop {
            if let Some(mut unpark) = self.unpark.try_borrow_mut() {
                *unpark = None;
                return;
            }
        }
    }
}
