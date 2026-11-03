//! A WebAssembly memory is reachable only after its host installer runs.
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;
static SET_MEMORY_BUFFER: Hook<fn(*mut ObjectHeader, f64) -> *mut ObjectHeader> = Hook::empty();
pub(crate) fn set_memory_buffer(object: *mut ObjectHeader, buffer: f64) -> *mut ObjectHeader {
    SET_MEMORY_BUFFER
        .get()
        .map_or(object, |run| run(object, buffer))
}
pub(crate) fn install() {
    SET_MEMORY_BUFFER.set(crate::webassembly::set_memory_buffer);
}
