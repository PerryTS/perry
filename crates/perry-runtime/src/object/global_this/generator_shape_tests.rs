//! Intrinsic construction shares the iterator builder's literal funnel and
//! leaves executable atom registration authoritative, even after bootstrap.
use super::*;

#[test]
fn generator_intrinsic_keys_use_inline_shapes_and_late_program_atoms() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::ensure_gc_initialized();
            let _no_move = crate::gc::GcSuppressScope::new();
            let return_hash = crate::object::key_bytes_hash(b"return".as_ptr(), 6);
            assert!(crate::string::atom_lookup(b"return", return_hash).is_none());
            ensure_generator_intrinsics();
            // Native providers can populate globalThis before program literals.
            // A premature heap atom would compete with the later image atom.
            assert!(crate::string::atom_lookup(b"return", return_hash).is_none());
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
                        let string = key.as_string_ptr();
                        let header = crate::gc::header_from_trusted_user_ptr(string.cast());
                        assert!(
                            crate::string::is_property_atom(string)
                                || (*header).gc_flags & crate::gc::GC_FLAG_INTERNED != 0,
                            "fixed names use the existing literal interner"
                        );
                    }
                    let storage = crate::object::key_attrs::key_storage_slot(keys.arr(), i);
                    assert!(storage < shape.live_inline_slot_count);
                }
            }
            for name in [b"next".as_slice(), b"return", b"throw"] {
                let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
                let atom =
                    crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
                for slot in [
                    &crate::object::GENERATOR_PROTOTYPE_PTR,
                    &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
                ] {
                    let proto = slot.load(Ordering::Acquire) as usize;
                    let result = crate::object::shapes::layout_probe::perry_probe_get(
                        crate::object::shapes::ordinary_dir_addr(),
                        proto.wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
                        crate::value::nanbox_string_key(atom).to_bits(),
                        hash,
                    );
                    assert!(
                        crate::proxy::is_callable_function(result),
                        "the current shape must answer each protocol read"
                    );
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
