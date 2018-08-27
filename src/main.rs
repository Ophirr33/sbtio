extern crate ctrlc;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate syslog;
extern crate url;

mod conn;
mod sbt;

use conn::Conn;
use sbt::{find_sbt_server_addr, LspMessageReader};

use std::io;
use std::net::Shutdown;
use std::process::exit;
use std::sync::mpsc::{channel, Sender};
use std::thread;

const RTHREAD: &'static str = "reader";
const WTHREAD: &'static str = "writer";

fn main() {
    match run() {
        Err(e) => {
            eprintln!("{}", e);
            error!("{}", e);
            exit(1);
        }
        Ok(()) => {
            exit(0);
        }
    }
}

fn run() -> io::Result<()> {
    syslog::init_unix(syslog::Facility::LOG_USER, log::LevelFilter::Info)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "could not init syslog"))?;

    let sbt_socket_addr = find_sbt_server_addr()?;
    let mut read_stream = Conn::connect(&sbt_socket_addr)?;
    let mut write_stream = read_stream.try_clone()?;
    let signal_stream = read_stream.try_clone()?;

    ctrlc::set_handler(move || {
        let _ = signal_stream.shutdown(Shutdown::Both);
    }).map_err(|e| {
        let _ = read_stream.shutdown(Shutdown::Both);
        io::Error::new(io::ErrorKind::Other, e)
    })?;

    let (read_sender, receiver) = channel();
    let write_sender = read_sender.clone();
    thread::Builder::new().name(RTHREAD.into()).spawn(move || {
        let stdin = io::stdin();
        let mut lock = stdin.lock();
        if let Err(e) = copy_messages(&mut lock, &mut write_stream) {
            error!("Could not copy message due to {}", e);
            cleanup_conn(write_stream, read_sender);
        }
    })?;
    thread::Builder::new().name(WTHREAD.into()).spawn(move || {
        let stdout = io::stdout();
        let mut lock = stdout.lock();
        if let Err(_) = copy_messages(&mut read_stream, &mut lock) {
            cleanup_conn(read_stream, write_sender);
        }
    })?;

    let _ = receiver
        .recv()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(())
}

fn cleanup_conn(stream: Conn, sender: Sender<usize>) {
    if let Err(e) = stream.shutdown(Shutdown::Both) {
        error!("Could not shutdown stream {:?} due to {}", stream, e);
    }
    sender.send(1).expect("Channel failure");
}

fn copy_messages<R: io::Read, W: io::Write>(read: &mut R, write: &mut W) -> io::Result<()> {
    let mut reader = LspMessageReader::new(read);
    loop {
        let msg = reader.read_message()?;
        msg.write_into(write)?;
        info!("Copying message {:?}", msg);
    }
}