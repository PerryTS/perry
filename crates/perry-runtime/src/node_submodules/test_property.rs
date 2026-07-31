//! `MockTracker.property()` and `MockPropertyContext` support.

use std::cell::RefCell;

use super::*;

#[derive(Clone, Copy)]
struct OnceValue {
    access_index: u64,
    value: f64,
}

struct MockPropertyState {
    id: i64,
    target: f64,
    property: String,
    original_value: f64,
    original_accessor: Option<crate::object::AccessorDescriptor>,
    original_attrs: Option<crate::object::PropertyAttrs>,
    writable: bool,
    value: f64,
    accesses: f64,
    access_count: u64,
    once: Vec<OnceValue>,
    context: f64,
}

thread_local! {
    static PROPERTY_STATES: RefCell<Vec<MockPropertyState>> = const { RefCell::new(Vec::new()) };
}

fn set_object_field(object: f64, name: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let value = scope.root_nanbox_f64(value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let raw = raw_ptr_from_value(object.get_nanbox_f64());
    js_object_set_field_by_name(
        raw as *mut crate::object::ObjectHeader,
        key,
        value.get_nanbox_f64(),
    );
}

fn property_context_object(id: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let context = scope.root_nanbox_f64(boxed_ptr(js_object_alloc(0, 6)));
    let access_count = scope.root_nanbox_f64(closure_value_with_id(
        mock_property_access_count as *const u8,
        0,
        id,
    ));
    set_object_field(
        context.get_nanbox_f64(),
        "accessCount",
        access_count.get_nanbox_f64(),
    );
    let reset_accesses = scope.root_nanbox_f64(closure_value_with_id(
        mock_property_reset_accesses as *const u8,
        0,
        id,
    ));
    set_object_field(
        context.get_nanbox_f64(),
        "resetAccesses",
        reset_accesses.get_nanbox_f64(),
    );
    let mock_implementation = scope.root_nanbox_f64(closure_value_with_id(
        mock_property_implementation as *const u8,
        1,
        id,
    ));
    set_object_field(
        context.get_nanbox_f64(),
        "mockImplementation",
        mock_implementation.get_nanbox_f64(),
    );
    let mock_implementation_once = scope.root_nanbox_f64(rest_closure_value_with_id(
        mock_property_implementation_once as *const u8,
        1,
        id,
    ));
    set_object_field(
        context.get_nanbox_f64(),
        "mockImplementationOnce",
        mock_implementation_once.get_nanbox_f64(),
    );
    let restore = scope.root_nanbox_f64(closure_value_with_id(
        mock_property_restore as *const u8,
        0,
        id,
    ));
    set_object_field(
        context.get_nanbox_f64(),
        "restore",
        restore.get_nanbox_f64(),
    );

    let accesses_getter = scope.root_nanbox_f64(closure_value_with_id(
        mock_property_accesses as *const u8,
        0,
        id,
    ));
    let key = scope.root_nanbox_f64(string_value("accesses"));
    let raw = raw_ptr_from_value(context.get_nanbox_f64());
    unsafe {
        crate::object::ensure_key_in_keys_array(
            raw as *mut crate::object::ObjectHeader,
            raw_ptr_from_value(key.get_nanbox_f64()) as *mut crate::StringHeader,
        );
    }
    crate::object::set_accessor_descriptor(
        raw,
        "accesses".to_string(),
        crate::object::AccessorDescriptor {
            get: accesses_getter.get_nanbox_f64().to_bits(),
            set: 0,
        },
    );
    crate::object::set_property_attrs(
        raw,
        "accesses".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );
    context.get_nanbox_f64()
}

fn install_property_accessor(
    target: f64,
    property: &str,
    getter: f64,
    setter: f64,
    original_attrs: Option<crate::object::PropertyAttrs>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let getter = scope.root_nanbox_f64(getter);
    let setter = scope.root_nanbox_f64(setter);
    let key = scope.root_nanbox_f64(string_value(property));
    let raw = raw_ptr_from_value(target.get_nanbox_f64());
    unsafe {
        crate::object::ensure_key_in_keys_array(
            raw as *mut crate::object::ObjectHeader,
            raw_ptr_from_value(key.get_nanbox_f64()) as *mut crate::StringHeader,
        );
    }
    crate::object::set_accessor_descriptor(
        raw,
        property.to_string(),
        crate::object::AccessorDescriptor {
            get: getter.get_nanbox_f64().to_bits(),
            set: setter.get_nanbox_f64().to_bits(),
        },
    );
    let attrs = original_attrs.unwrap_or(crate::object::PropertyAttrs::new(true, true, true));
    crate::object::set_property_attrs(
        raw,
        property.to_string(),
        crate::object::PropertyAttrs::new(true, attrs.enumerable(), attrs.configurable()),
    );
}

fn restore_property_state(id: i64) {
    let restore = PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| {
                (
                    state.target,
                    state.property.clone(),
                    state.original_value,
                    state.original_accessor,
                    state.original_attrs,
                )
            })
    });
    let Some((target, property, original_value, original_accessor, original_attrs)) = restore
    else {
        return;
    };

    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let original_value = scope.root_nanbox_f64(original_value);
    let raw = raw_ptr_from_value(target.get_nanbox_f64());
    if let Some(accessor) = original_accessor {
        crate::object::set_accessor_descriptor(raw, property.clone(), accessor);
    } else {
        crate::object::clear_accessor_descriptor(raw, &property);
        set_object_field(
            target.get_nanbox_f64(),
            &property,
            original_value.get_nanbox_f64(),
        );
    }
    let raw = raw_ptr_from_value(target.get_nanbox_f64());
    if let Some(attrs) = original_attrs {
        crate::object::set_property_attrs(raw, property, attrs);
    } else {
        crate::object::clear_property_attrs(raw, &property);
    }
}

