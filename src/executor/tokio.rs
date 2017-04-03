use Spawn;
use Executor;
use tokio_core::reactor::{Core, Handle, Remote};

use futures::{BoxFuture, Future};

impl Spawn for Remote {
    fn spawn_boxed(&self, f: BoxFuture<(), ()>) {
        Spawn::spawn(self, f)
    }

    fn spawn<F>(&self, fiber: F)
        where F: Future<Item = (), Error = ()> + Send + 'static
    {
        Remote::spawn(self, |h| ::futures::future::ok(h.spawn(fiber)))
    }
}


impl Executor for Core {
    type Handle = Remote;

    fn handle(&self) -> Self::Handle {
        self.remote()
    }

    fn run_once(&mut self) -> ::std::io::Result<()> {
        self.turn(None);
        Ok(())
    }
}

