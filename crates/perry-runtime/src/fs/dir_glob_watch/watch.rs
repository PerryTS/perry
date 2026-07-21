use super::*;
// Disambiguate from the private `crate::fs::string_value` pulled in by the
// `use crate::fs::*` glob below — this module wants the trunk's
// `(&[u8]) -> f64` helper.
use super::string_value;
// See the note in `opendir.rs`: the parent `fs` module's helpers are globbed in
// directly here (we are a grandchild of `fs`); the two private-to-`fs/mod.rs`
// helpers are named explicitly so a glob that skips privates can't drop them.
use crate::fs::{encoded_string_ptr, fs_encoding_option};
use crate::string::js_string_from_bytes;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Once;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_arity, ClosureHeader,
};

const FS_WATCH_POLL_INTERVAL_MS: f64 = 25.0;
const WATCH_FILE_DEFAULT_INTERVAL_MS: f64 = 5007.0;

#[derive(Clone, Copy)]
struct WatchListener {
    callback: f64,
    once: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct WatchEntry {
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    len: u64,
    mode: u32,
    modified_ns: i128,
    created_ns: i128,
}

type WatchSnapshot = BTreeMap<String, WatchEntry>;

#[derive(Clone)]
struct WatchEvent {
    event_type: &'static str,
    filename: String,
}

struct FsWatchState {
    path: String,
    recursive: bool,
    encoding: String,
    object_value: f64,
    timer_id: i64,
    snapshot: WatchSnapshot,
    listeners: HashMap<String, Vec<WatchListener>>,
    signal: f64,
    abort_listener: f64,
}

#[derive(Clone, PartialEq)]
struct StatSnapshot {
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    mode: u32,
    uid: f64,
    gid: f64,
    nlink: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
}

struct WatchFileState {
    path: String,
    object_value: f64,
    timer_id: i64,
    bigint: bool,
    previous: Option<StatSnapshot>,
    listeners: HashMap<String, Vec<WatchListener>>,
}

struct PromiseWatchState {
    path: String,
    recursive: bool,
    encoding: String,
    object_value: f64,
    timer_id: i64,
    persistent: bool,
    active: bool,
    snapshot: WatchSnapshot,
    queue: VecDeque<WatchEvent>,
    pending: VecDeque<*mut crate::promise::Promise>,
    signal: f64,
    abort_listener: f64,
    closed: bool,
    abort_reason: Option<f64>,
}

struct GlobIteratorState {
    entries: Vec<FsGlobMatch>,
    index: usize,
    with_file_types: bool,
    closed: bool,
    validation_error: Option<f64>,
}

thread_local! {
    static NEXT_WATCH_ID: RefCell<usize> = const { RefCell::new(1) };
    static NEXT_GLOB_ITERATOR_ID: RefCell<usize> = const { RefCell::new(1) };
    static FS_WATCHERS: RefCell<HashMap<usize, FsWatchState>> = RefCell::new(HashMap::new());
    static WATCH_FILE_STATES: RefCell<HashMap<usize, WatchFileState>> = RefCell::new(HashMap::new());
    static WATCH_FILE_PATHS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    static PROMISE_WATCHERS: RefCell<HashMap<usize, PromiseWatchState>> = RefCell::new(HashMap::new());
    static GLOB_ITERATORS: RefCell<HashMap<usize, GlobIteratorState>> = RefCell::new(HashMap::new());
}

fn next_watch_id() -> usize {
    NEXT_WATCH_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1);
        id
    })
}

fn next_glob_iterator_id() -> usize {
    NEXT_GLOB_ITERATOR_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1);
        id
    })
}

