use super::{
    boxed_object, closure_value, install_method, js_bool, js_undefined, key, set_object_prototype,
};
use crate::closure::{ClosureHeader, JsThis};
use crate::feature_hooks::Hook;
use crate::object::{self, ObjectHeader};
use crate::value::JSValue;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum BroadcastChannelOperation {
    Validate,
    PostMessage,
    Close,
    Ref,
    Unref,
}

type Dispatch = unsafe extern "C" fn(BroadcastChannelOperation, u64, f64, f64, *mut f64) -> bool;
static DISPATCH: Hook<Dispatch> = Hook::empty();
static ID: object::IntrinsicPrivateReadSite = object::IntrinsicPrivateReadSite::new(
    &object::runtime_layouts::intrinsic_private_atoms::BROADCAST_CHANNEL_ID,
);
static NAME: object::IntrinsicPrivateReadSite = object::IntrinsicPrivateReadSite::new(
    &object::runtime_layouts::intrinsic_private_atoms::BROADCAST_CHANNEL_NAME,
);
static CLOSED: object::IntrinsicPrivateReadSite = object::IntrinsicPrivateReadSite::new(
    &object::runtime_layouts::intrinsic_private_atoms::BROADCAST_CHANNEL_CLOSED,
);

pub fn register_broadcast_channel_dispatch(dispatch: Dispatch) {
    DISPATCH.set(dispatch);
}

pub fn broadcast_channel_id(receiver: f64) -> Option<u64> {
    if !JSValue::from_bits(receiver.to_bits()).is_pointer() {
        return None;
    }
    let address = crate::value::js_nanbox_get_pointer(receiver) as usize;
    match unsafe { crate::value::addr_class::try_read_gc_header(address) } {
        Some(header) if crate::gc_type_is_object!(header.obj_type) => {}
        _ => return None,
    }
    ID.read(receiver).map(f64::to_bits)
}

pub fn broadcast_channel_object_with_id(id: u64, name: f64) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    crate::feature_hooks::js_runtime_install_messaging();
    let object = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
    object::intrinsic_private_add(
        boxed_object(object.get_raw_mut_ptr()),
        "[[BroadcastChannelId]]",
        f64::from_bits(id),
    );
    object::intrinsic_private_add(
        boxed_object(object.get_raw_mut_ptr()),
        "[[BroadcastChannelName]]",
        name.get_nanbox_f64(),
    );
    // Only the runtime-only fallback has no provider-owned closed state.
    if id == 0 {
        object::intrinsic_private_add(
            boxed_object(object.get_raw_mut_ptr()),
            "[[BroadcastChannelClosed]]",
            js_bool(false),
        );
    }
    let constructor = scope.root_nanbox_f64(
        object::native_module::worker_messaging_constructor_value(
            "worker_threads",
            "BroadcastChannel",
        )
        .expect("messaging constructor is published during realm birth"),
    );
    let prototype = object::js_function_prototype_value_for_read(constructor.get_nanbox_f64());
    set_object_prototype(object.get_raw_mut_ptr(), prototype);
    object.get_raw_mut_ptr()
}

fn invalid_receiver() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "Value of \"this\" must be of type BroadcastChannel",
        "ERR_INVALID_THIS",
    )
}

fn resolved_receiver(receiver: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(receiver);
    let tortoise = scope.root_nanbox_f64(receiver);
    let mut power = 1usize;
    let mut lap = 0usize;
    for _ in 0..100_000 {
        let value = target.get_nanbox_f64();
        if broadcast_channel_id(value).is_some() {
            return value;
        }
        let next = if let Some(next) = crate::proxy_hooks::proxy_checked_target(value) {
            next
        } else {
            if !JSValue::from_bits(value.to_bits()).is_pointer() {
                invalid_receiver();
            }
            object::js_object_get_prototype_of(value)
        };
        target.set_nanbox_f64(next);
        if target.get_nanbox_u64() == tortoise.get_nanbox_u64() {
            invalid_receiver();
        }
        lap += 1;
        if lap == power {
            tortoise.set_nanbox_f64(target.get_nanbox_f64());
            power <<= 1;
            lap = 0;
        }
    }
    invalid_receiver()
}

