//! Shape-table unit suites, split out of `object/shapes.rs` to keep it under
//! the repo's 2000-line-per-file cap. Moved verbatim.

use super::*;

#[cfg(test)]
mod c3c_tests {
    use super::*;

    fn key(name: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    #[test]
    fn repeated_class_evaluations_reuse_the_class_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            const CID: u32 = 0x5268;
            let scope = crate::gc::RuntimeHandleScope::new();
            let first = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 0));
            let second = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 0));

            let ordinary =
                first.with_const_ptr::<crate::ObjectHeader, _>(|obj| object_shape_id(obj));
            assert_eq!(
                ordinary,
                second.with_const_ptr::<crate::ObjectHeader, _>(|obj| object_shape_id(obj)),
                "test premise: equal class evaluations start with one shape"
            );

            first.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                transition_object_shape_to_class(obj);
            });
            second.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                transition_object_shape_to_class(obj);
            });

            let first_class =
                first.with_const_ptr::<crate::ObjectHeader, _>(|obj| object_shape_id(obj));
            let second_class =
                second.with_const_ptr::<crate::ObjectHeader, _>(|obj| object_shape_id(obj));
            assert_ne!(
                ordinary, first_class,
                "becoming a class must invalidate guards"
            );
            assert_eq!(
                first_class, second_class,
                "equivalent class evaluations must not mint unbounded descriptors"
            );
        }
    }

    /// #6759 C3c: ids come from the dedicated range (disjoint from real and
    /// builtin class ids), are stable per exact descriptor facts, and distinct
    /// across identities.
    #[test]
    fn shape_ids_are_range_disjoint_and_stable() {
        let _lock = crate::gc::global_side_table_test_lock();
        let a: usize = 0xC3C0_0000_0000_1000;
        let b: usize = 0xC3C0_0000_0000_2000;
        let ida = shape_id_for_keys_ensure(a as *const ArrayHeader, 4);
        let idb = shape_id_for_keys_ensure(b as *const ArrayHeader, 4);
        assert!(is_shape_id(ida) && is_shape_id(idb));
        assert_ne!(ida, idb);
        assert_eq!(shape_id_for_keys_ensure(a as *const ArrayHeader, 4), ida);
        // Real class-id space must never classify as a shape id.
        assert!(!is_shape_id(0));
        assert!(!is_shape_id(1));
        assert!(!is_shape_id(0x7FFF_FF30));
        assert!(!is_shape_id(0xFFFF_0005));
        shape_drop(a as *const ArrayHeader);
        shape_drop(b as *const ArrayHeader);
        test_drop_shape_descriptors(a);
        test_drop_shape_descriptors(b);
    }

    /// #6759 C3 rung 2: the codegen-facing allocator receives the id minted
    /// beside its canonical keys global and installs it before the newborn
    /// instance is published to user code. No by-name lookup is allowed in
    /// this fixture: observing a stamp therefore proves it was present at
    /// birth rather than lazily self-healed by rung 1.
    #[test]
    fn compiled_class_allocator_stamps_the_canonical_shape_at_birth() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7902;
        let packed = b"birth_a\0birth_b";
        let keys =
            crate::object::js_build_class_keys_array(CID, 2, packed.as_ptr(), packed.len() as u32);
        let shape_id = js_object_shape_id_for_keys(keys as usize as u64, 2);
        assert!(
            is_shape_id(shape_id),
            "module init must mint a real ShapeId"
        );

        let obj =
            crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 2, keys, shape_id);
        let birth_word = unsafe { (*obj).parent_class_id };
        assert_eq!(
            birth_word, shape_id,
            "a fresh compiled class instance waited for a by-name lookup to stamp"
        );
        assert_eq!(
            unsafe { crate::object::object_keys_array(obj) },
            keys,
            "the stamp and canonical keys global must describe the same shape"
        );
    }

    /// A module-init ShapeId is already a complete proof of the immutable
    /// keys edge and live inline bound. The allocation fast path must be able
    /// to install that proof directly on a newborn without publishing the
    /// same facts through the reverse shape index again.
    #[test]
    fn preinstalled_shape_fast_path_stamps_matching_newborn() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7903;
        let packed = b"direct_a\0direct_b";
        let keys =
            crate::object::js_build_class_keys_array(CID, 2, packed.as_ptr(), packed.len() as u32);
        let shape_id = js_object_shape_id_for_keys(keys as usize as u64, 2);
        let payload = std::mem::size_of::<crate::object::ObjectHeader>()
            + crate::object::INLINE_SLOT_FLOOR * std::mem::size_of::<crate::value::JSValue>();
        let obj = crate::arena::arena_alloc_gc(payload, 8, crate::gc::GC_TYPE_OBJECT)
            as *mut crate::object::ObjectHeader;

        unsafe {
            (*obj).class_id = CID;
            (*obj).parent_class_id = 0;
            (*obj).meta = std::ptr::null_mut();
            let fields = (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *mut crate::value::JSValue;
            // GC_STORE_AUDIT(INIT): freshly allocated inline slots, filled with a
            // non-pointer immediate before the object is reachable from anything.
            for index in 0..crate::object::INLINE_SLOT_FLOOR {
                std::ptr::write(fields.add(index), crate::value::JSValue::undefined());
            }
            crate::gc::layout_init_pointer_free(obj as *mut u8);

            assert!(try_birth_stamp_preinstalled_shape(obj, shape_id, keys, 2));
            assert_eq!((*obj).parent_class_id, shape_id);
            assert_eq!(crate::object::object_keys_array(obj), keys);
            debug_assert_object_shape_parity(obj);
        }
    }

    /// Learned/hidden inline capacity can legitimately exceed the public key
    /// count. A module-init id for the narrow shape must fail closed, and the
    /// existing allocator fallback must publish and retain the exact wider
    /// descriptor rather than stamping the supplied id anyway.
    #[test]
    fn preinstalled_shape_live_bound_mismatch_uses_exact_fallback() {
        let _lock = crate::gc::global_side_table_test_lock();
        const CID: u32 = 0x0C3C_7904;
        let packed = b"wide_a\0wide_b";
        let keys =
            crate::object::js_build_class_keys_array(CID, 2, packed.as_ptr(), packed.len() as u32);
        let narrow_id = js_object_shape_id_for_keys(keys as usize as u64, 2);

        let obj =
            crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 3, keys, narrow_id);
        let actual_id = unsafe { (*obj).parent_class_id };
        assert_ne!(
            actual_id, narrow_id,
            "a narrow module ShapeId must not describe a wider allocation"
        );
        let descriptor = shape_descriptor_by_id(actual_id)
            .expect("the widened fallback must publish an exact descriptor");
        assert_eq!(descriptor.keys, keys as u64);
        assert_eq!(descriptor.logical_key_count, 2);
        assert_eq!(descriptor.live_inline_slot_count, 3);
        unsafe { debug_assert_object_shape_parity(obj) };
    }

    /// #6759 C3c stamp invariant on a REAL object through the real
    /// write/read paths: a read resolution stamps a shape id into the
    /// plain object's `parent_class_id`; after further appends any surviving
    /// stamp resolves to exact current pointer/logical/live facts. This fixture
    /// deliberately reserves eight live inline slots while owning fewer keys,
    /// so the old key-count-only compatibility mint is not the expected id.
    #[test]
    fn plain_object_stamp_lifecycle() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 8);
            for name in ["c3c_a", "c3c_b", "c3c_c"] {
                crate::object::js_object_set_field_by_name(obj, key(name), 1.0);
            }
            assert_eq!((*obj).class_id, 0, "test premise: plain object");
            let _ = crate::object::js_object_get_field_by_name(obj, key("c3c_b"));
            let stamp = (*obj).parent_class_id;
            assert!(
                is_shape_id(stamp),
                "read resolution must stamp a shape id, got {stamp:#x}"
            );

            crate::object::js_object_set_field_by_name(obj, key("c3c_d"), 2.0);
            crate::object::js_object_set_field_by_name(obj, key("c3c_e"), 3.0);
            let stamp2 = (*obj).parent_class_id;
            if stamp2 != 0 {
                assert!(is_shape_id(stamp2));
                let descriptor = shape_descriptor_by_id(stamp2)
                    .expect("a surviving stamp must resolve in this agent");
                assert_eq!(
                    descriptor.keys,
                    crate::object::object_keys_array(obj) as u64
                );
                assert_eq!(
                    descriptor.logical_key_count,
                    crate::array::js_array_length(crate::object::object_keys_array(obj))
                );
                assert_eq!(
                    descriptor.live_inline_slot_count,
                    crate::object::object_live_slot_count(obj)
                );
                debug_assert_object_shape_parity(obj);
            }

            // Reads still resolve correctly through the id-keyed cache.
            let v = crate::object::js_object_get_field_by_name(obj, key("c3c_d"));
            assert_eq!(f64::from_bits(v.bits()), 2.0);
        }
    }
}

