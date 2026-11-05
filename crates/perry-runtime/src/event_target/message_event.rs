//! Canonical MessageEvent values for both public construction and port delivery.
use super::*;
use crate::closure::{ClosureHeader, JsThis};
use crate::collection_iter::{is_entry_object, throw_type_error};

fn required(argc: usize, name: &str) {
    if argc == 0 {
        throw_type_error(&format!("{name}: 1 argument required, but 0 found."));
    }
}

fn receiver(this: JsThis) -> *mut ObjectHeader {
    let value = this.as_f64();
    if is_entry_object(value) {
        let obj = crate::value::js_nanbox_get_pointer(value) as *mut ObjectHeader;
        if state::has(obj, state::MESSAGE_DATA) {
            return obj;
        }
    }
    crate::fs::validate::throw_type_error_with_code("Illegal invocation", "ERR_INVALID_THIS")
}

fn option(value: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let name = key(name);
    value_as_ptr::<ObjectHeader>(value.get_nanbox_f64()).map_or(undefined_value(), |ptr| {
        js_object_get_field_by_name_f64(ptr, name)
    })
}

fn dom_string(value: f64, field: &str) -> *mut StringHeader {
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    if JSValue::from_bits(value.to_bits()).is_pointer() && crate::symbol::is_registered_symbol(raw)
    {
        throw_type_error(&format!("MessageEvent constructor: {field} is a symbol, which cannot be converted to a DOMString."));
    }
    crate::builtins::js_template_string_coerce(value)
}

fn port(value: f64, field: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let constructor =
        scope.root_nanbox_f64(crate::object::builtin_constructor_value("MessagePort"));
    if !crate::object::instanceof::ordinary_has_instance_prototype_walk(
        value.get_nanbox_f64(),
        constructor.get_nanbox_f64(),
    ) {
        let value = value.get_nanbox_f64();
        let shown = {
            let _inspect = crate::builtins::InspectCustomInspectGuard::new(false);
            if JSValue::from_bits(value.to_bits()).is_any_string() {
                format!("{:?}", crate::value::boxed_text::get_string_content(value))
            } else {
                crate::builtins::format_jsvalue(value, 0)
            }
        };
        throw_type_error(&format!("MessageEvent constructor: Expected eventInitDict.{field} (\"{shown}\") to be an instance of MessagePort."));
    }
}

fn sequence(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let output = scope.root_raw_mut_ptr(js_array_alloc(0));
    if is_undefined(value.get_nanbox_f64()) {
        return crate::object::js_object_freeze(boxed_ptr(output.get_raw_mut_ptr::<ArrayHeader>()));
    }
    if !is_entry_object(value.get_nanbox_f64()) {
        let shown = crate::builtins::format_jsvalue(value.get_nanbox_f64(), 0);
        throw_type_error(&format!(
            "MessageEvent constructor: eventInitDict.ports ({shown}) is not iterable."
        ));
    }
    // GetMethod once, including an overridden Array iterator. Do not use the
    // collection constructor's null-as-empty or array-copy shortcuts.
    let symbol = crate::symbol::well_known_symbol("iterator");
    let method = unsafe {
        crate::symbol::js_object_get_symbol_property(value.get_nanbox_f64(), boxed_ptr(symbol))
    };
    let method = scope.root_nanbox_f64(method);
    if !crate::proxy::is_callable_function(method.get_nanbox_f64()) {
        throw_type_error("MessageEvent constructor: eventInitDict.ports is not iterable.");
    }
    let iter = crate::collection_iter::call_with_this_capturing_throw(
        method.get_nanbox_f64(),
        value.get_nanbox_f64(),
        &[],
    )
    .unwrap_or_else(|error| crate::exception::rethrow_caught(error));
    let iter = scope.root_nanbox_f64(crate::symbol::js_iterator_result_validate(iter));
    let next = scope.root_nanbox_f64(crate::collection_iter::require_callable(
        option(iter.get_nanbox_f64(), b"next"),
        "next",
    ));
    let mut index = 0;
    loop {
        let result = crate::collection_iter::call_with_this_capturing_throw(
            next.get_nanbox_f64(),
            iter.get_nanbox_f64(),
            &[],
        )
        .unwrap_or_else(|error| crate::exception::rethrow_caught(error));
        let result = scope.root_nanbox_f64(crate::symbol::js_iterator_result_validate(result));
        if crate::value::js_is_truthy(option(result.get_nanbox_f64(), b"done")) != 0 {
            break;
        }
        let element = scope.root_nanbox_f64(option(result.get_nanbox_f64(), b"value"));
        port(element.get_nanbox_f64(), &format!("ports[{index}]"));
        output.with_mut_ptr(|ptr| {
            let updated = js_array_push_f64(ptr, element.get_nanbox_f64());
            output.set_raw_mut_ptr(updated);
        });
        index += 1;
    }
    crate::object::js_object_freeze(boxed_ptr(output.get_raw_mut_ptr::<ArrayHeader>()))
}

