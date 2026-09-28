//! The Web event surface lives on shared prototypes; state is object-owned.
use super::*;
use crate::closure::ClosureHeader;
use crate::native_class_ids as ids;

pub(super) fn throw_receiver(name: &str) -> ! {
    let message = format!("Value of \"this\" must be of type {name}");
    let text = key(message.as_bytes());
    let error = crate::error::js_typeerror_new(text);
    crate::exception::js_throw(boxed_ptr(error))
}

fn receiver(class: u32, name: &str) -> *mut ObjectHeader {
    let this = crate::object::js_implicit_this_get();
    if let Some(obj) = value_as_ptr::<ObjectHeader>(this) {
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(obj as usize) };
        if header
            .as_ref()
            .is_some_and(|h| unsafe { h.as_ref().obj_type } == crate::gc::GC_TYPE_OBJECT)
        {
            let branded = match class {
                ids::EVENT => is_event_instance(obj),
                ids::CUSTOM_EVENT => {
                    is_event_instance(obj) && !is_undefined(state::get(obj, state::DETAIL))
                }
                ids::ABORT_CONTROLLER => state::has(obj, state::CONTROLLER_SIGNAL),
                ids::ABORT_SIGNAL => !is_undefined(state::get(obj, state::SIGNAL_ABORTED)),
                ids::EVENT_TARGET => unsafe { is_event_target(obj) },
                ids::DOM_EXCEPTION => is_dom_exception_object(obj),
                _ => false,
            };
            if branded {
                return obj;
            }
        }
        if class == ids::DOM_EXCEPTION
            && header
                .as_ref()
                .is_some_and(|h| unsafe { h.as_ref().obj_type } == crate::gc::GC_TYPE_ERROR)
            && is_dom_exception_error(obj.cast())
        {
            return obj;
        }
    }
    throw_receiver(name)
}

