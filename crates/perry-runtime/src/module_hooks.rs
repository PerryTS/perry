//! Optional Node-API module bodies, reached through hook slots.
//!
//! `node:dgram` (UDP sockets and their reactor) and the `node:http2` constant
//! tables each have a few callers in always-live code: the event pump, the
//! loop-liveness probe, the turnloop completion router, the native-module
//! dispatch/property/key hubs and the GC root registry. Those callers must not
//! name the implementation, or the prebuilt runtime keeps both in every
//! binary. They call these forwarders, which reach the implementation only
//! through slots the `mod-dgram` / `mod-http2-constants` installs fill (see
//! `crate::feature_hooks`; the compiler installs them on a `dgram` /
//! `http2` import). An empty slot answers what a program that never imports
//! the module can observe: no sockets, no pending work, no constants.
//! (`node:test` has the same shape in `node_submodules`.)

use crate::array::ArrayHeader;
use crate::feature_hooks::Hook;
#[cfg(not(target_arch = "wasm32"))]
use crate::turnloop_proc::StreamEvent;

pub(crate) const DGRAM_ROOT_SCANNER_NAME: &str = "crate::dgram_reactor::scan_roots_mut";

static DGRAM_CREATE_SOCKET: Hook<fn(*const ArrayHeader) -> f64> = Hook::empty();
static DGRAM_PUMP: Hook<fn()> = Hook::empty();
static DGRAM_HAS_ACTIVE: Hook<fn() -> bool> = Hook::empty();
static DGRAM_TURNLOOP_SOCKETS: Hook<fn() -> u64> = Hook::empty();
#[cfg(not(target_arch = "wasm32"))]
static DGRAM_ON_COMPLETION: Hook<fn(u64, StreamEvent)> = Hook::empty();
#[cfg(target_os = "wasi")]
static DGRAM_NEXT_WAKE: Hook<fn() -> f64> = Hook::empty();
#[cfg(target_os = "wasi")]
static DGRAM_INTEREST: Hook<fn(&mut Vec<i32>, &mut Vec<u8>)> = Hook::empty();

