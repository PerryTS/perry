//! Change-detection backend for `fs.watch` / `fsPromises.watch` (#9591).
//!
//! Before #9591 every watcher was a 25 ms `setInterval` whose tick re-walked
//! the WHOLE watch target (`read_dir` + `symlink_metadata` per entry) and
//! diffed two `BTreeMap`s — ~3.4 µs per file per tick, 40 ticks a second, on
//! the main thread. A 3 000-file tree cost 41 % of a core; claude-code watches
//! its cwd, and a 362 k-file cwd extrapolates to ~1.2 s of walking per 25 ms
//! schedule, which is a wedged event loop (#9588's symptom exactly).
//!
//! Now the OS tells us. A [`NativeInstance`] is inotify on Linux/Android,
//! `ReadDirectoryChangesW` on Windows and kqueue on the BSDs (all through
//! `notify`), and FSEvents on macOS (`watch_fsevents`, bound at runtime via
//! `dlopen` so no binary grows a CoreServices load command). Each backend runs
//! on its own thread and hands events to [`EventQueue`], which follows the
//! event pump's producer protocol — push, then `js_notify_main_thread()` — so
//! the main loop wakes once per burst and delivers from a runtime pump slot
//! (`stdlib_pump::register_runtime_pump`). Nothing walks anything on a timer.
//!
//! Instances, mirroring libuv's sharing:
//!
//! * **Non-recursive watchers share one instance per JS thread**, refcounted
//!   by canonical root. libuv shares one inotify fd per loop for the same
//!   reason: `fs.inotify.max_user_instances` defaults to 128, and a
//!   chokidar / `tsc --watch` style consumer opens one `fs.watch` per
//!   directory — thousands of them. One instance per watcher would fail
//!   with EMFILE at the 129th directory.
//! * **Recursive watchers get their own instance.** notify keys its per-path
//!   bookkeeping (inotify's wd map, FSEvents' `recursive_info`) by path, so a
//!   recursive root and a non-recursive watch of one of its subdirectories on
//!   the SAME instance would overwrite each other's mode and, on unwatch,
//!   remove each other's descriptors. Separate instances have separate maps.
//!
//! Every event carries its [`Source`], so the router only offers a shared
//! instance's events to shared watchers (root == path, or root == parent) and
//! an own instance's events to its one owner. One known imperfection: a
//! shared instance watching both `/a` and `/a/f` (a directory and a file
//! inside it) sees two inotify descriptors report the same write as two
//! events with the same path, and each watcher receives both. Node itself
//! routinely delivers two `'change'` events per write on Linux, so consumers
//! already debounce.
//!
//! The walker survives only as [`PollHandle`]: the fallback when the native
//! watch cannot be established (inotify watch limit, an unsupported target,
//! or `PERRY_FS_WATCH_POLL=1`). It runs on its own thread — the walk never
//! blocks the loop — and paces itself to at most 1/[`POLL_DUTY_DIVISOR`] of
//! one core: each walk's duration times [`POLL_DUTY_DIVISOR`], clamped to
//! [[`POLL_MIN_INTERVAL_MS`], [`POLL_MAX_INTERVAL_MS`]].

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use super::watch_fsevents::NativeInstance;
#[cfg(not(target_os = "macos"))]
use native::NativeInstance;

/// Lower bound of the fallback poller's cadence — the pre-#9591 fixed rate,
/// kept for tiny trees where a walk is cheaper than the interval.
pub(super) const POLL_MIN_INTERVAL_MS: u64 = 25;
/// Upper bound of the fallback poller's cadence: `fs.watchFile`'s default
/// interval, Node's own number for "polling is all we have".
pub(super) const POLL_MAX_INTERVAL_MS: u64 = 5007;
/// The fallback poller sleeps `POLL_DUTY_DIVISOR` × the last walk's duration
/// between walks, so it never exceeds 1/20 = 5 % of one core.
pub(super) const POLL_DUTY_DIVISOR: u32 = 20;

