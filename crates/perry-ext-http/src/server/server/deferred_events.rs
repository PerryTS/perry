//! Deferred server lifecycle events shared by HTTP, HTTPS, and HTTP/2.

use super::*;

struct DeferredCallbacksCall {
    callbacks: *const perry_ffi::TransientRootedAddr,
    len: usize,
}

unsafe extern "C" fn call_deferred_callbacks(data: *mut std::ffi::c_void) -> f64 {
    let call = &*(data as *const DeferredCallbacksCall);
    let callbacks = std::slice::from_raw_parts(call.callbacks, call.len);
    let mut fired = 0;
    for callback in callbacks {
        let callback = callback.get();
        if callback == 0 {
            continue;
        }
        let closure = JsClosure::from_raw(callback as *const RawClosureHeader);
        if !closure.is_null() {
            let _ = closure.call0();
            fired += 1;
        }
    }
    fired as f64
}

/// #4903 — record a pending `'listening'` emit on a server (http / https /
/// http2 all share the `HttpServer` base). Node registers the
/// `listen(port, cb)` callback as a *once* `'listening'` listener inside
/// `listen()`, so the callback goes into the live listener list (correct
/// emit order vs. listeners added before/after `listen()`) and into
/// `deferred_listen_cbs`, which the pump uses to remove it again after
/// the emit fires.
pub(crate) fn queue_deferred_listening_emit(s: &mut HttpServer, callback: i64) {
    s.pending_listening_emit = true;
    if callback != 0 {
        s.listeners
            .entry("listening".to_string())
            .or_default()
            .push(callback);
        s.deferred_listen_cbs.push(callback);
    }
}

/// Register a `listen(port, cb)` callback ahead of the bind, for a `listen()`
/// whose bind was posted to the thread that owns the loop. The callback is
/// rooted from here on (it is in `deferred_listen_cbs`); the `'listening'`
/// emit itself is armed by `queue_deferred_listening_emit` once the bind has
/// succeeded on the owner.
pub(crate) fn register_listen_callback(s: &mut HttpServer, callback: i64) {
    if callback != 0 {
        s.listeners
            .entry("listening".to_string())
            .or_default()
            .push(callback);
        s.deferred_listen_cbs.push(callback);
    }
}

/// Undo `register_listen_callback` for a posted `listen()` whose bind failed:
/// Node never runs the callback of a failed listen, and a callback left in
/// `deferred_listen_cbs` would keep the event loop alive forever.
pub(crate) fn withdraw_listen_callbacks(s: &mut HttpServer) {
    let once = std::mem::take(&mut s.deferred_listen_cbs);
    if let Some(ls) = s.listeners.get_mut("listening") {
        for cb in &once {
            if let Some(pos) = ls.iter().position(|x| x == cb) {
                ls.remove(pos);
            }
        }
    }
}

pub(crate) fn queue_deferred_close_emit(s: &mut HttpServer, callback: i64) {
    s.pending_close_emit = true;
    if callback != 0 {
        s.listeners
            .entry("close".to_string())
            .or_default()
            .push(callback);
        s.deferred_close_cbs.push(callback);
    }
}

