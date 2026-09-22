//! #10444 — `net.Socket.prototype.pipe(dest[, options])` / `.unpipe(dest?)`.
//!
//! A `net.Socket` lives behind ext-net's own handle registry
//! (`statics::sockets()` / `statics::listeners()`), a completely different
//! representation from node:stream's own object+closure model
//! (`perry-runtime`'s `node_stream` module, which backs `Readable`/
//! `Writable`/`Duplex`/`Transform`/`PassThrough`). Bridging those two
//! independent state machines so a socket could reuse node:stream's own
//! `pipe()` implementation (with its full backpressure/unpipe-on-error/
//! `'pipe'`+`'unpipe'` event machinery) is a much larger undertaking than
//! this cluster fix covers.
//!
//! Instead this reuses the SAME generic `Get(dest, "write")` + call
//! duck-typed dispatch the runtime already relies on to resolve thenables
//! (`crate::promise::assimilate::assimilate_via_then_property` in
//! `perry-runtime`, which does `Get(value, "then")` then invokes it with
//! `this` bound to the thenable): fetch `dest.write` / `dest.end` by name
//! through `js_dynamic_object_get_property` and invoke whatever comes back
//! through `js_native_call_value` with `dest` as the implicit receiver.
//! That resolves correctly regardless of what representation `dest` is —
//! another handle-backed socket, a node:stream object, or a plain user
//! object that overrides `write` — the same way real Node duck-types its
//! destination.
//!
//! Scope: this forwards `'data'` to `dest.write(chunk)` and (unless
//! `{ end: false }`) calls `dest.end()` once the source's `'end'` fires, and
//! returns `dest` for chaining. It does NOT implement automatic
//! unpipe-on-error, backpressure-aware pause/resume of the source, or the
//! `'pipe'`/`'unpipe'` events on the destination that Node's real
//! `Readable.prototype.pipe` fires — those are follow-up work, not part of
//! the #10444 reproduction (a `PassThrough`/`Transform` destination reading
//! everything a socket writes).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use perry_ffi::{
    alloc_closure, closure_capture_f64, register_closure_arity, set_closure_capture_f64,
    GcRootVisitor, RawClosureHeader,
};

use crate::statics;

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL_BITS: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE_BITS: u64 = 0x7FFC_0000_0000_0003;

// `js_dynamic_object_get_property` / `js_implicit_this_set` /
// `js_native_call_value` aren't wrapped by perry-ffi (unlike the closure
// helpers above); declare them the same way `dispatch.rs` declares its own
// direct `perry-runtime` FFI symbols (`js_class_method_bind`,
// `js_promise_resolve`, …) — resolved at final link time, not a Rust-level
// crate dependency.
extern "C" {
    fn js_dynamic_object_get_property(
        obj_value: f64,
        property_name_ptr: *const i8,
        property_name_len: usize,
    ) -> f64;
    fn js_implicit_this_set(value: f64) -> f64;
    fn js_native_call_value(func_value: f64, args_ptr: *const f64, args_len: usize) -> f64;
}

fn is_nullish(v: f64) -> bool {
    let bits = v.to_bits();
    bits == TAG_UNDEFINED_BITS || bits == TAG_NULL_BITS
}

fn is_callable(v: f64) -> bool {
    // Native closures / bound handle methods / class methods are all
    // POINTER_TAG (0x7FFD) or the handle-method-bind shape; a non-callable
    // `Get` result (missing property, a plain data field) is either
    // undefined or some other tag entirely. This mirrors the coarse
    // callable check `assimilate_via_then_property` uses before invoking a
    // fetched `then` — good enough to avoid calling `undefined()` when
    // `dest` has no `write` at all, without re-implementing full
    // `IsCallable`.
    !is_nullish(v) && (v.to_bits() >> 48) == 0x7FFD
}

/// `Get(dest, "write")(chunk)` with `this` bound to `dest`.
fn generic_write(dest: f64, chunk: f64) {
    unsafe {
        let write_fn = js_dynamic_object_get_property(dest, c"write".as_ptr(), 5);
        if !is_callable(write_fn) {
            return;
        }
        let prev = js_implicit_this_set(dest);
        let args = [chunk];
        let _ = js_native_call_value(write_fn, args.as_ptr(), args.len());
        js_implicit_this_set(prev);
    }
}