/// Set `PERRY_FS_WATCH_POLL=1` to skip the OS watcher and use the walker for
/// every watcher (a diagnostic escape hatch; also how the fallback is tested).
const FORCE_POLL_ENV: &str = "PERRY_FS_WATCH_POLL";

// ============================================================================
// Snapshot types — the fallback poller's diff domain.
// ============================================================================

#[derive(Clone, PartialEq, Eq)]
pub(super) struct WatchEntry {
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    len: u64,
    mode: u32,
    modified_ns: i128,
    created_ns: i128,
}

pub(super) type WatchSnapshot = BTreeMap<String, WatchEntry>;

/// One `(eventType, filename)` pair as `fs.watch` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WatchEvent {
    pub(super) event_type: &'static str,
    pub(super) filename: String,
}

fn metadata_time_ns(time: std::io::Result<std::time::SystemTime>) -> i128 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0)
}

fn watch_entry_from_metadata(meta: &fs::Metadata) -> WatchEntry {
    let ft = meta.file_type();
    #[cfg(unix)]
    let mode = meta.permissions().mode();
    #[cfg(not(unix))]
    let mode = if meta.permissions().readonly() {
        0o444
    } else {
        0o666
    };
    WatchEntry {
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        len: meta.len(),
        mode,
        modified_ns: metadata_time_ns(meta.modified()),
        created_ns: metadata_time_ns(meta.created()),
    }
}

/// `path` relative to `root`, with forward slashes.
pub(super) fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk_watch_dir(root: &Path, dir: &Path, recursive: bool, out: &mut WatchSnapshot) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let rel = relative_path(root, &path);
        out.insert(rel, watch_entry_from_metadata(&meta));
        if recursive && meta.is_dir() {
            walk_watch_dir(root, &path, true, out);
        }
    }
}

/// Full walk of `path` (one level, or the whole tree when `recursive`).
/// This is the operation #9591 took off the main thread's 25 ms timer.
pub(super) fn snapshot_watch_target(path: &str, recursive: bool) -> std::io::Result<WatchSnapshot> {
    let root = Path::new(path);
    let meta = fs::symlink_metadata(root)?;
    let mut snapshot = WatchSnapshot::new();
    if meta.is_dir() {
        walk_watch_dir(root, root, recursive, &mut snapshot);
    } else {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        snapshot.insert(name, watch_entry_from_metadata(&meta));
    }
    Ok(snapshot)
}

pub(super) fn diff_watch_snapshots(
    previous: &WatchSnapshot,
    current: &WatchSnapshot,
) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    let mut keys = BTreeMap::<String, ()>::new();
    for key in previous.keys() {
        keys.insert(key.clone(), ());
    }
    for key in current.keys() {
        keys.insert(key.clone(), ());
    }
    for key in keys.keys() {
        match (previous.get(key), current.get(key)) {
            (None, Some(_)) | (Some(_), None) => events.push(WatchEvent {
                event_type: "rename",
                filename: key.clone(),
            }),
            (Some(a), Some(b)) if a != b => events.push(WatchEvent {
                event_type: "change",
                filename: key.clone(),
            }),
            _ => {}
        }
    }
    events
}

// ============================================================================
// Raw events — what the producers (the OS backend's thread, the poll thread)
// queue.
// ============================================================================

/// Node's two `fs.watch` event names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventClass {
    Rename,
    Change,
}

impl EventClass {
    pub(super) fn node_name(self) -> &'static str {
        match self {
            EventClass::Rename => "rename",
            EventClass::Change => "change",
        }
    }
}

/// Which native instance produced an event — the router only offers an event
/// to the watchers registered on that instance (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// This thread's shared non-recursive instance.
    Shared,
    /// The private instance owned by watcher `id`.
    Own(usize),
}

/// A backend failure, in a form that crosses threads and clones freely
/// (`std::io::Error` does neither); rebuilt into an `io::Error` for the
/// Node-shaped error object at delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WatchError {
    pub(super) errno: Option<i32>,
    pub(super) message: String,
}

impl WatchError {
    pub(super) fn from_io(err: &std::io::Error) -> Self {
        WatchError {
            errno: err.raw_os_error(),
            message: err.to_string(),
        }
    }

