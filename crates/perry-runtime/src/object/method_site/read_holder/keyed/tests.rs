use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

fn atom(bytes: &[u8]) -> f64 {
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    let p = crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, 0);
    f64::from_bits(crate::value::STRING_TAG | p as u64)
}
fn boxed(p: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(p as i64)
}
fn put(o: &RuntimeHandle<'_>, key: f64, value: f64) {
    o.with_mut_ptr(|p| {
        crate::object::js_object_set_field_by_name(
            p,
            (key.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader,
            value,
        )
    });
}
fn read(slot: &mut *mut KeyedCache, o: &RuntimeHandle<'_>, key: f64) -> f64 {
    o.with_mut_ptr(|p| unsafe { js_object_get_field_by_key_site(slot, 0, p, key, boxed(p)) })
}
fn hit(slot: *mut KeyedCache, o: &RuntimeHandle<'_>, key: f64) -> Option<u64> {
    o.with_mut_ptr(|p| unsafe { answer(&*slot, p, key.to_bits()) })
}
fn inheriting<'a>(scope: &'a RuntimeHandleScope, proto: &RuntimeHandle<'_>) -> RuntimeHandle<'a> {
    let bits = proto.with_mut_ptr(|p| crate::object::js_object_create(boxed(p)));
    scope.root_raw_mut_ptr((bits.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader)
}

#[test]
fn keyed_read_matches_get_across_mutations() {
    if !super::super::super::run_with_fresh_worker_gate("keyed_read_matches_get_across_mutations") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let k = scope.root_nanbox_f64(atom(b"keyedAnswer"));
    let pad = atom(b"pad");
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    put(&proto, k.get_nanbox_f64(), 17.0);
    let mid = inheriting(&scope, &proto);
    put(&mid, pad, 1.0);
    let o = inheriting(&scope, &mid);
    put(&o, pad, 2.0);
    let mut slot = std::ptr::null_mut();
    assert_eq!(read(&mut slot, &o, k.get_nanbox_f64()), 17.0);
    assert_eq!(hit(slot, &o, k.get_nanbox_f64()), Some(17.0f64.to_bits()));
    let o2 = inheriting(&scope, &mid);
    put(&o2, pad, 2.0);
    assert_eq!(hit(slot, &o2, k.get_nanbox_f64()), Some(17.0f64.to_bits()));
    put(&proto, k.get_nanbox_f64(), 18.0);
    assert_eq!(hit(slot, &o, k.get_nanbox_f64()), Some(18.0f64.to_bits()));
    put(&o, k.get_nanbox_f64(), 19.0);
    assert_eq!(
        read(&mut slot, &o, k.get_nanbox_f64()),
        19.0,
        "own shadow must invalidate receiver proof"
    );
    o.with_mut_ptr(|p| {
        crate::object::js_object_delete_field(
            p,
            (k.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader,
        )
    });
    assert_eq!(read(&mut slot, &o, k.get_nanbox_f64()), 18.0);
    proto.with_mut_ptr(|p| {
        crate::object::js_object_delete_field(
            p,
            (k.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader,
        )
    });
    assert_eq!(
        read(&mut slot, &o, k.get_nanbox_f64()).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        hit(slot, &o, k.get_nanbox_f64()),
        Some(crate::value::TAG_UNDEFINED)
    );
    put(&mid, k.get_nanbox_f64(), 21.0);
    assert_eq!(
        read(&mut slot, &o, k.get_nanbox_f64()),
        21.0,
        "deep absence must validate every hop"
    );
    let key_now = k.get_nanbox_f64();
    let generic = o.with_mut_ptr(|p| {
        crate::object::js_object_get_field_by_name_f64(
            p,
            (key_now.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader,
        )
    });
    assert_eq!(hit(slot, &o, key_now), Some(generic.to_bits()));
    let mut marked = Vec::new();
    let mut mark = |v: f64| marked.push(v.to_bits() & crate::value::POINTER_MASK);
    unsafe {
        scan_roots(
            slot as usize,
            &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
        )
    };
    assert!(marked.contains(&(key_now.to_bits() & crate::value::POINTER_MASK)));
    mid.with_mut_ptr::<ObjectHeader, _>(|p| assert!(marked.contains(&(p as u64))));
}

#[test]
fn keyed_rotating_keys_stop_eviction_and_own_values_are_receiver_local() {
    if !super::super::super::run_with_fresh_worker_gate(
        "keyed_rotating_keys_stop_eviction_and_own_values_are_receiver_local",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let o = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    let other = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    let key = atom(b"ownKey");
    put(&o, key, 1.0);
    put(&other, key, 2.0);
    let mut slot = std::ptr::null_mut();
    assert_eq!(read(&mut slot, &o, key), 1.0);
    assert_eq!(hit(slot, &other, key), Some(2.0f64.to_bits()));
    for round in 0..3 {
        for i in 0..64 {
            let key = atom(format!("rotatingKey{i}").as_bytes());
            assert_eq!(
                read(&mut slot, &o, key).to_bits(),
                crate::value::TAG_UNDEFINED
            );
        }
        assert_eq!(unsafe { (*slot).evictions }, MAX_EVICTIONS, "round {round}");
    }
    WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
    // The exported front must leave these primary-owned words untouched.
    assert_eq!(read(&mut slot, &o, key), 1.0);
}

#[test]
fn keyed_symbol_read_matches_get_across_mutations() {
    if !super::super::super::run_with_fresh_worker_gate(
        "keyed_symbol_read_matches_get_across_mutations",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let symbol = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new_empty() });
    let other = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new_empty() });
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    let mid = inheriting(&scope, &proto);
    put(&mid, atom(b"pad"), 1.0);
    let o = inheriting(&scope, &mid);
    put(&o, atom(b"pad"), 2.0);
    let set = |obj: &RuntimeHandle<'_>, value| {
        obj.with_mut_ptr(|p| unsafe {
            crate::symbol::js_object_set_symbol_property(boxed(p), symbol.get_nanbox_f64(), value)
        })
    };
    let delete = |obj: &RuntimeHandle<'_>| {
        obj.with_mut_ptr(|p| unsafe {
            crate::symbol::js_object_delete_symbol_property(boxed(p), symbol.get_nanbox_f64())
        })
    };
    let mut slot = std::ptr::null_mut();
    let check = |slot: &mut *mut KeyedCache, obj: &RuntimeHandle<'_>| {
        let key = symbol.get_nanbox_f64();
        let generic = obj.with_mut_ptr(|p| unsafe {
            crate::symbol::js_object_get_symbol_property(boxed(p), key)
        });
        assert_eq!(read(slot, obj, key).to_bits(), generic.to_bits());
        assert_eq!(hit(*slot, obj, key), Some(generic.to_bits()));
        obj.with_mut_ptr(|p| unsafe {
            assert_eq!(
                js_dyn_index_get_site(slot, boxed(p), key).to_bits(),
                generic.to_bits()
            );
        });
    };
    set(&proto, 17.0);
    check(&mut slot, &o);
    set(&proto, 18.0);
    check(&mut slot, &o);
    set(&o, 19.0);
    check(&mut slot, &o);
    delete(&o);
    check(&mut slot, &o);
    delete(&proto);
    check(&mut slot, &o);
    set(&mid, 21.0);
    check(&mut slot, &o);
    assert_eq!(
        read(&mut slot, &o, other.get_nanbox_f64()).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    let own1 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    let own2 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    set(&own1, 31.0);
    set(&own2, 32.0);
    let mut own_slot = std::ptr::null_mut();
    check(&mut own_slot, &own1);
    assert_eq!(
        hit(own_slot, &own2, symbol.get_nanbox_f64()),
        Some(32.0f64.to_bits())
    );
    let mut marked = Vec::new();
    let mut mark = |v: f64| marked.push(v.to_bits() & crate::value::POINTER_MASK);
    unsafe {
        scan_roots(
            slot as usize,
            &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
        );
    }
    assert!(marked.contains(&(symbol.get_nanbox_u64() & crate::value::POINTER_MASK)));
}

#[test]
fn keyed_native_alias_retains_generic_forwarding() {
    if !super::super::super::run_with_fresh_worker_gate(
        "keyed_native_alias_retains_generic_forwarding",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let key = crate::string::canonical_key(b"alias_keyed");
        let object = crate::object::js_object_alloc(0xC000_2001, 1);
        crate::object::native_this_alias::register_this_to_handle_alias(
            boxed(object),
            crate::value::js_nanbox_pointer(42),
            false,
        );
        crate::object::js_object_set_field_by_name(object, key, 7.0);
        assert_eq!(
            object_shape_descriptor(object).unwrap().object_kind,
            crate::object::shapes::ShapeObjectKind::OrdinaryNativeAlias,
        );
        assert!(dynamic_key_walk(
            object,
            KeyRef::Name {
                word: crate::value::STRING_TAG | key as u64,
                bytes: b"alias_keyed"
            },
        )
        .is_none());
    }
}

#[test]
fn region_consumes_keyed_absent_and_inherited_entries() {
    if !super::super::super::run_with_fresh_worker_gate(
        "region_consumes_keyed_absent_and_inherited_entries",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    crate::object::builtin_prototype_value("Object");
    let x = scope.root_nanbox_f64(atom(b"regionX"));
    let span = scope.root_nanbox_f64(atom(b"regionSpan"));
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    put(&proto, x.get_nanbox_f64(), 7.0);
    let mid = inheriting(&scope, &proto);
    let obj = inheriting(&scope, &mid);
    let mut slots = [std::ptr::null_mut(); 4];
    let mut values = [0u64; 2];
    assert_eq!(
        obj.with_mut_ptr(|p| unsafe {
            js_region_holder_read(
                slots.as_mut_ptr(),
                boxed(p),
                [x.get_nanbox_u64(), span.get_nanbox_u64()].as_ptr(),
                2,
                values.as_mut_ptr(),
                1,
            )
        }),
        0
    );
    read(&mut slots[0], &obj, x.get_nanbox_f64());
    read(&mut slots[1], &obj, span.get_nanbox_f64());
    let mut read_region = || {
        let keys = [x.get_nanbox_u64(), span.get_nanbox_u64()];
        obj.with_mut_ptr(|p| unsafe {
            js_region_holder_read(
                slots.as_mut_ptr(),
                boxed(p),
                keys.as_ptr(),
                2,
                values.as_mut_ptr(),
                1,
            )
        })
    };
    assert_eq!(
        read_region(),
        1,
        "the region must actually hit, not pass through generic Get"
    );
    assert_eq!(read_region(), 1);
    drop(read_region);
    assert_eq!(values, [7.0f64.to_bits(), crate::value::TAG_UNDEFINED]);
    assert_eq!(
        hit(slots[1], &obj, span.get_nanbox_f64()),
        Some(crate::value::TAG_UNDEFINED)
    );
    put(&mid, span.get_nanbox_f64(), 3.0);
    assert_eq!(
        hit(slots[1], &obj, span.get_nanbox_f64()),
        None,
        "an intermediate prototype key add invalidates absence"
    );
    let keys = [x.get_nanbox_u64(), span.get_nanbox_u64()];
    read(&mut slots[0], &obj, x.get_nanbox_f64());
    read(&mut slots[1], &obj, span.get_nanbox_f64());
    assert_eq!(
        obj.with_mut_ptr(|p| unsafe {
            js_region_holder_read(
                slots.as_mut_ptr(),
                boxed(p),
                keys.as_ptr(),
                2,
                values.as_mut_ptr(),
                1,
            )
        }),
        1
    );
    assert_eq!(values, [7.0f64.to_bits(), 3.0f64.to_bits()]);
    put(
        &obj,
        x.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_TRUE),
    );
    read(&mut slots[0], &obj, x.get_nanbox_f64());
    read(&mut slots[1], &obj, span.get_nanbox_f64());
    assert_eq!(
        obj.with_mut_ptr(|p| unsafe {
            js_region_holder_read(
                slots.as_mut_ptr(),
                boxed(p),
                keys.as_ptr(),
                2,
                values.as_mut_ptr(),
                1,
            )
        }),
        0,
        "non-numeric data refuses before operators"
    );
}

#[test]
fn keyed_runtime_string_identity_reuses_the_published_atom_proof() {
    if !super::super::super::run_with_fresh_worker_gate(
        "keyed_runtime_string_identity_reuses_the_published_atom_proof",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let name = b"runtimeAliasAnswer";
    let key = scope.root_nanbox_f64(atom(name));
    let alias = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let alias = scope.root_nanbox_f64(f64::from_bits(crate::value::STRING_TAG | alias as u64));
    assert_ne!(alias.get_nanbox_u64(), key.get_nanbox_u64());
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
    put(&proto, key.get_nanbox_f64(), 17.0);
    let o = inheriting(&scope, &proto);
    let mut slot = std::ptr::null_mut();
    assert_eq!(read(&mut slot, &o, key.get_nanbox_f64()), 17.0);
    assert_eq!(hit(slot, &o, alias.get_nanbox_f64()), None, "identity miss");
    assert_eq!(read(&mut slot, &o, alias.get_nanbox_f64()), 17.0);
    put(&proto, key.get_nanbox_f64(), 18.0);
    assert_eq!(read(&mut slot, &o, alias.get_nanbox_f64()), 18.0);
    put(&o, key.get_nanbox_f64(), 21.0);
    assert_eq!(read(&mut slot, &o, alias.get_nanbox_f64()), 21.0);
    o.with_mut_ptr(|p| {
        crate::object::js_object_delete_field(
            p,
            (key.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader,
        )
    });
    assert_eq!(read(&mut slot, &o, alias.get_nanbox_f64()), 18.0);
    proto.with_mut_ptr(|p| {
        crate::object::js_object_delete_field(
            p,
            (key.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader,
        )
    });
    assert_eq!(
        read(&mut slot, &o, alias.get_nanbox_f64()).to_bits(),
        crate::value::TAG_UNDEFINED,
        "negative control: canonical identity must not preserve a deleted lane"
    );
}

#[test]
fn keyed_position_uses_public_namespace_and_most_derived_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let key = atom(b"shadowed");
        let keys = crate::array::js_array_alloc_key_list(4, false);
        let keys = crate::array::js_array_push(keys, crate::JSValue::from_bits(key.to_bits()));
        let keys = crate::array::js_array_push(keys, crate::JSValue::from_bits(key.to_bits()));
        let keys = crate::object::key_attrs::ensure_owned_attrs(keys, 2);
        let k = KeyRef::Name {
            word: key.to_bits(),
            bytes: b"shadowed",
        };
        assert_eq!(k.position(keys, 2), Some(1), "derived declaration wins");
        let attrs = crate::object::key_attrs::keys_attrs(keys);
        crate::object::key_attrs::attrs_set_owned(
            keys,
            attrs,
            1,
            crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
        );
        assert_eq!(
            k.position(keys, 2),
            Some(0),
            "private spelling is not a property"
        );
        crate::object::key_attrs::attrs_set_owned(
            keys,
            attrs,
            0,
            crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
        );
        assert_eq!(
            k.position(keys, 2),
            None,
            "negative control hides both entries"
        );
    }
}
