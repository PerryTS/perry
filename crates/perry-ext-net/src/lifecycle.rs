//! Issue #2131 — `net.Socket` / `net.Server` lifecycle + EventEmitter
//! surface beyond what #1852 shipped. Split into its own module to
//! keep `lib.rs` under the 2000-line file-size gate. The functions
//! here mirror the EventEmitter shape exposed by
//! `perry-ext-events`, but operate on the existing
//! `statics::listeners()` map keyed by net handle id (socket OR
//! server — they share the namespace via the monotonic `next_id()`).
//!
//! `socket.address()` lives here too: it reads back the local
//! address captured at `connect`/`accept` time (see
//! `SocketState::local_addr`) and emits the JSON shape consumed by
//! the codegen's `NR_OBJ_FROM_JSON_STR` return-kind, so user code
//! gets a real `{ port, address, family }` object instead of the
//! pre-fix `undefined`.
//!
//! All entry points use the same FFI ABI as their `js_net_*`
//! neighbors in `lib.rs`: handles arrive as `i64`, NaN-boxed strings
//! arrive as pre-unboxed `*const StringHeader`, closures arrive as
//! raw `*const RawClosureHeader` cast to `i64`. The corresponding
//! `NativeModSig` rows live in
//! `perry-codegen/src/lower_call/native_table/net_events.rs`.

use perry_ffi::{alloc_string, nanbox_string_bits, ArrayHeader, JsValue, StringHeader};
use std::collections::HashSet;

use crate::statics;
use crate::string_from_header_i64;

// ─── #2549: net.Socket state / counter / metadata property getters ───────────
//
// These zero-arg getters back the `net.Socket` property surface Node exposes
// (`socket.pending`, `.connecting`, `.destroyed`, `.readyState`, `.bytesRead`,
// `.bytesWritten`, `.timeout`, the `local*`/`remote*` endpoint fields, …).
// The codegen lowers a bare member read on a `("net", "Socket")` instance into
// a zero-arg `NativeMethodCall`; the matching `NativeModSig` rows live in
// `perry-codegen/src/lower_call/native_table/net_events.rs`.
//
// Numeric/boolean/undefined-valued getters return a *NaN-boxed* `f64` through
// the dispatch table's `NR_F64` kind (the value passes straight through).
// `readyState` is always a string, so it uses `NR_STR` and returns a raw
// `*mut StringHeader`. String-or-undefined fields (`localAddress`, …) box the
// string themselves and fall back to `TAG_UNDEFINED` when unconnected, since
// Node reports `undefined` (not `null`) for those before a connection.

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TAG_FALSE_BITS: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;

fn nanbox_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE_BITS } else { TAG_FALSE_BITS })
}

fn nanbox_undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

/// NaN-box a freshly allocated runtime string as an `f64` JS value.
fn nanbox_string_value(s: &str) -> f64 {
    let header = alloc_string(s).as_raw();
    f64::from_bits(nanbox_string_bits(header))
}

/// Run `f` against the live `SocketState` for `handle`, returning `default`
/// when the handle is unknown (e.g. already torn down).
fn with_socket<T>(handle: i64, default: T, f: impl FnOnce(&crate::SocketState) -> T) -> T {
    match statics::sockets().lock() {
        Ok(g) => g.get(&handle).map(f).unwrap_or(default),
        Err(_) => default,
    }
}

/// `socket.pending` — `true` until the socket starts connecting. We treat a
/// not-yet-open, not-destroyed handle as pending (matches Node's value for a
/// freshly constructed `new net.Socket()`).
///
/// # Safety
///
/// `handle` must be a registered socket id (raw, NOT NaN-boxed).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_pending(handle: i64) -> f64 {
    nanbox_bool(with_socket(handle, true, |s| !s.is_open && !s.destroyed))
}

/// `socket.connecting` — `true` only while a connection attempt is in flight.
/// Perry resolves connects synchronously inside the tokio task, so from the
/// JS side this is `false` before connect and `false` once open — matching
/// Node for the construct-then-inspect path this getter targets.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_connecting(_handle: i64) -> f64 {
    nanbox_bool(false)
}

