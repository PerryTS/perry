//! Local IPC transport for `node:net` path overloads.
//!
//! Node maps `server.listen(path)` and `net.connect({ path })` to named pipes
//! on Windows and Unix-domain sockets on Unix. The streams join the same
//! SocketState command/event loop as TCP, so data, end, error, close, and
//! server connection events keep one implementation.

use std::io;

#[cfg(windows)]
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::{
    dispatch, ensure_gc_scanner_registered, mark_closed, next_id, next_id_or_throw, push_event,
    run_socket_task, server_state, statics, PendingNetEvent, SocketCommand, SocketState,
    TlsSocketMetadata, Transport,
};

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

fn allocate_socket() -> (i64, mpsc::UnboundedReceiver<SocketCommand>) {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    let id = next_id_or_throw();
    let (tx, rx) = mpsc::unbounded_channel::<SocketCommand>();
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            cmd_tx: tx,
            pending_rx: None,
            is_open: false,
            refed: true,
            local_addr: None,
            raw: None,
            destroyed: false,
            bytes_read: 0,
            bytes_written: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, Default::default());
    (id, rx)
}

/// Read a JS string without coercing closures or option objects through a
/// StringHeader layout.
pub(crate) unsafe fn string_value(value: f64) -> Option<String> {
    let value = perry_ffi::JsValue::from_bits(value.to_bits());
    value
        .is_string()
        .then(|| crate::string_from_header_i64(value.as_string_ptr() as i64))?
}

pub(crate) fn register_connect_cb(handle: i64, cb_f64: f64) {
    if handle == 0 || !crate::is_nanboxed_pointer(cb_f64) {
        return;
    }
    let cb_ptr = unsafe { crate::unbox_pointer(cb_f64) } as i64;
    if cb_ptr == 0 {
        return;
    }
    statics::listeners()
        .lock()
        .unwrap()
        .entry(handle)
        .or_default()
        .entry("connect".to_string())
        .or_default()
        .push(cb_ptr);
}