/// #4903 — fire a server's queued `'listening'` listeners + `listen(cb)`
/// callbacks with implicit `this` bound to the server. Runs from the
/// main-thread pump, never from inside `listen()` itself: Node emits
/// `'listening'` on a later event-loop tick, so the listen callback only
/// runs after the current synchronous script segment (including the
/// `const server = ...` assignment) has finished, and `'listening'`
/// listeners registered after `listen()` returned still fire. The
/// listener snapshot is taken here at drain time for that same reason,
/// and the queue is detached (`mem::take`) before any callback runs so
/// a re-entrant `listen()` from a callback can't double-fire.
/// Drain a server's pending `listen()` outcome.
///
/// A `listen()` either succeeded — a `'listening'` emit is pending — or failed,
/// in which case `queue_deferred_error_emit` cleared the `'listening'` emit and
/// left an `'error'` one. Never both, so the two are drained together and the
/// `'error'` goes first: Node emits it *instead of* `'listening'`, not after.
pub(crate) fn drain_deferred_listen_for<T, F>(server_handle: i64, base_of: F) -> i32
where
    T: Send + Sync + 'static,
    F: Fn(&mut T) -> &mut HttpServer + Copy,
{
    let errors = drain_deferred_error_for::<T, F>(server_handle, base_of);
    if errors > 0 {
        return errors;
    }
    let (cbs, async_id): (Vec<i64>, u64) = match get_handle_mut::<T>(server_handle) {
        Some(t) => {
            let s = base_of(t);
            if !std::mem::take(&mut s.pending_listening_emit) {
                return 0;
            }
            let snapshot = take_server_event_listeners(s, "listening");
            // The `listen(port, cb)` callbacks are once-listeners: now that
            // this emit has snapshotted them, drop them from the live list
            // so a future emit / listener introspection doesn't see them.
            let once: Vec<i64> = std::mem::take(&mut s.deferred_listen_cbs);
            if let Some(ls) = s.listeners.get_mut("listening") {
                for cb in &once {
                    if let Some(pos) = ls.iter().position(|x| x == cb) {
                        ls.remove(pos);
                    }
                }
            }
            (snapshot, s.async_id)
        }
        None => return 0,
    };
    let this_val = handle_to_pointer_f64(server_handle);
    // #8082: the drained snapshot crosses each callback — root it.
    let scope = perry_ffi::TransientRootScope::enter();
    let rooted = scope.root_addrs(&cbs);
    perry_ext_ws::attached_server_listening(server_handle);
    let mut call = DeferredCallbacksCall {
        callbacks: rooted.as_ptr(),
        len: rooted.len(),
    };
    unsafe {
        crate::js_async_hooks_provider_run_catching_with_this(
            async_id,
            this_val,
            0,
            call_deferred_callbacks,
            &mut call as *mut DeferredCallbacksCall as *mut std::ffi::c_void,
        ) as i32
    }
}

pub(crate) fn drain_deferred_close_for<T, F>(server_handle: i64, base_of: F) -> i32
where
    T: Send + Sync + 'static,
    F: FnOnce(&mut T) -> &mut HttpServer,
{
    let (callbacks, async_id) = match get_handle_mut::<T>(server_handle) {
        Some(server) => {
            let base = base_of(server);
            if !std::mem::take(&mut base.pending_close_emit) {
                return 0;
            }
            let callbacks = take_server_event_listeners(base, "close");
            let once = std::mem::take(&mut base.deferred_close_cbs);
            if let Some(listeners) = base.listeners.get_mut("close") {
                for callback in once {
                    if let Some(index) = listeners.iter().position(|entry| *entry == callback) {
                        listeners.remove(index);
                    }
                }
            }
            let async_id = std::mem::take(&mut base.async_id);
            (callbacks, async_id)
        }
        None => return 0,
    };
    let this_value = handle_to_pointer_f64(server_handle);
    let scope = perry_ffi::TransientRootScope::enter();
    let callbacks = scope.root_addrs(&callbacks);
    let mut call = DeferredCallbacksCall {
        callbacks: callbacks.as_ptr(),
        len: callbacks.len(),
    };
    unsafe {
        crate::js_async_hooks_provider_run_catching_with_this(
            async_id,
            this_value,
            1,
            call_deferred_callbacks,
            &mut call as *mut DeferredCallbacksCall as *mut std::ffi::c_void,
        ) as i32
    }
}
pub(super) fn server_is_active(s: &HttpServer) -> bool {
    // #5011 — an `unref()`ed server no longer keeps the event loop alive
    // just by being bound, so a quietly-listening unref'd server lets the
    // process exit (Node semantics). Pending listen callbacks and queued
    // requests below still keep the loop alive long enough to flush any
    // in-flight work.
    if s.listening && s.refed {
        return true;
    }
    // #4903 — a queued `'listening'` emit / listen callback must keep the
    // loop alive until the pump fires it, even if `close()` already ran.
    if s.pending_listening_emit
        || !s.deferred_listen_cbs.is_empty()
        || s.pending_close_emit
        || !s.deferred_close_cbs.is_empty()
    {
        return true;
    }
    false
}

// ── A failed `listen()` ─────────────────────────────────────────────────────