fn read_string_value(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(value, &mut scratch) {
        if ptr.is_null() {
            return Some(String::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    None
}

fn event_name(value: f64) -> String {
    read_string_value(value).unwrap_or_default()
}

fn validate_listener(value: f64) {
    unsafe {
        let _ = validate::js_validate_event_listener(
            value.to_bits() as i64,
            b"listener".as_ptr(),
            b"listener".len() as u32,
        );
    }
}

fn optional_listener(value: f64) -> Option<f64> {
    if is_nullish(value) {
        None
    } else {
        validate_listener(value);
        Some(value)
    }
}

fn option_bool_default_local(options_value: f64, field: &[u8], default_value: bool) -> bool {
    unsafe {
        match options_field_value(options_value, field) {
            Some(value) => crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0,
            None => default_value,
        }
    }
}

fn option_interval_ms(options_value: f64) -> f64 {
    unsafe {
        options_number_field(options_value, b"interval")
            .filter(|n| n.is_finite() && *n > 0.0)
            .unwrap_or(WATCH_FILE_DEFAULT_INTERVAL_MS)
    }
}

fn signal_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"options.signal\" property must be an instance of AbortSignal. Received {}",
        validate::describe_received(value)
    );
    validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn option_signal_value(options_value: f64) -> Result<Option<f64>, f64> {
    let options_js = crate::value::JSValue::from_bits(options_value.to_bits());
    if options_js.is_undefined() || options_js.is_null() || options_js.is_any_string() {
        return Ok(None);
    }
    unsafe {
        let Some(signal_value) = options_field_value(options_value, b"signal") else {
            return Ok(None);
        };
        let signal = f64::from_bits(signal_value.bits());
        if is_nullish(signal) {
            return Ok(None);
        }
        if crate::url::abort::abort_signal_ptr_from_value(signal).is_some() {
            Ok(Some(signal))
        } else {
            Err(signal_type_error(signal))
        }
    }
}

fn signal_is_aborted(signal: f64) -> bool {
    crate::url::abort::abort_signal_ptr_from_value(signal)
        .is_some_and(|ptr| crate::url::js_abort_signal_is_aborted(ptr) != 0)
}

fn signal_abort_reason(signal: f64) -> f64 {
    let Some(ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) else {
        return crate::url::js_abort_error_value();
    };
    let reason = crate::object::js_object_get_field_f64(ptr, 1);
    if crate::value::JSValue::from_bits(reason.to_bits()).is_undefined() {
        crate::url::js_abort_error_value()
    } else {
        reason
    }
}

fn add_abort_listener(
    signal: f64,
    id: usize,
    func: extern "C" fn(*const ClosureHeader) -> f64,
) -> f64 {
    let Some(signal_ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) else {
        return undefined_value();
    };
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, id as f64);
    let listener = boxed_ptr(closure as *const u8);
    crate::url::js_abort_signal_add_listener(signal_ptr, string_value(b"abort"), listener);
    listener
}

fn remove_abort_listener(signal: f64, listener: f64) {
    if is_nullish(signal) || is_nullish(listener) {
        return;
    }
    if let Some(signal_ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) {
        crate::url::js_abort_signal_remove_listener(signal_ptr, string_value(b"abort"), listener);
    }
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

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk_watch_dir(root: &Path, dir: &Path, recursive: bool, out: &mut WatchSnapshot) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
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

fn snapshot_watch_target(path: &str, recursive: bool) -> std::io::Result<WatchSnapshot> {
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

fn diff_watch_snapshots(previous: &WatchSnapshot, current: &WatchSnapshot) -> Vec<WatchEvent> {
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

fn stat_snapshot(path: &str) -> Option<StatSnapshot> {
    let meta = fs::metadata(path).ok()?;
    let ft = meta.file_type();
    #[cfg(unix)]
    let mode = meta.permissions().mode();
    #[cfg(not(unix))]
    let mode = if meta.permissions().readonly() {
        0o444
    } else {
        0o666
    };
    let (uid, gid) = metadata_owner_ids(&meta);
    let nlink = metadata_nlink(&meta);
    let (atime_ms, mtime_ms, ctime_ms, birthtime_ms) = metadata_times_ms(&meta);
    Some(StatSnapshot {
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        size: meta.len(),
        mode,
        uid,
        gid,
        nlink,
        atime_ms,
        mtime_ms,
        ctime_ms,
        birthtime_ms,
    })
}

fn zero_stat_snapshot() -> StatSnapshot {
    StatSnapshot {
        is_file: false,
        is_dir: false,
        is_symlink: false,
        size: 0,
        mode: 0,
        uid: -1.0,
        gid: -1.0,
        nlink: 0.0,
        atime_ms: 0.0,
        mtime_ms: 0.0,
        ctime_ms: 0.0,
        birthtime_ms: 0.0,
    }
}

fn build_stat_value(snapshot: &StatSnapshot, bigint: bool) -> f64 {
    unsafe {
        build_stats_object(
            snapshot.is_file,
            snapshot.is_dir,
            snapshot.is_symlink,
            snapshot.size,
            snapshot.mode,
            snapshot.uid,
            snapshot.gid,
            snapshot.nlink,
            snapshot.atime_ms,
            snapshot.mtime_ms,
            snapshot.ctime_ms,
            snapshot.birthtime_ms,
            bigint,
            None,
        )
    }
}

fn add_listener(
    listeners: &mut HashMap<String, Vec<WatchListener>>,
    event: String,
    callback: f64,
    once: bool,
) {
    listeners
        .entry(event)
        .or_default()
        .push(WatchListener { callback, once });
}

fn take_event_listeners(
    listeners: &mut HashMap<String, Vec<WatchListener>>,
    event: &str,
) -> Vec<WatchListener> {
    let snapshot = listeners.get(event).cloned().unwrap_or_default();
    if snapshot.iter().any(|listener| listener.once) {
        if let Some(list) = listeners.get_mut(event) {
            list.retain(|listener| !listener.once);
        }
    }
    snapshot
}

fn remove_listener(
    listeners: &mut HashMap<String, Vec<WatchListener>>,
    event: &str,
    callback: f64,
) {
    if let Some(list) = listeners.get_mut(event) {
        let bits = callback.to_bits();
        list.retain(|listener| listener.callback.to_bits() != bits);
    }
}

fn has_change_listeners(listeners: &HashMap<String, Vec<WatchListener>>) -> bool {
    listeners
        .get("change")
        .is_some_and(|listeners| !listeners.is_empty())
}

fn with_watcher_uncaught_trap<F: FnOnce()>(f: F) {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
    if jumped == 0 {
        f();
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::os::emit_process_uncaught_exception(exc);
    }
    crate::exception::js_try_end();
}

fn filename_arg_value(filename: &str, encoding: &str) -> f64 {
    let bytes = filename.as_bytes();
    if encoding == "buffer" {
        let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
        if !buf.is_null() && !bytes.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    crate::buffer::buffer_data_mut(buf),
                    bytes.len(),
                );
            }
        }
        boxed_ptr(buf as *const u8)
    } else {
        let ptr = encoded_string_ptr(bytes, encoding);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
}

fn emit_listener0(object_value: f64, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = scope.root_nanbox_f64(object_value);
    let callback_handle = scope.root_nanbox_f64(callback);
    let cb = extract_closure_ptr(callback_handle.get_nanbox_f64());
    if cb.is_null() {
        return;
    }
    let prev_this = crate::object::js_implicit_this_set(object_handle.get_nanbox_f64());
    with_watcher_uncaught_trap(|| {
        crate::closure::js_closure_call0(cb);
    });
    crate::object::js_implicit_this_set(prev_this);
}

fn emit_fs_watch_event(
    object_value: f64,
    callbacks: Vec<WatchListener>,
    event: &WatchEvent,
    encoding: &str,
) {
    if callbacks.is_empty() {
        return;
    }
    let raw_callbacks: Vec<f64> = callbacks.iter().map(|listener| listener.callback).collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handles = scope.root_nanbox_f64_slice(&raw_callbacks);
    let object_handle = scope.root_nanbox_f64(object_value);
    let event_type = string_value(event.event_type.as_bytes());
    let event_type_handle = scope.root_nanbox_f64(event_type);
    let filename = filename_arg_value(&event.filename, encoding);
    let args = [event_type_handle.get_nanbox_f64(), filename];
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let refreshed_callbacks =
        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&callback_handles);
    let refreshed_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    for callback in refreshed_callbacks {
        let cb = extract_closure_ptr(callback);
        if cb.is_null() {
            continue;
        }
        let prev_this = crate::object::js_implicit_this_set(object_handle.get_nanbox_f64());
        with_watcher_uncaught_trap(|| {
            crate::closure::js_closure_call2(cb, refreshed_args[0], refreshed_args[1]);
        });
        crate::object::js_implicit_this_set(prev_this);
    }
}

fn emit_watch_file_change(
    object_value: f64,
    callbacks: Vec<WatchListener>,
    curr: &StatSnapshot,
    prev: &StatSnapshot,
    bigint: bool,
) {
    if callbacks.is_empty() {
        return;
    }
    let raw_callbacks: Vec<f64> = callbacks.iter().map(|listener| listener.callback).collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handles = scope.root_nanbox_f64_slice(&raw_callbacks);
    let object_handle = scope.root_nanbox_f64(object_value);
    let curr_value = build_stat_value(curr, bigint);
    let curr_handle = scope.root_nanbox_f64(curr_value);
    let prev_value = build_stat_value(prev, bigint);
    let args = [curr_handle.get_nanbox_f64(), prev_value];
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let refreshed_callbacks =
        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&callback_handles);
    let refreshed_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    for callback in refreshed_callbacks {
        let cb = extract_closure_ptr(callback);
        if cb.is_null() {
            continue;
        }
        let prev_this = crate::object::js_implicit_this_set(object_handle.get_nanbox_f64());
        with_watcher_uncaught_trap(|| {
            crate::closure::js_closure_call2(cb, refreshed_args[0], refreshed_args[1]);
        });
        crate::object::js_implicit_this_set(prev_this);
    }
}

