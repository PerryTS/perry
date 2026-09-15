//! Per-agent, O(1) dispatchable-channel membership.
//!
//! The onmessage accessor observes handler changes at assignment time. Its
//! value lives in a captured JS cell, traced by the ordinary closure scanner;
//! liveness contains only Rust counters and never invokes a JavaScript getter.
use super::*;
use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    static ACTIVE: Rc<Cell<usize>> = Rc::new(Cell::new(0));
}

pub(super) struct Reference {
    count: Rc<Cell<usize>>,
    active: bool,
}

impl Default for Reference {
    fn default() -> Self {
        Self {
            count: ACTIVE.with(Rc::clone),
            active: false,
        }
    }
}

impl Reference {
    fn set(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            self.count.set(self.count.get() + 1);
        } else {
            debug_assert!(self.count.get() > 0, "channel activity underflow");
            self.count.set(self.count.get() - 1);
        }
    }
}

impl Drop for Reference {
    fn drop(&mut self) {
        self.set(false);
    }
}

impl MessagePortState {
    pub(super) fn refresh_activity(&mut self) {
        self.activity.set(
            self.close_pending
                || (!self.closed
                    && !self.inbox.is_empty()
                    && (self.handler_present
                        || (self.started
                            && (!self.message_cbs.is_empty()
                                || !self.message_event_cbs.is_empty())))),
        );
    }
}

impl BroadcastChannelState {
    pub(super) fn refresh_activity(&mut self) {
        self.activity.set(
            !self.closed
                && !self.inbox.is_empty()
                && (self.handler_present || !self.message_event_cbs.is_empty()),
        );
    }
}

pub(super) fn has_pending() -> i32 {
    ACTIVE.with(|count| i32::from(count.get() != 0))
}

/// Refresh the changed port and its peer once at the end of a native method.
/// No registry walk, including on close/error/unwind.
pub(super) struct PortChange(u64);
impl PortChange {
    pub(super) fn new(id: u64) -> Self {
        Self(id)
    }
}
impl Drop for PortChange {
    fn drop(&mut self) {
        MESSAGE_PORTS.with(|ports| {
            let mut ports = ports.borrow_mut();
            let peer = ports.get(&self.0).map(|state| state.peer);
            if let Some(state) = ports.get_mut(&self.0) {
                state.refresh_activity();
            }
            if let Some(state) = peer.and_then(|id| ports.get_mut(&id)) {
                state.refresh_activity();
            }
        });
    }
}

pub(super) fn install_handler(
    object: *mut perry_runtime::object::ObjectHeader,
    id: u64,
    broadcast: bool,
) -> *mut perry_runtime::object::ObjectHeader {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(object);
    let cell = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 1));
    perry_runtime::object::js_object_set_field(
        cell.get_raw_mut_ptr(),
        0,
        JSValue::from_bits(js_null().to_bits()),
    );
    let make = |function: *const u8, arity| {
        perry_runtime::closure::js_register_closure_arity(function, arity);
        let closure = perry_runtime::closure::js_closure_alloc(function, 3);
        perry_runtime::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(id));
        perry_runtime::closure::js_closure_set_capture_f64(
            closure,
            1,
            object_value(cell.get_raw_mut_ptr()),
        );
        perry_runtime::closure::js_closure_set_capture_f64(
            closure,
            2,
            if broadcast { 1.0 } else { 0.0 },
        );
        scope.root_raw_mut_ptr(closure)
    };
    let getter = make(get_handler as *const u8, 0);
    let setter = make(set_handler as *const u8, 1);
    let key = js_string_from_bytes(b"onmessage".as_ptr(), 9);
    perry_runtime::object::js_object_define_accessor(
        object_value(object.get_raw_mut_ptr()),
        f64::from_bits(JSValue::string_ptr(key).bits()),
        f64::from_bits(
            JSValue::pointer(getter.get_raw_mut_ptr::<ClosureHeader>() as *const u8).bits(),
        ),
        f64::from_bits(
            JSValue::pointer(setter.get_raw_mut_ptr::<ClosureHeader>() as *const u8).bits(),
        ),
    );
    object.get_raw_mut_ptr()
}

extern "C" fn get_handler(closure: *const ClosureHeader) -> f64 {
    let cell = perry_runtime::closure::js_closure_get_capture_f64(closure, 1);
    let cell = perry_runtime::value::js_nanbox_get_pointer(cell)
        as *const perry_runtime::object::ObjectHeader;
    perry_runtime::object::js_object_get_field_f64(cell, 0)
}

extern "C" fn set_handler(closure: *const ClosureHeader, value: f64) -> f64 {
    let id = port_id_from_closure(closure);
    let broadcast = perry_runtime::closure::js_closure_get_capture_f64(closure, 2) != 0.0;
    let present = callback_bits_from_value(value).is_some();
    let cell = perry_runtime::closure::js_closure_get_capture_f64(closure, 1);
    let cell = perry_runtime::value::js_nanbox_get_pointer(cell)
        as *mut perry_runtime::object::ObjectHeader;
    perry_runtime::object::js_object_set_field(cell, 0, JSValue::from_bits(value.to_bits()));
    if broadcast {
        BROADCAST_CHANNELS.with(|channels| {
            if let Some(state) = channels.borrow_mut().get_mut(&id) {
                state.handler_present = present;
                state.refresh_activity();
            }
        });
    } else {
        MESSAGE_PORTS.with(|ports| {
            if let Some(state) = ports.borrow_mut().get_mut(&id) {
                state.handler_present = present;
                state.refresh_activity();
            }
        });
    }
    perry_runtime::event_pump::js_notify_main_thread();
    js_undefined()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dispatchable_channel_count_balances_undeliverable_close_and_cancel() {
        assert_eq!(has_pending(), 0);
        let mut port = MessagePortState::default();
        port.inbox.push_back(serialize_message(1.0));
        port.refresh_activity();
        assert_eq!(has_pending(), 0, "no handler must not keep the loop alive");
        port.handler_present = true;
        port.refresh_activity();
        assert_eq!(has_pending(), 1);
        port.handler_present = false;
        port.refresh_activity();
        assert_eq!(has_pending(), 0);
        port.close_pending = true;
        port.refresh_activity();
        assert_eq!(has_pending(), 1);
        drop(port);
        assert_eq!(has_pending(), 0);
    }
}
