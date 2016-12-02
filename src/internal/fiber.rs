// Copyright (c) 2016 DWANGO Co., Ltd. All Rights Reserved.
// See the LICENSE file at the top-level directory of this distribution.

use std::fmt;
use futures::BoxFuture;

pub type FiberFuture = BoxFuture<(), ()>;

pub struct Task(pub FiberFuture);
impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Task(_)")
    }
}
