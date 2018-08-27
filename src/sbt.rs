use std::env::current_dir;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, ErrorKind, Read, Write};
use std::mem;
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

#[derive(Debug)]
pub struct LspMessageReader<R> {
    headers: Option<Vec<u8>>,
    unparsed_message: Vec<u8>,
    inner: R,
    pos: usize,
}

impl<R: Read> LspMessageReader<R> {
    pub fn new(reader: R) -> Self {
        LspMessageReader {
            inner: reader,
            headers: None,
            unparsed_message: Vec::with_capacity(256),
            pos: 0,
        }
    }

    pub fn read_message(&mut self) -> io::Result<LspMessage> {
        let mut buf = [0; 256];
        loop {
            match self.inner.read(&mut buf) {
                Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
                Ok(0) => {
                    return Err(io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "Could not read full lsp message",
                    ))
                }
                Ok(n) => {
                    self.unparsed_message.extend_from_slice(&mut buf[0..n]);
                    if let Some(msg) = self.try_parse()? {
                        return Ok(msg);
                    }
                }
            }
        }
    }

    fn try_parse(&mut self) -> io::Result<Option<LspMessage>> {
        if self.headers.is_none() {
            self.try_headers();
        }
        if self.headers.is_none() {
            return Ok(None);
        }
        match from_slice::<Value>(&self.unparsed_message[..]) {
            Err(ref e) if e.is_eof() => Ok(None),
            Err(e) => {
                if e.is_syntax() && e.column() > 0 {
                    // there's a chance the error is due to trailing characters
                    // back off and try again before returning error
                    let actual_bytes = self.unparsed_message.len() - e.column() - 1;
                    if let Ok(_) = from_slice::<Value>(&self.unparsed_message[..actual_bytes]) {
                        let mut headers = None;
                        mem::swap(&mut headers, &mut self.headers);
                        let msg = self.unparsed_message.drain(..actual_bytes).collect();
                        return Ok(Some(LspMessage::new(headers.unwrap(), msg)));
                    }
                }
                Err(io::Error::new(ErrorKind::InvalidData, e))
            }
            Ok(_) => {
                let mut headers = None;
                let mut msg = Vec::with_capacity(256);
                mem::swap(&mut headers, &mut self.headers);
                mem::swap(&mut msg, &mut self.unparsed_message);
                Ok(Some(LspMessage::new(headers.unwrap(), msg)))
            }
        }
    }

    fn try_headers(&mut self) {
        let p = self.pos;
        for (idx, slice) in self.unparsed_message[p..]
            .windows(4)
            .enumerate()
            .map(|(idx, slice)| (idx + p, slice))
        {
            if slice == &[b'\r', b'\n', b'\r', b'\n'] {
                let mut vec = Vec::with_capacity(idx + 4);
                vec.extend_from_slice(&self.unparsed_message[..idx + 4]);
                self.headers = Some(vec);
                self.pos = idx + 4;
            }
        }
        if self.headers.is_some() {
            self.unparsed_message.drain(..self.pos).for_each(|_| {});
            self.pos = 0;
        }
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