macro_rules! event_getter {
    ($function:ident, $key:expr) => {
        extern "C" fn $function(_: *const ClosureHeader) -> f64 {
            state::get(receiver(ids::EVENT, "Event"), $key)
        }
    };
}
event_getter!(get_type, state::TYPE);
event_getter!(get_target, state::TARGET);
extern "C" fn get_current_target(_: *const ClosureHeader) -> f64 {
    let owner = receiver(ids::EVENT, "Event");
    if event_bool_field(owner, state::DISPATCHED) {
        state::get(owner, state::TARGET)
    } else {
        null_value()
    }
}
event_getter!(get_bubbles, state::BUBBLES);
event_getter!(get_cancelable, state::CANCELABLE);
event_getter!(get_default_prevented, state::DEFAULT_PREVENTED);
event_getter!(get_timestamp, state::TIMESTAMP);
event_getter!(get_composed, state::COMPOSED);
extern "C" fn get_phase(_: *const ClosureHeader) -> f64 {
    let owner = receiver(ids::EVENT, "Event");
    if event_bool_field(owner, state::DISPATCHED) {
        2.0
    } else {
        0.0
    }
}
extern "C" fn get_trusted(_: *const ClosureHeader) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    let trusted = value_as_ptr::<ObjectHeader>(receiver)
        .filter(|&obj| state::has(obj, state::TRUSTED))
        .is_some_and(|obj| event_bool_field(obj, state::TRUSTED));
    bool_value(trusted)
}
event_getter!(get_cancel_bubble, state::STOPPED);
extern "C" fn get_return_value(_: *const ClosureHeader) -> f64 {
    let obj = receiver(ids::EVENT, "Event");
    bool_value(!event_bool_field(obj, state::DEFAULT_PREVENTED))
}
extern "C" fn set_return_value(_: *const ClosureHeader, value: f64) -> f64 {
    let obj = receiver(ids::EVENT, "Event");
    if crate::value::js_is_truthy(value) == 0
        && event_bool_field(obj, state::CANCELABLE)
        && !event_bool_field(obj, state::PASSIVE)
    {
        state::set(obj, state::DEFAULT_PREVENTED, bool_value(true));
    }
    undefined_value()
}
extern "C" fn set_cancel_bubble(_: *const ClosureHeader, value: f64) -> f64 {
    let obj = receiver(ids::EVENT, "Event");
    if crate::value::js_is_truthy(value) != 0 {
        state::require_private(obj, state::STOPPED);
        state::set(obj, state::STOPPED, bool_value(true));
    }
    undefined_value()
}
extern "C" fn get_detail(_: *const ClosureHeader) -> f64 {
    state::get(receiver(ids::CUSTOM_EVENT, "CustomEvent"), state::DETAIL)
}
extern "C" fn get_signal(_: *const ClosureHeader) -> f64 {
    boxed_ptr(crate::url::js_abort_controller_signal(receiver(
        ids::ABORT_CONTROLLER,
        "AbortController",
    )))
}
extern "C" fn get_aborted(_: *const ClosureHeader) -> f64 {
    state::get_slot(
        receiver(ids::ABORT_SIGNAL, "AbortSignal"),
        state::SIGNAL_ABORTED,
    )
}
extern "C" fn get_reason(_: *const ClosureHeader) -> f64 {
    state::get_slot(
        receiver(ids::ABORT_SIGNAL, "AbortSignal"),
        state::SIGNAL_REASON,
    )
}
extern "C" fn get_onabort(_: *const ClosureHeader) -> f64 {
    let obj = receiver(ids::ABORT_SIGNAL, "AbortSignal");
    let scope = crate::gc::RuntimeHandleScope::new();
    let handlers = scope.root_nanbox_f64(state::get(obj, state::HANDLERS));
    if is_undefined(handlers.get_nanbox_f64()) {
        return null_value();
    }
    let name = string_value(b"abort");
    let handler = crate::map::js_map_get(
        value_as_ptr::<crate::map::MapHeader>(handlers.get_nanbox_f64()).unwrap(),
        name,
    );
    if is_undefined(handler) {
        return null_value();
    }
    let handler = scope.root_nanbox_f64(handler);
    let name = key(b"handler");
    let value = js_object_get_field_by_name_f64(
        value_as_ptr::<ObjectHeader>(handler.get_nanbox_f64()).unwrap(),
        name,
    );
    if is_undefined(value) {
        null_value()
    } else {
        value
    }
}
extern "C" fn onabort_wrapper(closure: *const ClosureHeader, event: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure as *mut ClosureHeader);
    let event = scope.root_nanbox_f64(event);
    let name = key(b"handler");
    let handler = closure
        .with_mut_ptr::<ClosureHeader, _>(|ptr| js_object_get_field_by_name_f64(ptr.cast(), name));
    let bits = handler.to_bits();
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize)
    {
        let args = [event.get_nanbox_f64()];
        unsafe { crate::closure::js_native_call_value(handler, args.as_ptr(), 1) }
    } else {
        undefined_value()
    }
}
extern "C" fn set_onabort(_: *const ClosureHeader, value: f64) -> f64 {
    let _gc = crate::gc::GcSuppressScope::new();
    let obj = receiver(ids::ABORT_SIGNAL, "AbortSignal");
    let handlers = state::get(obj, state::HANDLERS);
    let handlers = if is_undefined(handlers) {
        let map = crate::map::js_map_alloc(0);
        state::set(obj, state::HANDLERS, boxed_ptr(map));
        map
    } else {
        value_as_ptr::<crate::map::MapHeader>(handlers).unwrap()
    };
    let event_name = string_value(b"abort");
    let mut wrapped = crate::map::js_map_get(handlers, event_name);
    if is_undefined(wrapped) {
        let function = onabort_wrapper as *const u8;
        crate::closure::js_register_closure_arity(function, 1);
        let closure = crate::closure::js_closure_alloc(function, 0);
        wrapped = boxed_ptr(closure);
        crate::object::set_bound_native_closure_name(closure, "eventHandler");
        crate::map::js_map_set(handlers, event_name, wrapped);
        unsafe {
            js_event_target_add_event_listener(obj, key(b"abort"), closure as i64);
        }
    }
    js_object_set_field_by_name(
        value_as_ptr::<ObjectHeader>(wrapped).unwrap(),
        key(b"handler"),
        value,
    );
    undefined_value()
}
extern "C" fn abort(_: *const ClosureHeader, reason: f64) -> f64 {
    crate::url::js_abort_controller_abort_reason(
        receiver(ids::ABORT_CONTROLLER, "AbortController"),
        reason,
    );
    undefined_value()
}
extern "C" fn throw_if_aborted(_: *const ClosureHeader) -> f64 {
    crate::url::js_abort_signal_throw_if_aborted(receiver(ids::ABORT_SIGNAL, "AbortSignal"))
}
extern "C" fn get_dom_name(_: *const ClosureHeader) -> f64 {
    let obj = receiver(ids::DOM_EXCEPTION, "DOMException");
    if unsafe { crate::error::ptr_is_native_error(obj as usize) } {
        crate::value::js_nanbox_string(crate::error::js_error_get_name(obj.cast()) as i64)
    } else {
        state::get(obj, state::DOM_NAME)
    }
}
extern "C" fn get_dom_message(_: *const ClosureHeader) -> f64 {
    let obj = receiver(ids::DOM_EXCEPTION, "DOMException");
    if unsafe { crate::error::ptr_is_native_error(obj as usize) } {
        crate::value::js_nanbox_string(crate::error::js_error_get_message(obj.cast()) as i64)
    } else {
        state::get(obj, state::DOM_MESSAGE)
    }
}
extern "C" fn get_dom_code(closure: *const ClosureHeader) -> f64 {
    let name = get_dom_name(closure);
    let name = crate::builtins::js_string_coerce(name);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (name as *const u8).add(std::mem::size_of::<StringHeader>()),
            (*name).byte_len as usize,
        )
    };
    dom_exception_code(&String::from_utf8_lossy(bytes))
}

