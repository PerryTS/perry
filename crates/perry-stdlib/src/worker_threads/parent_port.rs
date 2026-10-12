//! The worker's parent port uses the same state and surface as MessageChannel.
use super::*;

pub(super) fn worker_parent_port_object() -> *mut perry_runtime::object::ObjectHeader {
    let id = PARENT_PORT_HANDLE as u64;
    let existing = MESSAGE_PORTS.with(|ports| ports.borrow().get(&id).map(|p| p.object_bits));
    if let Some(bits) = existing {
        return perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(bits))
            as *mut perry_runtime::object::ObjectHeader;
    }
    ensure_environment_data_gc_scanner();
    MESSAGE_PORTS.with(|ports| {
        ports.borrow_mut().insert(id, MessagePortState::default());
    });
    message_port::message_port_object(id)
}

pub(super) fn is_closed() -> bool {
    MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&(PARENT_PORT_HANDLE as u64))
            .is_some_and(|p| p.closed)
    })
}
