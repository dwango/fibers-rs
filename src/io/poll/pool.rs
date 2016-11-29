use std::io;
use std::thread;
use num_cpus;

use super::PollerHandle;

#[derive(Debug)]
pub struct PollerPool {

}
impl PollerPool {
    pub fn new() -> io::Result<Self> {
        Self::with_thread_count(num_cpus::get())
    }
    pub fn with_thread_count(count: usize) -> io::Result<Self> {
        panic!()
    }
}

#[derive(Debug, Clone)]
pub struct PollerPoolHandle;
impl PollerPoolHandle {
    pub fn get_poller(&self) -> PollerHandle {
        panic!()
    }
}
