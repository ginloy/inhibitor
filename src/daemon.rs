use std::os::unix::net::UnixListener;

use crate::get_socket_path;
use anyhow::Result;

pub fn start() -> Result<()> {
    let socket_path = get_socket_path()?;
    if socket_path.try_exists()? {
        std::fs::remove_file(&socket_path)?;
    }

    let socket = UnixListener::bind(&socket_path)?;
    for connection in socket.incoming() {
        match connection {
            Err(e) => break,
            Ok(stream) => {
                std::thread::spawn(|| {
                    println!("connected (server)");
                });
            }
        }
    }

    Ok(())
}
