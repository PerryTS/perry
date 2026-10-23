//! Intrinsic construction must admit every protocol key to the ordinary
//! inherited probe, including keys absent from a program's literal pool.
use super::*;

#[test]
fn generator_intrinsic_keys_are_atoms_in_inline_shape_slots() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::ensure_gc_initialized();
            let _no_move = crate::gc::GcSuppressScope::new();
            ensure_generator_intrinsics();
            for slot in [
                &crate::object::GENERATOR_INTRINSIC_PROTO_PTR,
                &crate::object::GENERATOR_PROTOTYPE_PTR,
                &crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
                &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
            ] {
                let proto = slot.load(Ordering::Acquire) as *mut ObjectHeader;
                assert!(!proto.is_null());
                let shape = crate::object::shapes::object_shape_descriptor(proto).unwrap();
                let keys = crate::object::object_keys(proto);
                assert!(keys.count() >= 3);
                for i in 0..keys.count() {
                    let key = keys.get(i);
                    if key.is_string() {
                        assert!(
                            crate::string::is_property_atom(key.as_string_ptr()),
                            "every fixed intrinsic name must be a property atom"
                        );
                    }
                    let storage = crate::object::key_attrs::key_storage_slot(keys.arr(), i);
                    assert!(
                        storage < shape.live_inline_slot_count,
                        "every protocol key must be an inline shape slot"
                    );
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