/// A `listen()` that failed before the server ever listened, carried to the
/// main thread so the `'error'` event fires where Node fires it.
///
/// Node emits `'error'` on the server **asynchronously** — measured on 26.5.1,
/// the order is `after-listen-call, error`, `'listening'` never fires, the
/// `listen(cb)` callback never runs and `server.listening` stays false. So this
/// is queued exactly like `'listening'` and `'close'` are, and drained by the
/// same pump.
#[derive(Clone, Debug)]
pub struct ListenError {
    /// Node's `err.code`, e.g. `"EADDRINUSE"`.
    pub code: String,
    /// Node's `err.errno` — the negated OS code (`-98` for EADDRINUSE on Linux).
    pub errno: i32,
    /// Node's `err.syscall`, always `"listen"` here.
    pub syscall: String,
    /// Node's `err.address` — the host that was being bound.
    pub address: String,
    /// Node's `err.port`.
    pub port: u16,
}

/// libuv's description for the codes a `listen()` can fail with, which is the
/// middle of Node's message: `listen EADDRINUSE: address already in use
/// 127.0.0.1:47421`. Measured on Node 26.5.1 rather than transcribed.
fn listen_error_description(code: &str) -> &'static str {
    match code {
        "EADDRINUSE" => "address already in use",
        "EACCES" => "permission denied",
        "EADDRNOTAVAIL" => "address not available",
        "EINVAL" => "invalid argument",
        "ENOTFOUND" => "getaddrinfo ENOTFOUND",
        "EAFNOSUPPORT" => "address family not supported",
        "EMFILE" => "too many open files",
        "ENOTSUP" => "operation not supported on socket",
        _ => "listen failed",
    }
}

/// The fallback when the error object could not be allocated: hand the
/// listener the message string alone, so something still arrives.
fn listen_error_message_value(err: &ListenError) -> f64 {
    let s = alloc_string(&listen_error_message(err));
    f64::from_bits(JsValue::from_string_ptr(s.as_raw()).bits())
}

/// Node's `err.message` for a failed listen.
fn listen_error_message(err: &ListenError) -> String {
    if err.address.is_empty() {
        format!(
            "{} {}: {}",
            err.syscall,
            err.code,
            listen_error_description(&err.code)
        )
    } else {
        format!(
            "{} {}: {} {}:{}",
            err.syscall,
            err.code,
            listen_error_description(&err.code),
            err.address,
            err.port
        )
    }
}

/// Build the value the `'error'` listener receives.
///
/// This follows `perry-ext-net`'s `build_error_object` rather than inventing a
/// second shape: a plain object carrying `message` / `code` / `name` / `errno`
/// / `syscall`, plus the two fields Node adds for a listen failure (`address`,
/// `port`). One divergence, pre-existing and Perry-wide for `'error'` payloads:
/// `err instanceof Error` is false, because this is an object rather than a
/// real `Error`. Every field a program reads is present.
///
/// GC: each freshly allocated string is rooted through the same scope as the
/// receiver **before the next store**, because a field write can collect and a
/// raw local would be read back stale (#8082).
unsafe fn build_listen_error_value(err: &ListenError) -> f64 {
    let keys: [&str; 7] = [
        "message", "code", "name", "errno", "syscall", "address", "port",
    ];
    let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
    let obj: *mut perry_ffi::ObjectHeader = perry_ffi::js_object_alloc_with_shape(
        shape_id,
        keys.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj.is_null() {
        return listen_error_message_value(err);
    }
    // Root `obj` BEFORE anything in this function can allocate. The failure
    // path above is a separate function for the same reason: its `alloc_string`
    // is unreachable from here, but it sat lexically between the alloc and the
    // root, and `unrooted_local_shape.py` is flow-insensitive by design --
    // an unrooted window it cannot rule out is one a reader cannot either.
    let roots = perry_ffi::TransientRootScope::enter();
    let object = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(obj as *mut u8).bits(),
    ));
    let message = listen_error_message(err);
    let set_string = |index: u32, value: &str| {
        let rooted = roots.root_nanbox(f64::from_bits(
            JsValue::from_string_ptr(alloc_string(value).as_raw()).bits(),
        ));
        perry_ffi::js_object_set_field(
            (object.get().to_bits() & PTR_MASK) as *mut perry_ffi::ObjectHeader,
            index,
            JsValue::from_bits(rooted.get().to_bits()),
        );
    };
    set_string(0, &message);
    set_string(1, &err.code);
    set_string(2, "Error");
    perry_ffi::js_object_set_field(
        (object.get().to_bits() & PTR_MASK) as *mut perry_ffi::ObjectHeader,
        3,
        JsValue::from_number(err.errno as f64),
    );
    set_string(4, &err.syscall);
    set_string(5, &err.address);
    perry_ffi::js_object_set_field(
        (object.get().to_bits() & PTR_MASK) as *mut perry_ffi::ObjectHeader,
        6,
        JsValue::from_number(err.port as f64),
    );
    object.get()
}