pub(crate) fn construct(args: &[f64]) -> f64 {
    required(args.len(), "MessageEvent constructor");
    let scope = crate::gc::RuntimeHandleScope::new();
    let args: Vec<_> = args.iter().map(|&v| scope.root_nanbox_f64(v)).collect();
    let options = args
        .get(1)
        .map_or(undefined_value(), |v| v.get_nanbox_f64());
    let options = scope.root_nanbox_f64(options);
    let event_type = scope.root_string_ptr(dom_string(args[0].get_nanbox_f64(), "type"));
    if !crate::collection_iter::is_null_or_undefined(options.get_nanbox_f64())
        && !is_entry_object(options.get_nanbox_f64())
    {
        let shown = crate::builtins::format_jsvalue(options.get_nanbox_f64(), 0);
        throw_type_error(&format!(
            "MessageEvent constructor: Expected {shown} to be one of: Null, Undefined, Object."
        ));
    }
    let mut flags = [false; 3];
    for (index, name) in [b"bubbles".as_slice(), b"cancelable", b"composed"]
        .iter()
        .enumerate()
    {
        flags[index] = crate::value::js_is_truthy(option(options.get_nanbox_f64(), name)) != 0;
    }
    let data = option(options.get_nanbox_f64(), b"data");
    let data = scope.root_nanbox_f64(if is_undefined(data) {
        null_value()
    } else {
        data
    });
    let mut strings = Vec::new();
    for name in [b"lastEventId".as_slice(), b"origin"] {
        let value = option(options.get_nanbox_f64(), name);
        strings.push(scope.root_string_ptr(if is_undefined(value) {
            key(b"")
        } else {
            dom_string(value, std::str::from_utf8(name).unwrap())
        }));
    }
    let ports = option(options.get_nanbox_f64(), b"ports");
    let ports = scope.root_nanbox_f64(sequence(ports));
    let source = option(options.get_nanbox_f64(), b"source");
    let source = scope.root_nanbox_f64(if is_undefined(source) {
        null_value()
    } else {
        source
    });
    if source.get_nanbox_u64() != TAG_NULL {
        port(source.get_nanbox_f64(), "source");
    }
    let event = allocate(crate::value::js_nanbox_string(
        event_type.get_raw_const_ptr::<StringHeader>() as i64,
    ));
    let event = scope.root_raw_mut_ptr(event);
    let set = |slot, value| event.with_mut_ptr(|ptr| state::set(ptr, slot, value));
    set(state::BUBBLES, bool_value(flags[0]));
    set(state::CANCELABLE, bool_value(flags[1]));
    set(state::COMPOSED, bool_value(flags[2]));
    set(state::MESSAGE_DATA, data.get_nanbox_f64());
    set(
        state::MESSAGE_ID,
        crate::value::js_nanbox_string(strings[0].get_raw_const_ptr::<StringHeader>() as i64),
    );
    set(
        state::MESSAGE_ORIGIN,
        crate::value::js_nanbox_string(strings[1].get_raw_const_ptr::<StringHeader>() as i64),
    );
    set(state::MESSAGE_SOURCE, source.get_nanbox_f64());
    set(state::MESSAGE_PORTS, ports.get_nanbox_f64());
    boxed_ptr(event.get_raw_mut_ptr::<ObjectHeader>())
}

fn allocate(event_type: f64) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event_type = scope.root_nanbox_f64(event_type);
    let _ = crate::object::js_get_global_this();
    let event = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    event.with_mut_ptr(|ptr| state::link(ptr, "MessageEvent"));
    event.with_mut_ptr(|ptr| {
        init_event_fields(
            ptr,
            event_type.get_nanbox_f64(),
            undefined_value(),
            b"MessageEvent",
            None,
        )
    });
    event.get_raw_mut_ptr()
}

pub(crate) fn delivery(data: f64) -> f64 {
    delivery_with_ports(data, undefined_value())
}