fn close_fs_watcher(id: usize) {
    let removed = FS_WATCHERS.with(|watchers| watchers.borrow_mut().remove(&id));
    let Some(mut state) = removed else {
        return;
    };
    crate::timer::clearInterval(state.timer_id);
    remove_abort_listener(state.signal, state.abort_listener);
    let close_listeners = take_event_listeners(&mut state.listeners, "close");
    for listener in close_listeners {
        emit_listener0(state.object_value, listener.callback);
    }
}

fn close_watch_file_state(id: usize) {
    let removed = WATCH_FILE_STATES.with(|states| states.borrow_mut().remove(&id));
    if let Some(state) = removed {
        crate::timer::clearInterval(state.timer_id);
        WATCH_FILE_PATHS.with(|paths| {
            paths.borrow_mut().remove(&state.path);
        });
    }
}

fn close_promise_watcher_return(id: usize) -> Vec<*mut crate::promise::Promise> {
    let removed = PROMISE_WATCHERS.with(|watchers| watchers.borrow_mut().remove(&id));
    let Some(state) = removed else {
        return Vec::new();
    };
    if state.timer_id != 0 {
        crate::timer::clearInterval(state.timer_id);
    }
    remove_abort_listener(state.signal, state.abort_listener);
    state.pending.into_iter().collect()
}

