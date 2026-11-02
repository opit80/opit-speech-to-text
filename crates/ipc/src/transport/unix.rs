//! Local Unix socket transport, scoped to the current user's runtime or scratch data directory.

use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};

use crate::{Error, MAX_REQUEST, MAX_RESPONSE, Request, Response, VERSION, read_frame, write_frame};

pub fn socket_path() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("OPIT_DATA_DIR") {
        return Ok(PathBuf::from(path).join("control.sock"));
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unavailable"))?;
    Ok(PathBuf::from(runtime).join("opit-speech-to-text.sock"))
}

pub fn bind(path: &Path) -> io::Result<UnixListener> {
    let parent = path.parent().ok_or_else(|| io::Error::other("Invalid socket path"))?;
    std::fs::create_dir_all(parent)?;
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Untrusted socket path"));
        }
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, "App already running"));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

pub async fn call(request: &Request) -> Result<Response, Error> {
    let path = socket_path().map_err(|_| Error::new("transport", "Could not locate the local control socket."))?;
    call_at(&path, request).await
}

pub async fn call_at(path: &Path, request: &Request) -> Result<Response, Error> {
    request.validate()?;
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            Error::new("app_not_running", "Opit Speech to Text is not running in this user session.")
        } else {
            Error::new("transport", "Could not inspect local control socket.")
        }
    })?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(Error::new("untrusted_server", "Could not authenticate the local control server."));
    }
    let mut stream = UnixStream::connect(path)
        .await
        .map_err(|_| Error::new("app_not_running", "Opit Speech to Text is not running in this user session."))?;
    let peer = stream
        .peer_cred()
        .map_err(|_| Error::new("untrusted_server", "Could not authenticate the local control server."))?;
    if peer.uid() != unsafe { libc::geteuid() } {
        return Err(Error::new("untrusted_server", "Could not authenticate the local control server."));
    }
    write_frame(&mut stream, request, MAX_REQUEST).await?;
    let bytes = tokio::time::timeout(Duration::from_secs(7200), read_frame(&mut BufReader::new(stream), MAX_RESPONSE))
        .await
        .map_err(|_| Error::new("timeout", "Local operation timed out; inspect status before retrying."))??;
    let response: Response =
        serde_json::from_slice(&bytes).map_err(|_| Error::new("protocol", "Invalid local control response."))?;
    if response.version != VERSION {
        return Err(Error::new("protocol_version", "Unsupported local control protocol version."));
    }
    Ok(response)
}
