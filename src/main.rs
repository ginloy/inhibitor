use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use daemon::start;
use fork::Fork;
use rkyv::{Archive, Deserialize, Serialize};

mod daemon;

#[derive(Parser, Debug)]
struct Cli {
    #[command(subcommand)]
    command: Option<Signal>,
}

#[derive(Subcommand, Debug, Archive, Serialize, Deserialize, PartialEq, Eq)]
#[archive(check_bytes, compare(PartialEq))]
enum Signal {
    /// Query for inhibitor state
    Query,
    /// Activate idle inhibition
    On,
    /// Deactivate idle inhibition
    Off,
}

impl Signal {
    fn to_writer<W>(&self, writer: &mut W) -> Result<()>
    where
        W: Write,
    {
        let bytes = rkyv::to_bytes::<_, 64>(self)?;
        writer.write_all(&bytes)?;
        writer.flush()?;
        Ok(())
    }

    fn from_reader<R>(reader: &mut R) -> Result<Signal>
    where
        R: Read,
    {
        let mut buf: Vec<u8> = vec![0; 64];
        let bytes_read = reader.read(&mut buf)?;
        if bytes_read == 0 {
            bail!("EOF")
        }
        let res = rkyv::from_bytes::<Signal>(&buf[..bytes_read]);
        match res {
            Ok(s) => anyhow::Result::Ok(s),
            Err(e) => {
                buf[..bytes_read].chain(reader);
                bail!("Failed to deserialize bytes to Signal: {}", e)
            }
        }
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let signal = args.command.unwrap_or(Signal::Query);
    match get_stream() {
        Err(_) => match fork::fork() {
            Ok(Fork::Child) => {
                // let _ = fork::close_fd();
                smol::block_on(start())
            }
            Ok(Fork::Parent(_)) => {
                // println!("Daemon started with pid {c}");
                std::thread::sleep(Duration::from_millis(200));
                let mut stream = get_stream()?;
                handle_signal(&mut stream, signal)?;
                Ok(())
            }
            Err(e) => {
                bail!("Failed to start daemon: {e}");
            }
        },
        Ok(mut stream) => {
            handle_signal(&mut stream, signal)?;
            Ok(())
        }
    }
}

fn handle_signal(stream: &mut UnixStream, signal: Signal) -> Result<()> {
    let reply = send(stream, signal)?;
    match reply {
        Signal::On => println!("Idle inhibition ON"),
        Signal::Off => println!("Idle inhibition OFF"),
        _ => bail!("Unknown reply from daemon"),
    }
    Ok(())
}

fn get_socket_path() -> Result<PathBuf> {
    let runtime_dir = PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?);
    Ok(runtime_dir.join("inhibitor.sock"))
}

fn get_stream() -> Result<UnixStream> {
    Ok(UnixStream::connect(get_socket_path()?)?)
}

fn send(stream: &mut UnixStream, signal: Signal) -> Result<Signal> {
    signal.to_writer(stream)?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    match Signal::from_reader(stream) {
        Ok(x) => Ok(x),
        Err(_) => bail!("Failed to deserialize response"),
    }
}
