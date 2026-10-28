//! Existing Node environment/channel bootstrap state; one guard, unchanged.
use std::collections::VecDeque;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(unix)]
pub(super) type IpcStream = UnixStream;
#[cfg(windows)]
pub(super) type IpcStream = crate::child_process::ipc_transport::IpcStream;

pub(super) enum IpcEvent {
    Message(String),
    MessageAdvanced(Vec<u8>),
    Closed,
}

pub(super) struct ChildIpcState {
    pub(super) initialized: bool,
    pub(super) available: bool,
    pub(super) connected: bool,
    pub(super) refed: bool,
    pub(super) disconnect_emitted: bool,
    pub(super) advanced: bool,
    #[cfg(any(unix, windows))]
    pub(super) send: Option<IpcStream>,
    pub(super) queue: VecDeque<IpcEvent>,
}

impl ChildIpcState {
    pub(super) fn new() -> Self {
        Self {
            initialized: false,
            available: false,
            connected: false,
            refed: false,
            disconnect_emitted: false,
            advanced: false,
            #[cfg(any(unix, windows))]
            send: None,
            queue: VecDeque::new(),
        }
    }
}

static CHILD_IPC: OnceLock<Mutex<ChildIpcState>> = OnceLock::new();

fn ipc_state() -> &'static Mutex<ChildIpcState> {
    crate::once_init::get_or_init(&CHILD_IPC, || Mutex::new(ChildIpcState::new()))
}

pub(super) fn ipc_lock() -> MutexGuard<'static, ChildIpcState> {
    match ipc_state().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Initialize the inherited child IPC channel once. This also removes Node's
/// bootstrap-only env vars so `process.env.NODE_CHANNEL_FD` follows Node.
/// turnloop P0: whether `process_ipc_ensure_initialized` has finished probing
/// for an inherited IPC channel, and whether it found one. `available` never
/// reverts once set, so after a probe that found no channel the per-turn
/// keep-alive check answers from these two atomics without the IPC lock.
pub(super) static IPC_PROBED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub(super) static IPC_AVAILABLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn process_ipc_ensure_initialized() {
    {
        let mut state = ipc_lock();
        if state.initialized {
            return;
        }
        state.initialized = true;
    }

    let fd_var = std::env::var("NODE_CHANNEL_FD").ok();
    let serialization_mode =
        std::env::var("NODE_CHANNEL_SERIALIZATION_MODE").unwrap_or_else(|_| "json".to_string());
    std::env::remove_var("NODE_CHANNEL_FD");
    std::env::remove_var("NODE_CHANNEL_SERIALIZATION_MODE");

    super::ipc_hooks::initialize_channel(fd_var, &serialization_mode);

    IPC_PROBED.store(true, std::sync::atomic::Ordering::Release);
}
