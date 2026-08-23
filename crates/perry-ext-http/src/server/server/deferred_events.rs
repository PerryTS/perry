//! Deferred server lifecycle events shared by HTTP, HTTPS, and HTTP/2.

use super::*;

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
pub(crate) fn drain_deferred_listen_for<T, F>(server_handle: i64, base_of: F) -> i32
where
    T: Send + Sync + 'static,
    F: FnOnce(&mut T) -> &mut HttpServer,
{
    let cbs: Vec<i64> = match get_handle_mut::<T>(server_handle) {
        Some(t) => {
            let s = base_of(t);
            if !std::mem::take(&mut s.pending_listening_emit) {
                return 0;
            }
            let snapshot = s.listeners.get("listening").cloned().unwrap_or_default();
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
            snapshot
        }
        None => return 0,
    };
    let this_val = handle_to_pointer_f64(server_handle);
    let mut fired = 0i32;
    // #8082: the drained snapshot crosses each callback — root it.
    let scope = perry_ffi::TransientRootScope::enter();
    let rooted = scope.root_addrs(&cbs);
    for cb in &rooted {
        let addr = cb.get();
        if addr == 0 {
            continue;
        }
        let raw = addr as *const RawClosureHeader;
        let closure = unsafe { JsClosure::from_raw(raw) };
        if !closure.is_null() {
            with_implicit_this(this_val, || {
                let _ = unsafe { closure.call0() };
            });
            fired += 1;
        }
    }
    fired
}

pub(crate) fn drain_deferred_close_for<T, F>(server_handle: i64, base_of: F) -> i32
where
    T: Send + Sync + 'static,
    F: FnOnce(&mut T) -> &mut HttpServer,
{
    let callbacks = match get_handle_mut::<T>(server_handle) {
        Some(server) => {
            let base = base_of(server);
            if !std::mem::take(&mut base.pending_close_emit) {
                return 0;
            }
            let callbacks = base.listeners.get("close").cloned().unwrap_or_default();
            let once = std::mem::take(&mut base.deferred_close_cbs);
            if let Some(listeners) = base.listeners.get_mut("close") {
                for callback in once {
                    if let Some(index) = listeners.iter().position(|entry| *entry == callback) {
                        listeners.remove(index);
                    }
                }
            }
            callbacks
        }
        None => return 0,
    };
    let this_value = handle_to_pointer_f64(server_handle);
    let scope = perry_ffi::TransientRootScope::enter();
    let callbacks = scope.root_addrs(&callbacks);
    let mut fired = 0;
    for callback in &callbacks {
        let callback = callback.get();
        if callback == 0 {
            continue;
        }
        let closure = unsafe { JsClosure::from_raw(callback as *const RawClosureHeader) };
        if !closure.is_null() {
            with_implicit_this(this_value, || unsafe {
                let _ = closure.call0();
            });
            fired += 1;
        }
    }
    fired
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
    // Even if the user has called close(), the channels may still
    // hold queued items the pump needs to drain on a subsequent tick
    // before the program can exit cleanly.
    if let Some(rx) = s.request_rx.as_ref() {
        if !rx.is_closed() && rx.len() > 0 {
            return true;
        }
    }
    if let Some(rx) = s.upgrade_rx.as_ref() {
        if !rx.is_closed() && rx.len() > 0 {
            return true;
        }
    }
    false
}