    pub(super) fn generic(message: impl Into<String>) -> Self {
        WatchError {
            errno: None,
            message: message.into(),
        }
    }

    pub(super) fn to_io_error(&self) -> std::io::Error {
        match self.errno {
            Some(code) => std::io::Error::from_raw_os_error(code),
            None => std::io::Error::other(self.message.clone()),
        }
    }
}

pub(super) enum RawEvent {
    /// An OS observation of `path`, not yet attributed to a watcher.
    Native {
        source: Source,
        path: PathBuf,
        class: EventClass,
    },
    /// The fallback poller's diff for watcher `id` — already relative.
    Polled { id: usize, event: WatchEvent },
    /// A backend error; `paths` is what the backend attached (may be empty).
    Error {
        source: Source,
        paths: Vec<PathBuf>,
        error: WatchError,
    },
}

// ============================================================================
// The per-thread event queue — producer side of the event-pump protocol.
// ============================================================================

pub(super) struct EventQueue {
    items: Mutex<VecDeque<RawEvent>>,
    /// Mirror of `items.len()` so the per-tick pump can skip the mutex when
    /// nothing is queued (one relaxed load per event-loop turn).
    len: AtomicUsize,
}

impl EventQueue {
    fn new() -> Self {
        EventQueue {
            items: Mutex::new(VecDeque::new()),
            len: AtomicUsize::new(0),
        }
    }

    /// Queue a batch and wake the consumer ONCE. Safe from any thread.
    pub(super) fn push_all(&self, batch: impl IntoIterator<Item = RawEvent>) {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        let before = items.len();
        items.extend(batch);
        let after = items.len();
        self.len.store(after, Ordering::Release);
        drop(items);
        if after != before {
            crate::event_pump::js_notify_main_thread();
        }
    }

    fn drain(&self) -> Vec<RawEvent> {
        if self.len.load(Ordering::Acquire) == 0 {
            return Vec::new();
        }
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        let out: Vec<RawEvent> = items.drain(..).collect();
        self.len.store(0, Ordering::Release);
        out
    }
}

thread_local! {
    // One queue per JS thread: a watcher created on a `perry/thread` worker
    // is drained by that worker's pump, never by the main thread's.
    static QUEUE: Arc<EventQueue> = Arc::new(EventQueue::new());
}

fn queue_handle() -> Arc<EventQueue> {
    QUEUE.with(Arc::clone)
}

/// Take everything queued for this thread. Called by the pump each turn.
pub(super) fn drain_queue() -> Vec<RawEvent> {
    QUEUE.with(|queue| queue.drain())
}

// ============================================================================
// Native instances — `notify` everywhere but macOS (see `watch_fsevents`).
// ============================================================================