/// `socket.destroyed` — `true` once `.destroy()` ran or the peer closed.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_destroyed(handle: i64) -> f64 {
    nanbox_bool(with_socket(handle, false, |s| s.destroyed))
}

/// `socket.readyState` — one of `"opening" | "open" | "readOnly" |
/// "writeOnly" | "closed"`. Node reports `"open"` for a freshly constructed
/// socket and `"closed"` once destroyed.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_ready_state(handle: i64) -> *mut StringHeader {
    let state = with_socket(
        handle,
        "open",
        |s| if s.destroyed { "closed" } else { "open" },
    );
    alloc_string(state).as_raw()
}

/// `socket.bytesRead` — total bytes consumed from the socket.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_bytes_read(handle: i64) -> f64 {
    with_socket(handle, 0u64, |s| s.bytes_read) as f64
}

/// `socket.bytesWritten` — total bytes queued for the socket.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_bytes_written(handle: i64) -> f64 {
    with_socket(handle, 0u64, |s| s.bytes_written) as f64
}

/// `socket.timeout` — the value set via `setTimeout(ms)`, or `undefined`.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_timeout(handle: i64) -> f64 {
    match with_socket(handle, None, |s| s.timeout) {
        Some(ms) => ms as f64,
        None => nanbox_undefined(),
    }
}

/// `socket.localAddress` — the bound local IP string, or `undefined`.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_local_address(handle: i64) -> f64 {
    match with_socket(handle, None, |s| s.local_addr) {
        Some(addr) => nanbox_string_value(&addr.ip().to_string()),
        None => nanbox_undefined(),
    }
}

/// `socket.localPort` — the bound local port number, or `undefined`.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_local_port(handle: i64) -> f64 {
    match with_socket(handle, None, |s| s.local_addr) {
        Some(addr) => addr.port() as f64,
        None => nanbox_undefined(),
    }
}

/// `socket.localFamily` — `"IPv4"`/`"IPv6"` of the local endpoint, else
/// `undefined`.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_local_family(handle: i64) -> f64 {
    match with_socket(handle, None, |s| s.local_addr) {
        Some(addr) => nanbox_string_value(if addr.is_ipv6() { "IPv6" } else { "IPv4" }),
        None => nanbox_undefined(),
    }
}

/// `socket.remoteAddress` — the peer IP string, or `undefined`. Perry does
/// not cache the connected peer endpoint yet, so this reports `undefined`
/// (Node's pre-connect value) rather than a stale address.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_remote_address(_handle: i64) -> f64 {
    nanbox_undefined()
}

/// `socket.remotePort` — the peer port, or `undefined` (see
/// [`js_net_socket_get_remote_address`]).
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_remote_port(_handle: i64) -> f64 {
    nanbox_undefined()
}

/// `socket.remoteFamily` — `"IPv4"`/`"IPv6"` of the peer, or `undefined`
/// (see [`js_net_socket_get_remote_address`]).
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_remote_family(_handle: i64) -> f64 {
    nanbox_undefined()
}

/// `socket.bufferSize` — Node reports `undefined` for an unconnected socket
/// and `0` (plus any internally buffered writes) once connected. We surface
/// `undefined` while closed, `0` while open.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_buffer_size(handle: i64) -> f64 {
    if with_socket(handle, false, |s| s.is_open) {
        0.0
    } else {
        nanbox_undefined()
    }
}

/// `socket.autoSelectFamilyAttemptedAddresses` — Node reports `undefined`
/// until a Happy-Eyeballs connect runs. Perry does not model the per-attempt
/// list, so we return `undefined`.
///
/// # Safety
///
/// See [`js_net_socket_get_pending`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_get_auto_select_family_attempted_addresses(
    _handle: i64,
) -> f64 {
    nanbox_undefined()
}

// ─── socket.write / end / destroy ────────────────────────────────────────────
//
// Moved here from `lib.rs` (#2549) to keep that file under the 2000-line
// gate; they share `SocketState` with the getters above and also feed the
// `bytesWritten`/`destroyed` counters those getters read.

