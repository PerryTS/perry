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

fn receiver(this: crate::closure::JsThis, class: u32, name: &str) -> *mut ObjectHeader {
    let this = this.as_f64();
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
        extern "C" fn $function(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
            state::get(receiver(this, ids::EVENT, "Event"), $key)
        }
    };
}
event_getter!(get_type, state::TYPE);
event_getter!(get_target, state::TARGET);
extern "C" fn get_current_target(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let owner = receiver(this, ids::EVENT, "Event");
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
extern "C" fn get_phase(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let owner = receiver(this, ids::EVENT, "Event");
    if event_bool_field(owner, state::DISPATCHED) {
        2.0
    } else {
        0.0
    }
}
extern "C" fn get_trusted(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let receiver = this.as_f64();
    let trusted = value_as_ptr::<ObjectHeader>(receiver)
        .filter(|&obj| state::has(obj, state::TRUSTED))
        .is_some_and(|obj| event_bool_field(obj, state::TRUSTED));
    bool_value(trusted)
}
event_getter!(get_cancel_bubble, state::STOPPED);
extern "C" fn get_return_value(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let obj = receiver(this, ids::EVENT, "Event");
    bool_value(!event_bool_field(obj, state::DEFAULT_PREVENTED))
}
extern "C" fn set_return_value(
    _: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let obj = receiver(this, ids::EVENT, "Event");
    if crate::value::js_is_truthy(value) == 0
        && event_bool_field(obj, state::CANCELABLE)
        && !event_bool_field(obj, state::PASSIVE)
    {
        state::set(obj, state::DEFAULT_PREVENTED, bool_value(true));
    }
    undefined_value()
}
extern "C" fn set_cancel_bubble(
    _: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let obj = receiver(this, ids::EVENT, "Event");
    if crate::value::js_is_truthy(value) != 0 {
        state::require_private(obj, state::STOPPED);
        state::set(obj, state::STOPPED, bool_value(true));
    }
    undefined_value()
}
extern "C" fn get_detail(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    state::get(
        receiver(this, ids::CUSTOM_EVENT, "CustomEvent"),
        state::DETAIL,
    )
}
extern "C" fn get_signal(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    boxed_ptr(crate::url::js_abort_controller_signal(receiver(
        this,
        ids::ABORT_CONTROLLER,
        "AbortController",
    )))
}
extern "C" fn get_aborted(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    state::get_slot(
        receiver(this, ids::ABORT_SIGNAL, "AbortSignal"),
        state::SIGNAL_ABORTED,
    )
}
extern "C" fn get_reason(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    state::get_slot(
        receiver(this, ids::ABORT_SIGNAL, "AbortSignal"),
        state::SIGNAL_REASON,
    )
}
extern "C" fn get_onabort(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let obj = receiver(this, ids::ABORT_SIGNAL, "AbortSignal");
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
extern "C" fn onabort_wrapper(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
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
        unsafe { crate::closure::js_native_call_value(handler, this, args.as_ptr(), 1) }
    } else {
        undefined_value()
    }
}
extern "C" fn set_onabort(
    _: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let _gc = crate::gc::GcSuppressScope::new();
    let obj = receiver(this, ids::ABORT_SIGNAL, "AbortSignal");
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
        let closure = crate::closure::js_closure_alloc(
            crate::fn_info!(onabort_wrapper, 1; with_declared(1)),
            0,
        );
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
extern "C" fn abort(_: *const ClosureHeader, this: crate::closure::JsThis, reason: f64) -> f64 {
    crate::url::js_abort_controller_abort_reason(
        receiver(this, ids::ABORT_CONTROLLER, "AbortController"),
        reason,
    );
    undefined_value()
}
extern "C" fn throw_if_aborted(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    crate::url::js_abort_signal_throw_if_aborted(receiver(this, ids::ABORT_SIGNAL, "AbortSignal"))
}
extern "C" fn get_dom_name(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let obj = receiver(this, ids::DOM_EXCEPTION, "DOMException");
    if unsafe { crate::error::ptr_is_native_error(obj as usize) } {
        crate::value::js_nanbox_string(crate::error::js_error_get_name(obj.cast()) as i64)
    } else {
        state::get(obj, state::DOM_NAME)
    }
}
extern "C" fn get_dom_message(_: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let obj = receiver(this, ids::DOM_EXCEPTION, "DOMException");
    if unsafe { crate::error::ptr_is_native_error(obj as usize) } {
        crate::value::js_nanbox_string(crate::error::js_error_get_message(obj.cast()) as i64)
    } else {
        state::get(obj, state::DOM_MESSAGE)
    }
}
extern "C" fn get_dom_code(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let name = get_dom_name(closure, this);
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
    getter: *const crate::closure::JsFunctionInfo,
    setter: Option<*const crate::closure::JsFunctionInfo>,
    enumerable: bool,
) {
    // All callers hold the short prototype-construction suppression scope.
    let closure = |func: *const crate::closure::JsFunctionInfo, prefix: &str, arity: u32| {
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
    let method = |name: &str,
                  func: *const crate::closure::JsFunctionInfo,
                  arity: u32,
                  length: u32,
                  enumerable: bool| {
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
    let get = |name: &str, func: *const crate::closure::JsFunctionInfo| {
        accessor(proto, name, func, None, true)
    };
    match name {
        "EventTarget" => {
            method(
                "addEventListener",
                crate::fn_info!(event_target_add_event_listener_thunk, 3; with_flags(crate::closure::FN_BUILTIN)),
                3,
                2,
                true,
            );
            method(
                "removeEventListener",
                crate::fn_info!(event_target_remove_event_listener_thunk, 3; with_flags(crate::closure::FN_BUILTIN)),
                3,
                2,
                true,
            );
            method(
                "dispatchEvent",
                crate::fn_info!(event_target_dispatch_event_thunk, 1; with_flags(crate::closure::FN_BUILTIN)),
                1,
                1,
                true,
            );
        }
        "Event" => {
            method(
                "initEvent",
                crate::fn_info!(event_proto_init_event_thunk, 3; with_flags(crate::closure::FN_BUILTIN)),
                3,
                1,
                true,
            );
            method(
                "stopImmediatePropagation",
                crate::fn_info!(event_proto_stop_immediate_propagation_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
                0,
                0,
                true,
            );
            method(
                "preventDefault",
                crate::fn_info!(event_proto_prevent_default_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
                0,
                0,
                true,
            );
            get(
                "target",
                crate::fn_info!(get_target, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "currentTarget",
                crate::fn_info!(get_current_target, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "srcElement",
                crate::fn_info!(get_target, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "type",
                crate::fn_info!(get_type, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "cancelable",
                crate::fn_info!(get_cancelable, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "defaultPrevented",
                crate::fn_info!(get_default_prevented, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "timeStamp",
                crate::fn_info!(get_timestamp, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            method(
                "composedPath",
                crate::fn_info!(event_proto_composed_path_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
                0,
                0,
                true,
            );
            accessor(
                proto,
                "returnValue",
                crate::fn_info!(get_return_value, 0; with_flags(crate::closure::FN_BUILTIN)),
                Some(crate::fn_info!(set_return_value, 1; with_flags(crate::closure::FN_BUILTIN))),
                true,
            );
            get(
                "bubbles",
                crate::fn_info!(get_bubbles, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "composed",
                crate::fn_info!(get_composed, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "eventPhase",
                crate::fn_info!(get_phase, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            accessor(
                proto,
                "cancelBubble",
                crate::fn_info!(get_cancel_bubble, 0; with_flags(crate::closure::FN_BUILTIN)),
                Some(crate::fn_info!(set_cancel_bubble, 1; with_flags(crate::closure::FN_BUILTIN))),
                true,
            );
            method(
                "stopPropagation",
                crate::fn_info!(event_proto_stop_propagation_thunk, 0; with_flags(crate::closure::FN_BUILTIN)),
                0,
                0,
                true,
            );
            get(
                "isTrusted",
                crate::fn_info!(get_trusted, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
        }
        "CustomEvent" => get(
            "detail",
            crate::fn_info!(get_detail, 0; with_flags(crate::closure::FN_BUILTIN)),
        ),
        "AbortController" => {
            get(
                "signal",
                crate::fn_info!(get_signal, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            method(
                "abort",
                crate::fn_info!(abort, 1; with_flags(crate::closure::FN_BUILTIN)),
                1,
                0,
                true,
            );
        }
        "AbortSignal" => {
            get(
                "aborted",
                crate::fn_info!(get_aborted, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            accessor(
                proto,
                "reason",
                crate::fn_info!(get_reason, 0; with_flags(crate::closure::FN_BUILTIN)),
                None,
                false,
            );
            method(
                "throwIfAborted",
                crate::fn_info!(throw_if_aborted, 0; with_flags(crate::closure::FN_BUILTIN)),
                0,
                0,
                false,
            );
            accessor(
                proto,
                "onabort",
                crate::fn_info!(get_onabort, 0; with_flags(crate::closure::FN_BUILTIN)),
                Some(crate::fn_info!(set_onabort, 1; with_flags(crate::closure::FN_BUILTIN))),
                true,
            );
        }
        "DOMException" => {
            get(
                "name",
                crate::fn_info!(get_dom_name, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "message",
                crate::fn_info!(get_dom_message, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
            get(
                "code",
                crate::fn_info!(get_dom_code, 0; with_flags(crate::closure::FN_BUILTIN)),
            );
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
