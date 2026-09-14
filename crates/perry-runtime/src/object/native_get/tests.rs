use super::*;
use crate::object::{
    js_object_alloc, js_object_create, js_object_delete_field, js_object_freeze,
    js_object_set_field_by_name,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::canonical_key(name.as_bytes())
}

fn boxed(object: *const ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(object as i64)
}

fn object(name: &str, value: f64) -> *mut ObjectHeader {
    let object = js_object_alloc(0, 0);
    js_object_set_field_by_name(object, key(name), value);
    object
}

struct Slow(bool);
impl Slow {
    fn enter() -> Self {
        Self(FORCE_SLOW.with(|value| value.replace(true)))
    }
}
impl Drop for Slow {
    fn drop(&mut self) {
        FORCE_SLOW.with(|value| value.set(self.0));
    }
}

fn get(object: f64, name: &str) -> f64 {
    unsafe { get_by_canonical_key(object, key(name)) }
}

fn differential(object: f64, name: &str, expected: f64) {
    assert_eq!(get(object, name).to_bits(), expected.to_bits());
    let _slow = Slow::enter();
    assert_eq!(get(object, name).to_bits(), expected.to_bits());
}

#[test]
fn own_data_is_served_and_matches_forced_slow_for_all_value_kinds() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let child = object("child", 1.0);
    let values = [
        7.0,
        -0.0,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        f64::from_bits(crate::value::TAG_NULL),
        f64::from_bits(crate::value::TAG_TRUE),
        crate::value::js_nanbox_string(key("a heap string value") as i64),
        boxed(child),
    ];
    for value in values {
        let object = object("value", value);
        let fast = unsafe { try_data_get_by_name(object, key("value")) };
        assert_eq!(fast.map(|value| value.bits()), Some(value.to_bits()));
        differential(boxed(object), "value", value);
    }
}

#[test]
fn prototype_data_mutation_shadow_delete_and_freeze_match_forced_slow() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let prototype = object("value", 7.0);
    let middle = js_object_create(boxed(prototype));
    let child = js_object_create(middle);
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    assert_eq!(
        unsafe { try_data_get_by_name(child_ptr, key("value")) }
            .unwrap()
            .bits(),
        7.0f64.to_bits()
    );
    differential(child, "value", 7.0);
    js_object_set_field_by_name(prototype, key("value"), 11.0);
    differential(child, "value", 11.0);
    js_object_set_field_by_name(child_ptr, key("value"), 13.0);
    differential(child, "value", 13.0);
    assert_eq!(js_object_delete_field(child_ptr, key("value")), 1);
    differential(child, "value", 11.0);
    js_object_freeze(boxed(prototype));
    assert!(unsafe { try_data_get_by_name(child_ptr, key("value")) }.is_some());
    differential(child, "value", 11.0);
    let replacement = object("value", 17.0);
    crate::object::prototype_chain::object_set_user_prototype(
        child_ptr as usize,
        boxed(replacement).to_bits(),
    );
    differential(child, "value", 17.0);
}

static GETTER_CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn getter(_closure: *const crate::closure::ClosureHeader) -> f64 {
    GETTER_CALLS.fetch_add(1, Ordering::Relaxed);
    47.0
}

fn install_getter(object: *mut ObjectHeader, name: &str, has_getter: bool) {
    let closure = crate::closure::js_closure_alloc(getter as *const u8, 0);
    crate::object::descriptor_state::set_accessor_descriptor(
        object as usize,
        name.to_string(),
        crate::object::descriptor_state::AccessorDescriptor {
            get: if has_getter {
                crate::value::js_nanbox_pointer(closure as i64).to_bits()
            } else {
                0
            },
            set: 0,
        },
    );
}