fn accessor(
    proto: *mut ObjectHeader,
    name: &str,
    getter: *const u8,
    setter: Option<*const u8>,
    enumerable: bool,
) {
    // All callers hold the short prototype-construction suppression scope.
    let closure = |func: *const u8, prefix: &str, arity: u32| {
        crate::closure::js_register_closure_arity(func, arity);
        let value = crate::closure::js_closure_alloc(func, 0);
        crate::object::set_bound_native_closure_name(value, &format!("{prefix} {name}"));
        crate::object::native_module::set_builtin_closure_length(value as usize, arity);
        crate::object::native_module::set_builtin_closure_non_constructable(value as usize);
        boxed_ptr(value).to_bits()
    };
    let get = closure(getter, "get", 0);
    let set = setter.map_or(0, |setter| closure(setter, "set", 1));
    unsafe {
        crate::object::install_builtin_getter(proto, name, get);
    }
    crate::object::set_builtin_accessor_descriptor(
        proto as usize,
        name.to_owned(),
        crate::object::AccessorDescriptor { get, set },
        crate::object::PropertyAttrs::new(true, enumerable, true),
    );
}

pub(super) fn install(name: &str, proto: *mut ObjectHeader) {
    let _gc = crate::gc::GcSuppressScope::new();
    let method = |name: &str, func: *const u8, arity: u32, length: u32, enumerable: bool| {
        let value = crate::object::install_proto_method(proto, name, func, arity);
        crate::object::native_module::set_builtin_closure_length(
            crate::value::js_nanbox_get_pointer(value) as usize,
            length,
        );
        crate::object::set_builtin_property_attrs(
            proto as usize,
            name.to_owned(),
            crate::object::PropertyAttrs::new(true, enumerable, true),
        );
    };
    let get = |name: &str, func: *const u8| accessor(proto, name, func, None, true);
    match name {
        "EventTarget" => {
            method(
                "addEventListener",
                event_target_add_event_listener_thunk as *const u8,
                3,
                2,
                true,
            );
            method(
                "removeEventListener",
                event_target_remove_event_listener_thunk as *const u8,
                3,
                2,
                true,
            );
            method(
                "dispatchEvent",
                event_target_dispatch_event_thunk as *const u8,
                1,
                1,
                true,
            );
        }
        "Event" => {
            method(
                "initEvent",
                event_proto_init_event_thunk as *const u8,
                3,
                1,
                true,
            );
            method(
                "stopImmediatePropagation",
                event_proto_stop_immediate_propagation_thunk as *const u8,
                0,
                0,
                true,
            );
            method(
                "preventDefault",
                event_proto_prevent_default_thunk as *const u8,
                0,
                0,
                true,
            );
            get("target", get_target as *const u8);
            get("currentTarget", get_current_target as *const u8);
            get("srcElement", get_target as *const u8);
            get("type", get_type as *const u8);
            get("cancelable", get_cancelable as *const u8);
            get("defaultPrevented", get_default_prevented as *const u8);
            get("timeStamp", get_timestamp as *const u8);
            method(
                "composedPath",
                event_proto_composed_path_thunk as *const u8,
                0,
                0,
                true,
            );
            accessor(
                proto,
                "returnValue",
                get_return_value as *const u8,
                Some(set_return_value as *const u8),
                true,
            );
            get("bubbles", get_bubbles as *const u8);
            get("composed", get_composed as *const u8);
            get("eventPhase", get_phase as *const u8);
            accessor(
                proto,
                "cancelBubble",
                get_cancel_bubble as *const u8,
                Some(set_cancel_bubble as *const u8),
                true,
            );
            method(
                "stopPropagation",
                event_proto_stop_propagation_thunk as *const u8,
                0,
                0,
                true,
            );
            get("isTrusted", get_trusted as *const u8);
        }
        "CustomEvent" => get("detail", get_detail as *const u8),
        "AbortController" => {
            get("signal", get_signal as *const u8);
            method("abort", abort as *const u8, 1, 0, true);
        }
        "AbortSignal" => {
            get("aborted", get_aborted as *const u8);
            accessor(proto, "reason", get_reason as *const u8, None, false);
            method("throwIfAborted", throw_if_aborted as *const u8, 0, 0, false);
            accessor(
                proto,
                "onabort",
                get_onabort as *const u8,
                Some(set_onabort as *const u8),
                true,
            );
        }
        "DOMException" => {
            get("name", get_dom_name as *const u8);
            get("message", get_dom_message as *const u8);
            get("code", get_dom_code as *const u8);
            for (index, name) in DOM_CODES.iter().enumerate() {
                js_object_set_field_by_name(proto, key(name.as_bytes()), (index + 1) as f64);
                crate::object::set_builtin_property_attrs(
                    proto as usize,
                    (*name).to_owned(),
                    crate::object::PropertyAttrs::new(false, true, false),
                );
            }
        }
        _ => unreachable!(),
    }
}