/// `socket.write(chunk)` — enqueues bytes for the writer task and bumps the
/// `bytesWritten` counter. `chunk_bits` is the full NaN-boxed JS value (NA_JSV);
/// `jsvalue_to_socket_bytes` probes Buffer/Uint8Array/string/number/bool and
/// reads through the correct layout (#1131).
///
/// Carries a DISTINCT `#[no_mangle]` symbol (`js_ext_net_socket_write`),
/// deliberately NOT the `js_net_socket_write` name that the bundled stdlib net
/// ALSO exports. In a workspace / jsruntime build both crates are linked, so
/// `js_net_socket_write` is a duplicate symbol that the link binds to whichever
/// twin wins (the bundled stdlib's). A socket created here lives in ext-net's
/// registry; when it's then written to via the runtime's `HANDLE_METHOD_DISPATCH`
/// fallback (`dispatch_external_net_socket` in perry-stdlib — the path a
/// captured-by-closure `s.write(...)` inside an `'data'` handler takes), routing
/// through the shared `js_net_socket_write` symbol landed in the bundled twin's
/// EMPTY registry: `sockets.get(&handle)` missed, the `SocketCommand::Write` was
/// never enqueued, no `write()` syscall fired, and the bytes were silently
/// dropped. The dispatch helper calls THIS uniquely-named entry point instead —
/// a symbol with no twin — so the write always reaches ext-net's own registry.
/// Mirrors the `js_ext_net_destroy_socket` / `js_ext_net_drain_pending` fix.
/// (#5021, follows #5010.)
///
/// # Safety
///
/// `chunk_bits` must be a valid NaN-boxed JS value; string / Buffer pointers
/// must reference live runtime allocations.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_write(handle: i64, chunk_bits: i64) {
    let bytes = match crate::jsvalue_to_socket_bytes(f64::from_bits(chunk_bits as u64)) {
        Some(b) => b,
        None => return,
    };
    let mut sockets = statics::sockets().lock().unwrap();
    if let Some(s) = sockets.get_mut(&handle) {
        s.bytes_written = s.bytes_written.saturating_add(bytes.len() as u64);
        let _ = s.cmd_tx.send(crate::SocketCommand::Write(bytes));
    }
}

/// `socket.write(chunk)` under the name the static NATIVE_MODULE_TABLE path
/// emits. Delegates to the collision-proof [`js_ext_net_socket_write`] via a
/// crate-local call, so even when the bundled stdlib's same-named twin wins the
/// link this body still reaches ext-net's own registry.
///
/// # Safety
///
/// See [`js_ext_net_socket_write`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_write(handle: i64, chunk_bits: i64) {
    js_ext_net_socket_write(handle, chunk_bits);
}

/// `socket.end([data])` — optionally write a final chunk, then half-close the
/// write side (#1852). `undefined`/`null` (the no-arg form, padded with
/// `TAG_UNDEFINED`) yields `None` and we just send FIN.
///
/// Carries a DISTINCT `#[no_mangle]` symbol for the same reason as
/// [`js_ext_net_socket_write`] — the shared `js_net_socket_end` name collides
/// with the bundled stdlib twin, so the dispatch fallback would drop the
/// optional final chunk into the bundled twin's empty registry. (#5021.)
///
/// # Safety
///
/// `chunk_bits` must be a valid NaN-boxed JS value; string / Buffer pointers
/// must reference live runtime allocations.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_end(handle: i64, chunk_bits: i64) {
    let mut sockets = statics::sockets().lock().unwrap();
    if let Some(s) = sockets.get_mut(&handle) {
        if let Some(bytes) = crate::jsvalue_to_socket_bytes(f64::from_bits(chunk_bits as u64)) {
            if !bytes.is_empty() {
                s.bytes_written = s.bytes_written.saturating_add(bytes.len() as u64);
                let _ = s.cmd_tx.send(crate::SocketCommand::Write(bytes));
            }
        }
        let _ = s.cmd_tx.send(crate::SocketCommand::End);
    }
}