struct DeferredErrorCall {
    callbacks: *const perry_ffi::TransientRootedAddr,
    len: usize,
    error: *const ListenError,
}

unsafe extern "C" fn call_deferred_error_callbacks(data: *mut std::ffi::c_void) -> f64 {
    let call = &*(data as *const DeferredErrorCall);
    let callbacks = std::slice::from_raw_parts(call.callbacks, call.len);
    // The error value is built and rooted HERE, not by the caller: each
    // `call1` can run arbitrary JS and collect, so the value is re-read from
    // its root slot for every listener rather than carried in a bare local.
    let roots = perry_ffi::TransientRootScope::enter();
    let error = roots.root_nanbox(build_listen_error_value(&*call.error));
    let mut fired = 0;
    for callback in callbacks {
        let callback = callback.get();
        if callback == 0 {
            continue;
        }
        let closure = JsClosure::from_raw(callback as *const RawClosureHeader);
        if !closure.is_null() {
            let _ = closure.call1(error.get());
            fired += 1;
        }
    }
    fired as f64
}

/// Record a failed `listen()` for the pump to emit as `'error'`.
pub(crate) fn queue_deferred_error_emit(s: &mut HttpServer, err: ListenError) {
    // Node never emits `'listening'` for a listen that failed, and the
    // `listen(cb)` callback never runs, so a queued one is dropped here.
    s.pending_listening_emit = false;
    s.listening = false;
    s.pending_error_emit = Some(err);
}

/// Queue a failed `listen()` from a `turnloop_net::NetError`.
pub(crate) fn queue_listen_error_parts(
    server_handle: i64,
    address: &str,
    port: u16,
    code: &str,
    errno: i32,
    syscall: &str,
) {
    let errno = if errno != 0 {
        errno
    } else {
        // `errno_for_code` returns the negated OS code Node reports.
        perry_ffi::turnloop_net::errno_for_code(code)
    };
    let err = ListenError {
        code: code.to_string(),
        errno,
        syscall: if syscall.is_empty() {
            "listen".to_string()
        } else {
            syscall.to_string()
        },
        address: address.to_string(),
        port,
    };
    if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        queue_deferred_error_emit(s, err);
        return;
    }
    if let Some(s) = get_handle_mut::<crate::server::https_server::HttpsServer>(server_handle) {
        queue_deferred_error_emit(&mut s.base, err);
        return;
    }
    if let Some(s) = get_handle_mut::<crate::server::http2_server::Http2SecureServer>(server_handle)
    {
        queue_deferred_error_emit(&mut s.base, err);
    }
}

