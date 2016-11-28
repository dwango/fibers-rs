use super::PollerHandle;

#[derive(Debug)]
pub struct PollerPool;

#[derive(Debug, Clone)]
pub struct PollerPoolHandle;
impl PollerPoolHandle {
    pub fn get_poller(&self) -> PollerHandle {
        panic!()
    }
}
