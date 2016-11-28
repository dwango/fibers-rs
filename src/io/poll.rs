#[derive(Debug)]
pub struct Poller;

#[derive(Debug, Clone)]
pub struct PollerHandle;
impl PollerHandle {
    pub fn is_alive(&self) -> bool {
        true
    }
}

#[derive(Debug)]
pub struct PollerPool;

#[derive(Debug, Clone)]
pub struct PollerPoolHandle;
impl PollerPoolHandle {
    pub fn get_poller(&self) -> PollerHandle {
        PollerHandle
    }
}