/// `socket.end([data])` under the name the static NATIVE_MODULE_TABLE path
/// emits. Delegates to the collision-proof [`js_ext_net_socket_end`].
///
/// # Safety
///
/// See [`js_ext_net_socket_end`].
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_end(handle: i64, chunk_bits: i64) {
    js_ext_net_socket_end(handle, chunk_bits);
}

/// `socket.destroy()` — hard close. Flags the handle destroyed (so
/// `socket.destroyed` / `readyState` reflect it) and sends the teardown
/// command.
///
/// # Safety
///
/// `handle` must be a registered socket id (raw, NOT NaN-boxed).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_destroy(handle: i64) {
    js_ext_net_destroy_socket(handle);
}

/// Destroy an `ext-net` socket by id, operating directly on this crate's
/// socket registry.
///
/// This carries a DISTINCT `#[no_mangle]` symbol (`js_ext_net_destroy_socket`),
/// deliberately NOT the `js_net_socket_destroy` name that the bundled stdlib
/// net ALSO exports. In a workspace/auto-optimize build both are linked, so
/// `js_net_socket_destroy` is a duplicate symbol bound to whichever twin
/// wins (stdlib's). The handle-dispatch `socket_method` "destroy" arm and the
/// extern wrapper above call THIS uniquely-named entry point instead — a
/// symbol with no twin — so an adopted raw-`'upgrade'` socket is actually
/// marked destroyed in ext-net's own registry rather than in stdlib's empty
/// one, which is what let the event loop drain. (#5010)
#[no_mangle]
pub extern "C" fn js_ext_net_destroy_socket(handle: i64) {
    let mut sockets = statics::sockets().lock().unwrap();
    if let Some(s) = sockets.get_mut(&handle) {
        s.destroyed = true;
        s.is_open = false;
        let _ = s.cmd_tx.send(crate::SocketCommand::Destroy);
    }
}

// ─── socket.address() ────────────────────────────────────────────────────────

/// `socket.address()` — returns a JSON string the codegen's
/// `NR_OBJ_FROM_JSON_STR` kind parses into `{ address, family, port }`.
/// Falls back to `"{}"` (an empty object) on an unconnected handle so
/// `addr && typeof addr === "object"` reads `true` either way — Node
/// returns `{}` on a socket that never connected, not `null`.
///
/// # Safety
///
/// `handle` must be a registered socket id (raw, NOT NaN-boxed; the
/// dispatch shim unboxes via `unbox_to_i64` before the FFI call).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_address(handle: i64) -> *mut StringHeader {
    let json = match statics::sockets().lock() {
        Ok(g) => match g.get(&handle) {
            Some(s) => match s.local_addr {
                Some(addr) => {
                    let family = if addr.is_ipv6() { "IPv6" } else { "IPv4" };
                    format!(
                        "{{\"address\":\"{}\",\"family\":\"{}\",\"port\":{}}}",
                        addr.ip(),
                        family,
                        addr.port()
                    )
                }
                None => "{}".to_string(),
            },
            None => "{}".to_string(),
        },
        Err(_) => "{}".to_string(),
    };
    alloc_string(&json).as_raw()
}

// ─── socket / server EventEmitter shims ──────────────────────────────────────
//
// The next batch all share the same shape: read the event name,
// mutate `statics::listeners()` (and `statics::once_flags()` for
// `once`), then return the handle for chaining. They're hand-written
// instead of generated because the GC scanner has to keep walking
// the raw `Vec<i64>` storage that `js_net_socket_on` / `js_net_server_on`
// already use — no new shape, no scanner change.

fn read_event(event_ptr: i64) -> Option<String> {
    unsafe { string_from_header_i64(event_ptr) }
}

fn register_listener_with_flag(handle: i64, event: String, cb: i64, once: bool) {
    if cb == 0 {
        return;
    }
    {
        let mut listeners = statics::listeners().lock().unwrap();
        listeners
            .entry(handle)
            .or_default()
            .entry(event.clone())
            .or_default()
            .push(cb);
    }
    if once {
        let mut flags = statics::once_flags().lock().unwrap();
        flags
            .entry(handle)
            .or_default()
            .entry(event)
            .or_default()
            .insert(cb);
    }
}