#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;
    use notify::event::ModifyKind;
    use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    /// Map one notify event onto Node's vocabulary.
    ///
    /// Node subscribes to create / delete / move / modify / attribute changes
    /// and nothing else — libuv's inotify mask has no `IN_ACCESS` /
    /// `IN_CLOSE_WRITE`, so `Access` events are dropped rather than surfacing
    /// as a third `'change'` per write. `Other` covers rescan / mount /
    /// overflow notices that libuv also does not forward.
    pub(super) fn classify(source: Source, event: Event) -> Vec<RawEvent> {
        let class = match event.kind {
            EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_)) => EventClass::Rename,
            EventKind::Modify(_) | EventKind::Any => EventClass::Change,
            EventKind::Access(_) | EventKind::Other => return Vec::new(),
        };
        event
            .paths
            .into_iter()
            .map(|path| RawEvent::Native {
                source,
                path,
                class,
            })
            .collect()
    }

    #[cfg(unix)]
    const ENOENT_CODE: i32 = libc::ENOENT;
    #[cfg(not(unix))]
    const ENOENT_CODE: i32 = 2;
    #[cfg(unix)]
    const ENOSPC_CODE: i32 = libc::ENOSPC;
    #[cfg(not(unix))]
    const ENOSPC_CODE: i32 = 28;

    fn errno_error(code: i32) -> WatchError {
        WatchError {
            errno: Some(code),
            message: std::io::Error::from_raw_os_error(code).to_string(),
        }
    }

    pub(super) fn watch_error_from_notify(error: notify::Error) -> (Vec<PathBuf>, WatchError) {
        let notify::Error { kind, paths } = error;
        let error = match kind {
            notify::ErrorKind::Io(err) => WatchError::from_io(&err),
            notify::ErrorKind::PathNotFound => errno_error(ENOENT_CODE),
            notify::ErrorKind::MaxFilesWatch => errno_error(ENOSPC_CODE),
            notify::ErrorKind::WatchNotFound => WatchError::generic("watch not found"),
            notify::ErrorKind::Generic(message) => WatchError::generic(message),
            notify::ErrorKind::InvalidConfig(_) => {
                WatchError::generic("invalid watcher configuration")
            }
        };
        (paths, error)
    }

    // As visible as `Backend::Own`, which carries it (private_interfaces).
    pub(in crate::fs::dir_glob_watch) struct NativeInstance(RecommendedWatcher);

    impl NativeInstance {
        pub(super) fn new(source: Source, queue: Arc<EventQueue>) -> Result<Self, WatchError> {
            let handler = move |result: notify::Result<Event>| match result {
                Ok(event) => queue.push_all(classify(source, event)),
                Err(error) => {
                    let (paths, error) = watch_error_from_notify(error);
                    queue.push_all(std::iter::once(RawEvent::Error {
                        source,
                        paths,
                        error,
                    }));
                }
            };
            // The interval only matters where `RecommendedWatcher` is notify's
            // own `PollWatcher` (targets with no OS facility); inotify and
            // ReadDirectoryChangesW ignore it. Match the fallback poller's ceiling.
            RecommendedWatcher::new(
                handler,
                Config::default().with_poll_interval(Duration::from_millis(POLL_MAX_INTERVAL_MS)),
            )
            .map(NativeInstance)
            .map_err(|error| watch_error_from_notify(error).1)
        }

        pub(super) fn watch(&mut self, root: &Path, recursive: bool) -> Result<(), WatchError> {
            let mode = if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            self.0
                .watch(root, mode)
                .map_err(|error| watch_error_from_notify(error).1)
        }

        pub(super) fn unwatch(&mut self, root: &Path) {
            let _ = self.0.unwatch(root);
        }
    }
}

fn new_native_watcher(source: Source) -> Result<NativeInstance, WatchError> {
    NativeInstance::new(source, queue_handle())
}

/// This thread's shared instance for non-recursive watchers.
struct SharedInstance {
    watcher: NativeInstance,
    /// canonical root → number of JS watchers registered on it. The OS watch
    /// is added on 0 → 1 and removed on 1 → 0 (libuv's per-path refcount).
    refs: HashMap<PathBuf, usize>,
}

thread_local! {
    // Outer `None`: never tried. `Some(None)`: construction failed — every
    // non-recursive watcher on this thread uses the poller from then on.
    static SHARED: RefCell<Option<Option<SharedInstance>>> = const { RefCell::new(None) };
}

fn with_shared<R>(f: impl FnOnce(&mut SharedInstance) -> R) -> Option<R> {
    SHARED
        .try_with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot =
                    Some(
                        new_native_watcher(Source::Shared)
                            .ok()
                            .map(|watcher| SharedInstance {
                                watcher,
                                refs: HashMap::new(),
                            }),
                    );
            }
            slot.as_mut().and_then(|inst| inst.as_mut()).map(f)
        })
        .ok()
        .flatten()
}

fn force_poll() -> bool {
    static FORCE: OnceLock<bool> = OnceLock::new();
    *FORCE.get_or_init(|| std::env::var_os(FORCE_POLL_ENV).is_some_and(|v| v == "1"))
}

/// The change source behind one JS watcher. Dropping it releases the OS
/// resource (or stops the poll thread).
pub(super) enum Backend {
    /// Registered on this thread's shared instance under `root`.
    Shared { root: PathBuf },
    /// Owns a private instance (recursive watchers).
    Own {
        root: PathBuf,
        _watcher: NativeInstance,
    },
    /// The walker fallback; the handle is held for its `Drop`, which stops
    /// the thread.
    Poll { _handle: PollHandle },
}

