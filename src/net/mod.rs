use std::io;

pub use self::tcp::{TcpStream, Connect};
pub use self::tcp::{TcpListener, Bind, Incoming};

mod tcp;

#[derive(Debug)]
pub struct ReadHalf<T>(T);
impl<T> io::Read for ReadHalf<T>
    where T: io::Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

#[derive(Debug)]
pub struct WriteHalf<T>(T);
impl<T> io::Write for WriteHalf<T>
    where T: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
