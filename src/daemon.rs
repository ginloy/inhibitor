use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    time::Duration,
};

use crate::{get_socket_path, Signal};
use anyhow::{bail, Result};
use zbus::{proxy, zvariant::OwnedFd};

#[proxy(
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    interface = "org.freedesktop.login1.Manager"
)]
trait Systemd {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;
}

pub async fn start() -> Result<()> {
    let system = zbus::Connection::system().await?;
    let mut fd: Option<OwnedFd> = None;
    let systemd_proxy = SystemdProxy::new(&system).await?;
    let socket_path = get_socket_path()?;
    if socket_path.try_exists()? {
        std::fs::remove_file(&socket_path)?;
        println!("Removed old socket");
    }

    let socket = UnixListener::bind(&socket_path)?;
    for mut stream in socket.incoming().filter_map(|c| c.ok()) {
        handle_connection(&mut stream, &systemd_proxy, &mut fd).await?;
    }
    Ok(())
}

async fn handle_connection(
    stream: &mut UnixStream,
    proxy1: &SystemdProxy<'_>,
    fd: &mut Option<OwnedFd>,
) -> Result<()> {
    let mut buf: Vec<u8> = vec![0; 64];
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    let bytes = stream.read(&mut buf)?;
    let request = rkyv::from_bytes::<Signal>(&buf[..bytes]);
    match request {
        Err(_) => bail!("Failed to deserialize request"),
        Ok(s) => match s {
            Signal::Query => {
                if fd.is_some() {
                    send(stream, Signal::On)
                } else {
                    send(stream, Signal::Off)
                }
            }
            Signal::On => {
                if fd.is_none() {
                    let new_fd = proxy1
                        .inhibit("idle", "inhibitor", "User request", "block")
                        .await?;
                    *fd = Some(new_fd);
                }
                send(stream, Signal::On)
            }
            Signal::Off => {
                *fd = None;
                send(stream, Signal::Off)
            }
        },
    }
}

fn send(stream: &mut UnixStream, signal: Signal) -> Result<()> {
    let msg = rkyv::to_bytes::<_, 64>(&signal)?;
    stream.write_all(&msg)?;
    stream.flush()?;
    Ok(())
}
