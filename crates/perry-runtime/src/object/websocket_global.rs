//! WebSocket global constructor/prototype shape helpers.
//!
//! Kept out of `global_this.rs` so that file stays under the CI size gate.

use super::*;

fn install_data_property(
    obj: *mut ObjectHeader,
    name: &str,
    value: f64,
    attrs: super::PropertyAttrs,
) {
    if obj.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
    super::set_builtin_property_attrs(obj as usize, name.to_string(), attrs);
}

pub(super) fn install_constructor_shape(
    ctor: *mut crate::closure::ClosureHeader,
    proto_obj: *mut ObjectHeader,
) {
    if ctor.is_null() || proto_obj.is_null() {
        return;
    }
    let ctor_value = crate::value::js_nanbox_pointer(ctor as i64);
    install_data_property(
        proto_obj,
        "constructor",
        ctor_value,
        super::PropertyAttrs::new(true, false, true),
    );

    let const_attrs = super::PropertyAttrs::new(false, true, false);
    for (name, value) in [
        ("CONNECTING", 0.0),
        ("OPEN", 1.0),
        ("CLOSING", 2.0),
        ("CLOSED", 3.0),
    ] {
        install_data_property(ctor as *mut ObjectHeader, name, value, const_attrs);
        install_data_property(proto_obj, name, value, const_attrs);
    }
}

pub(super) fn install_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::install_proto_method;
    install_proto_method(
        proto_obj,
        "close",
        global_this_builtin_noop_thunk as *const u8,
        0,
    );
    install_proto_method(
        proto_obj,
        "send",
        global_this_builtin_noop_thunk as *const u8,
        1,
    );
    super::set_builtin_property_attrs(
        proto_obj as usize,
        "close".to_string(),
        super::PropertyAttrs::new(true, true, true),
    );
    super::set_builtin_property_attrs(
        proto_obj as usize,
        "send".to_string(),
        super::PropertyAttrs::new(true, true, true),
    );
}