fn dispatch(operation: BroadcastChannelOperation, this: JsThis, data: f64, supplied: bool) -> f64 {
    use BroadcastChannelOperation::*;
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let data = scope.root_nanbox_f64(data);
    let target = scope.root_nanbox_f64(resolved_receiver(receiver.get_nanbox_f64()));
    let id = broadcast_channel_id(target.get_nanbox_f64()).unwrap_or_else(|| invalid_receiver());
    let provider = DISPATCH.peek();
    let mut result = js_undefined();
    if id != 0
        && !provider.is_some_and(|provider| unsafe {
            provider(
                Validate,
                id,
                target.get_nanbox_f64(),
                js_undefined(),
                &mut result,
            )
        })
    {
        invalid_receiver();
    }
    if matches!(operation, PostMessage) && !supplied {
        crate::fs::validate::throw_type_error_with_code(
            "The \"message\" argument must be specified",
            "ERR_MISSING_ARGS",
        );
    }
    if matches!(operation, Validate) {
        return NAME
            .read(target.get_nanbox_f64())
            .unwrap_or_else(|| invalid_receiver());
    }
    if id != 0 {
        if !unsafe {
            provider.unwrap()(
                operation,
                id,
                target.get_nanbox_f64(),
                data.get_nanbox_f64(),
                &mut result,
            )
        } {
            invalid_receiver();
        }
    } else {
        match operation {
            Close => {
                object::intrinsic_private_set(
                    target.get_nanbox_f64(),
                    "[[BroadcastChannelClosed]]",
                    js_bool(true),
                );
            }
            PostMessage
                if CLOSED.read(target.get_nanbox_f64()).map(f64::to_bits)
                    == Some(JSValue::bool(true).bits()) =>
            {
                let message = scope.root_nanbox_f64(f64::from_bits(
                    JSValue::string_ptr(key("BroadcastChannel is closed")).bits(),
                ));
                let name = f64::from_bits(JSValue::string_ptr(key("InvalidStateError")).bits());
                let error = crate::event_target_hook_slots::js_dom_exception_new(
                    message.get_nanbox_f64(),
                    name,
                );
                // The DOMException hook constructs this value outside the core constructors.
                crate::exception::throw_open(crate::value::js_nanbox_pointer(error as i64));
            }
            _ => {}
        }
    }
    if matches!(operation, Ref | Unref) {
        receiver.get_nanbox_f64()
    } else {
        result
    }
}

unsafe extern "C" fn post_message(
    _closure: *const ClosureHeader,
    this: JsThis,
    args: *const f64,
    len: usize,
) -> f64 {
    let data = if len == 0 {
        js_undefined()
    } else {
        unsafe { *args }
    };
    dispatch(BroadcastChannelOperation::PostMessage, this, data, len != 0)
}

extern "C" fn close(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(BroadcastChannelOperation::Close, this, js_undefined(), true)
}
extern "C" fn ref_channel(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(BroadcastChannelOperation::Ref, this, js_undefined(), true)
}
extern "C" fn unref_channel(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(BroadcastChannelOperation::Unref, this, js_undefined(), true)
}
extern "C" fn name_getter(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    dispatch(
        BroadcastChannelOperation::Validate,
        this,
        js_undefined(),
        true,
    )
}

pub(crate) fn install_core_methods(prototype: *mut ObjectHeader) {
    let _no_move = crate::gc::GcSuppressScope::new();
    install_method(prototype, "postMessage", crate::fn_info!(#[realm(BroadcastChannel.prototype.postMessage, constructible = true)] export native_args crate::messaging::broadcast_operations::post_message, 1; with_declared(1)), 1);
    install_method(prototype, "close", crate::fn_info!(#[realm(BroadcastChannel.prototype.close, constructible = true)] export crate::messaging::broadcast_operations::close, 0; with_declared(0)), 0);
    install_method(prototype, "ref", crate::fn_info!(#[realm(BroadcastChannel.prototype.ref, constructible = true)] export crate::messaging::broadcast_operations::ref_channel, 0; with_declared(0)), 0);
    install_method(prototype, "unref", crate::fn_info!(#[realm(BroadcastChannel.prototype.unref, constructible = true)] export crate::messaging::broadcast_operations::unref_channel, 0; with_declared(0)), 0);
    let getter = closure_value(crate::fn_info!(name_getter, 0; with_declared(0), with_flags(crate::closure::FN_NON_CONSTRUCTOR)), "get name", 0);
    object::install_fresh_accessor_property(
        prototype as usize,
        "name".to_string(),
        object::AccessorDescriptor {
            get: getter.to_bits(),
            set: 0,
        },
        object::PropertyAttrs::new(false, true, true),
    );
}

#[cfg(test)]
#[path = "broadcast_operations_tests.rs"]
mod tests;