/// `dgram.createSocket(...)` through the native-module dispatch table.
pub(crate) fn dgram_create_socket(args: *const ArrayHeader) -> f64 {
    match DGRAM_CREATE_SOCKET.get() {
        Some(create) => create(args),
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// #4911: deliver queued UDP datagrams as `'message'` events. Zero-cost (one
/// relaxed load) when no sockets are bound.
pub(crate) fn dgram_pump() {
    if let Some(pump) = DGRAM_PUMP.get() {
        pump();
    }
}

/// #4911: a bound + `ref`'d `node:dgram` socket keeps the loop alive.
pub(crate) fn dgram_has_active() -> bool {
    DGRAM_HAS_ACTIVE.get().is_some_and(|f| f())
}

pub(crate) fn dgram_turnloop_sockets() -> u64 {
    DGRAM_TURNLOOP_SOCKETS.get().map_or(0, |f| f())
}

/// WASI: the bound the UDP reactor puts on the next event-loop park, in
/// milliseconds: 0 while outcomes wait for delivery, else `-1`; its sockets
/// wake the park through [`dgram_interest`] (`wasi_net/dgram.rs`).
#[cfg(target_os = "wasi")]
pub(crate) fn dgram_next_wake_ms() -> f64 {
    DGRAM_NEXT_WAKE.get().map_or(-1.0, |f| f())
}

/// WASI: append each open UDP socket's descriptor and interest to the park's
/// wait set (`wasi_net::park`).
#[cfg(target_os = "wasi")]
pub(crate) fn dgram_interest(fds: &mut Vec<i32>, kinds: &mut Vec<u8>) {
    if let Some(interest) = DGRAM_INTEREST.get() {
        interest(fds, kinds);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dgram_on_completion(socket: u64, event: StreamEvent) {
    // A completion can only name a socket a dgram install created.
    if let Some(on_completion) = DGRAM_ON_COMPLETION.get() {
        on_completion(socket, event);
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_os = "wasi"))]
fn dgram_create_socket_impl(args: *const ArrayHeader) -> f64 {
    crate::dgram::js_dgram_create_socket(args)
}

/// The `mod-dgram` install. Native UDP sockets ride turnloop; a WASI guest's
/// ride the polled reactor in `wasi_net/dgram.rs`, which has the same API. The
/// web target has no sockets, so there the slots stay empty.
pub(crate) fn install_dgram() {
    #[cfg(any(not(target_arch = "wasm32"), target_os = "wasi"))]
    {
        crate::dgram_registry_owner::install();
        DGRAM_CREATE_SOCKET.set(dgram_create_socket_impl);
        DGRAM_PUMP.set(crate::dgram_reactor::pump);
        DGRAM_HAS_ACTIVE.set(crate::dgram_reactor::has_active);
        DGRAM_TURNLOOP_SOCKETS.set(crate::dgram_reactor::turnloop_sockets);
        #[cfg(not(target_arch = "wasm32"))]
        DGRAM_ON_COMPLETION.set(crate::dgram_reactor::on_completion);
        #[cfg(target_os = "wasi")]
        DGRAM_NEXT_WAKE.set(crate::dgram_reactor::next_wake_ms);
        #[cfg(target_os = "wasi")]
        DGRAM_INTEREST.set(crate::dgram_reactor::interest);
        crate::feature_hooks::DGRAM_ROOT_SCANNER.set(crate::dgram_reactor::scan_roots_mut);
        crate::gc::gc_register_named_mutable_root_scanner(
            DGRAM_ROOT_SCANNER_NAME,
            crate::dgram_reactor::scan_roots_mut,
        );
    }
}

static HTTP2_SCAN_ROOTS: Hook<for<'a, 'b> fn(&'a mut crate::gc::RuntimeRootVisitor<'b>)> =
    Hook::empty();
static HTTP2_SENSITIVE_HEADERS: Hook<fn() -> f64> = Hook::empty();
static HTTP2_CONSTANT: Hook<fn(&str) -> Option<f64>> = Hook::empty();
static HTTP2_KEYS: Hook<fn(&str) -> Option<&'static [&'static [u8]]>> = Hook::empty();

/// #6468: the `sensitiveHeaders` symbol slot's root, present once installed.
pub(crate) fn http2_scan_roots(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if let Some(scan) = HTTP2_SCAN_ROOTS.get() {
        scan(visitor);
    }
}

/// `http2.sensitiveHeaders`. The http2 namespace object only materializes on a
/// `node:http2` import, the same signal that installs the tables, so the
/// `None` of an empty slot is never observed.
pub(crate) fn http2_sensitive_headers() -> Option<f64> {
    HTTP2_SENSITIVE_HEADERS.get().map(|f| f())
}

/// `http2.constants.<prop>`.
pub(crate) fn http2_constant(prop: &str) -> Option<f64> {
    HTTP2_CONSTANT.get().and_then(|f| f(prop))
}

/// The key list of the `http2` / `http2.constants` namespace.
pub(crate) fn http2_keys(namespace: &str) -> Option<&'static [&'static [u8]]> {
    HTTP2_KEYS.get().and_then(|f| f(namespace))
}

fn http2_keys_impl(namespace: &str) -> Option<&'static [&'static [u8]]> {
    match namespace {
        "http2" => Some(crate::node_http2_constants::HTTP2_NAMESPACE_KEYS),
        "http2.constants" => Some(crate::node_http2_constants::HTTP2_CONSTANTS_KEYS),
        _ => None,
    }
}

/// The `mod-http2-constants` install.
pub(crate) fn install_http2_constants() {
    HTTP2_SCAN_ROOTS.set(crate::node_http2_constants::scan_roots_mut);
    HTTP2_SENSITIVE_HEADERS.set(crate::node_http2_constants::sensitive_headers_symbol);
    HTTP2_CONSTANT.set(crate::node_http2_constants::constant);
    HTTP2_KEYS.set(http2_keys_impl);
}