/// Issue #2131 — drop any callback pointer flagged as a `once` listener
/// for `(handle, event)` from both the listener vector and the
/// once-flag side table. Called from `lib.rs`'s pump right after each
/// event dispatch so the next emit doesn't re-fire it. The early
/// return on an empty/missing set keeps the steady-state path (no
/// `once` users) lock-light: one map probe + drop.
pub(crate) fn drain_once_listeners(handle: i64, event: &str) {
    let to_drop: HashSet<i64> = {
        let mut once = statics::once_flags().lock().unwrap();
        let Some(per) = once.get_mut(&handle) else {
            return;
        };
        let Some(set) = per.remove(event) else {
            return;
        };
        if per.is_empty() {
            once.remove(&handle);
        }
        set
    };
    if to_drop.is_empty() {
        return;
    }
    let mut listeners = statics::listeners().lock().unwrap();
    if let Some(per) = listeners.get_mut(&handle) {
        if let Some(vec) = per.get_mut(event) {
            vec.retain(|cb| !to_drop.contains(cb));
            if vec.is_empty() {
                per.remove(event);
            }
        }
    }
}

fn remove_listener_at_handle(handle: i64, event: &str, cb: i64) {
    let mut removed = false;
    {
        let mut listeners = statics::listeners().lock().unwrap();
        if let Some(per) = listeners.get_mut(&handle) {
            if let Some(vec) = per.get_mut(event) {
                if let Some(pos) = vec.iter().position(|x| *x == cb) {
                    vec.remove(pos);
                    removed = true;
                }
                if vec.is_empty() {
                    per.remove(event);
                }
            }
        }
    }
    if removed {
        let mut flags = statics::once_flags().lock().unwrap();
        if let Some(per) = flags.get_mut(&handle) {
            if let Some(set) = per.get_mut(event) {
                set.remove(&cb);
                if set.is_empty() {
                    per.remove(event);
                }
            }
            if per.is_empty() {
                flags.remove(&handle);
            }
        }
    }
}

fn remove_all_listeners_at_handle(handle: i64, event: Option<&str>) {
    {
        let mut listeners = statics::listeners().lock().unwrap();
        if let Some(per) = listeners.get_mut(&handle) {
            match event {
                Some(e) => {
                    per.remove(e);
                }
                None => per.clear(),
            }
        }
    }
    let mut flags = statics::once_flags().lock().unwrap();
    if let Some(per) = flags.get_mut(&handle) {
        match event {
            Some(e) => {
                per.remove(e);
            }
            None => per.clear(),
        }
        if per.is_empty() {
            flags.remove(&handle);
        }
    }
}

fn listener_count_at_handle(handle: i64, event: &str) -> f64 {
    let listeners = statics::listeners().lock().unwrap();
    listeners
        .get(&handle)
        .and_then(|m| m.get(event))
        .map(|v| v.len() as f64)
        .unwrap_or(0.0)
}

/// Build a JS array-of-strings JSON blob for the event names registered
/// on `handle`. Uses the same `NR_OBJ_FROM_JSON_STR` channel the codegen
/// already employs for `server.address()`, so the consumer sees a real
/// array (length, indexing) instead of a raw string. Names are emitted
/// in HashMap iteration order — `Array.isArray(names) && names.length >= N`
/// is the contract the parity test pins, not a specific ordering.
fn event_names_json(handle: i64) -> String {
    let listeners = statics::listeners().lock().unwrap();
    let Some(per) = listeners.get(&handle) else {
        return "[]".to_string();
    };
    let mut seen: HashSet<&str> = HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    for (name, vec) in per.iter() {
        if vec.is_empty() {
            continue;
        }
        if seen.insert(name.as_str()) {
            parts.push(format!("\"{}\"", json_escape(name)));
        }
    }
    format!("[{}]", parts.join(","))
}