fn validate_on_access(value: f64, minimum: u64) -> u64 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return minimum;
    }
    if !crate::fs::validate::is_numeric(js) {
        throw_invalid_arg_type("onAccess", "number", value);
    }
    let number = crate::builtins::js_number_coerce(value);
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    if !number.is_finite()
        || number.fract() != 0.0
        || number < minimum as f64
        || number > MAX_SAFE_INTEGER
    {
        let message = format!(
            "The value of \"onAccess\" is out of range. It must be an integer >= {minimum}."
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_OUT_OF_RANGE");
    }
    number as u64
}

fn record_property_access(id: i64, access_type: &str, fallback: f64) -> f64 {
    let (value, accesses, scheduled_index) = PROPERTY_STATES.with(|states| {
        let states = states.borrow();
        let Some(state) = states.iter().find(|state| state.id == id) else {
            return (fallback, undefined_value(), None);
        };
        let scheduled = state
            .once
            .iter()
            .find(|once| once.access_index == state.access_count);
        (
            scheduled.map(|once| once.value).unwrap_or(fallback),
            state.accesses,
            scheduled.map(|once| once.access_index),
        )
    });
    if !is_array_value(accesses) {
        return fallback;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let accesses = scope.root_nanbox_f64(accesses);
    let access_type = scope.root_nanbox_f64(string_value(access_type));
    let stack_message = scope.root_nanbox_f64(string_value("Error"));
    let stack = crate::error::js_error_new_with_message(raw_ptr_from_value(
        stack_message.get_nanbox_f64(),
    ) as *mut crate::StringHeader);
    let stack = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(stack as i64));
    let record = scope.root_nanbox_f64(boxed_ptr(js_object_alloc(0, 3)));
    set_object_field(
        record.get_nanbox_f64(),
        "type",
        access_type.get_nanbox_f64(),
    );
    set_object_field(record.get_nanbox_f64(), "value", value.get_nanbox_f64());
    set_object_field(record.get_nanbox_f64(), "stack", stack.get_nanbox_f64());

    let accesses_ptr =
        raw_ptr_from_value(accesses.get_nanbox_f64()) as *mut crate::array::ArrayHeader;
    let accesses_ptr = crate::array::js_array_push_f64(accesses_ptr, record.get_nanbox_f64());
    let accesses_value = boxed_ptr(accesses_ptr);
    PROPERTY_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            state.accesses = accesses_value;
            if let Some(access_index) = scheduled_index {
                if let Some(index) = state
                    .once
                    .iter()
                    .position(|once| once.access_index == access_index)
                {
                    state.once.remove(index);
                }
            }
            state.access_count += 1;
        }
    });
    value.get_nanbox_f64()
}

extern "C" fn mock_property_get(closure: *const ClosureHeader) -> f64 {
    let id = closure_id(closure);
    let value = PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| state.value)
            .unwrap_or_else(undefined_value)
    });
    record_property_access(id, "get", value)
}

extern "C" fn mock_property_set(closure: *const ClosureHeader, value: f64) -> f64 {
    let id = closure_id(closure);
    let (writable, property) = PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| (state.writable, state.property.clone()))
            .unwrap_or((true, String::new()))
    });
    if !writable {
        let message = format!("The argument 'propertyName' {property} cannot be set");
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    }
    let value = record_property_access(id, "set", value);
    PROPERTY_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            state.value = value;
        }
    });
    undefined_value()
}

extern "C" fn mock_property_access_count(closure: *const ClosureHeader) -> f64 {
    let id = closure_id(closure);
    PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| state.access_count as f64)
            .unwrap_or(0.0)
    })
}