/// `Get(dest, "end")()` with `this` bound to `dest`.
fn generic_end(dest: f64) {
    unsafe {
        let end_fn = js_dynamic_object_get_property(dest, c"end".as_ptr(), 3);
        if !is_callable(end_fn) {
            return;
        }
        let prev = js_implicit_this_set(dest);
        let _ = js_native_call_value(end_fn, std::ptr::null(), 0);
        js_implicit_this_set(prev);
    }
}

extern "C" fn pipe_data_forward(closure: *const RawClosureHeader, chunk: f64) -> f64 {
    if !closure.is_null() {
        let dest = unsafe { closure_capture_f64(closure, 0) };
        generic_write(dest, chunk);
    }
    f64::from_bits(TAG_UNDEFINED_BITS)
}

extern "C" fn pipe_end_forward(closure: *const RawClosureHeader) -> f64 {
    if !closure.is_null() {
        let dest = unsafe { closure_capture_f64(closure, 0) };
        let end_on_finish = unsafe { closure_capture_f64(closure, 1) };
        if end_on_finish.to_bits() != TAG_FALSE_BITS {
            generic_end(dest);
        }
    }
    f64::from_bits(TAG_UNDEFINED_BITS)
}

static ARITY_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_pipe_closure_arities_registered() {
    ARITY_REGISTERED.call_once(|| {
        register_closure_arity(pipe_data_forward as *const u8, 1);
        register_closure_arity(pipe_end_forward as *const u8, 0);
    });
}

/// One socket -> destination pipe route, tracked so `unpipe` can remove
/// exactly the listener closures a matching `pipe()` call installed.
///
/// Deliberately does NOT cache `dest`'s bits here: `dest` is a NaN-boxed
/// value that can be a heap pointer, and a second, un-rooted copy of it
/// would go stale the moment a GC cycle moves the object — the closure's
/// OWN capture slot 0 (scanned automatically once `data_cb` is reachable
/// via `statics::listeners()`, see `install_pipe_listeners`) is the only
/// copy this module keeps, and `matches_dest` below reads it back live at
/// comparison time instead of trusting a cache the collector cannot see.
struct PipeRoute {
    data_cb: i64,
    end_cb: i64,
}

impl PipeRoute {
    fn matches_dest(&self, dest_bits: u64) -> bool {
        let live_dest = unsafe { closure_capture_f64(self.data_cb as *const RawClosureHeader, 0) };
        live_dest.to_bits() == dest_bits
    }
}

fn pipe_routes() -> &'static Mutex<HashMap<i64, Vec<PipeRoute>>> {
    static ROUTES: OnceLock<Mutex<HashMap<i64, Vec<PipeRoute>>>> = OnceLock::new();
    ROUTES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register `data_cb`/`end_cb` as normal `'data'`/`'end'` listeners on
/// `handle`, reusing the SAME `statics::listeners()` registry every other
/// socket listener goes through — so they get the same GC-root scanning
/// (`gc_roots::scan_net_roots`) and the same dispatch path
/// (`socket_events::js_ext_net_drain_pending`) as a user's own `.on(...)`,
/// with no new plumbing.
fn install_pipe_listeners(handle: i64, data_cb: i64, end_cb: i64) {
    let mut listeners = statics::listeners().lock().unwrap();
    let per_socket = listeners.entry(handle).or_default();
    per_socket
        .entry("data".to_string())
        .or_default()
        .push(data_cb);
    per_socket
        .entry("end".to_string())
        .or_default()
        .push(end_cb);
}

fn uninstall_pipe_listeners(handle: i64, data_cb: i64, end_cb: i64) {
    let mut listeners = statics::listeners().lock().unwrap();
    if let Some(per_socket) = listeners.get_mut(&handle) {
        if let Some(vec) = per_socket.get_mut("data") {
            vec.retain(|cb| *cb != data_cb);
        }
        if let Some(vec) = per_socket.get_mut("end") {
            vec.retain(|cb| *cb != end_cb);
        }
    }
}

