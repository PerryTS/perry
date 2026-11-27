//! A shadowed holder's layout key that a later define moved to its overflow.
use super::*;
use crate::object::{js_object_alloc, js_object_set_field_by_name};

extern "C" fn getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn key(text: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

fn boxed(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

#[test]
fn an_accessor_redefined_after_a_delete_is_answered_by_its_holder() {
    crate::gc::initialize_runtime_fixture();
    let _no_move = crate::gc::GcSuppressScope::new();
    let parent = js_object_alloc(0, 0);
    js_object_set_field_by_name(parent, key("m"), 1.0);
    let holder = js_object_alloc(0, 0);
    js_object_set_field_by_name(holder, key("m"), 2.0);
    js_object_set_field_by_name(holder, key("n"), 3.0);
    crate::object::js_object_set_prototype_of(boxed(holder), boxed(parent));
    crate::object::js_object_delete_field_value(boxed(holder), key("n"));

    let descriptor = js_object_alloc(0, 0);
    let get = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
    js_object_set_field_by_name(
        descriptor,
        key("get"),
        crate::value::js_nanbox_pointer(get as i64),
    );
    js_object_set_field_by_name(
        descriptor,
        key("configurable"),
        f64::from_bits(crate::value::TAG_TRUE),
    );
    crate::object::js_object_define_property(
        boxed(holder),
        crate::value::js_nanbox_string(key("m") as i64),
        boxed(descriptor),
    );

    let found = unsafe { chain_method(holder, &MethodKey::bytes(b"m")) };
    assert!(
        matches!(found, ChainMethod::Get { holder: h } if h == holder.cast_const()),
        "the accessor's holder must answer, not the parent's data `m`"
    );
}