extern "C" fn mock_property_accesses(closure: *const ClosureHeader) -> f64 {
    let id = closure_id(closure);
    let accesses = PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| state.accesses)
            .unwrap_or_else(undefined_value)
    });
    if !is_array_value(accesses) {
        return boxed_ptr(crate::array::js_array_alloc(0));
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let accesses = scope.root_nanbox_f64(accesses);
    let ptr = raw_ptr_from_value(accesses.get_nanbox_f64()) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(ptr);
    boxed_ptr(crate::array::js_array_slice(ptr, 0, len as i32))
}

extern "C" fn mock_property_reset_accesses(closure: *const ClosureHeader) -> f64 {
    let id = closure_id(closure);
    let accesses = boxed_ptr(crate::array::js_array_alloc(0));
    PROPERTY_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            state.accesses = accesses;
            state.access_count = 0;
        }
    });
    undefined_value()
}

extern "C" fn mock_property_implementation(closure: *const ClosureHeader, value: f64) -> f64 {
    mock_property_set(closure, value)
}

extern "C" fn mock_property_implementation_once(
    closure: *const ClosureHeader,
    value: f64,
    rest: f64,
) -> f64 {
    let id = closure_id(closure);
    let next_access = PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| state.access_count)
            .unwrap_or(0)
    });
    let on_access = array_values(rest)
        .and_then(|values| values.first().copied())
        .unwrap_or_else(undefined_value);
    let access_index = validate_on_access(on_access, next_access);
    PROPERTY_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            if let Some(existing) = state
                .once
                .iter_mut()
                .find(|once| once.access_index == access_index)
            {
                existing.value = value;
            } else {
                state.once.push(OnceValue {
                    access_index,
                    value,
                });
            }
        }
    });
    undefined_value()
}

extern "C" fn mock_property_restore(closure: *const ClosureHeader) -> f64 {
    restore_property_state(closure_id(closure));
    undefined_value()
}

extern "C" fn mock_property_proxy_get(
    closure: *const ClosureHeader,
    target: f64,
    key: f64,
    receiver: f64,
) -> f64 {
    if value_to_string(key).as_deref() == Some("mock") {
        let id = closure_id(closure);
        return PROPERTY_STATES.with(|states| {
            states
                .borrow()
                .iter()
                .find(|state| state.id == id)
                .map(|state| state.context)
                .unwrap_or_else(undefined_value)
        });
    }
    crate::proxy::js_reflect_get(target, key, receiver)
}

extern "C" fn tracker_property_thunk(
    _closure: *const ClosureHeader,
    target: f64,
    property: f64,
    rest: f64,
) -> f64 {
    let values = array_values(rest).unwrap_or_default();
    let value_present = !values.is_empty();
    let value = values.first().copied().unwrap_or_else(undefined_value);
    create(target, property, value_present, value)
}

pub(super) fn tracker_property_value() -> f64 {
    let value = rest_closure_value_with_id(tracker_property_thunk as *const u8, 2, 0);
    let raw = raw_ptr_from_value(value);
    crate::object::set_builtin_closure_length(raw, 3);
    value
}

pub(super) fn create(target: f64, property: f64, value_present: bool, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let property_value = scope.root_nanbox_f64(property);
    let value = scope.root_nanbox_f64(value);
    object_target_addr(target.get_nanbox_f64());
    let property = property_name(property_value.get_nanbox_f64());
    let key = scope.root_nanbox_f64(string_value(&property));
    if crate::value::js_is_truthy(crate::object::js_object_has_own(
        target.get_nanbox_f64(),
        key.get_nanbox_f64(),
    )) == 0
    {
        let message =
            format!("The argument 'propertyName' {property} is not a property of the object");
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    }

    let original_value =
        scope.root_nanbox_f64(get_property_value(target.get_nanbox_f64(), &property));
    let raw = raw_ptr_from_value(target.get_nanbox_f64());
    let original_accessor = crate::object::get_accessor_descriptor(raw, &property);
    let original_attrs = crate::object::get_property_attrs(raw, &property);
    let attrs = original_attrs.unwrap_or(crate::object::PropertyAttrs::new(true, true, true));
    if !attrs.configurable() {
        let message = format!("Cannot redefine property: {property}");
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    }
    let writable = original_accessor.is_none() && attrs.writable();
    let mocked_value = scope.root_nanbox_f64(if value_present {
        value.get_nanbox_f64()
    } else {
        original_value.get_nanbox_f64()
    });

    let id = next_mock_id();
    let accesses = scope.root_nanbox_f64(boxed_ptr(crate::array::js_array_alloc(0)));
    let context = scope.root_nanbox_f64(property_context_object(id));
    let getter =
        scope.root_nanbox_f64(closure_value_with_id(mock_property_get as *const u8, 0, id));
    let setter =
        scope.root_nanbox_f64(closure_value_with_id(mock_property_set as *const u8, 1, id));

    PROPERTY_STATES.with(|states| {
        states.borrow_mut().push(MockPropertyState {
            id,
            target: target.get_nanbox_f64(),
            property: property.clone(),
            original_value: original_value.get_nanbox_f64(),
            original_accessor,
            original_attrs,
            writable,
            value: mocked_value.get_nanbox_f64(),
            accesses: accesses.get_nanbox_f64(),
            access_count: 0,
            once: Vec::new(),
            context: context.get_nanbox_f64(),
        });
    });
    install_property_accessor(
        target.get_nanbox_f64(),
        &property,
        getter.get_nanbox_f64(),
        setter.get_nanbox_f64(),
        original_attrs,
    );

    let handler = scope.root_nanbox_f64(boxed_ptr(js_object_alloc(0, 1)));
    let get_trap = scope.root_nanbox_f64(closure_value_with_id(
        mock_property_proxy_get as *const u8,
        3,
        id,
    ));
    set_object_field(handler.get_nanbox_f64(), "get", get_trap.get_nanbox_f64());
    crate::proxy::js_proxy_new(target.get_nanbox_f64(), handler.get_nanbox_f64())
}

