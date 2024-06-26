use std::{path::PathBuf, time::Duration};

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use codec::SignalCodec;
use daemon::start;
use fork::Fork;
use futures::SinkExt;
use rkyv::{Archive, Deserialize, Serialize};
use tokio::{net::UnixStream, time::timeout};
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

mod codec;
mod daemon;

#[derive(Parser, Debug)]
struct Cli {
    #[command(subcommand)]
    command: Option<Signal>,
}

#[derive(Subcommand, Debug, Archive, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[archive(check_bytes, compare(PartialEq))]
pub enum Signal {
    /// Query for inhibitor state
    Query,
    /// Activate idle inhibition
    On,
    /// Deactivate idle inhibition
    Off,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let signal = args.command.unwrap_or(Signal::Query);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    match check_socket() {
        Err(_) => match fork::fork() {
            Ok(Fork::Child) => {
                // let _ = fork::close_fd();
                start()
            }
            Ok(Fork::Parent(_)) => {
                // println!("Daemon started with pid {c}");
                runtime.block_on(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let mut stream = get_stream().await?;
                    handle_signal(&mut stream, signal).await?;
                    Ok(())
                })
            }
            Err(e) => {
                bail!("Failed to start daemon: {e}");
            }
        },
        Ok(_) => runtime.block_on(async {
            let mut stream = get_stream().await?;
            handle_signal(&mut stream, signal).await?;
            Ok(())
        }),
    }
}

async fn handle_signal(stream: &mut Framed<UnixStream, SignalCodec>, signal: Signal) -> Result<()> {
    let reply = send(stream, signal).await?;
    match reply {
        Signal::On => println!("ON"),
        Signal::Off => println!("OFF"),
        _ => bail!("Unknown reply from daemon"),
    }
    Ok(())
}

fn get_socket_path() -> Result<PathBuf> {
    let runtime_dir = PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?);
    Ok(runtime_dir.join("inhibitor.sock"))
}

async fn get_stream() -> Result<Framed<UnixStream, SignalCodec>> {
    let stream = UnixStream::connect(get_socket_path()?).await?;
    Ok(Framed::new(stream, SignalCodec {}))
}

fn check_socket() -> Result<()> {
    Ok(std::os::unix::net::UnixStream::connect(get_socket_path()?).map(|_| ())?)
}

async fn send(stream: &mut Framed<UnixStream, SignalCodec>, signal: Signal) -> Result<Signal> {
    stream.send(signal).await?;
    let reply = timeout(Duration::from_secs(2), stream.next()).await?;
    reply
        .map(|e| Ok(e?))
        .unwrap_or(Err(anyhow!("No response from daemon")))
}
