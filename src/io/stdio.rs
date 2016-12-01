use std::io::{self, Read};
use std::thread;
use std::sync::mpsc as std_mpsc;

pub fn stdin() -> Stdin {
    // TODO: refactoring
    let (req_tx, req_rx) = std_mpsc::channel();
    let (res_tx, res_rx) = std_mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        while let Ok(size) = stdin.read(&mut []) {
            println!("# SIZE: {}", size);

            // Sends marker data
            res_tx.send(vec![]).unwrap(); // TODO

            let size = req_rx.recv().unwrap();
            let mut buf = vec![0; size];
            let size = stdin.read(&mut buf).unwrap();
            buf.truncate(size);
            res_tx.send(buf).unwrap();
        }
    });
    Stdin {
        req_tx: req_tx,
        res_rx: res_rx,
    }
}

#[derive(Debug)]
pub struct Stdin {
    req_tx: std_mpsc::Sender<usize>,
    res_rx: std_mpsc::Receiver<Vec<u8>>,
}
impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.res_rx.try_recv() {
            Err(std_mpsc::TryRecvError::Empty) => {
                Err(io::Error::new(io::ErrorKind::WouldBlock, "Standard input would block"))
            }
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, Box::new(e))),
            Ok(_) => {
                // The standard input stream is ready to read
                self.req_tx.send(buf.len()).unwrap();
                let data = self.res_rx.recv().unwrap();
                let len = data.len();
                buf[..len].copy_from_slice(&data[..]);
                Ok(len)
            }
        }
    }
}
