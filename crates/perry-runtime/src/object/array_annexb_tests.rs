use crate::{array, object, value};
fn key(s: &str) -> *mut crate::StringHeader {
    crate::string::canonical_key(s.as_bytes())
}
fn strval(s: &str) -> f64 {
    value::nanbox_string_key(key(s))
}
extern "C" fn getter(
    _c: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    43.0
}
#[test]
fn array_annexb_named_and_indexed_accessors_read() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let arr = array::js_array_alloc(8);
    array::js_array_push(arr, value::JSValue::number(1.0));
    array::js_array_push(arr, value::JSValue::number(2.0));
    let alias = arr;
    for n in 2..40 {
        array::js_array_push(arr, value::JSValue::number(n as f64));
    }
    assert_ne!(alias as usize, array::clean_arr_ptr(alias) as usize);
    let receiver = value::js_nanbox_pointer(alias as i64);
    let getter = value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(getter, 0),
        0,
    ) as i64);
    for name in ["x", "0"] {
        object::js_object_define_getter(receiver, strval(name), getter);
        assert!(array::array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0);
        assert!(
            object::get_accessor_descriptor(array::clean_arr_ptr(arr) as usize, name).is_some()
        );
        assert_eq!(
            object::js_object_get_field_by_name(arr.cast(), key(name)).bits(),
            43.0f64.to_bits(),
            "{name}: named ABI"
        );
        assert_eq!(
            array::js_array_get_index_or_string(arr, strval(name)),
            43.0,
            "{name}: array key ABI"
        );
        let mut cache: object::PicCacheSlot = std::ptr::null_mut();
        assert_eq!(
            object::js_object_get_field_ic(receiver.to_bits() as i64, key(name), 0, &mut cache),
            43.0,
            "{name}: IC ABI"
        );
        assert_eq!(
            unsafe {
                value::js_dynamic_object_get_property(receiver, name.as_ptr().cast(), name.len())
            },
            43.0,
            "{name}: dynamic ABI"
        );
        let found = object::js_object_lookup_getter(receiver, strval(name));
        assert!(object::value_is_callable(found));
    }
    assert_eq!(array::js_array_get_f64(arr, 0), 43.0);
}

extern "C" fn setter(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let arr = value::js_nanbox_get_pointer(this.as_f64()) as *mut object::ObjectHeader;
    object::js_object_set_field_by_name(arr, key("seen"), value);
    f64::from_bits(value::TAG_UNDEFINED)
}
#[test]
fn array_annexb_setter_write_through_growth_alias_uses_live_descriptor_owner() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let alias = array::js_array_alloc(2);
    let receiver = value::js_nanbox_pointer(alias as i64);
    let setter = value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(setter, 1),
        0,
    ) as i64);
    object::js_object_define_setter(receiver, strval("s"), setter);
    for n in 0..40 {
        array::js_array_push(alias, value::JSValue::number(n as f64));
    }
    assert_ne!(alias as usize, array::clean_arr_ptr(alias) as usize);
    assert!(object::get_accessor_descriptor(array::clean_arr_ptr(alias) as usize, "s").is_some());
    object::js_object_set_field_by_name(alias.cast(), key("s"), 51.0);
    assert_eq!(
        object::js_object_get_field_by_name(alias.cast(), key("seen")).bits(),
        51.0f64.to_bits()
    );
    assert!(object::value_is_callable(object::js_object_lookup_setter(
        receiver,
        strval("s")
    )));
}

