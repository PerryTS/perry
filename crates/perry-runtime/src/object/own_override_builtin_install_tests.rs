//! #10697: the runtime's own builtin installs do not arm the own-override
//! guard unless their owner is a Map, Set or Date cell; user installs do.

use super::own_override::ARMS_NOTED;
use crate::object::descriptor_state::{define_builtin_data_property, PropertyAttrs};

extern "C" fn intrinsic_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    0.0
}

static INTRINSIC_FIXTURE: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
    intrinsic_fixture as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
);

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn arms() -> usize {
    ARMS_NOTED.with(std::cell::Cell::get)
}

/// Sabotage: installing through `js_object_set_field_by_name` directly, as
/// `define_builtin_data_property` did, makes `arms` read 1.
#[test]
fn a_builtin_install_on_an_intrinsic_does_not_arm_the_guard() {
    let _lock = crate::gc::global_side_table_test_lock();
    let ctor = crate::closure::js_closure_alloc(&INTRINSIC_FIXTURE, 0);
    let before = arms();
    define_builtin_data_property(
        ctor as *mut crate::object::ObjectHeader,
        key("from"),
        1.0,
        "from".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    assert_eq!(
        arms() - before,
        0,
        "a builtin static on a constructor must not arm"
    );
    // `Array.prototype` is an array cell and takes `constructor` this way.
    let proto = crate::array::js_array_alloc(0);
    let before = arms();
    define_builtin_data_property(
        proto as *mut crate::object::ObjectHeader,
        key("constructor"),
        1.0,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    assert_eq!(
        arms() - before,
        0,
        "`constructor` on an array prototype must not arm"
    );
    // A Map owner is a kind the guard answers from the flag: still armed.
    let map = crate::map::js_map_alloc(0);
    let before = arms();
    define_builtin_data_property(
        map as *mut crate::object::ObjectHeader,
        key("size2"),
        1.0,
        "size2".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    assert!(
        arms() > before,
        "a builtin install onto a Map must still arm"
    );
    // The scope must not outlive the install.
    let before = arms();
    crate::object::js_object_set_field_by_name(
        map as *mut crate::object::ObjectHeader,
        key("get"),
        1.0,
    );
    assert!(
        arms() > before,
        "an own `get` on a Map must still arm the guard"
    );
}
