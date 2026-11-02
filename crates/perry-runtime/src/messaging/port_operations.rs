use super::*;
use crate::closure::JsThis;
use crate::feature_hooks::Hook;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum MessagePortOperation {
    PostMessage,
    Start,
    Close,
    Ref,
    Unref,
    HasRef,
}

type MessagePortDispatch =
    unsafe extern "C" fn(MessagePortOperation, f64, f64, f64, *mut f64) -> bool;
static DISPATCH: Hook<MessagePortDispatch> = Hook::empty();
static PORT_ID: object::IntrinsicPrivateReadSite = object::IntrinsicPrivateReadSite::new(
    &object::runtime_layouts::intrinsic_private_atoms::MESSAGE_PORT_ID,
);

#[no_mangle]
pub extern "C" fn js_register_worker_threads_message_port_dispatch(dispatch: MessagePortDispatch) {
    DISPATCH.set(dispatch);
}

/// The private ID remains after close, without retaining the port in a root table.
/// Zero identifies the runtime-only implementation; stdlib IDs start at one.
pub fn message_port_object_with_id(id: u64) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
    object::intrinsic_private_add(
        crate::value::js_nanbox_pointer(object.get_raw_mut_ptr::<ObjectHeader>() as i64),
        "[[MessagePortId]]",
        f64::from_bits(id),
    );
    object.get_raw_mut_ptr()
}

pub fn message_port_id(receiver: f64) -> Option<u64> {
    let value = JSValue::from_bits(receiver.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let address = crate::value::js_nanbox_get_pointer(receiver) as usize;
    match unsafe { crate::value::addr_class::try_read_gc_header(address) } {
        Some(header) if crate::gc_type_is_object!(header.obj_type) => {}
        _ => return None,
    }
    PORT_ID.read(receiver).map(f64::to_bits)
}

fn dispatch(operation: MessagePortOperation, this: JsThis, data: f64, transfer: f64) -> f64 {
    let receiver = this.as_f64();
    let Some(id) = message_port_id(receiver) else {
        invalid_receiver();
    };
    if id != 0 {
        if let Some(provider) = DISPATCH.peek() {
            let mut result = js_undefined();
            if unsafe { provider(operation, receiver, data, transfer, &mut result) } {
                return result;
            }
        }
        invalid_receiver();
    }
    let ptr = crate::value::js_nanbox_get_pointer(receiver) as usize;
    match operation {
        MessagePortOperation::PostMessage => {
            port_post_message(std::ptr::null(), this, data, transfer)
        }
        MessagePortOperation::Start => port_start(std::ptr::null(), this),
        MessagePortOperation::Close => port_close(std::ptr::null(), this),
        MessagePortOperation::Ref | MessagePortOperation::Unref => {
            with_port_states(|map| {
                if let Some(state) = map.get_mut(&ptr) {
                    state.refed = !state.closed && matches!(operation, MessagePortOperation::Ref);
                }
            });
            js_undefined()
        }
        MessagePortOperation::HasRef => js_bool(with_port_states(|map| {
            map.get(&ptr)
                .is_some_and(|state| !state.closed && state.refed)
        })),
    }
}

fn invalid_receiver() -> ! {
    let message = key("Illegal invocation");
    let error = crate::error::js_typeerror_new(message);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64))
}

extern "C" fn post_message(
    _closure: *const ClosureHeader,
    this: JsThis,
    data: f64,
    transfer: f64,
) -> f64 {
    dispatch(MessagePortOperation::PostMessage, this, data, transfer)
}

extern "C" fn start(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(
        MessagePortOperation::Start,
        this,
        js_undefined(),
        js_undefined(),
    )
}
extern "C" fn close(_closure: *const ClosureHeader, this: JsThis, callback: f64) -> f64 {
    dispatch(MessagePortOperation::Close, this, callback, js_undefined())
}
extern "C" fn ref_port(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(
        MessagePortOperation::Ref,
        this,
        js_undefined(),
        js_undefined(),
    )
}
extern "C" fn unref_port(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(
        MessagePortOperation::Unref,
        this,
        js_undefined(),
        js_undefined(),
    )
}
extern "C" fn has_ref_port(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(
        MessagePortOperation::HasRef,
        this,
        js_undefined(),
        js_undefined(),
    )
}

pub fn install_core_methods(object: *mut ObjectHeader) {
    let _no_move = crate::gc::GcSuppressScope::new();
    install_method(object, "postMessage", crate::fn_info!(#[realm(MessagePort.prototype.postMessage, constructible = true)] export crate::messaging::port_operations::post_message, 2; with_declared(2)), 2);
    install_method(object, "start", crate::fn_info!(#[realm(MessagePort.prototype.start, constructible = true)] export crate::messaging::port_operations::start, 0; with_declared(0)), 0);
    install_method(object, "close", crate::fn_info!(#[realm(MessagePort.prototype.close, constructible = true)] export crate::messaging::port_operations::close, 1; with_declared(1)), 1);
    install_method(object, "ref", crate::fn_info!(#[realm(MessagePort.prototype.ref, constructible = true)] export crate::messaging::port_operations::ref_port, 0; with_declared(0)), 0);
    install_method(object, "unref", crate::fn_info!(#[realm(MessagePort.prototype.unref, constructible = true)] export crate::messaging::port_operations::unref_port, 0; with_declared(0)), 0);
    install_method(object, "hasRef", crate::fn_info!(#[realm(MessagePort.prototype.hasRef, constructible = true)] export crate::messaging::port_operations::has_ref_port, 0; with_declared(0)), 0);
}

#[cfg(test)]
#[path = "port_operations_tests.rs"]
mod tests;