#[cfg(test)]
mod c6804_tests {
    use super::*;

    /// #6804: shape-cached literal allocation birth-stamps the runtime
    /// ShapeId, and siblings of one shape share one id.
    #[test]
    fn alloc_with_shape_birth_stamps_shared_id() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"m6804_a\0m6804_b\0m6804_c";
            let a = crate::object::js_object_alloc_with_shape(
                0x0C3C_6804,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let b = crate::object::js_object_alloc_with_shape(
                0x0C3C_6804,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let stamp_a = (*a).parent_class_id;
            let stamp_b = (*b).parent_class_id;
            assert!(
                is_shape_id(stamp_a),
                "newborn literal must carry a runtime ShapeId, got {stamp_a:#x}"
            );
            assert_eq!(
                stamp_a, stamp_b,
                "siblings of one literal shape must share one id"
            );
            assert_eq!(
                crate::object::object_keys_array(a),
                crate::object::object_keys_array(b),
                "test premise: shared keys"
            );
        }
    }

    /// #6804 wanted "no pre/post-stamp token split", and got it with a
    /// self-heal inside `object_shape()`. #8113 removes the self-heal and keeps
    /// the property, by a stronger route: **the split population is empty**,
    /// because every allocator birth-stamps.
    ///
    /// The self-heal had to go because it derived the live inline-slot bound
    /// from `ObjectHeader::field_count`. With that word deleted, healing an
    /// unstamped receiver would publish a descriptor claiming a bound of ZERO —
    /// a read-only observation silently truncating the object's traced and
    /// writable payload. Missing closed costs a PIC miss; healing wrongly loses
    /// fields.
    #[test]
    fn object_shape_token_is_birth_stamped_and_an_unstamped_one_misses_closed() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"m6804_x\0m6804_y";
            let obj = crate::object::js_object_alloc_with_shape(
                0x0C3C_6805,
                2,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let birth_stamp = (*obj).parent_class_id;
            assert!(is_shape_id(birth_stamp), "every literal is birth-stamped");
            assert_eq!(
                crate::typed_feedback::test_object_shape_token(obj as usize),
                birth_stamp as usize,
                "the observed token is the birth stamp — no split to heal"
            );
            assert_eq!(
                shape_descriptor_by_id(birth_stamp)
                    .expect("birth descriptor")
                    .live_inline_slot_count,
                2
            );

            // Manufacture the pre-#6804 unstamped state and prove observing it
            // is INERT: no token, no descriptor, and — the part that matters —
            // no rewritten live-slot bound.
            (*obj).parent_class_id = 0;
            assert_eq!(
                crate::typed_feedback::test_object_shape_token(obj as usize),
                0,
                "an unstamped receiver must miss closed, not be re-stamped"
            );
            assert_eq!(
                (*obj).parent_class_id,
                0,
                "observation must not publish a descriptor for an unstamped receiver"
            );

            // Restoring the birth stamp restores the exact bound, which is the
            // proof that nothing was lost by refusing to heal.
            (*obj).parent_class_id = birth_stamp;
            assert_eq!(crate::object::object_live_slot_count(obj), 2);
        }
    }

    /// #6804: the first dynamic key on a fresh `{}` births a stamped shape.
    #[test]
    fn fresh_dynamic_shape_birth_stamps() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 8);
            let key = crate::string::js_string_from_bytes(b"m6804_first".as_ptr(), 11);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);
            let stamp = (*obj).parent_class_id;
            // Either stamped at the null-branch birth, or (for a sibling
            // adopting a cached transition edge) still 0 until first read
            // — but THIS test allocates a unique key, so the null branch
            // ran and must have stamped.
            assert!(
                is_shape_id(stamp),
                "first-key birth must stamp the new shape, got {stamp:#x}"
            );
        }
    }
}