pub(super) fn restore_all() {
    let ids = PROPERTY_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .map(|state| state.id)
            .collect::<Vec<_>>()
    });
    for id in ids {
        restore_property_state(id);
    }
}

pub(super) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROPERTY_STATES.with(|states| {
        for state in states.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.target);
            visitor.visit_nanbox_f64_slot(&mut state.original_value);
            visitor.visit_nanbox_f64_slot(&mut state.value);
            visitor.visit_nanbox_f64_slot(&mut state.accesses);
            visitor.visit_nanbox_f64_slot(&mut state.context);
            for once in state.once.iter_mut() {
                visitor.visit_nanbox_f64_slot(&mut once.value);
            }
            if let Some(accessor) = state.original_accessor.as_mut() {
                for bits in [&mut accessor.get, &mut accessor.set] {
                    if *bits != 0 {
                        let mut value = f64::from_bits(*bits);
                        visitor.visit_nanbox_f64_slot(&mut value);
                        *bits = value.to_bits();
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_with_value(value: f64) -> f64 {
        let object = boxed_ptr(js_object_alloc(0, 1));
        set_object_field(object, "value", value);
        object
    }

    fn proxy_context(proxy: f64) -> f64 {
        crate::proxy::js_proxy_get(proxy, string_value("mock"))
    }

    #[test]
    fn omitted_value_spies_while_explicit_undefined_replaces() {
        let target = object_with_value(5.0);
        let proxy = create(target, string_value("value"), false, undefined_value());
        assert_eq!(get_property_value(target, "value"), 5.0);
        let context = proxy_context(proxy);
        let count = object_property(context, b"accessCount").expect("accessCount");
        assert_eq!(
            js_closure_call0(raw_ptr_from_value(count) as *const ClosureHeader),
            1.0
        );
        restore_all();
        assert_eq!(get_property_value(target, "value"), 5.0);

        let target = object_with_value(5.0);
        create(target, string_value("value"), true, undefined_value());
        assert!(is_undefined_value(get_property_value(target, "value")));
        restore_all();
        assert_eq!(get_property_value(target, "value"), 5.0);
    }

    #[test]
    fn property_context_tracks_and_schedules_accesses() {
        let target = object_with_value(1.0);
        let proxy = create(target, string_value("value"), true, 2.0);
        let context = proxy_context(proxy);
        let implementation =
            object_property(context, b"mockImplementation").expect("mockImplementation");
        js_closure_call1(
            raw_ptr_from_value(implementation) as *const ClosureHeader,
            9.0,
        );
        let once =
            object_property(context, b"mockImplementationOnce").expect("mockImplementationOnce");
        crate::closure::js_closure_call2(
            raw_ptr_from_value(once) as *const ClosureHeader,
            4.0,
            1.0,
        );

        assert_eq!(get_property_value(target, "value"), 4.0);
        assert_eq!(get_property_value(target, "value"), 9.0);
        let count = object_property(context, b"accessCount").expect("accessCount");
        assert_eq!(
            js_closure_call0(raw_ptr_from_value(count) as *const ClosureHeader),
            3.0
        );
        let accesses = object_property(context, b"accesses").expect("accesses");
        assert_eq!(array_values(accesses).expect("access array").len(), 3);
        restore_all();
        assert_eq!(get_property_value(target, "value"), 1.0);
    }
}