pub(crate) fn delivery_with_ports(data: f64, ports: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let data = scope.root_nanbox_f64(data);
    let ports = scope.root_nanbox_f64(ports);
    let event_type = string_value(b"message");
    let event = scope.root_raw_mut_ptr(allocate(event_type));
    let ports = scope.root_nanbox_f64(if is_undefined(ports.get_nanbox_f64()) {
        boxed_ptr(js_array_alloc(0))
    } else {
        ports.get_nanbox_f64()
    });
    crate::object::js_object_freeze(ports.get_nanbox_f64());
    event.with_mut_ptr(|ptr| state::set(ptr, state::MESSAGE_DATA, data.get_nanbox_f64()));
    for slot in [state::MESSAGE_ORIGIN, state::MESSAGE_ID] {
        let value = string_value(b"");
        event.with_mut_ptr(|ptr| state::set(ptr, slot, value));
    }
    event.with_mut_ptr(|ptr| state::set(ptr, state::MESSAGE_SOURCE, null_value()));
    event.with_mut_ptr(|ptr| state::set(ptr, state::MESSAGE_PORTS, ports.get_nanbox_f64()));
    boxed_ptr(event.get_raw_mut_ptr::<ObjectHeader>())
}

pub(crate) fn dispatch_state(event: f64, target: f64, active: bool) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let target = scope.root_nanbox_f64(target);
    let ptr = || value_as_ptr::<ObjectHeader>(event.get_nanbox_f64()).unwrap();
    if active {
        if event_bool_field(ptr(), state::IMMEDIATE_STOPPED) {
            return false;
        }
        state::set(ptr(), state::TARGET, target.get_nanbox_f64());
        state::set(ptr(), state::TRUSTED, bool_value(false));
    }
    state::set(ptr(), state::DISPATCHED, bool_value(active));
    true
}

extern "C" fn init(_: *const ClosureHeader, this: JsThis, args: f64) -> f64 {
    receiver(this);
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = scope.root_nanbox_f64(args);
    let array = || crate::value::js_nanbox_get_pointer(args.get_nanbox_f64()) as *const ArrayHeader;
    required(
        js_array_length(array()) as usize,
        "MessageEvent.initMessageEvent",
    );
    let values: Vec<_> = (0..8)
        .map(|i| scope.root_nanbox_f64(crate::array::js_array_get_f64(array(), i)))
        .collect();
    let options = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    for (index, name) in [
        "bubbles",
        "cancelable",
        "data",
        "origin",
        "lastEventId",
        "source",
        "ports",
    ]
    .iter()
    .enumerate()
    {
        let key = key(name.as_bytes());
        options.with_mut_ptr(|ptr| {
            js_object_set_field_by_name(ptr, key, values[index + 1].get_nanbox_f64())
        });
    }
    construct(&[
        values[0].get_nanbox_f64(),
        boxed_ptr(options.get_raw_mut_ptr::<ObjectHeader>()),
    ])
}

macro_rules! getter {
    ($name:ident, $slot:expr) => {
        extern "C" fn $name(_: *const ClosureHeader, this: JsThis) -> f64 {
            state::get(receiver(this), $slot)
        }
    };
}
getter!(data, state::MESSAGE_DATA);
getter!(origin, state::MESSAGE_ORIGIN);
getter!(last_event_id, state::MESSAGE_ID);
getter!(source, state::MESSAGE_SOURCE);
getter!(ports, state::MESSAGE_PORTS);

pub(super) fn install(proto: *mut ObjectHeader) {
    for (name, info) in [
        ("data", crate::fn_info!(#[realm("MessageEvent.prototype.data.<get>")] export crate::event_target::message_event::data, 0; with_flags(crate::closure::FN_BUILTIN))),
        ("origin", crate::fn_info!(#[realm("MessageEvent.prototype.origin.<get>")] export crate::event_target::message_event::origin, 0; with_flags(crate::closure::FN_BUILTIN))),
        ("lastEventId", crate::fn_info!(#[realm("MessageEvent.prototype.lastEventId.<get>")] export crate::event_target::message_event::last_event_id, 0; with_flags(crate::closure::FN_BUILTIN))),
        ("source", crate::fn_info!(#[realm("MessageEvent.prototype.source.<get>")] export crate::event_target::message_event::source, 0; with_flags(crate::closure::FN_BUILTIN))),
        ("ports", crate::fn_info!(#[realm("MessageEvent.prototype.ports.<get>")] export crate::event_target::message_event::ports, 0; with_flags(crate::closure::FN_BUILTIN))),
    ] {
        prototype::accessor(proto, name, info, None, true);
    }
    let method = crate::object::install_proto_method(proto, "initMessageEvent", crate::fn_info!(#[realm(MessageEvent.prototype.initMessageEvent)] export crate::event_target::message_event::init, 1; with_rest(0), with_flags(crate::closure::FN_BUILTIN)), 1);
    crate::object::set_builtin_property_attrs(
        proto as usize,
        "initMessageEvent".to_owned(),
        crate::object::PropertyAttrs::new(true, true, true),
    );
    crate::object::native_module::set_builtin_closure_length(
        crate::value::js_nanbox_get_pointer(method) as usize,
        1,
    );
}