fn abort_promise_watcher(id: usize, reason: f64) -> Vec<*mut crate::promise::Promise> {
    PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return Vec::new();
        };
        if state.timer_id != 0 {
            crate::timer::clearInterval(state.timer_id);
        }
        remove_abort_listener(state.signal, state.abort_listener);
        state.timer_id = 0;
        state.active = false;
        state.signal = undefined_value();
        state.abort_listener = undefined_value();
        state.object_value = undefined_value();
        state.closed = true;
        state.abort_reason = Some(reason);
        state.queue.clear();
        state.pending.drain(..).collect()
    })
}

fn iterator_result(value: f64, done: bool) -> f64 {
    let value_key = js_string_from_bytes(b"value".as_ptr(), b"value".len() as u32);
    let done_key = js_string_from_bytes(b"done".as_ptr(), b"done".len() as u32);
    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field_by_name(obj, value_key, value);
    crate::object::js_object_set_field_by_name(obj, done_key, bool_value(done));
    boxed_ptr(obj as *const u8)
}

fn set_named_field(obj: *mut crate::object::ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

fn watch_event_object(event: &WatchEvent, encoding: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event_type = string_value(event.event_type.as_bytes());
    let event_type_handle = scope.root_nanbox_f64(event_type);
    let filename = filename_arg_value(&event.filename, encoding);
    let filename_handle = scope.root_nanbox_f64(filename);
    let event_type_key = js_string_from_bytes(b"eventType".as_ptr(), b"eventType".len() as u32);
    let filename_key = js_string_from_bytes(b"filename".as_ptr(), b"filename".len() as u32);
    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field_by_name(
        obj,
        event_type_key,
        event_type_handle.get_nanbox_f64(),
    );
    crate::object::js_object_set_field_by_name(obj, filename_key, filename_handle.get_nanbox_f64());
    boxed_ptr(obj as *const u8)
}

fn promise_value_from_ptr(promise: *mut crate::promise::Promise) -> f64 {
    boxed_ptr(promise as *const u8)
}

fn resolved_iterator_promise(value: f64, done: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let result = iterator_result(value_handle.get_nanbox_f64(), done);
    let result_handle = scope.root_nanbox_f64(result);
    promise_value_from_ptr(crate::promise::js_promise_resolved(
        result_handle.get_nanbox_f64(),
    ))
}

fn rejected_promise_value(reason: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason_handle = scope.root_nanbox_f64(reason);
    promise_value_from_ptr(crate::promise::js_promise_rejected(
        reason_handle.get_nanbox_f64(),
    ))
}

fn resolve_promise_with_event(
    promise: *mut crate::promise::Promise,
    event: WatchEvent,
    encoding: String,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let event_value = watch_event_object(&event, &encoding);
    let event_handle = scope.root_nanbox_f64(event_value);
    let result = iterator_result(event_handle.get_nanbox_f64(), false);
    let result_handle = scope.root_nanbox_f64(result);
    crate::promise::js_promise_resolve(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        result_handle.get_nanbox_f64(),
    );
}

fn resolve_promise_done(promise: *mut crate::promise::Promise) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let result = iterator_result(undefined_value(), true);
    let result_handle = scope.root_nanbox_f64(result);
    crate::promise::js_promise_resolve(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        result_handle.get_nanbox_f64(),
    );
}

fn reject_promise(promise: *mut crate::promise::Promise, reason: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let reason_handle = scope.root_nanbox_f64(reason);
    crate::promise::js_promise_reject(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        reason_handle.get_nanbox_f64(),
    );
}

extern "C" fn fs_watcher_poll_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let deliveries = FS_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return Vec::new();
        };
        let current = snapshot_watch_target(&state.path, state.recursive).unwrap_or_default();
        let events = diff_watch_snapshots(&state.snapshot, &current);
        state.snapshot = current;
        events
            .into_iter()
            .map(|event| {
                let callbacks = take_event_listeners(&mut state.listeners, "change");
                (state.object_value, callbacks, event, state.encoding.clone())
            })
            .collect()
    });
    for (object_value, callbacks, event, encoding) in deliveries {
        emit_fs_watch_event(object_value, callbacks, &event, &encoding);
    }
    undefined_value()
}