#[test]
fn accessor_bloom_guard_preserves_own_and_inherited_getter_calls() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    GETTER_CALLS.store(0, Ordering::Relaxed);
    let prototype = object("value", 1.0);
    install_getter(prototype, "value", true);
    let child = js_object_create(boxed(prototype));
    for receiver in [boxed(prototype), child] {
        assert!(
            unsafe { try_data_get_bytes(JSValue::from_bits(receiver.to_bits()), b"value") }
                .is_none()
        );
        differential(receiver, "value", 47.0);
    }
    assert_eq!(
        GETTER_CALLS.load(Ordering::Relaxed),
        4,
        "one getter invocation per Get"
    );
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    js_object_set_field_by_name(child_ptr, key("own"), 53.0);
    differential(child, "own", 53.0);
    assert_eq!(GETTER_CALLS.load(Ordering::Relaxed), 4);
    install_getter(prototype, "value", false);
    differential(
        boxed(prototype),
        "value",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

#[test]
fn unsupported_receivers_and_private_names_decline_and_preserve_results() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let target = object("value", 59.0);
    let handler = js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(boxed(target), boxed(handler));
    let array = crate::array::js_array_alloc(4);
    let array_value = crate::value::js_nanbox_pointer(array as i64);
    js_object_set_field_by_name(array as *mut ObjectHeader, key("value"), 59.0);
    let closure = crate::closure::js_closure_alloc(getter as *const u8, 0);
    js_object_set_field_by_name(closure as *mut ObjectHeader, key("value"), 59.0);
    for receiver in [
        proxy,
        array_value,
        crate::value::js_nanbox_pointer(closure as i64),
    ] {
        assert!(
            unsafe { try_data_get_bytes(JSValue::from_bits(receiver.to_bits()), b"value") }
                .is_none()
        );
        differential(receiver, "value", 59.0);
    }
    let private = object("#value", 61.0);
    assert!(unsafe { try_data_get_by_name(private, key("#value")) }.is_none());
    differential(boxed(private), "#value", 61.0);
    assert!(unsafe { try_data_get_bytes(JSValue::number(3.5), b"value") }.is_none());
}

#[cfg(feature = "regex-engine")]
#[test]
fn regexp_expandos_and_accessors_remain_on_the_exotic_path() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let regexp = crate::regex::js_regexp_new(key("x"), key("g"));
    let regexp_obj = regexp as *mut ObjectHeader;
    js_object_set_field_by_name(regexp_obj, key("value"), 67.0);
    assert!(unsafe { try_data_get_by_name(regexp_obj, key("value")) }.is_none());
    differential(boxed(regexp_obj), "value", 67.0);
    GETTER_CALLS.store(0, Ordering::Relaxed);
    install_getter(regexp_obj, "value", true);
    differential(boxed(regexp_obj), "value", 47.0);
    assert_eq!(GETTER_CALLS.load(Ordering::Relaxed), 2);
}

#[test]
fn borrowed_long_and_inline_keys_match_without_materializing_for_the_fast_read() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    for name in [
        "x",
        "a property name longer than the inline copy buffer; keep borrowing through this lookup",
    ] {
        let object = object(name, 71.0);
        assert_eq!(
            unsafe {
                try_data_get_bytes(JSValue::from_bits(boxed(object).to_bits()), name.as_bytes())
            }
            .unwrap()
            .bits(),
            71.0f64.to_bits()
        );
        differential(boxed(object), name, 71.0);
    }
}

#[test]
fn wide_spilled_and_deleted_keys_match_forced_slow() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let object = js_object_alloc(0, 0);
    for i in 0..100 {
        js_object_set_field_by_name(object, key(&format!("field{i}")), i as f64);
    }
    for i in [0, 4, 50, 99] {
        let name = format!("field{i}");
        assert!(unsafe { try_data_get_by_name(object, key(&name)) }.is_some());
        differential(boxed(object), &name, i as f64);
        assert_eq!(js_object_delete_field(object, key(&name)), 1);
        assert!(unsafe { try_data_get_by_name(object, key(&name)) }.is_none());
        differential(
            boxed(object),
            &name,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        js_object_set_field_by_name(object, key(&name), 101.0);
        differential(boxed(object), &name, 101.0);
    }
}

#[test]
fn class_statics_buffers_and_mapped_arguments_use_the_slow_path() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let class_id = 0x0065_5234;
        crate::object::js_register_class_id(class_id);
        crate::object::js_class_register_static_field(
            class_id,
            b"value".as_ptr(),
            5,
            73.0,
            std::ptr::null_mut(),
        );
        let class_value = f64::from_bits(crate::value::INT32_TAG | u64::from(class_id));
        assert!(try_data_get_bytes(JSValue::from_bits(class_value.to_bits()), b"value").is_none());
        differential(class_value, "value", 73.0);

        let buffer = crate::buffer::js_buffer_alloc(8, 11);
        let buffer_value = crate::value::js_nanbox_pointer(buffer as i64);
        assert!(
            try_data_get_bytes(JSValue::from_bits(buffer_value.to_bits()), b"length").is_none()
        );
        differential(buffer_value, "length", 8.0);

        let args = crate::array::js_array_alloc(1);
        crate::array::js_array_push(args, JSValue::number(1.0));
        let arguments = crate::object::js_arguments_object_alloc(
            crate::value::js_nanbox_pointer(args as i64),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            0,
        );
        let cell = crate::r#box::js_box_alloc(79.0);
        crate::object::js_arguments_object_map_index(arguments, 0, cell);
        assert!(try_data_get_by_name(arguments, key("0")).is_none());
        differential(boxed(arguments), "0", 79.0);
    }
}