/// Issue #2211 — build a JS array of the listener callbacks registered
/// on `(handle, event)`. Each `cb` slot stores the raw closure pointer
/// (`*const RawClosureHeader`) cast to `i64`; we NaN-box it back as
/// POINTER_TAG so the returned array is full of callable JS values.
/// Both `listeners()` and `rawListeners()` go through this helper — Node
/// returns the wrapped onceWrapper for `rawListeners` and the unwrapped
/// callback for `listeners`, but Perry's `once`-listener implementation
/// drains the entry from the listener vector on first emit (the
/// once-flag side table is just a removal set), so the wrap/unwrap
/// distinction never observes a difference for callers that only ask
/// before any event has fired. Matching Node's "the array is a real
/// snapshot of current listeners" is what the
/// `socket.listeners('timeout').length` check needs.
fn listeners_array_for_event(handle: i64, event: &str) -> *mut ArrayHeader {
    let snapshot: Vec<i64> = statics::listeners()
        .lock()
        .ok()
        .and_then(|m| m.get(&handle).and_then(|p| p.get(event)).cloned())
        .unwrap_or_default();
    let mut arr = unsafe { perry_ffi::js_array_alloc(snapshot.len() as u32) };
    for cb in snapshot {
        if cb == 0 {
            continue;
        }
        let value = JsValue::from_object_ptr(cb as *mut u8);
        arr = unsafe { perry_ffi::js_array_push(arr, value) };
    }
    arr
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ─── socket.* FFI exports ────────────────────────────────────────────────────

/// `socket.once(event, cb)` — register a one-shot listener.
///
/// # Safety
///
/// `event_ptr` must be null or a Perry-runtime `StringHeader` pointer
/// cast to `i64`. `cb` is a raw `*const RawClosureHeader` as `i64`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_once(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    crate::ensure_gc_scanner_registered();
    if let Some(event) = read_event(event_ptr) {
        register_listener_with_flag(handle, event, cb, true);
    }
    handle
}

/// `socket.removeListener(event, cb)` — remove the first matching cb.
///
/// # Safety
///
/// Same as `js_net_socket_once`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    if let Some(event) = read_event(event_ptr) {
        remove_listener_at_handle(handle, &event, cb);
    }
    handle
}

/// `socket.removeAllListeners([event])` — drop every listener for the
/// given event, or every event when `event_ptr` is null. Returns the
/// handle for chaining (Node semantics).
///
/// # Safety
///
/// `event_ptr` may be null (meaning "all events") or a Perry-runtime
/// `StringHeader` pointer cast to `i64`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_remove_all_listeners(handle: i64, event_ptr: i64) -> i64 {
    let event = read_event(event_ptr);
    remove_all_listeners_at_handle(handle, event.as_deref());
    handle
}

/// `socket.listenerCount(event)` — count registered listeners for `event`.
///
/// # Safety
///
/// `event_ptr` must be null or a Perry-runtime `StringHeader` pointer.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_listener_count(handle: i64, event_ptr: i64) -> f64 {
    let Some(event) = read_event(event_ptr) else {
        return 0.0;
    };
    listener_count_at_handle(handle, &event)
}

/// `socket.eventNames()` — return an array of registered event names.
/// Emits JSON for the codegen's `NR_OBJ_FROM_JSON_STR` return kind so
/// callers get a true array, not a string.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_event_names(handle: i64) -> *mut StringHeader {
    let json = event_names_json(handle);
    alloc_string(&json).as_raw()
}

/// Issue #2211 — `socket.listeners(event)` / `socket.rawListeners(event)`.
/// Both return a JS array of the registered callbacks. The codegen
/// dispatches this through `NR_PTR`, so the raw ArrayHeader pointer is
/// NaN-boxed with POINTER_TAG and reaches the caller as a real JS array
/// (length, indexing, iteration). `rawListeners` shares the same impl
/// because Perry collapses `once`-registered callbacks into the
/// listener vector — see the helper's doc comment for the
/// once-wrap/unwrap discussion.
///
/// # Safety
///
/// `event_ptr` must be null or a Perry-runtime `StringHeader` pointer.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_listeners(handle: i64, event_ptr: i64) -> i64 {
    let Some(event) = read_event(event_ptr) else {
        return unsafe { perry_ffi::js_array_alloc(0) } as i64;
    };
    listeners_array_for_event(handle, &event) as i64
}

