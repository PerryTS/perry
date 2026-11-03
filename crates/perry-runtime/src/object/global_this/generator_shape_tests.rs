//! Linker-shared intrinsic keys keep one identity through late executable
//! literal registration, and the holder shapes keep protocol slots inline.
use super::*;

#[test]
fn generator_intrinsic_keys_share_images_and_inline_shapes() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::ensure_gc_initialized();
            let _no_move = crate::gc::GcSuppressScope::new();
            crate::object::layout_desc::seed_static_atoms();
            let return_atom = crate::object::runtime_layouts::BP_ATOM_RETURN.string;
            ensure_generator_intrinsics();
            // Native providers can populate globalThis before program literals.
            // The runtime and executable must still share the same image.
            let return_hash = crate::object::key_bytes_hash(b"return".as_ptr(), 6);
            assert_eq!(
                crate::string::atom_lookup(b"return", return_hash),
                Some(return_atom)
            );
            for slot in [
                &crate::object::GENERATOR_INTRINSIC_PROTO_PTR,
                &crate::object::GENERATOR_PROTOTYPE_PTR,
                &crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
                &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
            ] {
                let proto = slot.load(Ordering::Acquire) as *mut ObjectHeader;
                assert!(!proto.is_null());
                let shape = crate::object::shapes::object_shape_descriptor(proto).unwrap();
                let header = (proto as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                    as *const crate::gc::GcHeader;
                assert!(crate::gc::never_move::is_never_move(header));
                assert_eq!(
                    (*header).size as usize,
                    crate::gc::GC_HEADER_SIZE
                        + std::mem::size_of::<ObjectHeader>()
                        + shape.live_inline_slot_count as usize * 8,
                    "old intrinsic holders retain only their live inline slots"
                );
                let keys = crate::object::object_keys(proto);
                assert!(keys.count() >= 3);
                for i in 0..keys.count() {
                    let key = keys.get(i);
                    if key.is_string() {
                        assert!(
                            crate::string::is_property_atom(key.as_string_ptr()),
                            "fixed intrinsic keys are shared image atoms"
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
                if name == b"return" {
                    assert_eq!(atom as *const crate::StringHeader, return_atom);
                }
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

#[test]
fn generator_prototype_inherits_from_iterator_prototype() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| {
            crate::gc::ensure_gc_initialized();
            ensure_generator_intrinsics();
            let parent_of = |slot: &crate::object::RealmAtomicI64| {
                let proto = slot.load(Ordering::Acquire);
                crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(proto))
                    .to_bits()
            };
            let iterator_proto = crate::value::js_nanbox_pointer(
                crate::object::iterator_ctor::iterator_prototype_object() as i64,
            )
            .to_bits();
            assert_eq!(
                parent_of(&crate::object::GENERATOR_PROTOTYPE_PTR),
                iterator_proto
            );
            assert_ne!(
                parent_of(&crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR),
                iterator_proto,
                "async generators keep %AsyncIteratorPrototype%"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