extern "C" fn promise_watcher_poll_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let actions = PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return Vec::new();
        };
        if state.closed {
            return Vec::new();
        }
        let current = snapshot_watch_target(&state.path, state.recursive).unwrap_or_default();
        let events = diff_watch_snapshots(&state.snapshot, &current);
        state.snapshot = current;
        let mut actions = Vec::new();
        for event in events {
            if let Some(promise) = state.pending.pop_front() {
                actions.push((promise, event, state.encoding.clone()));
            } else {
                state.queue.push_back(event);
            }
        }
        actions
    });
    for (promise, event, encoding) in actions {
        resolve_promise_with_event(promise, event, encoding);
    }
    undefined_value()
}

fn start_promise_watcher(id: usize, state: &mut PromiseWatchState) {
    if state.active || state.closed {
        return;
    }
    // Re-baseline the snapshot at the moment iteration actually begins (the
    // first `.next()` pull), then let `promise_watcher_poll_impl` advance the
    // baseline after every poll. This makes the watcher's two behaviors match
    // Node:
    //   * Events emitted between `watch()` and the first `.next()` are NOT
    //     delivered — Node's async iterator only starts collecting once you
    //     iterate, so a write before the first pull is ignored. Folding the
    //     current directory state into the baseline here drops those.
    //   * A write that happens AFTER a pull is begun is delivered, because each
    //     subsequent poll diffs against the post-pull baseline (which advanced
    //     past the now-consumed state) and so detects the fresh change.
    // Seeding the baseline at creation time (in `js_fs_promises_watch`) without
    // this refresh broke the post-pull case: the first poll would report the
    // pre-pull write to the pending pull, and—more importantly—left the
    // bookkeeping seeded against stale creation-time state. Refreshing here
    // restores both halves.
    state.snapshot = snapshot_watch_target(&state.path, state.recursive).unwrap_or_default();
    let timer_callback = poll_closure_value(promise_watcher_poll_impl as *const u8, id);
    let timer_id = crate::timer::setInterval(timer_callback as i64, FS_WATCH_POLL_INTERVAL_MS);
    if !state.persistent {
        crate::timer::js_timer_unref(timer_id);
    }
    state.timer_id = timer_id;
    state.active = true;
}

extern "C" fn watch_file_poll_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let delivery = WATCH_FILE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(&id) else {
            return None;
        };
        let current = stat_snapshot(&state.path);
        if current == state.previous {
            return None;
        }
        let prev = state.previous.clone().unwrap_or_else(zero_stat_snapshot);
        let curr = current.clone().unwrap_or_else(zero_stat_snapshot);
        state.previous = current;
        let callbacks = take_event_listeners(&mut state.listeners, "change");
        Some((state.object_value, callbacks, curr, prev, state.bigint))
    });
    if let Some((object_value, callbacks, curr, prev, bigint)) = delivery {
        emit_watch_file_change(object_value, callbacks, &curr, &prev, bigint);
    }
    undefined_value()
}

extern "C" fn fs_watcher_abort_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    close_fs_watcher(id);
    undefined_value()
}

extern "C" fn promise_watcher_abort_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let signal = PROMISE_WATCHERS.with(|watchers| {
        watchers
            .borrow()
            .get(&id)
            .map(|state| state.signal)
            .unwrap_or_else(undefined_value)
    });
    let reason = signal_abort_reason(signal);
    let pending = abort_promise_watcher(id, reason);
    for promise in pending {
        reject_promise(promise, reason);
    }
    undefined_value()
}

extern "C" fn fs_watcher_close_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    close_fs_watcher(id);
    self_value
}

extern "C" fn fs_watcher_ref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow().get(&id) {
            crate::timer::js_timer_ref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn fs_watcher_unref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow().get(&id) {
            crate::timer::js_timer_unref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn fs_watcher_on_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, false);
        }
    });
    self_value
}

extern "C" fn fs_watcher_once_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, true);
        }
    });
    self_value
}

extern "C" fn fs_watcher_off_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow_mut().get_mut(&id) {
            remove_listener(&mut state.listeners, &event, listener);
        }
    });
    self_value
}

extern "C" fn stat_watcher_ref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow().get(&id) {
            crate::timer::js_timer_ref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn stat_watcher_unref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow().get(&id) {
            crate::timer::js_timer_unref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn stat_watcher_on_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, false);
        }
    });
    self_value
}

extern "C" fn stat_watcher_once_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, true);
        }
    });
    self_value
}

extern "C" fn stat_watcher_off_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(&id) {
            remove_listener(&mut state.listeners, &event, listener);
        }
    });
    self_value
}

enum PromiseNextAction {
    Done,
    Reject(f64),
    Event(WatchEvent, String),
    Pending,
}

enum GlobNextAction {
    Done,
    Reject(f64),
    Entry(FsGlobMatch, bool),
}

