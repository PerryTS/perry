//! Async provider objects are ordinary JS edges, never SocketFields members.

use super::payload_transport as p;
use perry_ffi::{JsValue, TransientRootScope};

extern "C" {
    fn js_async_hooks_owned_provider_new(name: *const u8, len: usize, trigger: u64) -> f64;
    fn js_async_hooks_owned_provider_emit_init(resource: f64);
    fn js_async_resource_async_id(owner: i64) -> f64;
    fn js_async_hooks_owned_provider_destroy(resource: f64);
}

pub(crate) const TCP: &str = "tcpResource";
pub(crate) const CONNECT: &str = "connectResource";
pub(crate) const SHUTDOWN: &str = "shutdownResource";
pub(crate) const SERVER: &str = "serverResource";

pub(crate) fn new(name: &'static [u8], trigger: u64) -> f64 {
    unsafe { js_async_hooks_owned_provider_new(name.as_ptr(), name.len(), trigger) }
}
pub(crate) fn notify(resource: f64) {
    unsafe {
        js_async_hooks_owned_provider_emit_init(resource);
    }
}
pub(crate) fn id(resource: f64) -> u64 {
    if !JsValue::from_bits(resource.to_bits()).is_pointer() {
        return 0;
    }
    let value = unsafe { js_async_resource_async_id(p::raw_owner(resource)) };
    JsValue::from_bits(value.to_bits()).to_number().max(0.0) as u64
}
pub(crate) fn retire(resource: f64) {
    unsafe {
        js_async_hooks_owned_provider_destroy(resource);
    }
}

/// The state edge is installed before init hooks can call JS. The native ids
/// are installed by the caller before notify, with no native borrow across it.
pub(crate) fn publish(state: f64, key: &str, name: &'static [u8], trigger: u64) -> f64 {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(state);
    let resource = scope.root_nanbox(new(name, trigger));
    p::record_set(state.get(), key, resource.get());
    resource.get()
}
pub(crate) fn resource(owner: f64) -> f64 {
    if let Ok(link) = p::server_link(owner) {
        p::record_get(unsafe { p::server_state(link, false) }, SERVER)
    } else if let Ok(link) = p::socket_link(owner) {
        p::record_get(unsafe { p::socket_state(link, false) }, TCP)
    } else {
        p::undefined()
    }
}

pub(crate) fn capture_socket(state: f64, record: f64) {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(state);
    let record = scope.root_nanbox(record);
    for key in [TCP, CONNECT, SHUTDOWN] {
        let resource = scope.root_nanbox(p::record_get(state.get(), key));
        p::record_set(record.get(), key, resource.get());
        p::record_set(state.get(), key, p::undefined());
    }
}
pub(crate) fn retire_socket(record: f64) {
    let scope = TransientRootScope::enter();
    let record = scope.root_nanbox(record);
    for key in [CONNECT, SHUTDOWN, TCP] {
        let resource = scope.root_nanbox(p::record_get(record.get(), key));
        retire(resource.get());
        p::record_set(record.get(), key, p::undefined());
    }
}