impl Backend {
    /// Start watching `path` for watcher `id`. Never fails: if the OS watch
    /// cannot be established the walker takes over (the caller has already
    /// validated that `path` exists, so ENOENT was thrown before this).
    pub(super) fn start(id: usize, path: &str, recursive: bool) -> Backend {
        if !force_poll() {
            if let Ok(backend) = start_native(id, path, recursive) {
                return backend;
            }
        }
        // Resolve once, like the native path: a later `process.chdir` must
        // not move a relative watch target under the poller.
        let resolved = fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        Backend::Poll {
            _handle: PollHandle::spawn(id, resolved, recursive),
        }
    }
}

fn start_native(id: usize, path: &str, recursive: bool) -> Result<Backend, WatchError> {
    // Events arrive with the OS's idea of the path (FSEvents reports the
    // resolved real path, inotify echoes what it was given); registering the
    // canonical root makes both strip cleanly at routing time.
    let root = fs::canonicalize(path).map_err(|err| WatchError::from_io(&err))?;
    if recursive {
        let mut watcher = new_native_watcher(Source::Own(id))?;
        watcher.watch(&root, true)?;
        return Ok(Backend::Own {
            root,
            _watcher: watcher,
        });
    }
    let registered = with_shared(|inst| -> Result<(), WatchError> {
        let count = inst.refs.entry(root.clone()).or_insert(0);
        if *count == 0 {
            inst.watcher.watch(&root, false)?;
        }
        *count += 1;
        Ok(())
    });
    match registered {
        Some(Ok(())) => Ok(Backend::Shared { root }),
        Some(Err(err)) => Err(err),
        None => Err(WatchError::generic("shared watcher unavailable")),
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Backend::Shared { root } = self {
            let root = std::mem::take(root);
            let _ = with_shared(|inst| {
                let Some(count) = inst.refs.get_mut(&root) else {
                    return;
                };
                *count = count.saturating_sub(1);
                if *count == 0 {
                    inst.refs.remove(&root);
                    inst.watcher.unwatch(&root);
                }
            });
        }
    }
}

// ============================================================================
// Routing helpers (pure).
// ============================================================================

/// The `filename` a watcher rooted at `root` reports for an event on `path`,
/// or `None` when `path` is outside the watcher's scope: a non-recursive
/// watcher sees the root itself and its direct children; a recursive one sees
/// everything beneath. An event on the root itself (the watched directory
/// deleted or renamed) reports the root's own name, as libuv does.
pub(super) fn filename_for(root: &Path, path: &Path, recursive: bool) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut components = rel.components();
    match (components.next(), components.next()) {
        (None, _) => Some(
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        (Some(_), None) => Some(relative_path(root, path)),
        (Some(_), Some(_)) if recursive => Some(relative_path(root, path)),
        _ => None,
    }
}

/// Whether a shared-instance error attached to `paths` concerns the watcher
/// rooted at `root`. An error with no paths concerns every shared watcher.
pub(super) fn error_concerns_root(root: &Path, paths: &[PathBuf]) -> bool {
    paths.is_empty()
        || paths
            .iter()
            .any(|path| path == root || path.parent() == Some(root))
}

// ============================================================================
// The fallback poller.
// ============================================================================

/// Sleep between walks so the poller's duty cycle stays at or below
/// 1/`POLL_DUTY_DIVISOR`, within [`POLL_MIN_INTERVAL_MS`, `POLL_MAX_INTERVAL_MS`].
pub(super) fn poll_interval_for(walk: Duration) -> Duration {
    walk.saturating_mul(POLL_DUTY_DIVISOR).clamp(
        Duration::from_millis(POLL_MIN_INTERVAL_MS),
        Duration::from_millis(POLL_MAX_INTERVAL_MS),
    )
}

