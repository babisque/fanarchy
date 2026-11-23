//! Unix socket server speaking JSON Lines (protocol in `docs/protocol.md`).
//!
//! The socket is `root:fanarchy`, mode `0660`, so members of the
//! `fanarchy` group can use it without root. It is never world-writable:
//! without the group it falls back to `0600` (root only).
//!
//! `status`, `get_config`, `list_outputs` and `subscribe` are answered
//! here from what the control loop publishes; none of them writes
//! anything. Changes (`set_curve`, `set_mode`, `reload`, `apply_setup`) go
//! to the control loop over a channel, which validates them with the
//! same rules as the config file. Only `apply_setup` names a `pwmN`, and
//! only on chips root declared in the config; pumps are refused.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fanarchy_protocol::{MAX_LINE_BYTES, Request, Response, SUBSCRIBE_INTERVAL_SECS};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, watch};

pub use fanarchy_protocol::SOCKET_PATH;

use crate::control::{ControlRequest, Published};
use crate::gpu::GpuSensor;
use crate::setup;
use crate::status::{GpuReading, build_status};

/// Group whose members may use the socket.
pub const SOCKET_GROUP: &str = "fanarchy";

/// Socket mode when [`SOCKET_GROUP`] exists.
pub const GROUP_MODE: u32 = 0o660;

/// Socket mode when it does not (root only).
pub const ROOT_ONLY_MODE: u32 = 0o600;

/// A client that does not read its replies is dropped after this long.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the GPU temperature is refreshed.
pub const GPU_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("{path} exists and is not a socket; remove it or pick another path")]
    NotASocket { path: PathBuf },
    #[error("cannot create socket {path}: {source}")]
    Bind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Find a group's id in `/etc/group` text. `None` when it is missing.
pub fn parse_group_gid(etc_group: &str, name: &str) -> Option<u32> {
    etc_group.lines().find_map(|line| {
        let mut fields = line.split(':');
        let group = fields.next()?;
        let _password = fields.next()?;
        let gid = fields.next()?;
        (group == name).then(|| gid.trim().parse().ok()).flatten()
    })
}

/// Look up [`SOCKET_GROUP`] in `/etc/group`.
pub fn lookup_socket_group() -> Option<u32> {
    let text = fs::read_to_string("/etc/group").ok()?;
    parse_group_gid(&text, SOCKET_GROUP)
}

/// Removes the socket file when dropped.
#[derive(Debug)]
pub struct SocketFile {
    path: PathBuf,
}

impl SocketFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        if is_socket(&self.path) {
            match fs::remove_file(&self.path) {
                Ok(()) => tracing::debug!(path = %self.path.display(), "socket removed"),
                Err(err) => {
                    tracing::warn!(path = %self.path.display(), %err, "cannot remove socket")
                }
            }
        }
    }
}

fn is_socket(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket())
}

/// Create the listening socket at `path`.
///
/// The socket is created in a private `0700` directory, given its group
/// and mode there, and only then renamed to `path`, so it is never
/// reachable with looser permissions. A stale socket at `path` is
/// replaced; anything else there is an error. With `gid` the socket is
/// `0660` and owned by that group, without it `0600`.
///
/// Must be called inside a tokio runtime.
pub fn bind(path: &Path, gid: Option<u32>) -> Result<(UnixListener, SocketFile), IpcError> {
    let bind_error = |source| IpcError::Bind {
        path: path.to_path_buf(),
        source,
    };
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => {
            tracing::info!(path = %path.display(), "replacing stale socket");
        }
        Ok(_) => {
            return Err(IpcError::NotASocket {
                path: path.to_path_buf(),
            });
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(bind_error(err)),
    }

    let staging = staging_dir(path);
    match fs::remove_dir_all(&staging) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(bind_error(err)),
    }
    let result = bind_in(&staging, path, gid);
    // Best effort: the directory is empty after a successful rename.
    let _ = fs::remove_dir_all(&staging);
    let listener = result.map_err(bind_error)?;

    match gid {
        Some(gid) => {
            tracing::info!(path = %path.display(), gid, mode = "0660", "listening")
        }
        None => tracing::warn!(
            path = %path.display(),
            "group '{SOCKET_GROUP}' not found, socket is root-only (0600); create it with: sudo groupadd -r {SOCKET_GROUP}"
        ),
    }
    Ok((
        listener,
        SocketFile {
            path: path.to_path_buf(),
        },
    ))
}

/// `.<name>.staging` next to `path`, so the rename stays on one filesystem.
fn staging_dir(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.staging"))
}