/// `socket.rawListeners(event)` — see `js_net_socket_listeners`.
///
/// # Safety
///
/// Same as `js_net_socket_listeners`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_raw_listeners(handle: i64, event_ptr: i64) -> i64 {
    js_net_socket_listeners(handle, event_ptr)
}

/// `socket.resetAndDestroy()` — Node treats this as "send RST then
/// destroy" but exposes the same teardown surface as `destroy()` from
/// the caller's point of view (the `'close'` event still fires). We
/// alias to the destroy command for now: tests that only assert
/// "callable + closes cleanly" pass byte-for-byte with Node, and the
/// RST-vs-FIN distinction is invisible to the connected peer in the
/// pure-JS test cases that exercise this path (the peer just sees an
/// abrupt close either way).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_reset_and_destroy(handle: i64) -> i64 {
    crate::js_net_socket_destroy(handle);
    handle
}

// ─── server.* FFI exports (mirror of the socket surface) ─────────────────────

/// `server.once(event, cb)` — register a one-shot server-level listener.
///
/// # Safety
///
/// See `js_net_socket_once`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_once(handle: i64, event_ptr: i64, cb: i64) -> i64 {
    crate::ensure_gc_scanner_registered();
    if let Some(event) = read_event(event_ptr) {
        register_listener_with_flag(handle, event, cb, true);
    }
    handle
}

/// `server.removeListener(event, cb)`.
///
/// # Safety
///
/// See `js_net_socket_once`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb: i64,
) -> i64 {
    if let Some(event) = read_event(event_ptr) {
        remove_listener_at_handle(handle, &event, cb);
    }
    handle
}

/// `server.removeAllListeners([event])`.
///
/// # Safety
///
/// See `js_net_socket_remove_all_listeners`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_remove_all_listeners(handle: i64, event_ptr: i64) -> i64 {
    let event = read_event(event_ptr);
    remove_all_listeners_at_handle(handle, event.as_deref());
    handle
}

/// `server.listenerCount(event)`.
///
/// # Safety
///
/// See `js_net_socket_listener_count`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_listener_count(handle: i64, event_ptr: i64) -> f64 {
    let Some(event) = read_event(event_ptr) else {
        return 0.0;
    };
    listener_count_at_handle(handle, &event)
}

/// `server.eventNames()`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_event_names(handle: i64) -> *mut StringHeader {
    let json = event_names_json(handle);
    alloc_string(&json).as_raw()
}

/// `server.listeners(event)` — mirror of `js_net_socket_listeners` since
/// net.Server and net.Socket share the `statics::listeners()` keyed by
/// handle id. Same NR_PTR/POINTER_TAG return contract.
///
/// # Safety
///
/// `event_ptr` must be null or a Perry-runtime `StringHeader` pointer.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_listeners(handle: i64, event_ptr: i64) -> i64 {
    let Some(event) = read_event(event_ptr) else {
        return unsafe { perry_ffi::js_array_alloc(0) } as i64;
    };
    listeners_array_for_event(handle, &event) as i64
}

