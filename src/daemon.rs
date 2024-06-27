use crate::codec::SignalCodec;
use crate::{get_socket_path, Signal};
use anyhow::{anyhow, Result};
use futures::sink::SinkExt;
use futures::StreamExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::Framed;
use zbus::{proxy, zvariant::OwnedFd};

#[proxy(
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    interface = "org.freedesktop.login1.Manager"
)]
trait Systemd {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;
}

enum Command {
    Query(oneshot::Sender<Result<bool>>),
    Inhibit(oneshot::Sender<Result<()>>),
    Uninhibit(oneshot::Sender<Result<()>>),
}
struct Inhibitor {
    connection: zbus::Connection,
    fd: Option<OwnedFd>,
    receiver: mpsc::UnboundedReceiver<Command>,
}

impl Inhibitor {
    async fn run(&mut self) {
        let proxy = SystemdProxy::new(&self.connection)
            .await
            .expect("Unable to get dbus proxy");
        while let Some(cmd) = self.receiver.recv().await {
            let _ = self.handle_command(cmd, &proxy).await;
        }
    }

    async fn handle_command(&mut self, cmd: Command, proxy: &SystemdProxy<'_>) {
        match cmd {
            Command::Query(ch) => {
                let _ = ch.send(Ok(self.fd.is_some()));
            }
            Command::Inhibit(ch) => {
                match proxy
                    .inhibit("idle", "inhibitor", "User request", "block")
                    .await
                {
                    Ok(f) => {
                        self.fd = Some(f);
                        let _ = ch.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = ch.send(Err(anyhow!(e)));
                    }
                }
            }
            Command::Uninhibit(ch) => {
                self.fd = None;
                let _ = ch.send(Ok(()));
            }
        }
    }

    async fn spawn() -> InhibitorHandler {
        let (sender, receiver) = mpsc::unbounded_channel();
        let connection = zbus::Connection::system()
            .await
            .expect("Failed to connect to system dbus");
        let mut inhibitor = Inhibitor {
            connection,
            fd: None,
            receiver,
        };
        tokio::spawn(async move { inhibitor.run().await });
        InhibitorHandler { sender }
    }
}

#[derive(Clone)]
struct InhibitorHandler {
    sender: mpsc::UnboundedSender<Command>,
}

impl InhibitorHandler {
    async fn query(&self) -> Result<bool> {
        let (send, receive) = oneshot::channel();
        let _ = self.sender.send(Command::Query(send));
        receive.await?
    }
    async fn inhibit(&self) -> Result<()> {
        let (send, receive) = oneshot::channel();
        let _ = self.sender.send(Command::Inhibit(send));
        receive.await?
    }
    async fn uninhibit(&self) -> Result<()> {
        let (send, receive) = oneshot::channel();
        let _ = self.sender.send(Command::Uninhibit(send));
        receive.await?
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn start() -> Result<()> {
    let socket_path = get_socket_path()?;
    if socket_path.try_exists()? {
        std::fs::remove_file(&socket_path)?;
        println!("Removed old socket");
    }
    let inhibitor = Inhibitor::spawn().await;

    let socket = UnixListener::bind(&socket_path)?;
    loop {
        let (stream, _) = socket.accept().await?;
        tokio::spawn({
            let inhibitor = inhibitor.clone();
            async move {
                let _ = handle_connection(stream, inhibitor).await;
            }
        });
    }
}

async fn handle_connection(stream: UnixStream, inhibitor: InhibitorHandler) -> Result<()> {
    let test = Framed::new(stream, SignalCodec {});
    let (mut write, mut read) = test.split();
    if let Some(Ok(s)) = read.next().await {
        match s {
            Signal::Query => {
                if inhibitor.query().await? {
                    write.send(Signal::On).await?;
                } else {
                    write.send(Signal::Off).await?;
                }
            }
            Signal::On => {
                inhibitor.inhibit().await?;
                write.send(Signal::On).await?;
            }
            Signal::Off => {
                inhibitor.uninhibit().await?;
                write.send(Signal::Off).await?;
            }
        }
    }
    Ok(())
}