/// Publish an accepted TCP or IPC stream as a normal net.Socket and start its
/// shared command/read loop. Admission accounting has already reserved one
/// pending connection before this helper is called.
pub(crate) fn register_accepted_transport(
    server_id: i64,
    transport: Transport,
    local_addr: Option<std::net::SocketAddr>,
) {
    let socket_id = next_id();
    if socket_id == perry_ffi::INVALID_HANDLE {
        server_state::cancel_pending_connection(server_id);
        return;
    }
    let (tx, rx) = mpsc::unbounded_channel::<SocketCommand>();
    statics::sockets().lock().unwrap().insert(
        socket_id,
        SocketState {
            cmd_tx: tx,
            pending_rx: None,
            is_open: true,
            refed: true,
            local_addr,
            raw: None,
            destroyed: false,
            bytes_read: 0,
            bytes_written: 0,
            timeout: None,
            type_of_service: 0,
            server_id: Some(server_id),
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(socket_id, Default::default());
    push_event(PendingNetEvent::ServerConnection(
        server_id, socket_id, false,
    ));
    tokio::spawn(async move {
        let mut rx = rx;
        run_socket_task(socket_id, transport, &mut rx).await;
    });
}

pub(crate) fn spawn_socket(path: String) -> i64 {
    let (id, rx) = allocate_socket();
    spawn_connect(id, path, rx);
    id
}

pub(crate) fn connect_existing(handle: i64, path: String) {
    let rx = {
        let mut sockets = statics::sockets().lock().unwrap();
        match sockets
            .get_mut(&handle)
            .and_then(|socket| socket.pending_rx.take())
        {
            Some(rx) => rx,
            None => {
                push_event(PendingNetEvent::Error(
                    handle,
                    "socket already connected (or unknown handle)".to_string(),
                ));
                return;
            }
        }
    };
    spawn_connect(handle, path, rx);
}

fn spawn_connect(id: i64, path: String, mut rx: mpsc::UnboundedReceiver<SocketCommand>) {
    let local_server = server_state::begin_local_path_connect(&path);
    crate::spawn_socket_runner(move || {
        Box::pin(async move {
            let stream = match connect_path(&path).await {
                Ok(stream) => stream,
                Err(error) => {
                    server_state::cancel_local_connect(local_server);
                    push_event(PendingNetEvent::Error(
                        id,
                        format!("connect {path}: {error}"),
                    ));
                    push_event(PendingNetEvent::Close(id));
                    mark_closed(id);
                    return;
                }
            };

            if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&id) {
                socket.is_open = true;
            }
            tokio::task::yield_now().await;
            push_event(PendingNetEvent::Connect(id, local_server));
            run_socket_task(id, Transport::Ipc(stream), &mut rx).await;
        })
    });
}

pub(crate) fn spawn_listener(server_id: i64, path: String, shutdown_rx: oneshot::Receiver<()>) {
    perry_ffi::spawn_async(async move {
        if let Err(error) = run_listener(server_id, path.clone(), shutdown_rx).await {
            push_event(PendingNetEvent::ServerError(
                server_id,
                format!("bind {path}: {error}"),
            ));
        }
        push_event(PendingNetEvent::ServerClose(server_id));
        if let Ok(mut servers) = statics::servers().lock() {
            if let Some(server) = servers.get_mut(&server_id) {
                server.listening = false;
            }
        }
    });
}

#[cfg(unix)]
async fn connect_path(path: &str) -> io::Result<Box<dyn crate::transport::IpcStream>> {
    UnixStream::connect(path)
        .await
        .map(|stream| Box::new(stream) as Box<dyn crate::transport::IpcStream>)
}

#[cfg(unix)]
async fn run_listener(
    server_id: i64,
    path: String,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let listener = UnixListener::bind(&path)?;
    push_event(PendingNetEvent::ServerListening(server_id));

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    if let Some(info) = server_state::should_drop_ipc_connection(server_id) {
                        push_event(PendingNetEvent::ServerDrop(server_id, info));
                    } else {
                        register_accepted_transport(
                            server_id,
                            Transport::Ipc(Box::new(stream)),
                            None,
                        );
                    }
                }
                Err(error) => {
                    push_event(PendingNetEvent::ServerError(
                        server_id,
                        format!("accept: {error}"),
                    ));
                }
            },
            _ = &mut shutdown_rx => break,
        }
    }

    drop(listener);
    // Tokio deliberately leaves filesystem socket nodes behind. Only unlink
    // after our listener has closed; bind failures never remove someone else's
    // endpoint.
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
async fn connect_path(path: &str) -> io::Result<Box<dyn crate::transport::IpcStream>> {
    loop {
        match ClientOptions::new().open(path) {
            Ok(stream) => {
                return Ok(Box::new(stream) as Box<dyn crate::transport::IpcStream>);
            }
            // ERROR_PIPE_BUSY: all instances are serving clients. Match
            // Node/libuv's wait-and-retry behavior rather than reporting a
            // transient connector failure.
            Err(error) if error.raw_os_error() == Some(231) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
fn create_pipe_server(path: &str, first: bool) -> io::Result<NamedPipeServer> {
    ServerOptions::new().first_pipe_instance(first).create(path)
}

#[cfg(windows)]
async fn run_listener(
    server_id: i64,
    path: String,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    let mut listener = create_pipe_server(&path, true)?;
    push_event(PendingNetEvent::ServerListening(server_id));

    loop {
        tokio::select! {
            connected = listener.connect() => {
                connected?;
                let stream = listener;
                // A Windows named-pipe instance accepts exactly one client.
                // Create the next instance before publishing the accepted one
                // so concurrent connectors do not observe a needless gap.
                listener = create_pipe_server(&path, false)?;
                if let Some(info) = server_state::should_drop_ipc_connection(server_id) {
                    push_event(PendingNetEvent::ServerDrop(server_id, info));
                    drop(stream);
                } else {
                    register_accepted_transport(
                        server_id,
                        Transport::Ipc(Box::new(stream)),
                        None,
                    );
                }
            }
            _ = &mut shutdown_rx => break,
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
async fn connect_path(_path: &str) -> io::Result<Box<dyn crate::transport::IpcStream>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "local IPC sockets are unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    static NEXT_TEST_PIPE: AtomicU64 = AtomicU64::new(1);

    fn unique_name() -> String {
        let suffix = NEXT_TEST_PIPE.fetch_add(1, Ordering::Relaxed);
        #[cfg(windows)]
        return format!(r"\\.\pipe\perry-ext-net-{}-{suffix}", std::process::id());
        #[cfg(unix)]
        return std::env::temp_dir()
            .join(format!(
                "perry-ext-net-{}-{suffix}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned();
        #[cfg(not(any(unix, windows)))]
        return String::new();
    }

    #[test]
    fn nanboxed_string_is_recognized_as_an_ipc_path() {
        let path = unique_name();
        let header = perry_ffi::alloc_string(&path).as_raw();
        let value = f64::from_bits(perry_ffi::nanbox_string_bits(header));
        assert_eq!(unsafe { super::string_value(value) }, Some(path));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn named_pipe_stream_round_trip() {
        let path = unique_name();
        let mut server = super::create_pipe_server(&path, true).unwrap();
        let connect_path = path.clone();
        let client = tokio::spawn(async move { super::connect_path(&connect_path).await });
        server.connect().await.unwrap();
        let mut client = super::Transport::Ipc(client.await.unwrap().unwrap());

        client.write_all(b"ping").await.unwrap();
        let mut request = [0; 4];
        server.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"ping");

        server.write_all(b"pong").await.unwrap();
        let mut response = [0; 4];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_socket_stream_round_trip() {
        let path = unique_name();
        let listener = super::UnixListener::bind(&path).unwrap();
        let connect_path = path.clone();
        let client = tokio::spawn(async move { super::connect_path(&connect_path).await });
        let (mut server, _) = listener.accept().await.unwrap();
        let mut client = super::Transport::Ipc(client.await.unwrap().unwrap());

        client.write_all(b"ping").await.unwrap();
        let mut request = [0; 4];
        server.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"ping");

        server.write_all(b"pong").await.unwrap();
        let mut response = [0; 4];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"pong");
        drop(listener);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(not(any(unix, windows)))]
async fn run_listener(
    _server_id: i64,
    _path: String,
    _shutdown_rx: oneshot::Receiver<()>,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "local IPC sockets are unsupported on this platform",
    ))
}