/// Fire a server's queued `'error'` listeners, with implicit `this` bound to
/// the server — on the pump's tick, never inside `listen()`, because Node emits
/// it asynchronously (measured: `after-listen-call, error`).
///
/// An `'error'` with no listener is left to the existing uncaught path rather
/// than being invented here: `fired == 0` simply reports zero.
fn drain_deferred_error_for<T, F>(server_handle: i64, base_of: F) -> i32
where
    T: Send + Sync + 'static,
    F: FnOnce(&mut T) -> &mut HttpServer,
{
    let (cbs, async_id, error): (Vec<i64>, u64, ListenError) =
        match get_handle_mut::<T>(server_handle) {
            Some(t) => {
                let s = base_of(t);
                let Some(error) = s.pending_error_emit.take() else {
                    return 0;
                };
                let snapshot = take_server_event_listeners(s, "error");
                (snapshot, s.async_id, error)
            }
            None => return 0,
        };
    let this_val = handle_to_pointer_f64(server_handle);
    let scope = perry_ffi::TransientRootScope::enter();
    let rooted = scope.root_addrs(&cbs);
    let mut call = DeferredErrorCall {
        callbacks: rooted.as_ptr(),
        len: rooted.len(),
        error: &error as *const ListenError,
    };
    unsafe {
        crate::js_async_hooks_provider_run_catching_with_this(
            async_id,
            this_val,
            0,
            call_deferred_error_callbacks,
            &mut call as *mut DeferredErrorCall as *mut std::ffi::c_void,
        ) as i32
    }
}

#[cfg(test)]
mod listen_error_tests {
    use super::*;

    fn err(code: &str, address: &str, port: u16) -> ListenError {
        ListenError {
            code: code.to_string(),
            errno: -98,
            syscall: "listen".to_string(),
            address: address.to_string(),
            port,
        }
    }

    /// Node 26.5.1, measured: `listen EADDRINUSE: address already in use
    /// 127.0.0.1:47421`. The shape is `<syscall> <CODE>: <description>
    /// <address>:<port>`, and a program that matches on `err.message` sees it.
    #[test]
    fn message_matches_nodes_shape() {
        assert_eq!(
            listen_error_message(&err("EADDRINUSE", "127.0.0.1", 47421)),
            "listen EADDRINUSE: address already in use 127.0.0.1:47421"
        );
        assert_eq!(
            listen_error_message(&err("EACCES", "0.0.0.0", 80)),
            "listen EACCES: permission denied 0.0.0.0:80"
        );
    }

    /// A pipe listen has no address; the tail is omitted rather than rendered
    /// as a stray `:0`.
    #[test]
    fn message_without_an_address_omits_the_tail() {
        assert_eq!(
            listen_error_message(&err("EADDRINUSE", "", 0)),
            "listen EADDRINUSE: address already in use"
        );
    }

    /// A `listen(cb)` whose bind was posted to the loop's owner registers its
    /// callback up front (so the scanner roots it) without arming the
    /// `'listening'` emit; a bind that then fails must take the callback back
    /// out, or `server_is_active` would keep the process alive for it forever
    /// and a later `'listening'` emit would still run it.
    #[test]
    fn a_withdrawn_listen_callback_leaves_nothing_behind() {
        let mut server = HttpServer::with_handler(0);
        server.listeners.insert("listening".to_string(), vec![11]);
        register_listen_callback(&mut server, 22);
        assert!(
            !server.pending_listening_emit,
            "registering must not arm the emit; only a successful bind does"
        );
        assert_eq!(server.listeners["listening"], vec![11, 22]);
        assert!(server_is_active(&server));

        withdraw_listen_callbacks(&mut server);
        assert!(server.deferred_listen_cbs.is_empty());
        assert_eq!(
            server.listeners["listening"],
            vec![11],
            "a listener the program added itself must survive"
        );
        assert!(!server_is_active(&server));
    }

    /// A listen with no loop anywhere (a host where `Loop::new` failed) is
    /// reported as Node would report an unsupported socket operation.
    #[test]
    fn no_loop_message_names_the_operation() {
        assert_eq!(
            listen_error_message(&err("ENOTSUP", "127.0.0.1", 8080)),
            "listen ENOTSUP: operation not supported on socket 127.0.0.1:8080"
        );
    }

    /// A failed listen never became a listening server, so a `'listening'`
    /// emit queued before the bind was attempted must not survive it — Node
    /// fires `'error'` and nothing else.
    #[test]
    fn queueing_an_error_cancels_a_pending_listening_emit() {
        let mut server = HttpServer::with_handler(0);
        server.pending_listening_emit = true;
        server.listening = true;
        queue_deferred_error_emit(&mut server, err("EADDRINUSE", "127.0.0.1", 47421));
        assert!(!server.pending_listening_emit);
        assert!(!server.listening);
        assert!(server.pending_error_emit.is_some());
    }
}