/// `socket.pipe(dest[, options])`. Returns `dest` unchanged (Node's
/// chaining contract), or `undefined` when `dest` is missing/nullish.
pub(crate) fn socket_pipe(handle: i64, dest: f64, options: f64) -> f64 {
    if is_nullish(dest) {
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }
    crate::ensure_gc_scanner_registered();
    ensure_pipe_closure_arities_registered();

    let end_on_finish = unsafe {
        if is_nullish(options) {
            f64::from_bits(0x7FFC_0000_0000_0004) // default true
        } else {
            let v = js_dynamic_object_get_property(options, c"end".as_ptr(), 3);
            if is_nullish(v) {
                f64::from_bits(0x7FFC_0000_0000_0004)
            } else {
                v
            }
        }
    };

    let data_closure = alloc_closure(pipe_data_forward as *const u8, 1);
    let end_closure = alloc_closure(pipe_end_forward as *const u8, 2);
    if data_closure.is_null() || end_closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED_BITS);
    }
    unsafe {
        set_closure_capture_f64(data_closure, 0, dest);
        set_closure_capture_f64(end_closure, 0, dest);
        set_closure_capture_f64(end_closure, 1, end_on_finish);
    }
    let data_cb = data_closure as i64;
    let end_cb = end_closure as i64;
    install_pipe_listeners(handle, data_cb, end_cb);
    pipe_routes()
        .lock()
        .unwrap()
        .entry(handle)
        .or_default()
        .push(PipeRoute { data_cb, end_cb });

    dest
}

/// `socket.unpipe([dest])`. Removes the pipe route(s) installed by a prior
/// `pipe()` call — all of them when `dest` is omitted, only the ones whose
/// destination matches otherwise. Always returns the socket handle.
pub(crate) fn socket_unpipe(handle: i64, dest: f64) {
    let filter_bits = (!is_nullish(dest)).then(|| dest.to_bits());
    let removed: Vec<(i64, i64)> = {
        let mut routes = pipe_routes().lock().unwrap();
        let Some(list) = routes.get_mut(&handle) else {
            return;
        };
        let mut removed = Vec::new();
        list.retain(|route| {
            let matches = filter_bits.is_none_or(|bits| route.matches_dest(bits));
            if matches {
                removed.push((route.data_cb, route.end_cb));
            }
            !matches
        });
        if list.is_empty() {
            routes.remove(&handle);
        }
        removed
    };
    for (data_cb, end_cb) in removed {
        uninstall_pipe_listeners(handle, data_cb, end_cb);
    }
}

/// Drop every tracked pipe route for `handle` without touching the listener
/// registry — called from the `'close'` teardown, which already clears the
/// whole `statics::listeners()` entry for `handle` (see
/// `socket_events::js_ext_net_drain_pending`'s `Close` arm), so removing the
/// individual callbacks there would be redundant.
pub(crate) fn drop_routes(handle: i64) {
    pipe_routes().lock().unwrap().remove(&handle);
}

/// GC root scanner for `pipe_routes()` — called from
/// `gc_roots::scan_net_roots` alongside the sibling `statics::listeners()`
/// scan. `data_cb`/`end_cb` are a SECOND copy of pointers already rooted via
/// `statics::listeners()` (`install_pipe_listeners` pushes the same values
/// there), but a copying GC cycle only rewrites addresses IN PLACE at
/// wherever the scanner visits them — the two copies are independent slots
/// as far as the collector is concerned, so this copy needs its own visit or
/// it keeps the pre-evacuation address after the `statics::listeners()` copy
/// has already been updated (`matches_dest`'s capture-slot read would then
/// dereference a stale/forwarded pointer — exactly the class of bug
/// `scripts/gc_runtime_root_holders.py` exists to catch).
pub(crate) fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut routes) = pipe_routes().lock() {
        for per_socket in routes.values_mut() {
            for route in per_socket.iter_mut() {
                visitor.visit_i64_slot(&mut route.data_cb);
                visitor.visit_i64_slot(&mut route.end_cb);
            }
        }
    }
}

// ─── FFI: typed `net.Socket.prototype.pipe`/`.unpipe` ────────────────────────
//
// The `NativeModSig` rows in
// `crates/perry-codegen/src/lower_call/native_table/net_events.rs` call
// these two symbols directly for a statically-typed `net.Socket` receiver.
// The untyped/dynamic-dispatch path (`dispatch.rs`'s `socket_method`) calls
// `socket_pipe`/`socket_unpipe` above instead of going through here, since
// it already has its own handle-nanboxing conventions.

/// `socket.pipe(dest[, options])` for a statically-typed `net.Socket`
/// receiver. See the module doc for what this does and does not implement.
///
/// # Safety
///
/// `dest`/`options` must be valid NaN-boxed JS values (or `undefined`).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_pipe(handle: i64, dest: f64, options: f64) -> f64 {
    socket_pipe(handle, dest, options)
}

/// `socket.unpipe([dest])` for a statically-typed `net.Socket` receiver.
/// Returns the socket handle for chaining, matching Node.
///
/// # Safety
///
/// `dest` must be a valid NaN-boxed JS value (or `undefined`).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_unpipe(handle: i64, dest: f64) -> i64 {
    socket_unpipe(handle, dest);
    handle
}
