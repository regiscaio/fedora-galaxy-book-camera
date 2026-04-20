use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use super::localization::trf;
use super::paths::cache_dir;

fn singleton_socket_path() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let runtime_dir = PathBuf::from(runtime_dir);
        if !runtime_dir.as_os_str().is_empty() {
            return runtime_dir.join("galaxybook-camera.sock");
        }
    }

    cache_dir().join("galaxybook-camera.sock")
}

pub struct SingletonState {
    pub signal_rx: Receiver<()>,
    pub socket_path: PathBuf,
}

pub fn setup_singleton() -> Result<Option<SingletonState>, String> {
    let socket_path = singleton_socket_path();
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| {
                trf(
                    "Falha ao preparar a pasta do singleton: {error}",
                    &[("error", error.to_string())],
                )
            })?;
    }

    if UnixStream::connect(&socket_path).is_ok() {
        return Ok(None);
    }

    if socket_path.exists() {
        let _ = fs::remove_file(&socket_path);
    }

    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| {
            trf(
                "Falha ao criar o socket singleton do app: {error}",
                &[("error", error.to_string())],
            )
        })?;
    let (signal_tx, signal_rx) = mpsc::channel();
    let listener_socket_path = socket_path.clone();

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(_) => {
                    let _ = signal_tx.send(());
                }
                Err(error) => {
                    if error.kind() == std::io::ErrorKind::WouldBlock {
                        continue;
                    }
                    break;
                }
            }
        }
        let _ = fs::remove_file(listener_socket_path);
    });

    Ok(Some(SingletonState {
        signal_rx,
        socket_path,
    }))
}