extern "C" fn glob_iterator_next_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let action = GLOB_ITERATORS.with(|iterators| {
        let mut iterators = iterators.borrow_mut();
        let Some(state) = iterators.get_mut(&id) else {
            return GlobNextAction::Done;
        };
        if let Some(reason) = state.validation_error.take() {
            state.closed = true;
            return GlobNextAction::Reject(reason);
        }
        if state.closed || state.index >= state.entries.len() {
            state.closed = true;
            return GlobNextAction::Done;
        }
        let entry = state.entries[state.index].clone();
        state.index += 1;
        GlobNextAction::Entry(entry, state.with_file_types)
    });
    match action {
        GlobNextAction::Done => resolved_iterator_promise(undefined_value(), true),
        GlobNextAction::Reject(reason) => rejected_promise_value(reason),
        GlobNextAction::Entry(entry, with_file_types) => {
            resolved_iterator_promise(glob_entry_value(&entry, with_file_types), false)
        }
    }
}

extern "C" fn glob_iterator_return_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    GLOB_ITERATORS.with(|iterators| {
        iterators.borrow_mut().remove(&id);
    });
    resolved_iterator_promise(undefined_value(), true)
}

extern "C" fn glob_iterator_self_impl(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 1)
}

extern "C" fn promise_watcher_next_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let action = PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return PromiseNextAction::Done;
        };
        if let Some(reason) = state.abort_reason {
            return PromiseNextAction::Reject(reason);
        }
        if state.closed {
            return PromiseNextAction::Done;
        }
        start_promise_watcher(id, state);
        if let Some(event) = state.queue.pop_front() {
            return PromiseNextAction::Event(event, state.encoding.clone());
        }
        PromiseNextAction::Pending
    });
    match action {
        PromiseNextAction::Done => resolved_iterator_promise(undefined_value(), true),
        PromiseNextAction::Reject(reason) => rejected_promise_value(reason),
        PromiseNextAction::Event(event, encoding) => {
            let value = watch_event_object(&event, &encoding);
            resolved_iterator_promise(value, false)
        }
        PromiseNextAction::Pending => {
            let promise = crate::promise::js_promise_new();
            PROMISE_WATCHERS.with(|watchers| {
                if let Some(state) = watchers.borrow_mut().get_mut(&id) {
                    state.pending.push_back(promise);
                }
            });
            promise_value_from_ptr(promise)
        }
    }
}

extern "C" fn promise_watcher_return_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let pending = close_promise_watcher_return(id);
    for promise in pending {
        resolve_promise_done(promise);
    }
    resolved_iterator_promise(undefined_value(), true)
}

extern "C" fn promise_watcher_self_impl(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 1)
}

fn ensure_watch_method_arities() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        js_register_closure_arity(fs_watcher_poll_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_poll_impl as *const u8, 0);
        js_register_closure_arity(watch_file_poll_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_abort_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_abort_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_close_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_ref_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_unref_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_on_impl as *const u8, 2);
        js_register_closure_arity(fs_watcher_once_impl as *const u8, 2);
        js_register_closure_arity(fs_watcher_off_impl as *const u8, 2);
        js_register_closure_arity(stat_watcher_ref_impl as *const u8, 0);
        js_register_closure_arity(stat_watcher_unref_impl as *const u8, 0);
        js_register_closure_arity(stat_watcher_on_impl as *const u8, 2);
        js_register_closure_arity(stat_watcher_once_impl as *const u8, 2);
        js_register_closure_arity(stat_watcher_off_impl as *const u8, 2);
        js_register_closure_arity(promise_watcher_next_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_return_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_self_impl as *const u8, 0);
        js_register_closure_arity(glob_iterator_next_impl as *const u8, 0);
        js_register_closure_arity(glob_iterator_return_impl as *const u8, 0);
        js_register_closure_arity(glob_iterator_self_impl as *const u8, 0);
    });
}

fn method_value(func: *const u8, id: usize, self_value: f64) -> f64 {
    let closure = js_closure_alloc(func, 2);
    js_closure_set_capture_f64(closure, 0, id as f64);
    js_closure_set_capture_f64(closure, 1, self_value);
    boxed_ptr(closure as *const u8)
}

fn poll_closure_value(func: *const u8, id: usize) -> *mut ClosureHeader {
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_f64(closure, 0, id as f64);
    closure
}

