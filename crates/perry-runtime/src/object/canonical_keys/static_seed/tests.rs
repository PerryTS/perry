use super::*;

unsafe fn image_keys() -> &'static [PerryLayoutKey] {
    let descriptor = std::ptr::addr_of!(crate::object::runtime_layouts::ITERATOR_RESULT);
    std::slice::from_raw_parts((*descriptor).keys, (*descriptor).key_count as usize)
}

unsafe fn generic(keys: &[PerryLayoutKey], mapped: bool) -> CanonicalKeys {
    let list = crate::object::key_attrs::alloc_canonical_key_list(
        keys.len() as u32,
        true,
        mapped || keys.iter().any(|key| key.attrs != 0),
    );
    let values = crate::array::array_elements_ptr(list);
    let attrs = crate::object::key_attrs::keys_attrs(list);
    for (i, key) in keys.iter().enumerate() {
        *values.add(i) = JSValue::string_ptr((*key.atom).string.cast_mut()).bits();
        if !attrs.is_null() {
            crate::object::key_attrs::attrs_write(attrs, i as u32, key.attrs, &*key.atom);
            if mapped {
                crate::object::key_attrs::attrs_write_slot(attrs, i as u32, key.slot);
            }
        }
    }
    (*list).length = keys.len() as u32;
    canonicalize_owned(&SharedLayout::shape_cache_entry(), list, keys.len() as u32)
}

#[test]
fn image_and_generic_initializers_share_identity_in_both_orders() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    unsafe {
        let _no_gc = crate::gc::GcSuppressScope::new();
        let keys = image_keys();
        let proof = SharedLayout::shape_cache_entry();
        for image_first in [false, true] {
            reset_for_test();
            let first = if image_first {
                from_static_layout(&proof, keys, false).unwrap()
            } else {
                generic(keys, false)
            };
            let second = if image_first {
                generic(keys, false)
            } else {
                from_static_layout(&proof, keys, false).unwrap()
            };
            assert_eq!(first, second);
            // One list, one node, whichever initializer ran first: the
            // node is the identity holders carry (CKI S1).
            assert_eq!(first.node(), second.node());
            assert!(node_publishes_for_test(first.view()));
            let before = with_table_or((0, 0), |t| (t.minted, t.allocated_slots));
            assert_eq!(from_static_layout(&proof, keys, false), Some(first));
            assert_eq!(
                before,
                with_table_or((0, 0), |t| (t.minted, t.allocated_slots))
            );
            // Dynamic heap strings must reach the same identity too.
            let mut dynamic = CanonicalKeys::EMPTY;
            for entry in keys {
                let image = (*entry.atom).string;
                let string = crate::string::js_string_from_bytes(
                    crate::string::string_data(image),
                    (*image).byte_len,
                );
                dynamic = extend_key(&proof, dynamic, string);
            }
            assert_eq!(dynamic, first);
            assert_eq!(dynamic.node(), first.node());
        }
    }
}

#[test]
fn image_initializer_preserves_attributes_slots_and_unpublished_prefixes() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    unsafe {
        let _no_gc = crate::gc::GcSuppressScope::new();
        reset_for_test();
        let original = image_keys();
        assert_eq!(original.len(), 2);
        let keys = [
            PerryLayoutKey {
                atom: original[0].atom,
                slot: 1,
                attrs: crate::object::key_attrs::ENTRY_NON_WRITABLE,
                _pad: 0,
                off: crate::object::layout_desc::layout_key_off(1 as u32),
            },
            PerryLayoutKey {
                atom: original[1].atom,
                slot: 0,
                attrs: crate::object::key_attrs::ENTRY_NON_ENUMERABLE,
                _pad: 0,
                off: crate::object::layout_desc::layout_key_off(0 as u32),
            },
        ];
        let proof = SharedLayout::shape_cache_entry();
        let leaf = from_static_layout(&proof, &keys, true).unwrap();
        assert_eq!(leaf, generic(&keys, true));
        let prefix = from_static_layout(&proof, &keys[..1], true).unwrap();
        assert_eq!(prefix, generic(&keys[..1], true));
        assert_eq!(from_static_layout(&proof, &keys, true), Some(leaf));
        assert_ne!(leaf, from_static_layout(&proof, &keys, false).unwrap());
        assert_ne!(leaf, from_static_layout(&proof, original, false).unwrap());
        for (i, key) in keys.iter().enumerate() {
            let word = crate::object::key_attrs::key_word(leaf.as_ptr(), i as u32);
            assert_eq!(
                crate::object::key_attrs::token_of_word(word),
                u64::from(key.attrs) | ((u64::from(key.slot) + 1) << 8)
            );
        }
    }
}

#[test]
fn image_initializer_reuses_live_gc_witness_and_rebuilds_dead_prefix() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    reset_for_test();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let proof = SharedLayout::shape_cache_entry();
        let keys = image_keys();
        let leaf = {
            let _no_gc = crate::gc::GcSuppressScope::new();
            from_static_layout(&proof, &keys[..1], false).unwrap();
            from_static_layout(&proof, keys, false).unwrap()
        };
        let rooted = scope.root_raw_mut_ptr(leaf.as_ptr());
        crate::gc::js_gc_collect();
        let _no_gc = crate::gc::GcSuppressScope::new();
        assert_eq!(
            from_static_layout(&proof, keys, false).unwrap().as_ptr(),
            rooted.get_raw_mut_ptr::<ArrayHeader>()
        );
        let prefix = from_static_layout(&proof, &keys[..1], false).unwrap();
        assert_eq!(prefix, generic(&keys[..1], false));
        assert_eq!(Some(leaf), from_static_layout(&proof, keys, false));
    }
}

#[test]
fn non_image_keys_use_the_generic_initializer() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    unsafe {
        let _no_gc = crate::gc::GcSuppressScope::new();
        let string = crate::string::js_string_from_bytes(b"heap-only".as_ptr(), 9);
        let atom = crate::object::layout_desc::StaticAtom {
            hash: crate::object::runtime_layouts::atom_hash(b"heap-only"),
            string,
            kind: crate::codegen_abi::ATOM_KIND_STRING,
            _pad: [0; 3],
            index: 0,
        };
        let keys = [PerryLayoutKey {
            atom: &atom,
            slot: 0,
            attrs: 0,
            _pad: 0,
            off: crate::object::layout_desc::layout_key_off(0 as u32),
        }];
        assert!(from_static_layout(&SharedLayout::shape_cache_entry(), &keys, false).is_none());
        let symbol = crate::object::layout_desc::StaticAtom {
            hash: 0,
            string: std::ptr::null(),
            kind: crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
            _pad: [0; 3],
            index: 0,
        };
        let symbols = [PerryLayoutKey {
            atom: &symbol,
            slot: 0,
            attrs: 0,
            _pad: 0,
            off: crate::object::layout_desc::layout_key_off(0 as u32),
        }];
        assert!(from_static_layout(&SharedLayout::shape_cache_entry(), &symbols, false).is_none());
    }
}