#[cfg(test)]
mod descriptor_tests_8067 {
    use super::*;

    fn key(name: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    #[test]
    fn every_keyless_runtime_allocator_publishes_a_shape_id() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            for obj in [
                crate::object::js_object_alloc(0, 0),
                crate::object::js_object_alloc_fast(0, 0),
                crate::object::js_object_alloc_with_parent(0x8067_0101, 0, 0),
                crate::object::js_object_alloc_fast_with_parent(0x8067_0102, 0, 0),
            ] {
                let id = object_shape_id(obj);
                assert!(is_shape_id(id), "newborn keyless object has no ShapeId");
                let facts = object_shape_descriptor(obj).expect("keyless descriptor");
                assert_eq!(facts.keys, 0);
                assert_eq!(facts.logical_key_count, 0);
                assert_eq!(facts.live_inline_slot_count, 0);
            }
        }
    }

    #[test]
    fn descriptor_and_prototype_changes_mint_semantic_successors() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(obj, key("semantic8067"), 1.0);
            let structural = object_shape_id(obj);

            crate::object::descriptor_state::set_property_attrs(
                obj as usize,
                "semantic8067".to_string(),
                crate::object::descriptor_state::PropertyAttrs::new(false, true, true),
            );
            let described = object_shape_id(obj);
            assert_ne!(described, structural);
            let described_facts = object_shape_descriptor(obj).unwrap();
            assert_ne!(described_facts.semantic_generation, 0);

            crate::object::prototype_chain::object_set_static_prototype(
                obj as usize,
                crate::value::TAG_NULL,
            );
            let reparented = object_shape_id(obj);
            assert_ne!(reparented, described);
            assert_eq!(
                object_shape_descriptor(obj).unwrap().keys,
                described_facts.keys,
                "semantic transitions must preserve the rooted ordered keys edge"
            );
        }
    }

    #[test]
    fn absent_descriptor_clears_do_not_mint_semantic_successors() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 1);
            let addr = obj as usize;
            let initial = object_shape_id(obj);

            crate::object::descriptor_state::clear_property_attrs(addr, "missing8067");
            crate::object::descriptor_state::clear_accessor_descriptor(addr, "missing8067");
            assert_eq!(object_shape_id(obj), initial);

            crate::object::descriptor_state::set_property_attrs(
                addr,
                "attrs8067".to_string(),
                crate::object::descriptor_state::PropertyAttrs::new(false, true, true),
            );
            crate::object::descriptor_state::clear_property_attrs(addr, "attrs8067");
            let after_real_attr_clear = object_shape_id(obj);
            crate::object::descriptor_state::clear_property_attrs(addr, "attrs8067");
            assert_eq!(object_shape_id(obj), after_real_attr_clear);

            crate::object::descriptor_state::set_accessor_descriptor(
                addr,
                "accessor8067".to_string(),
                crate::object::descriptor_state::AccessorDescriptor::default(),
            );
            crate::object::descriptor_state::clear_accessor_descriptor(addr, "accessor8067");
            let after_real_accessor_clear = object_shape_id(obj);
            crate::object::descriptor_state::clear_accessor_descriptor(addr, "accessor8067");
            assert_eq!(object_shape_id(obj), after_real_accessor_clear);
        }
    }

    #[test]
    fn delete_compaction_never_compares_equal_to_the_predelete_layout() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 3);
            let a = key("delete8067_a");
            let b = key("delete8067_b");
            let c = key("delete8067_c");
            crate::object::js_object_set_field_by_name(obj, a, 1.0);
            crate::object::js_object_set_field_by_name(obj, b, 2.0);
            crate::object::js_object_set_field_by_name(obj, c, 3.0);
            let before = object_shape_id(obj);
            assert_eq!(crate::object::js_object_delete_field(obj, a), 1);
            let after = object_shape_id(obj);
            assert_ne!(after, before);
            let facts = object_shape_descriptor(obj).unwrap();
            assert_eq!(facts.logical_key_count, 2);
            assert_eq!(facts.live_inline_slot_count, 2);
            assert_eq!(
                crate::object::js_object_get_field_by_name_f64(obj, b),
                2.0,
                "middle-field lookup used a stale pre-delete slot mapping"
            );
        }
    }

    #[test]
    fn exhaustion_parks_without_reuse_or_alias() {
        let next = std::sync::atomic::AtomicU32::new(SHAPE_ID_END - 1);
        assert_eq!(alloc_shape_id_from(&next), Ok(SHAPE_ID_END - 1));
        assert_eq!(alloc_shape_id_from(&next), Err(ShapeIdExhausted));
        assert_eq!(alloc_shape_id_from(&next), Err(ShapeIdExhausted));
        assert_eq!(
            next.load(std::sync::atomic::Ordering::Relaxed),
            SHAPE_ID_END,
            "exhaustion must park instead of wrapping into an alias"
        );
    }

    #[test]
    fn inconsistent_facts_are_not_reported_as_id_exhaustion() {
        assert_eq!(
            shape_descriptor_ensure(std::ptr::null(), 1, 1),
            Err(ShapeDescriptorError::InvalidFacts)
        );
    }

    #[test]
    fn equivalent_local_and_external_ids_remain_resolvable() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_1700usize;
        let local = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");
        let external = alloc_shape_id().expect("shape range unexpectedly exhausted");
        assert!(install_external_shape_id(
            external,
            keys as *const ArrayHeader,
            1,
            1,
        ));

        assert_eq!(
            shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1).unwrap(),
            external,
            "the process-global id should be preferred for later births"
        );
        retain_key_count_versions(keys as u64);
        assert!(shape_descriptor_by_id(local).is_some());
        assert!(shape_descriptor_by_id(external).is_some());

        test_drop_shape_descriptors(keys);
    }

    #[test]
    fn a_foreign_agent_id_misses_instead_of_aliasing_same_address() {
        let _lock = crate::gc::global_side_table_test_lock();
        let fake_keys = 0x8067_0000_0000_1000usize;
        let local = shape_descriptor_ensure(fake_keys as *const ArrayHeader, 2, 2)
            .expect("shape range unexpectedly exhausted");
        let foreign = std::thread::spawn(move || {
            assert_eq!(
                shape_descriptor_by_id(local),
                None,
                "another RuntimeState resolved a foreign agent's ShapeId"
            );
            shape_descriptor_ensure(fake_keys as *const ArrayHeader, 2, 2)
                .expect("shape range unexpectedly exhausted")
        })
        .join()
        .expect("agent-isolation thread panicked");
        assert_ne!(
            local, foreign,
            "process-global ids must not alias by address"
        );
        shape_drop(fake_keys as *const ArrayHeader);
        test_drop_shape_descriptors(fake_keys);
    }

    #[test]
    fn process_global_module_shape_id_installs_with_agent_local_keys() {
        let _lock = crate::gc::global_side_table_test_lock();
        let module_keys = 0x8067_0000_0000_1800usize;
        let module_id = shape_descriptor_ensure(module_keys as *const ArrayHeader, 2, 2)
            .expect("shape range unexpectedly exhausted");
        let worker_keys = 0x8067_0000_0000_1900usize;
        std::thread::spawn(move || {
            assert!(install_external_shape_id(
                module_id,
                worker_keys as *const ArrayHeader,
                2,
                2,
            ));
            assert_eq!(
                shape_descriptor_by_id(module_id).unwrap().keys,
                worker_keys as u64,
                "worker resolved a module ShapeId to another agent's keys pointer"
            );
        })
        .join()
        .expect("worker shape installation panicked");
        test_drop_shape_descriptors(module_keys);
    }

    #[test]
    fn the_descriptor_keys_slot_is_the_record_the_collector_rewrites() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_2000usize;
        let id = shape_descriptor_ensure(keys as *const ArrayHeader, 3, 2)
            .expect("shape range unexpectedly exhausted");

        // A foreign / never-minted id has no slot: the collector emits no edge
        // rather than rewriting an unrelated record.
        assert_eq!(shape_descriptor_keys_slot(0), None);
        assert_eq!(shape_descriptor_keys_slot(SHAPE_ID_END - 1), None);

        let slot = shape_descriptor_keys_slot(id).expect("minted id has a keys slot");
        assert_eq!(
            Some(slot),
            shape_descriptor_by_id(id).unwrap().keys_slot(),
            "the lifted descriptor must name the boxed record's own keys word"
        );
        assert_eq!(unsafe { *slot }, keys as u64);
        assert_eq!(
            shape_descriptor_by_id(id).unwrap().indexed_keys,
            keys as u64,
            "newly minted descriptor must record its indexed keys address"
        );

        // Writing THROUGH the slot is what an evacuating visitor does. The
        // table must observe it with no write-back callback of any kind.
        let moved_keys = keys as u64 + 0x3000;
        unsafe { *slot = moved_keys };
        assert_eq!(shape_descriptor_by_id(id).unwrap().keys, moved_keys);
        assert_eq!(
            shape_descriptor_by_id(id).unwrap().indexed_keys,
            keys as u64,
            "an object-edge rewrite must retain the old indexed address until metadata repair"
        );

        // The keys-address reverse index is repaired incrementally by the
        // metadata pass, not by the store; force the same one-id repair here.
        {
            let mut inner = crate::state::state().shapes.inner.borrow_mut();
            sync_descriptor_reverse_indices(&mut inner, id);
        }
        assert_eq!(shape_descriptor_by_id(id).unwrap().indexed_keys, moved_keys);
        assert_eq!(
            shape_descriptor_ensure(moved_keys as *const ArrayHeader, 3, 2),
            Ok(id),
            "incremental repair must publish the moved facts under the original id"
        );
        let old_address_id = shape_descriptor_ensure(keys as *const ArrayHeader, 3, 2)
            .expect("shape range unexpectedly exhausted");
        assert_ne!(
            old_address_id, id,
            "incremental repair must remove the stale old-address facts entry"
        );
        test_drop_shape_descriptors(moved_keys as usize);
        assert_eq!(
            shape_descriptor_by_id(id),
            None,
            "descriptor rekey did not update the keys-address index"
        );
        test_drop_shape_descriptors(keys);
    }

    #[test]
    fn a_boxed_record_keeps_its_keys_slot_across_table_growth() {
        let _lock = crate::gc::global_side_table_test_lock();
        // The prohibition #8067 recorded — "descriptor insertion can reallocate
        // the table" — is what BOXING answers. Mint one descriptor, take its
        // slot, then mint enough siblings to force several rehashes and assert
        // the address never moved. Without the box this fails.
        let keys = 0x8112_0000_0000_1000usize;
        let id = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");
        let slot = shape_descriptor_keys_slot(id).expect("minted id has a keys slot");

        let mut minted = Vec::new();
        for i in 1..512usize {
            let sibling = keys + i * 0x40;
            minted.push(
                shape_descriptor_ensure(sibling as *const ArrayHeader, 1, 1)
                    .expect("shape range unexpectedly exhausted"),
            );
        }
        assert_eq!(
            shape_descriptor_keys_slot(id),
            Some(slot),
            "descriptor insertion moved a keys slot the collector may still hold"
        );
        assert_eq!(unsafe { *slot }, keys as u64);

        test_drop_shape_descriptors(keys);
        for i in 1..512usize {
            test_drop_shape_descriptors(keys + i * 0x40);
        }
    }

    #[test]
    fn key_count_versions_remain_resolvable_until_the_keys_die() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_2100usize;
        let unrelated_keys = 0x8067_0000_0000_2200usize;
        let stale_a = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");
        let stale_b = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 2)
            .expect("shape range unexpectedly exhausted");
        let current = shape_descriptor_ensure(keys as *const ArrayHeader, 2, 2)
            .expect("shape range unexpectedly exhausted");
        let unrelated = shape_descriptor_ensure(unrelated_keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");

        retain_key_count_versions(keys as u64);

        assert!(shape_descriptor_by_id(stale_a).is_some());
        assert!(shape_descriptor_by_id(stale_b).is_some());
        assert!(shape_descriptor_by_id(current).is_some());
        assert!(shape_descriptor_by_id(unrelated).is_some());
        let inner = crate::state::state().shapes.inner.borrow();
        let current_ids = inner
            .ids_by_keys
            .get(&(keys as u64))
            .expect("keys identity disappeared from descriptor index");
        assert_eq!(current_ids.as_slice(), &[stale_a, stale_b, current]);
        drop(inner);

        test_drop_shape_descriptors(keys);
        test_drop_shape_descriptors(unrelated_keys);
    }

    #[test]
    fn shape_drop_does_not_delete_a_potential_siblings_descriptor() {
        let _lock = crate::gc::global_side_table_test_lock();
        let keys = 0x8067_0000_0000_3000usize;
        let id = shape_descriptor_ensure(keys as *const ArrayHeader, 1, 1)
            .expect("shape range unexpectedly exhausted");

        shape_drop(keys as *const ArrayHeader);

        assert_eq!(
            shape_descriptor_by_id(id).map(|descriptor| descriptor.keys),
            Some(keys as u64),
            "shape_drop eagerly invalidated a descriptor a sibling may still name"
        );
        test_drop_shape_descriptors(keys);
    }

    #[test]
    fn live_slot_growth_versions_descriptor_before_value_publication() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"slot8067_a";
            let obj = crate::object::js_object_alloc_with_shape(
                0x8067_1001,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let keys = crate::object::object_keys_array(obj) as usize;
            let before = (*obj).parent_class_id;
            let before_descriptor = shape_descriptor_by_id(before).expect("birth descriptor");
            assert_eq!(before_descriptor.live_inline_slot_count, 1);

            crate::object::js_object_set_field(obj, 1, crate::JSValue::string_ptr(key("value")));
            let after = (*obj).parent_class_id;
            assert_ne!(before, after);
            let after_descriptor = shape_descriptor_by_id(after).expect("grown descriptor");
            assert_eq!(after_descriptor.keys, keys as u64);
            assert_eq!(after_descriptor.logical_key_count, 1);
            assert_eq!(after_descriptor.live_inline_slot_count, 2);
            debug_assert_object_shape_parity(obj);
        }
    }

    #[test]
    fn shared_sibling_append_clones_before_descriptor_version_changes() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let packed = b"sib8067_a";
            let a = crate::object::js_object_alloc_with_shape(
                0x8067_1002,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let b = crate::object::js_object_alloc_with_shape(
                0x8067_1002,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let shared_keys = crate::object::object_keys_array(a);
            let shared_id = (*a).parent_class_id;
            assert_eq!(shared_keys, crate::object::object_keys_array(b));
            assert_eq!(shared_id, (*b).parent_class_id);

            crate::object::js_object_set_field_by_name(a, key("sib8067_b"), 2.0);

            assert_ne!(crate::object::object_keys_array(a), shared_keys);
            assert_eq!(crate::object::object_keys_array(b), shared_keys);
            assert_eq!((*b).parent_class_id, shared_id);
            assert_ne!((*a).parent_class_id, shared_id);
            assert_eq!(
                shape_descriptor_by_id(shared_id)
                    .expect("untouched sibling descriptor")
                    .logical_key_count,
                1
            );
            let transitioned =
                shape_descriptor_by_id((*a).parent_class_id).expect("transitioned descriptor");
            assert_eq!(
                transitioned.keys,
                crate::object::object_keys_array(a) as u64
            );
            assert_eq!(transitioned.logical_key_count, 2);
            assert_eq!(transitioned.live_inline_slot_count, 2);
        }
    }
}
