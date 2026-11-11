use super::*;
use crate::object::handle_expando::test_support::TestSmallHandleOwner;

const HANDLE: i64 = 0x31;

unsafe extern "C" fn fetch_kind(id: usize) -> u8 {
    if id == HANDLE as usize || id == HANDLE as usize + 1 {
        3
    } else {
        0
    }
}

fn constructor_read(handle: f64) -> u64 {
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_get_field_by_name(handle.to_bits() as *const ObjectHeader, key).bits()
}

#[test]
fn fetch_handle_own_constructor_precedes_intrinsic_including_undefined() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let owner = TestSmallHandleOwner::new(HANDLE);
    unsafe {
        crate::object::js_register_fetch_handle_kind_probe(fetch_kind);
    }
    let value = f64::from_bits(crate::value::HANDLE_TAG | HANDLE as u64);
    for constructor in [123.0, f64::from_bits(crate::value::TAG_UNDEFINED)] {
        crate::object::handle_expando::handle_expando_set(HANDLE, "constructor", constructor);
        assert_eq!(constructor_read(value), constructor.to_bits());
    }
    drop(owner);
}

#[test]
fn fetch_handle_constructor_follows_actual_prototype() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let _owner = TestSmallHandleOwner::new(HANDLE);
    unsafe {
        crate::object::js_register_fetch_handle_kind_probe(fetch_kind);
    }
    let proto = js_object_alloc(0, 0);
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    js_object_set_field_by_name(proto, key, 456.0);
    crate::object::prototype_chain::object_set_static_prototype(
        HANDLE as usize,
        crate::value::js_nanbox_pointer(proto as i64).to_bits(),
    );
    let value = f64::from_bits(crate::value::HANDLE_TAG | HANDLE as u64);
    assert_eq!(constructor_read(value), 456.0f64.to_bits());
    let _prototype_owner = TestSmallHandleOwner::new(HANDLE + 1);
    crate::object::handle_expando::handle_expando_set(HANDLE + 1, "constructor", 789.0);
    crate::object::prototype_chain::object_set_static_prototype(
        HANDLE as usize,
        crate::value::HANDLE_TAG | (HANDLE + 1) as u64,
    );
    assert_eq!(constructor_read(value), 789.0f64.to_bits());
}

#[test]
fn fetch_handle_symbol_presence_preserves_undefined_and_accessors() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let _owner = TestSmallHandleOwner::new(HANDLE);
    let value = f64::from_bits(crate::value::HANDLE_TAG | HANDLE as u64);
    unsafe {
        let symbol = crate::symbol::js_symbol_new_empty();
        crate::symbol::js_object_set_symbol_property(
            value,
            symbol,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(
            js_object_has_property(value, symbol).to_bits(),
            crate::value::TAG_TRUE
        );
        let accessor = crate::symbol::js_symbol_new_empty();
        crate::symbol::set_symbol_accessor_property(value, accessor, 0, 0);
        assert_eq!(
            js_object_has_property(value, accessor).to_bits(),
            crate::value::TAG_TRUE
        );
        let _prototype_owner = TestSmallHandleOwner::new(HANDLE + 1);
        let inherited = crate::symbol::js_symbol_new_empty();
        let prototype = f64::from_bits(crate::value::HANDLE_TAG | (HANDLE + 1) as u64);
        crate::symbol::js_object_set_symbol_property(
            prototype,
            inherited,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        crate::object::prototype_chain::object_set_static_prototype(
            HANDLE as usize,
            prototype.to_bits(),
        );
        assert_eq!(
            js_object_has_property(value, inherited).to_bits(),
            crate::value::TAG_TRUE
        );
    }
}