fn build_fs_watcher_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 8);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"close",
        method_value(fs_watcher_close_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"ref",
        method_value(fs_watcher_ref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"unref",
        method_value(fs_watcher_unref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"on",
        method_value(fs_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"once",
        method_value(fs_watcher_once_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"addListener",
        method_value(fs_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"removeListener",
        method_value(fs_watcher_off_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"off",
        method_value(fs_watcher_off_impl as *const u8, id, self_value),
    );
    self_value
}

fn build_stat_watcher_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 7);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"ref",
        method_value(stat_watcher_ref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"unref",
        method_value(stat_watcher_unref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"on",
        method_value(stat_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"once",
        method_value(stat_watcher_once_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"addListener",
        method_value(stat_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"removeListener",
        method_value(stat_watcher_off_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"off",
        method_value(stat_watcher_off_impl as *const u8, id, self_value),
    );
    self_value
}

fn build_promise_watcher_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 2);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"next",
        method_value(promise_watcher_next_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"return",
        method_value(promise_watcher_return_impl as *const u8, id, self_value),
    );
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let symbol_value = boxed_ptr(async_iterator as *const u8);
        let method = method_value(promise_watcher_self_impl as *const u8, id, self_value);
        unsafe {
            crate::symbol::js_object_set_symbol_property(self_value, symbol_value, method);
        }
    }
    self_value
}

fn build_glob_iterator_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 3);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"next",
        method_value(glob_iterator_next_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"return",
        method_value(glob_iterator_return_impl as *const u8, id, self_value),
    );
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let symbol_value = boxed_ptr(async_iterator as *const u8);
        let method = method_value(glob_iterator_self_impl as *const u8, id, self_value);
        unsafe {
            crate::symbol::js_object_set_symbol_property(self_value, symbol_value, method);
        }
    }
    self_value
}

pub(crate) fn js_fs_promises_glob_iterator(pattern_value: f64, options_value: f64) -> f64 {
    let (entries, with_file_types, validation_error) =
        match run_fs_glob_result(pattern_value, options_value) {
            Ok(run) => (run.matches, run.with_file_types, None),
            Err(err) => (Vec::new(), false, Some(err)),
        };
    let id = next_glob_iterator_id();
    GLOB_ITERATORS.with(|iterators| {
        iterators.borrow_mut().insert(
            id,
            GlobIteratorState {
                entries,
                index: 0,
                with_file_types,
                closed: false,
                validation_error,
            },
        );
    });
    build_glob_iterator_object(id)
}

fn normalized_watch_args(arg1: f64, arg2: f64) -> (f64, Option<f64>) {
    if is_callable(arg1) {
        (undefined_value(), Some(arg1))
    } else {
        let listener = optional_listener(arg2);
        (arg1, listener)
    }
}

/// `fs.watch(path[, options][, listener])` — polling-backed watcher.
#[no_mangle]
pub extern "C" fn js_fs_watch(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let (options_value, listener) = normalized_watch_args(arg1, arg2);
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    let encoding = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let persistent = option_bool_default_local(options_value, b"persistent", true);
    let recursive = option_bool_default_local(options_value, b"recursive", false);
    let signal = match option_signal_value(options_value) {
        Ok(signal) => signal,
        Err(err) => crate::exception::js_throw(err),
    };
    let snapshot = match snapshot_watch_target(&path, recursive) {
        Ok(snapshot) => snapshot,
        Err(err) => unsafe {
            crate::exception::js_throw(build_fs_error_value(&err, "watch", &path));
        },
    };
    let id = next_watch_id();
    let object_value = build_fs_watcher_object(id);
    let timer_callback = poll_closure_value(fs_watcher_poll_impl as *const u8, id);
    let timer_id = crate::timer::setInterval(timer_callback as i64, FS_WATCH_POLL_INTERVAL_MS);
    if !persistent {
        crate::timer::js_timer_unref(timer_id);
    }
    let abort_listener = signal
        .map(|signal| add_abort_listener(signal, id, fs_watcher_abort_impl))
        .unwrap_or_else(undefined_value);
    let signal_value = signal.unwrap_or_else(undefined_value);
    let mut listeners = HashMap::new();
    if let Some(listener) = listener {
        add_listener(&mut listeners, "change".to_string(), listener, false);
    }
    FS_WATCHERS.with(|watchers| {
        watchers.borrow_mut().insert(
            id,
            FsWatchState {
                path,
                recursive,
                encoding,
                object_value,
                timer_id,
                snapshot,
                listeners,
                signal: signal_value,
                abort_listener,
            },
        );
    });
    if signal.map(signal_is_aborted).unwrap_or(false) {
        close_fs_watcher(id);
    }
    object_value
}

/// `fs.watchFile(path[, options], listener)` — stat-polling watcher.
#[no_mangle]
pub extern "C" fn js_fs_watch_file(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let (options_value, listener) = if is_callable(arg1) {
        (undefined_value(), arg1)
    } else {
        validate_listener(arg2);
        (arg1, arg2)
    };
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    if let Some(existing_id) = WATCH_FILE_PATHS.with(|paths| paths.borrow().get(&path).copied()) {
        WATCH_FILE_STATES.with(|states| {
            if let Some(state) = states.borrow_mut().get_mut(&existing_id) {
                add_listener(&mut state.listeners, "change".to_string(), listener, false);
            }
        });
        return WATCH_FILE_STATES.with(|states| {
            states
                .borrow()
                .get(&existing_id)
                .map(|state| state.object_value)
                .unwrap_or_else(undefined_value)
        });
    }
    let id = next_watch_id();
    let object_value = build_stat_watcher_object(id);
    let interval = option_interval_ms(options_value);
    let persistent = option_bool_default_local(options_value, b"persistent", true);
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    let timer_callback = poll_closure_value(watch_file_poll_impl as *const u8, id);
    let timer_id = crate::timer::setInterval(timer_callback as i64, interval);
    if !persistent {
        crate::timer::js_timer_unref(timer_id);
    }
    let mut listeners = HashMap::new();
    add_listener(&mut listeners, "change".to_string(), listener, false);
    WATCH_FILE_STATES.with(|states| {
        states.borrow_mut().insert(
            id,
            WatchFileState {
                path: path.clone(),
                object_value,
                timer_id,
                bigint,
                previous: stat_snapshot(&path),
                listeners,
            },
        );
    });
    WATCH_FILE_PATHS.with(|paths| {
        paths.borrow_mut().insert(path, id);
    });
    object_value
}

/// `fs.unwatchFile(path[, listener])`.
#[no_mangle]
pub extern "C" fn js_fs_unwatch_file(path_value: f64, listener: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    let Some(id) = WATCH_FILE_PATHS.with(|paths| paths.borrow().get(&path).copied()) else {
        return undefined_value();
    };
    if is_nullish(listener) {
        close_watch_file_state(id);
        return undefined_value();
    }
    validate_listener(listener);
    let should_close = WATCH_FILE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(&id) else {
            return false;
        };
        remove_listener(&mut state.listeners, "change", listener);
        !has_change_listeners(&state.listeners)
    });
    if should_close {
        close_watch_file_state(id);
    }
    undefined_value()
}

