use std::os::unix::net::{UnixListener, UnixStream};

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
    match Signal::from_reader(stream) {
        Err(_) => bail!("Failed to deserialize request"),
        Ok(s) => match s {
            Signal::Query => {
                if fd.is_some() {
                    Signal::On.to_writer(stream)
                } else {
                    Signal::Off.to_writer(stream)
                }
            }
            Signal::On => {
                if fd.is_none() {
                    let new_fd = proxy1
                        .inhibit("idle", "inhibitor", "User request", "block")
                        .await?;
                    *fd = Some(new_fd);
                }
                Signal::On.to_writer(stream)
            }
            Signal::Off => {
                *fd = None;
                Signal::Off.to_writer(stream)
            }
        },
    }
}