fn grown_array() -> (*mut crate::ArrayHeader, f64) {
    let alias = array::js_array_alloc(2);
    for n in 0..40 {
        array::js_array_push(alias, value::JSValue::number(n as f64));
    }
    assert_ne!(alias as usize, array::clean_arr_ptr(alias) as usize);
    (alias, value::js_nanbox_pointer(alias as i64))
}
fn descriptor_field(receiver: f64, name: &str, field: &str) -> value::JSValue {
    let descriptor = object::js_object_get_own_property_descriptor(receiver, strval(name));
    assert_ne!(
        descriptor.to_bits(),
        value::TAG_UNDEFINED,
        "{name}: descriptor"
    );
    object::js_object_get_field_by_name(
        value::js_nanbox_get_pointer(descriptor) as *mut object::ObjectHeader,
        key(field),
    )
}
#[test]
fn array_growth_alias_reflection_and_delete_use_live_owner() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let (alias, receiver) = grown_array();
    let getter = value::js_nanbox_pointer(crate::closure::js_closure_alloc(
        crate::fn_info!(getter, 0),
        0,
    ) as i64);
    object::js_object_define_getter(receiver, strval("x"), getter);
    assert!(object::value_is_callable(f64::from_bits(
        descriptor_field(receiver, "x", "get").bits()
    )));
    let live = array::clean_arr_ptr(alias);
    object::set_property_attrs(
        live as usize,
        "x".to_owned(),
        object::PropertyAttrs::new(false, false, false),
    );
    assert_eq!(
        object::js_object_property_is_enumerable(receiver, strval("x")).to_bits(),
        value::JSValue::bool(false).bits()
    );
    assert_eq!(
        object::reflect_support::obj_value_attrs(receiver, strval("x")),
        Some((false, false))
    );
    assert_eq!(object::js_object_delete_field(alias.cast(), key("x")), 0);
    object::set_property_attrs(
        live as usize,
        "x".to_owned(),
        object::PropertyAttrs::new(false, true, true),
    );
    assert_eq!(object::js_object_delete_field(alias.cast(), key("x")), 1);
    assert!(object::js_object_get_field_by_name(alias.cast(), key("x")).is_undefined());
    assert_eq!(
        object::js_object_lookup_getter(receiver, strval("x")).to_bits(),
        value::TAG_UNDEFINED
    );
}
#[test]
fn array_growth_alias_freeze_marks_live_owner() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let (alias, receiver) = grown_array();
    object::js_object_set_field_by_name(alias.cast(), key("named"), 8.0);
    object::js_object_freeze(receiver);
    for name in ["0", "named", "length"] {
        assert_eq!(
            descriptor_field(receiver, name, "writable").bits(),
            value::JSValue::bool(false).bits(),
            "{name}: writable"
        );
        assert_eq!(
            descriptor_field(receiver, name, "configurable").bits(),
            value::JSValue::bool(false).bits(),
            "{name}: configurable"
        );
    }
}

extern "C" fn grow_during_descriptor_read(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let receiver = crate::closure::js_closure_get_capture_f64(closure, 0);
    let alias = value::js_nanbox_get_pointer(receiver) as *mut crate::ArrayHeader;
    for n in 0..40 {
        array::js_array_push(alias, value::JSValue::number(n as f64));
    }
    f64::from_bits(value::JSValue::bool(false).bits())
}
#[test]
fn array_growth_during_descriptor_getter_keeps_named_attributes_on_live_owner() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let alias = array::js_array_alloc(2);
    let receiver = value::js_nanbox_pointer(alias as i64);
    let descriptor = object::js_object_alloc(0, 0);
    let descriptor_value = value::js_nanbox_pointer(descriptor as i64);
    object::js_object_set_field_by_name(descriptor, key("value"), 71.0);
    let grow = crate::closure::js_closure_alloc(crate::fn_info!(grow_during_descriptor_read, 0), 1);
    crate::closure::js_closure_set_capture_f64(grow, 0, receiver);
    object::js_object_define_getter(
        descriptor_value,
        strval("enumerable"),
        value::js_nanbox_pointer(grow as i64),
    );
    object::js_object_define_property(receiver, strval("late"), descriptor_value);
    assert_ne!(alias as usize, array::clean_arr_ptr(alias) as usize);
    assert_eq!(
        object::js_object_get_field_by_name(alias.cast(), key("late")).bits(),
        71.0f64.to_bits()
    );
    assert_eq!(
        object::js_object_property_is_enumerable(receiver, strval("late")).to_bits(),
        value::JSValue::bool(false).bits()
    );
    assert_eq!(
        descriptor_field(receiver, "late", "enumerable").bits(),
        value::JSValue::bool(false).bits()
    );
}