const DOM_CODES: [&str; 25] = [
    "INDEX_SIZE_ERR",
    "DOMSTRING_SIZE_ERR",
    "HIERARCHY_REQUEST_ERR",
    "WRONG_DOCUMENT_ERR",
    "INVALID_CHARACTER_ERR",
    "NO_DATA_ALLOWED_ERR",
    "NO_MODIFICATION_ALLOWED_ERR",
    "NOT_FOUND_ERR",
    "NOT_SUPPORTED_ERR",
    "INUSE_ATTRIBUTE_ERR",
    "INVALID_STATE_ERR",
    "SYNTAX_ERR",
    "INVALID_MODIFICATION_ERR",
    "NAMESPACE_ERR",
    "INVALID_ACCESS_ERR",
    "VALIDATION_ERR",
    "TYPE_MISMATCH_ERR",
    "SECURITY_ERR",
    "NETWORK_ERR",
    "ABORT_ERR",
    "URL_MISMATCH_ERR",
    "QUOTA_EXCEEDED_ERR",
    "TIMEOUT_ERR",
    "INVALID_NODE_TYPE_ERR",
    "DATA_CLONE_ERR",
];

pub(crate) fn install_constructor_constants(name: &str, constructor: *mut ObjectHeader) {
    if name == "EventTarget" {
        let _gc = crate::gc::GcSuppressScope::new();
        let marker = unsafe { crate::symbol::js_symbol_for(string_value(b"nodejs.event_target")) };
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                boxed_ptr(constructor),
                marker,
                bool_value(true),
            );
        }
        return;
    }
    let (names, start): (&[&str], usize) = match name {
        "Event" => (
            &["NONE", "CAPTURING_PHASE", "AT_TARGET", "BUBBLING_PHASE"],
            0,
        ),
        "DOMException" => (&DOM_CODES, 1),
        _ => return,
    };
    let _gc = crate::gc::GcSuppressScope::new();
    for (index, name) in names.iter().enumerate() {
        crate::object::define_builtin_data_property(
            constructor,
            key(name.as_bytes()),
            (index + start) as f64,
            (*name).to_owned(),
            crate::object::PropertyAttrs::new(false, true, false),
        );
    }
}