fn bind_in(staging: &Path, path: &Path, gid: Option<u32>) -> std::io::Result<UnixListener> {
    fs::DirBuilder::new().mode(0o700).create(staging)?;
    let staged = staging.join("sock");
    let listener = UnixListener::bind(&staged)?;
    let mode = match gid {
        Some(gid) => {
            std::os::unix::fs::chown(&staged, None, Some(gid))?;
            GROUP_MODE
        }
        None => ROOT_ONLY_MODE,
    };
    fs::set_permissions(&staged, fs::Permissions::from_mode(mode))?;
    fs::rename(&staged, path)?;
    Ok(listener)
}

/// What every client task needs.
#[derive(Debug, Clone)]
pub struct Context {
    pub requests: mpsc::Sender<ControlRequest>,
    pub published: watch::Receiver<Option<Arc<Published>>>,
    pub gpu: watch::Receiver<GpuReading>,
}

/// Accept clients forever, one task each.
pub async fn serve(listener: UnixListener, context: Context) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tracing::debug!("client connected");
                tokio::spawn(handle_client(stream, context.clone()));
            }
            Err(err) => {
                tracing::warn!(%err, "cannot accept client");
                // Avoid a busy loop on e.g. EMFILE.
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Refresh the GPU reading every [`GPU_INTERVAL`] without blocking the
/// runtime (`nvidia-smi` is a separate process).
pub async fn poll_gpus<G>(sensor: G, reading: watch::Sender<GpuReading>)
where
    G: GpuSensor + Clone + Send + 'static,
{
    let mut interval = tokio::time::interval(GPU_INTERVAL);
    loop {
        interval.tick().await;
        let current = sensor.clone();
        let next = match tokio::task::spawn_blocking(move || current.read()).await {
            Ok(Ok(gpus)) => GpuReading { gpus, note: None },
            Ok(Err(err)) => GpuReading {
                gpus: Vec::new(),
                note: Some(err.to_string()),
            },
            Err(err) => GpuReading {
                gpus: Vec::new(),
                note: Some(format!("GPU reader failed: {err}")),
            },
        };
        if next.note.is_some() && next.note != reading.borrow().note {
            tracing::warn!(note = ?next.note, "cannot read GPU temperature");
        }
        reading.send_replace(next);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Line {
    Text(String),
    TooLong,
    Closed,
}

/// Read one line of at most [`MAX_LINE_BYTES`], newline included.
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Line> {
    let mut buf = Vec::new();
    let limit = u64::try_from(MAX_LINE_BYTES).unwrap_or(u64::MAX);
    let read = (&mut *reader)
        .take(limit)
        .read_until(b'\n', &mut buf)
        .await?;
    if read == 0 {
        return Ok(Line::Closed);
    }
    if buf.last() != Some(&b'\n') && buf.len() >= MAX_LINE_BYTES {
        return Ok(Line::TooLong);
    }
    Ok(Line::Text(String::from_utf8_lossy(&buf).into_owned()))
}

/// Discard input up to and including the next newline, without keeping
/// it in memory. `false` when the client closed the connection first.
async fn skip_rest_of_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<bool> {
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            return Ok(false);
        }
        match buf.iter().position(|&b| b == b'\n') {
            Some(newline) => {
                reader.consume(newline + 1);
                return Ok(true);
            }
            None => {
                let len = buf.len();
                reader.consume(len);
            }
        }
    }
}

async fn handle_client(stream: UnixStream, context: Context) {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    loop {
        let line = match read_line(&mut reader).await {
            Ok(Line::Text(line)) => line,
            Ok(Line::TooLong) => {
                let message = format!("request too long (limit {MAX_LINE_BYTES} bytes)");
                if !send(&mut write, &Response::error(message)).await {
                    return;
                }
                match skip_rest_of_line(&mut reader).await {
                    Ok(true) => continue,
                    Ok(false) | Err(_) => return,
                }
            }
            Ok(Line::Closed) => return,
            Err(err) => {
                tracing::debug!(%err, "client read failed");
                return;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let response = match Request::parse(line) {
            Ok(Request::Subscribe) => {
                subscribe(&mut write, &context).await;
                return;
            }
            Ok(request) => respond(&context, request).await,
            Err(message) => Response::error(message),
        };
        if !send(&mut write, &response).await {
            return;
        }
    }
}

/// Write one response line. `false` when the client is gone or stuck.
async fn send(write: &mut OwnedWriteHalf, response: &Response) -> bool {
    let line = response.to_line();
    match tokio::time::timeout(WRITE_TIMEOUT, write.write_all(line.as_bytes())).await {
        Ok(Ok(())) => true,
        Ok(Err(err)) => {
            tracing::debug!(%err, "client write failed");
            false
        }
        Err(_) => {
            tracing::debug!("client not reading, dropping it");
            false
        }
    }
}

async fn subscribe(write: &mut OwnedWriteHalf, context: &Context) {
    let mut interval = tokio::time::interval(Duration::from_secs(SUBSCRIBE_INTERVAL_SECS));
    loop {
        interval.tick().await;
        if !send(write, &status_response(context)).await {
            return;
        }
    }
}

async fn respond(context: &Context, request: Request) -> Response {
    match request {
        Request::Status => status_response(context),
        Request::GetConfig => match current(context) {
            Ok(published) => to_response(&published.config),
            Err(response) => response,
        },
        Request::ListOutputs => match current(context) {
            Ok(published) => to_response(&setup::list_outputs(
                &published.config,
                &published.snapshot.chips,
            )),
            Err(response) => response,
        },
        change => forward(context, change).await,
    }
}

fn current(context: &Context) -> Result<Arc<Published>, Response> {
    context
        .published
        .borrow()
        .clone()
        .ok_or_else(|| Response::error("daemon is starting, try again"))
}

fn status_response(context: &Context) -> Response {
    match current(context) {
        Ok(published) => {
            let gpu = context.gpu.borrow().clone();
            to_response(&build_status(&published.config, &published.snapshot, &gpu))
        }
        Err(response) => response,
    }
}

fn to_response<T: serde::Serialize>(data: &T) -> Response {
    match serde_json::to_value(data) {
        Ok(value) => Response::ok(value),
        Err(err) => Response::error(format!("cannot encode the response: {err}")),
    }
}

/// Hand a change to the control loop and wait for its answer.
async fn forward(context: &Context, request: Request) -> Response {
    let stopping = || Response::error("daemon is shutting down");
    let (reply, answer) = oneshot::channel();
    if context
        .requests
        .send(ControlRequest { request, reply })
        .await
        .is_err()
    {
        return stopping();
    }
    answer.await.unwrap_or_else(|_| stopping())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_stable() {
        assert_eq!(SOCKET_PATH, "/run/fanarchy.sock");
    }

    #[test]
    fn finds_group_gid() {
        let etc_group = "root:x:0:root\nwheel:x:998:d0c\nfanarchy:x:962:d0c,alice\nbroken\n";
        assert_eq!(parse_group_gid(etc_group, "fanarchy"), Some(962));
        assert_eq!(parse_group_gid(etc_group, "root"), Some(0));
        assert_eq!(parse_group_gid(etc_group, "fan"), None);
        assert_eq!(parse_group_gid(etc_group, "broken"), None);
        assert_eq!(parse_group_gid("fanarchy:x:abc:\n", "fanarchy"), None);
        assert_eq!(parse_group_gid("", "fanarchy"), None);
    }

    #[test]
    fn staging_dir_is_hidden_next_to_the_socket() {
        assert_eq!(
            staging_dir(Path::new("/run/fanarchy.sock")),
            PathBuf::from("/run/.fanarchy.sock.staging")
        );
    }

    #[tokio::test]
    async fn reads_limited_lines() {
        let long = "x".repeat(MAX_LINE_BYTES + 10);
        let input = "{\"cmd\":\"status\"}\n\nlast";
        let mut reader = BufReader::new(input.as_bytes());
        assert_eq!(
            read_line(&mut reader).await.unwrap(),
            Line::Text("{\"cmd\":\"status\"}\n".into())
        );
        assert_eq!(
            read_line(&mut reader).await.unwrap(),
            Line::Text("\n".into())
        );
        assert_eq!(
            read_line(&mut reader).await.unwrap(),
            Line::Text("last".into())
        );
        assert_eq!(read_line(&mut reader).await.unwrap(), Line::Closed);

        let input = format!("{long}\n{{\"cmd\":\"status\"}}\n");
        let mut reader = BufReader::new(input.as_bytes());
        assert_eq!(read_line(&mut reader).await.unwrap(), Line::TooLong);
        assert!(skip_rest_of_line(&mut reader).await.unwrap());
        assert_eq!(
            read_line(&mut reader).await.unwrap(),
            Line::Text("{\"cmd\":\"status\"}\n".into())
        );
        assert!(!skip_rest_of_line(&mut reader).await.unwrap());
    }
}
