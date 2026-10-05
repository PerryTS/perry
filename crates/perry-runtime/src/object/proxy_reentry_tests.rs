use super::*;
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::canonical_key(name.as_bytes())
}
extern "C" fn reflecting_getter(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let receiver = crate::value::js_nanbox_get_pointer(this.as_f64()) as *const ObjectHeader;
    f64::from_bits(super::super::js_object_get_field_by_name(receiver, key("inherited")).bits())
}

extern "C" fn reflecting_trap(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    target: f64,
    property: f64,
    receiver: f64,
) -> f64 {
    crate::proxy::js_reflect_get(target, property, receiver)
}

#[test]
fn protowalk_proxy_reflect_getter_allows_nested_inherited_read() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let target = super::super::js_object_alloc_null_proto(0, 0);
    js_object_set_field_by_name(target, key("inherited"), 97.0);
    let getter = crate::closure::js_closure_alloc(crate::fn_info!(reflecting_getter, 0), 0);
    super::super::descriptor_state::set_accessor_descriptor(
        target as usize,
        "nested".to_string(),
        crate::object::descriptor_state::AccessorDescriptor {
            get: crate::value::js_nanbox_pointer(getter as i64).to_bits(),
            set: 0,
        },
    );
    let handler = js_object_alloc(0, 0);
    let trap = crate::closure::js_closure_alloc(crate::fn_info!(reflecting_trap, 3), 0);
    js_object_set_field_by_name(
        handler,
        key("get"),
        crate::value::js_nanbox_pointer(trap as i64),
    );
    let proxy = crate::proxy::js_proxy_new(
        crate::value::js_nanbox_pointer(target as i64),
        crate::value::js_nanbox_pointer(handler as i64),
    );
    let receiver = js_object_alloc(0, 0);
    object_set_static_prototype(receiver as usize, proxy.to_bits());
    assert_eq!(
        resolve_inherited_field(receiver as usize, key("nested")).map(|v| v.bits()),
        Some(97.0f64.to_bits())
    );
}

extern "C" fn reading_trap(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    target: f64,
    property: f64,
    receiver: f64,
) -> f64 {
    let name = unsafe {
        crate::string::header_str_checked(
            crate::value::JSValue::from_bits(property.to_bits()).as_string_ptr(),
        )
    };
    if name == Some("receiver") {
        return receiver;
    }
    if name == Some("outer") {
        let obj = crate::value::js_nanbox_get_pointer(receiver) as *const ObjectHeader;
        return f64::from_bits(
            super::super::js_object_get_field_by_name(obj, key("inherited")).bits(),
        );
    }
    crate::proxy::js_reflect_get(target, property, receiver)
}

#[test]
fn protowalk_proxy_trap_allows_a_new_get_on_its_receiver() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let target = super::super::js_object_alloc_null_proto(0, 0);
    js_object_set_field_by_name(target, key("inherited"), 101.0);
    let handler = js_object_alloc(0, 0);
    let trap = crate::closure::js_closure_alloc(crate::fn_info!(reading_trap, 3), 0);
    js_object_set_field_by_name(
        handler,
        key("get"),
        crate::value::js_nanbox_pointer(trap as i64),
    );
    let proxy = crate::proxy::js_proxy_new(
        crate::value::js_nanbox_pointer(target as i64),
        crate::value::js_nanbox_pointer(handler as i64),
    );
    let receiver = js_object_alloc(0, 0);
    object_set_static_prototype(receiver as usize, proxy.to_bits());
    assert_eq!(
        resolve_inherited_field(receiver as usize, key("outer")).map(|v| v.bits()),
        Some(101.0f64.to_bits())
    );
}

#[test]
fn proxy_reentry_transparent_target_preserves_resolution_mode_and_receiver() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let target = crate::object::js_object_alloc_null_proto(0, 0);
    js_object_set_field_by_name(target, key("inherited"), 101.0);
    let handler = js_object_alloc(0, 0);
    let trap = crate::closure::js_closure_alloc(crate::fn_info!(reading_trap, 3), 0);
    js_object_set_field_by_name(
        handler,
        key("get"),
        crate::value::js_nanbox_pointer(trap as i64),
    );
    let proxy = crate::proxy::js_proxy_new(
        crate::value::js_nanbox_pointer(target as i64),
        crate::value::js_nanbox_pointer(handler as i64),
    );
    let empty = js_object_alloc(0, 0);
    let transparent =
        crate::proxy::js_proxy_new(proxy, crate::value::js_nanbox_pointer(empty as i64));
    for holder in [proxy, transparent] {
        let receiver = js_object_alloc(0, 0);
        object_set_static_prototype(receiver as usize, holder.to_bits());
        assert_eq!(
            resolve_inherited_field(receiver as usize, key("outer")).map(|v| v.bits()),
            Some(101.0f64.to_bits())
        );
        assert_eq!(
            resolve_inherited_field(receiver as usize, key("receiver")).map(|v| v.bits()),
            Some(crate::value::js_nanbox_pointer(receiver as i64).to_bits())
        );
    }
}
