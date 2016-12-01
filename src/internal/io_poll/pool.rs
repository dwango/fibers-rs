use std::io;
use std::time;
use std::thread;
use std::sync::{Arc, Mutex};
use rand;
use futures::{Async, Future};

use io::poll;
use sync::oneshot::{self, Link};
use super::PollerHandle;

#[derive(Debug)]
pub struct PollerPool {
    links: Vec<Link<io::Error>>,
    pollers: Arc<Mutex<Vec<PollerHandle>>>,
}
impl PollerPool {
    pub fn with_thread_count(count: usize) -> io::Result<Self> {
        assert!(count > 0);
        let mut links = Vec::new();
        let mut pollers = Vec::new();
        for _ in 0..count {
            let (link0, mut link1) = oneshot::link();
            let mut poller = poll::Poller::new()?;
            links.push(link0);
            pollers.push(poller.handle());
            thread::spawn(move || {
                while let Ok(Async::NotReady) = link1.poll().map_err(|e| e) {
                    if let Err(e) = poller.poll(Some(time::Duration::from_millis(1))) {
                        link1.exit(Err(e));
                        return;
                    }
                }
            });
        }
        Ok(PollerPool {
            links: links,
            pollers: Arc::new(Mutex::new(pollers)),
        })
    }
    pub fn thread_count(&self) -> usize {
        self.links.len()
    }
    pub fn handle(&self) -> PollerPoolHandle {
        PollerPoolHandle { pollers: self.pollers.clone() }
    }
    // TODO: restart aborted thread
}

#[derive(Debug, Clone)]
pub struct PollerPoolHandle {
    pollers: Arc<Mutex<Vec<PollerHandle>>>,
}
impl PollerPoolHandle {
    pub fn allocate_poller(&self) -> PollerHandle {
        let pollers = assert_ok!(self.pollers.lock());
        assert!(!pollers.is_empty(), "No available poller");

        // TODO: improve selection logic
        let i = rand::random::<usize>() % pollers.len();
        pollers[i].clone()
    }
}