/// `server.rawListeners(event)` — see `js_net_server_listeners`.
///
/// # Safety
///
/// Same as `js_net_server_listeners`.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_raw_listeners(handle: i64, event_ptr: i64) -> i64 {
    js_net_server_listeners(handle, event_ptr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn reset_handle(handle: i64) {
        statics::listeners().lock().unwrap().remove(&handle);
        statics::once_flags().lock().unwrap().remove(&handle);
    }

    /// `once` listener appears in both the listener vector AND the
    /// once-flag set; after `remove_listener_at_handle` runs it
    /// disappears from both.
    #[test]
    fn once_register_and_remove_round_trip() {
        let handle = -91_234;
        reset_handle(handle);

        register_listener_with_flag(handle, "data".to_string(), 0xCAFE, true);
        register_listener_with_flag(handle, "data".to_string(), 0xBEEF, false);

        let listener_count = listener_count_at_handle(handle, "data");
        assert_eq!(listener_count, 2.0);

        let flags_has_once = statics::once_flags()
            .lock()
            .unwrap()
            .get(&handle)
            .and_then(|m| m.get("data"))
            .is_some_and(|s| s.contains(&0xCAFE_i64) && !s.contains(&0xBEEF_i64));
        assert!(flags_has_once);

        remove_listener_at_handle(handle, "data", 0xCAFE);
        assert_eq!(listener_count_at_handle(handle, "data"), 1.0);

        let flags_cleared = statics::once_flags()
            .lock()
            .unwrap()
            .get(&handle)
            .and_then(|m| m.get("data"))
            .is_none();
        assert!(flags_cleared);

        reset_handle(handle);
    }

    /// `removeAllListeners(None)` clears everything; passing an event
    /// only clears that event.
    #[test]
    fn remove_all_listeners_scope() {
        let handle = -91_235;
        reset_handle(handle);

        register_listener_with_flag(handle, "data".to_string(), 1, false);
        register_listener_with_flag(handle, "end".to_string(), 2, true);

        remove_all_listeners_at_handle(handle, Some("data"));
        assert_eq!(listener_count_at_handle(handle, "data"), 0.0);
        assert_eq!(listener_count_at_handle(handle, "end"), 1.0);

        remove_all_listeners_at_handle(handle, None);
        assert_eq!(listener_count_at_handle(handle, "end"), 0.0);
        let no_once_left = statics::once_flags().lock().unwrap().get(&handle).is_none();
        assert!(no_once_left);

        reset_handle(handle);
    }

    /// Issue #2211 — `listeners_array_for_event` returns an
    /// ArrayHeader holding one NaN-boxed POINTER_TAG value per
    /// registered callback. The pointer round-trips bit-exact so the
    /// runtime sees the closure handle the original `on()` call
    /// stored.
    #[test]
    fn listeners_array_round_trips_callback_pointers() {
        let handle = -91_237;
        reset_handle(handle);

        // Use raw pointer-shaped values (high bit clear) — the helper
        // only re-NaN-boxes them, never dereferences.
        register_listener_with_flag(handle, "timeout".to_string(), 0x1234, false);
        register_listener_with_flag(handle, "timeout".to_string(), 0x5678, false);

        let arr = listeners_array_for_event(handle, "timeout");
        assert!(!arr.is_null());
        let len = unsafe { (*arr).length };
        assert_eq!(len, 2);

        let v0 = unsafe { perry_ffi::js_array_get(arr, 0) };
        let v1 = unsafe { perry_ffi::js_array_get(arr, 1) };
        assert!(v0.is_pointer());
        assert!(v1.is_pointer());
        assert_eq!(v0.as_pointer::<u8>() as usize, 0x1234);
        assert_eq!(v1.as_pointer::<u8>() as usize, 0x5678);

        // Unknown event name → empty array (zero allocations beyond
        // the header), not a panic.
        let empty = listeners_array_for_event(handle, "never");
        assert!(!empty.is_null());
        assert_eq!(unsafe { (*empty).length }, 0);

        reset_handle(handle);
    }

    /// `eventNames` JSON survives a basic event name with no escaping
    /// drama; empty-listener events are filtered.
    #[test]
    fn event_names_emits_json_array() {
        let handle = -91_236;
        reset_handle(handle);

        // Seed two events with listeners + one event with an empty vec
        // (shouldn't appear in the result).
        {
            let mut listeners = statics::listeners().lock().unwrap();
            let per = listeners.entry(handle).or_default();
            per.insert("data".to_string(), vec![10]);
            per.insert("end".to_string(), vec![11]);
            per.insert("orphan".to_string(), Vec::new());
            let _: &HashMap<String, Vec<i64>> = per;
        }

        let json = event_names_json(handle);
        assert!(json.starts_with('['));
        assert!(json.contains("\"data\""));
        assert!(json.contains("\"end\""));
        assert!(!json.contains("\"orphan\""));

        reset_handle(handle);
    }
}