pub(super) struct PollHandle {
    active: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PollHandle {
    fn spawn(id: usize, path: String, recursive: bool) -> PollHandle {
        let active = Arc::new(AtomicBool::new(true));
        let queue = queue_handle();
        let flag = Arc::clone(&active);
        let thread = std::thread::Builder::new()
            .name(format!("perry-fs-watch-poll-{id}"))
            .spawn(move || poll_loop(id, &path, recursive, &queue, &flag))
            .ok();
        PollHandle { active, thread }
    }
}

impl Drop for PollHandle {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        // Wake the sleeper so it exits now; never join — a walk may be in
        // flight and the caller is the JS thread.
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
        }
    }
}

fn sleep_unless_stopped(interval: Duration, active: &AtomicBool) -> bool {
    let deadline = Instant::now() + interval;
    loop {
        if !active.load(Ordering::Acquire) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        std::thread::park_timeout(deadline - now);
    }
}

fn poll_loop(id: usize, path: &str, recursive: bool, queue: &EventQueue, active: &AtomicBool) {
    let mut previous: Option<WatchSnapshot> = None;
    let mut interval = Duration::from_millis(POLL_MIN_INTERVAL_MS);
    loop {
        // The first pass is the baseline and runs immediately.
        if previous.is_some() && !sleep_unless_stopped(interval, active) {
            return;
        }
        if !active.load(Ordering::Acquire) {
            return;
        }
        let started = Instant::now();
        let current = snapshot_watch_target(path, recursive).unwrap_or_default();
        let walk = started.elapsed();
        if let Some(prev) = &previous {
            let events = diff_watch_snapshots(prev, &current);
            if !events.is_empty() {
                queue.push_all(
                    events
                        .into_iter()
                        .map(|event| RawEvent::Polled { id, event }),
                );
            }
        }
        previous = Some(current);
        interval = poll_interval_for(walk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_for_scopes_by_depth() {
        let root = Path::new("/w/root");
        assert_eq!(
            filename_for(root, Path::new("/w/root/a.txt"), false).as_deref(),
            Some("a.txt")
        );
        assert_eq!(
            filename_for(root, Path::new("/w/root/sub/b.txt"), false),
            None,
            "a non-recursive watcher must not see grandchildren"
        );
        assert_eq!(
            filename_for(root, Path::new("/w/root/sub/b.txt"), true).as_deref(),
            Some("sub/b.txt")
        );
        assert_eq!(
            filename_for(root, Path::new("/w/root"), false).as_deref(),
            Some("root"),
            "an event on the watched entry itself reports its own name (DELETE_SELF)"
        );
        assert_eq!(
            filename_for(root, Path::new("/w/other/a.txt"), true),
            None,
            "paths outside the root are never delivered"
        );
        assert_eq!(
            filename_for(root, Path::new("/w/rooted/a.txt"), true),
            None,
            "prefix matching is by path component, not by string prefix"
        );
    }

    #[test]
    fn error_concerns_root_matches_self_parent_or_everything() {
        let root = Path::new("/w/root");
        assert!(error_concerns_root(root, &[]));
        assert!(error_concerns_root(root, &[PathBuf::from("/w/root")]));
        assert!(error_concerns_root(root, &[PathBuf::from("/w/root/x")]));
        assert!(!error_concerns_root(
            root,
            &[PathBuf::from("/w/elsewhere/x")]
        ));
    }

    #[test]
    fn poll_interval_is_twenty_walks_wide_and_clamped() {
        assert_eq!(
            poll_interval_for(Duration::from_micros(200)),
            Duration::from_millis(POLL_MIN_INTERVAL_MS),
            "a 0.2 ms walk keeps the old 25 ms cadence"
        );
        assert_eq!(
            poll_interval_for(Duration::from_millis(10)),
            Duration::from_millis(200),
            "a 10 ms walk (3k files) polls every 200 ms — 5 % of a core, not 41 %"
        );
        assert_eq!(
            poll_interval_for(Duration::from_millis(1200)),
            Duration::from_millis(POLL_MAX_INTERVAL_MS),
            "the 362k-file walk is capped at watchFile's default interval"
        );
    }

    #[test]
    fn watch_errors_keep_their_errno() {
        let error = WatchError::from_io(&std::io::Error::from_raw_os_error(28));
        assert_eq!(error.errno, Some(28));
        assert_eq!(error.to_io_error().raw_os_error(), Some(28));
        let generic = WatchError::generic("boom");
        assert_eq!(generic.errno, None);
        assert_eq!(generic.to_io_error().to_string(), "boom");
    }

    #[test]
    fn queue_batches_wake_once_and_drain_in_order() {
        let queue = EventQueue::new();
        assert!(queue.drain().is_empty());
        queue.push_all((0..3).map(|i| RawEvent::Polled {
            id: i,
            event: WatchEvent {
                event_type: "change",
                filename: format!("f{i}"),
            },
        }));
        assert_eq!(queue.len.load(Ordering::Acquire), 3);
        let drained = queue.drain();
        let ids: Vec<usize> = drained
            .iter()
            .map(|item| match item {
                RawEvent::Polled { id, .. } => *id,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(ids, vec![0, 1, 2]);
        assert_eq!(queue.len.load(Ordering::Acquire), 0);
        assert!(queue.drain().is_empty());
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod notify_tests {
    use super::native::{classify, watch_error_from_notify};
    use super::*;
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode};
    use notify::{Event, EventKind};

    fn classes(kind: EventKind) -> Vec<Option<EventClass>> {
        let event = Event::new(kind).add_path(PathBuf::from("/w/a"));
        let raw = classify(Source::Shared, event);
        if raw.is_empty() {
            return vec![None];
        }
        raw.into_iter()
            .map(|item| match item {
                RawEvent::Native { class, .. } => Some(class),
                _ => panic!("classify only produces Native events"),
            })
            .collect()
    }

    #[test]
    fn classify_maps_onto_nodes_two_event_names() {
        assert_eq!(
            classes(EventKind::Create(CreateKind::File)),
            vec![Some(EventClass::Rename)]
        );
        assert_eq!(
            classes(EventKind::Remove(RemoveKind::Any)),
            vec![Some(EventClass::Rename)]
        );
        assert_eq!(
            classes(EventKind::Modify(ModifyKind::Name(RenameMode::Any))),
            vec![Some(EventClass::Rename)]
        );
        assert_eq!(
            classes(EventKind::Modify(ModifyKind::Data(DataChange::Content))),
            vec![Some(EventClass::Change)]
        );
        assert_eq!(
            classes(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))),
            vec![Some(EventClass::Change)]
        );
        assert_eq!(classes(EventKind::Any), vec![Some(EventClass::Change)]);
    }

    #[test]
    fn classify_drops_what_libuv_never_subscribes_to() {
        use notify::event::{AccessKind, AccessMode};
        assert_eq!(
            classes(EventKind::Access(AccessKind::Close(AccessMode::Write))),
            vec![None],
            "IN_CLOSE_WRITE is not in libuv's inotify mask — it must not become a third 'change'"
        );
        assert_eq!(classes(EventKind::Other), vec![None]);
    }

    #[test]
    fn classify_emits_one_event_per_path() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from("/w/old"))
            .add_path(PathBuf::from("/w/new"));
        let raw = classify(Source::Own(7), event);
        let paths: Vec<PathBuf> = raw
            .into_iter()
            .map(|item| match item {
                RawEvent::Native {
                    source,
                    path,
                    class,
                } => {
                    assert_eq!(source, Source::Own(7));
                    assert_eq!(class, EventClass::Rename);
                    path
                }
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            paths,
            vec![PathBuf::from("/w/old"), PathBuf::from("/w/new")]
        );
    }

    #[test]
    fn notify_errors_keep_their_errno() {
        let (paths, error) = watch_error_from_notify(
            notify::Error::io(std::io::Error::from_raw_os_error(28))
                .add_path(PathBuf::from("/w/root")),
        );
        assert_eq!(paths, vec![PathBuf::from("/w/root")]);
        assert_eq!(error.errno, Some(28));
        let (_, generic) = watch_error_from_notify(notify::Error::generic("boom"));
        assert_eq!(generic.errno, None);
        assert_eq!(generic.message, "boom");
    }
}
