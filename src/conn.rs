use std::io;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

use url::Url;

/// Abstracts over different connections
#[derive(Debug)]
pub enum Conn {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
    //TODO: windows named pipes
}

impl Conn {
    /// Inspects the address to determine what type of connection to build
    pub fn connect(url: &str) -> io::Result<Self> {
        let invalid = io::ErrorKind::InvalidInput;
        let url = Url::parse(url).map_err(|e| io::Error::new(invalid, e))?;
        let conn = match url.scheme() {
            "tcp" => {
                let tcp = url
                    .to_socket_addrs()?
                    .next()
                    .ok_or(io::Error::new(invalid, "Invalid tcp address"))
                    .and_then(TcpStream::connect)?;
                Conn::Tcp(tcp)
            }
            #[cfg(unix)]
            "local" => {
                let unix = UnixStream::connect(url.path())?;
                Conn::Unix(unix)
            }
            _ => {
                return Err(io::Error::new(invalid, "Could not match the given url"));
            }
        };
        Ok(conn)
    }

    /// Delegates to the underlying connection's `try_clone` method
    pub fn try_clone(&self) -> io::Result<Self> {
        match self {
            Conn::Tcp(stream) => stream.try_clone().map(|new| Conn::Tcp(new)),
            #[cfg(unix)]
            Conn::Unix(stream) => stream.try_clone().map(|new| Conn::Unix(new)),
        }
    }

    /// Delegates to the underlying connection's `shutdown` method
    pub fn shutdown(&self, shutdown_type: Shutdown) -> io::Result<()> {
        match self {
            Conn::Tcp(stream) => stream.shutdown(shutdown_type),
            #[cfg(unix)]
            Conn::Unix(stream) => stream.shutdown(shutdown_type),
        }
    }
}

impl io::Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Tcp(stream) => stream.read(buf),
            #[cfg(unix)]
            Conn::Unix(stream) => stream.read(buf),
        }
    }
}

impl io::Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Tcp(stream) => stream.write(buf),
            #[cfg(unix)]
            Conn::Unix(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Conn::Unix(stream) => stream.flush(),
        }
    }
}
