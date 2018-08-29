use std::env::current_dir;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Bytes, ErrorKind, Read, Write};
use std::path::Path;

use serde_json::{from_reader, from_slice, Value};

/// Searches upwards from the current directory to find `active.json`
pub fn find_sbt_server_addr() -> io::Result<String> {
    let cwd = current_dir()?;
    for path in Path::ancestors(&cwd) {
        let active = path.join("project").join("target").join("active.json");
        if active.exists() {
            return parse_active(&active).map(Active::to_uri);
        }
    }
    Err(io::Error::new(ErrorKind::NotFound, "No active.json found"))
}

fn parse_active(active: &Path) -> io::Result<Active> {
    let f = File::open(active)?;
    let br = BufReader::new(f);
    let parsed: Active = from_reader(br).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
    Ok(parsed)
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Active {
    OnlyUri {
        uri: String,
    },
    // TODO: Are tokens really ever used?
    WithToken {
        uri: String,
        #[serde(rename = "tokenfilePath")]
        _tokenfile_path: String,
        #[serde(rename = "tokenfileUri")]
        _tokenfile_uri: String,
    },
}

impl Active {
    fn to_uri(self) -> String {
        match self {
            Active::OnlyUri { uri } => uri,
            Active::WithToken { uri, .. } => uri,
        }
    }
}

pub struct LspMessageReader<R: io::Read> {
    inner: Bytes<R>,
    headers: Vec<u8>,
    message: Vec<u8>,
}

impl<R: Read> LspMessageReader<R> {
    pub fn new(reader: R) -> Self {
        LspMessageReader {
            inner: reader.bytes(),
            headers: Vec::with_capacity(64),
            message: Vec::with_capacity(64),
        }
    }

    pub fn read_message(&mut self) -> io::Result<LspMessage> {
        self.headers.clear();
        self.message.clear();
        self.parse_headers()?;
        self.parse_message()?;
        Ok(LspMessage::new(self.headers.clone(), self.message.clone()))
    }

    fn parse_headers(&mut self) -> io::Result<()> {
        loop {
            let bo = self.inner.next();
            let b = match self.match_byte(bo)? {
                Some(b) => b,
                None => continue,
            };
            self.headers.push(b);
            let len = self.headers.len();
            if len >= 4 && &self.headers[len - 4..] == &[b'\r', b'\n', b'\r', b'\n'] {
                return Ok(());
            }
        }
    }

    fn match_byte(&self, bo: Option<io::Result<u8>>) -> io::Result<Option<u8>> {
        match bo {
            None => {
                error!("End of Buffer with {:?}", &self);
                Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "Reached end of reader",
                ))
            }
            Some(Err(e)) => {
                if e.kind() == ErrorKind::Interrupted {
                    Ok(None)
                } else {
                    error!("Some error {:?} with {:?}", e, &self);
                    Err(e)
                }
            }
            Some(Ok(b)) => Ok(Some(b)),
        }
    }

    fn parse_message(&mut self) -> io::Result<()> {
        let mut brace_count = 0;
        loop {
            let bo = self.inner.next();
            let b = match self.match_byte(bo)? {
                Some(b) => b,
                None => continue,
            };
            self.message.push(b);
            match b {
                b'{' => brace_count += 1,
                b'}' => brace_count -= 1,
                b'"' => self.parse_string()?,
                _ => continue,
            };
            if brace_count > 0 {
                continue;
            }
            if let Err(e) = from_slice::<Value>(&self.message[..]) {
                return Err(io::Error::new(ErrorKind::InvalidData, e));
            } else {
                return Ok(());
            }
        }
    }

    fn parse_string(&mut self) -> io::Result<()> {
        let mut escape = false;
        loop {
            let bo = self.inner.next();
            let b = match self.match_byte(bo)? {
                Some(b) => b,
                None => continue,
            };
            self.message.push(b);
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                return Ok(());
            }
        }
    }
}

impl<R: io::Read> fmt::Debug for LspMessageReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "LspMessageReader({:?}, {:?})",
            String::from_utf8_lossy(&self.headers[..]),
            String::from_utf8_lossy(&self.message[..])
        )
    }
}

pub struct LspMessage {
    pub headers: Vec<u8>,
    pub message: Vec<u8>,
}

impl LspMessage {
    fn new(headers: Vec<u8>, message: Vec<u8>) -> Self {
        LspMessage { headers, message }
    }

    pub fn write_into<W: Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(&self.headers)?;
        w.write_all(&self.message)?;
        w.flush()?;
        Ok(())
    }
}

impl fmt::Debug for LspMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let headers: Vec<String> = String::from_utf8_lossy(&self.headers[..])
            .split("\r\n")
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let message = String::from_utf8_lossy(&self.message);
        write!(f, "LspMessage({:?}, {:?})", headers, message)
    }
}