pub extern "C" fn js_fs_promises_watch(path_value: f64, options_value: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    let encoding = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let persistent = option_bool_default_local(options_value, b"persistent", true);
    let recursive = option_bool_default_local(options_value, b"recursive", false);
    let signal = match option_signal_value(options_value) {
        Ok(signal) => signal,
        Err(err) => crate::exception::js_throw(err),
    };
    // Snapshot the watch target at creation time. This serves two purposes:
    //   1. It validates the path synchronously, matching Node's `watch()` which
    //      throws (ENOENT etc.) at call time rather than at first iteration.
    //   2. It seeds an initial baseline for the state.
    // The baseline is intentionally re-taken in `start_promise_watcher` at the
    // first `.next()` pull (so pre-iteration writes are ignored, per Node) and
    // then advanced by every poll (so post-pull writes are delivered). The
    // value seeded here is therefore a placeholder that the first pull refreshes.
    let initial_snapshot = match snapshot_watch_target(&path, recursive) {
        Ok(snapshot) => snapshot,
        Err(err) => unsafe {
            crate::exception::js_throw(build_fs_error_value(&err, "watch", &path));
        },
    };
    let id = next_watch_id();
    let object_value = build_promise_watcher_object(id);
    let abort_listener = signal
        .filter(|signal| !signal_is_aborted(*signal))
        .map(|signal| add_abort_listener(signal, id, promise_watcher_abort_impl))
        .unwrap_or_else(undefined_value);
    let signal_value = signal.unwrap_or_else(undefined_value);
    let abort_reason = if signal.map(signal_is_aborted).unwrap_or(false) {
        Some(signal_abort_reason(signal_value))
    } else {
        None
    };
    PROMISE_WATCHERS.with(|watchers| {
        watchers.borrow_mut().insert(
            id,
            PromiseWatchState {
                path,
                recursive,
                encoding,
                object_value,
                timer_id: 0,
                persistent,
                active: false,
                snapshot: initial_snapshot,
                queue: VecDeque::new(),
                pending: VecDeque::new(),
                signal: signal_value,
                abort_listener,
                closed: abort_reason.is_some(),
                abort_reason,
            },
        );
    });
    object_value
}

pub(crate) fn scan_fs_watcher_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    FS_WATCHERS.with(|watchers| {
        for state in watchers.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            visitor.visit_nanbox_f64_slot(&mut state.signal);
            visitor.visit_nanbox_f64_slot(&mut state.abort_listener);
            for listeners in state.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_f64_slot(&mut listener.callback);
                }
            }
        }
    });
    WATCH_FILE_STATES.with(|states| {
        for state in states.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            for listeners in state.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_f64_slot(&mut listener.callback);
                }
            }
        }
    });
    PROMISE_WATCHERS.with(|watchers| {
        for state in watchers.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            visitor.visit_nanbox_f64_slot(&mut state.signal);
            visitor.visit_nanbox_f64_slot(&mut state.abort_listener);
            if let Some(reason) = &mut state.abort_reason {
                visitor.visit_nanbox_f64_slot(reason);
            }
            for promise in state.pending.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(promise);
            }
        }
    });
}

pub(crate) fn promise_value_fs(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

pub(crate) fn promise_undefined_fs() -> f64 {
    promise_value_fs(f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) fn promise_rejected_fs(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_reject(promise, reason);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}
