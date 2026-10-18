//! Computed-key probes must agree across string representations, layouts and
//! descriptor changes. The fixture is a real immutable key list, with slots.
use super::*;

unsafe fn record() -> f64 {
    crate::object::ensure_object_intrinsics();
    let names: &[&[u8]] = &[b"alpha", b"beta", b"longer_key"];
    let keys = crate::object::static_shapes::canonical_keys_for_names(names);
    let obj = crate::object::alloc_plain::alloc_plain_record_with_keys(3, keys);
    for slot in 0..3 {
        crate::object::js_object_set_field(obj, slot, crate::JSValue::number(slot as f64 + 1.0));
    }
    crate::js_nanbox_pointer(obj as i64)
}

#[test]
fn computed_store_uses_the_layout_for_heap_and_inline_keys() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = record();
        let dir = crate::object::shapes::ordinary_dir_addr();
        let obj_ptr = (obj.to_bits() & POINTER_MASK) as *mut ObjectHeader;
        let key = crate::JSValue::try_short_string(b"alpha").unwrap().bits();
        let before = crate::arena::arena_live_allocated_bytes();
        assert_eq!(perry_probe_set_keyed(dir, obj.to_bits(), key, 7.0), 1);
        assert_eq!(crate::arena::arena_live_allocated_bytes(), before);
        assert_eq!(perry_probe_get_keyed(dir, obj.to_bits(), key), 7.0);
        let key_ptr = crate::string::js_string_from_bytes(b"longer_key".as_ptr(), 10);
        let key = crate::js_nanbox_string(key_ptr as i64).to_bits();
        assert_eq!(perry_probe_set_keyed(dir, obj.to_bits(), key, 9.0), 1);
        assert_eq!(perry_probe_get_keyed(dir, obj.to_bits(), key), 9.0);
        // An unrelated descriptor does not veto this ordinary writable slot.
        crate::object::set_property_attrs(
            obj_ptr as usize,
            "beta".into(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert_eq!(perry_probe_set_keyed(dir, obj.to_bits(), key, 11.0), 1);
        let beta = crate::JSValue::try_short_string(b"beta").unwrap().bits();
        assert_eq!(perry_probe_set_keyed(dir, obj.to_bits(), beta, 99.0), 0);
        assert_eq!(perry_probe_get_keyed(dir, obj.to_bits(), beta), 2.0);
        crate::object::js_object_freeze(obj);
        assert_eq!(perry_probe_set_keyed(dir, obj.to_bits(), key, 99.0), 0);
        assert_eq!(perry_probe_get_keyed(dir, obj.to_bits(), key), 11.0);
    }
}

#[test]
fn uninterned_heap_key_proves_absence_without_allocating_an_atom() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = record();
        let text = b"__CK_no_atom_has_ever_named_this_key_20261018";
        let key = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let bits = crate::js_nanbox_string(key as i64).to_bits();
        let hash = crate::object::key_bytes_hash(text.as_ptr(), text.len());
        assert!(crate::string::atom_lookup(text, hash).is_none());
        let before = crate::arena::arena_live_allocated_bytes();
        let result = perry_probe_get_keyed(
            crate::object::shapes::ordinary_dir_addr(),
            obj.to_bits(),
            bits,
        );
        assert_eq!(result.to_bits(), crate::value::TAG_UNDEFINED);
        assert_eq!(crate::arena::arena_live_allocated_bytes(), before);
        assert!(crate::string::atom_lookup(text, hash).is_none());
        // Numeric/object keys still require complete indexed/coercion semantics.
        assert_eq!(
            perry_probe_set_keyed(
                crate::object::shapes::ordinary_dir_addr(),
                obj.to_bits(),
                0.0f64.to_bits(),
                4.0
            ),
            0
        );
    }
}
