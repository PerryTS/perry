use super::*;

fn markers() -> Vec<&'static [u8]> {
    let mut names = vec![
        b"toJSON".as_slice(),
        b"__module__",
        crate::object::FETCH_SUBCLASS_HANDLE_FIELD,
    ];
    #[cfg(feature = "temporal")]
    names.push(crate::object::TEMPORAL_SUBCLASS_CELL_FIELD);
    names
}

unsafe fn check(bytes: &[u8]) {
    let expected = markers().iter().any(|name| *name == bytes);
    let header = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let value = JSValue::string_ptr(header);
    assert_eq!(key_may_carry_to_json(value), expected, "{bytes:?}");
    assert_eq!(marker_bytes_may_carry_to_json(bytes), expected, "{bytes:?}");
    assert_eq!(key_bytes_may_carry_to_json(bytes), expected, "{bytes:?}");
    // Preserve the previous probe's contract for runtime-valid stored values.
    let previous = markers()
        .iter()
        .any(|name| crate::string::js_string_key_matches_bytes(value, name));
    assert_eq!(key_may_carry_to_json(value), previous, "{bytes:?}");
}

#[test]
fn json_tojson_key_probe_matches_markers_and_all_single_byte_changes() {
    unsafe {
        for marker in markers() {
            check(marker);
            for at in 0..marker.len() {
                let mut bytes = marker.to_vec();
                for byte in 0..=255u8 {
                    bytes[at] = byte;
                    check(&bytes);
                }
            }
            for len in 0..marker.len() {
                check(&marker[..len]);
            }
            for byte in [0, b'_', b't', b'x', 0x80, 0xff] {
                let mut bytes = marker.to_vec();
                bytes.push(byte);
                check(&bytes);
                let mut bytes = vec![byte];
                bytes.extend_from_slice(marker);
                check(&bytes);
            }
        }
        for bytes in [
            b"active".as_slice(),
            b"toJson",
            b"TOJSON",
            b"_module_",
            b"__module___",
            b"t",
            b"_",
            b"",
            b"\xed\xa0\x80",
            b"t\0JSON",
        ] {
            check(bytes);
        }
        check(&vec![b't'; 4097]);
        check(&vec![b'_'; 4097]);
    }
}

#[test]
fn json_tojson_key_probe_rejects_inline_and_nonstring_values_without_allocation() {
    unsafe {
        let before = crate::arena::arena_total_bytes();
        for len in 0..=crate::value::SHORT_STRING_MAX_LEN {
            let mut bytes = vec![b'_'; len];
            for at in 0..len {
                for byte in 0..=127u8 {
                    bytes[at] = byte;
                    if let Some(value) = JSValue::try_short_string(&bytes) {
                        assert!(!key_may_carry_to_json(value));
                    }
                }
            }
            assert!(!key_may_carry_to_json(
                JSValue::try_short_string(&vec![b't'; len]).unwrap()
            ));
        }
        for value in [
            JSValue::null(),
            JSValue::undefined(),
            JSValue::int32(42),
            JSValue::bool(true),
            JSValue::from_bits(crate::value::TAG_HOLE),
            JSValue::string_ptr(std::ptr::null_mut()),
        ] {
            assert!(!key_may_carry_to_json(value));
        }
        assert_eq!(crate::arena::arena_total_bytes(), before);
    }
}

#[test]
fn json_tojson_key_array_probe_observes_replacement_without_managed_scratch() {
    unsafe {
        let arr = crate::array::js_array_alloc_with_length(4);
        let scope = crate::gc::RuntimeHandleScope::new();
        let keys = scope.root_raw_mut_ptr(arr);
        let ordinary = JSValue::try_short_string(b"name").unwrap();
        for i in 0..4 {
            keys.with_mut_ptr(|keys| crate::array::js_array_set(keys, i, ordinary));
        }
        let view = |keys: *mut crate::ArrayHeader| crate::object::ObjectKeys::owned(keys);
        assert!(!keys.with_mut_ptr(|keys| keys_array_may_carry_to_json(view(keys))));
        for marker in markers() {
            for at in 0..4 {
                let value = JSValue::string_ptr(crate::js_string_from_bytes(
                    marker.as_ptr(),
                    marker.len() as u32,
                ));
                keys.with_mut_ptr(|keys| crate::array::js_array_set(keys, at, value));
                let before = crate::arena::arena_total_bytes();
                let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
                for _ in 0..100 {
                    assert!(keys.with_mut_ptr(|keys| keys_array_may_carry_to_json(view(keys))));
                }
                assert_eq!(crate::arena::arena_total_bytes(), before);
                assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
                keys.with_mut_ptr(|keys| crate::array::js_array_set(keys, at, ordinary));
                assert!(!keys.with_mut_ptr(|keys| keys_array_may_carry_to_json(view(keys))));
            }
        }
        keys.with_mut_ptr(|arr: *mut crate::ArrayHeader| {
            let len = (*arr).length;
            (*arr).length = (*arr).capacity + 1;
            // A malformed header: the count it reports runs past its capacity.
            assert!(keys_array_may_carry_to_json(
                crate::object::ObjectKeys::new(arr, (*arr).length)
            ));
            (*arr).length = len;
            assert!(keys_array_may_carry_to_json(
                crate::object::ObjectKeys::new((arr as *mut u8).add(1).cast(), len)
            ));
        });
        assert!(keys_array_may_carry_to_json(
            crate::object::ObjectKeys::NONE
        ));
    }
}

#[cfg(unix)]
#[test]
fn json_tojson_marker_comparisons_stop_before_guard_page() {
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let raw = libc::mmap(
            std::ptr::null_mut(),
            page * 2,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANON | libc::MAP_PRIVATE,
            -1,
            0,
        );
        assert_ne!(raw, libc::MAP_FAILED);
        let base = raw.cast::<u8>();
        assert_eq!(
            libc::mprotect(base.add(page).cast(), page, libc::PROT_NONE),
            0
        );
        for marker in markers() {
            for len in 0..=marker.len() + 1 {
                let start = base.add(page - len);
                std::ptr::write_bytes(start, b'_', len);
                if len == marker.len() {
                    std::ptr::copy_nonoverlapping(marker.as_ptr(), start, len);
                }
                let bytes = std::slice::from_raw_parts(start, len);
                assert_eq!(
                    marker_bytes_may_carry_to_json(bytes),
                    markers().iter().any(|name| *name == bytes)
                );
            }
        }
        assert_eq!(libc::munmap(raw, page * 2), 0);
    }
}

// ─── #10696: the invalidation funnel behind the per-`class_id` memo ──────────
//
// `class_chain_may_have_to_json` is memoized per class id under three
// generations. The dangerous staleness direction is a cached `false` that
// should have become `true`, so every writer that can flip it that way must be
// IN the funnel. Each test below primes the memo with a `false`, performs one
// such write, and requires the answer to flip — so deleting that writer's bump
// makes exactly one test fail, by name.

/// A unique-per-test class id that no other test or builtin claims.
fn probe_test_class_id(n: u32) -> u32 {
    0x7EEE_0000 | n
}

/// A non-pointer JS value: enough to make `lookup_prototype_method` answer
/// `Some`, and safe for the root store's write barrier.
fn probe_test_method_bits() -> u64 {
    JSValue::int32(1).bits()
}

#[test]
fn class_chain_tojson_memo_reuses_a_verdict_until_a_generation_moves() {
    let class_id = probe_test_class_id(0x01);
    super::test_reset_class_chain_tojson_recomputes();
    let first = super::test_class_chain_may_have_to_json(class_id);
    assert_eq!(super::test_class_chain_tojson_recomputes(), 1);
    assert_eq!(super::test_class_chain_may_have_to_json(class_id), first);
    assert_eq!(
        super::test_class_chain_tojson_recomputes(),
        1,
        "an unchanged generation triple must reuse the recorded verdict"
    );

    crate::object::test_bump_vtable_generation();
    assert_eq!(super::test_class_chain_may_have_to_json(class_id), first);
    assert_eq!(
        super::test_class_chain_tojson_recomputes(),
        2,
        "a vtable registration must retire the entry"
    );

    crate::object::prop_plan::prop_plan_epoch_bump();
    assert_eq!(super::test_class_chain_may_have_to_json(class_id), first);
    assert_eq!(
        super::test_class_chain_tojson_recomputes(),
        3,
        "a semantic property mutation must retire the entry"
    );

    crate::object::class_lookup_surface_gen_bump();
    assert_eq!(super::test_class_chain_may_have_to_json(class_id), first);
    assert_eq!(
        super::test_class_chain_tojson_recomputes(),
        4,
        "a class lookup-surface mutation must retire the entry"
    );
}

#[test]
fn class_chain_tojson_memo_never_disagrees_with_the_uncached_walk() {
    for n in 0x10..0x18u32 {
        let class_id = probe_test_class_id(n);
        assert_eq!(
            super::test_class_chain_may_have_to_json(class_id),
            super::test_class_chain_may_have_to_json_uncached(class_id),
        );
    }
}

#[test]
fn a_late_prototype_method_retires_the_cached_chain_verdict() {
    let class_id = probe_test_class_id(0x21);
    assert!(!super::test_class_chain_may_have_to_json(class_id));
    crate::object::class_prototype_method_root_store(
        class_id,
        "toJSON".to_string(),
        probe_test_method_bits(),
    );
    assert!(
        super::test_class_chain_may_have_to_json(class_id),
        "`C.prototype.toJSON = fn` after a first stringify must be observed"
    );
}

#[test]
fn a_late_class_prototype_object_retires_the_cached_chain_verdict() {
    let class_id = probe_test_class_id(0x31);
    assert!(!super::test_class_chain_may_have_to_json(class_id));
    let proto = crate::object::js_object_alloc(0, 0);
    assert!(!proto.is_null());
    crate::object::class_prototype_object_root_store(class_id, proto);
    assert!(
        super::test_class_chain_may_have_to_json(class_id),
        "a materialized `F.prototype` object can carry a later-added `toJSON`, \
         so it must retire the cached verdict"
    );
}

#[test]
fn a_late_decl_prototype_object_retires_the_cached_chain_verdict() {
    let class_id = probe_test_class_id(0x41);
    assert!(!super::test_class_chain_may_have_to_json(class_id));
    let proto = crate::object::js_object_alloc(0, 0);
    assert!(!proto.is_null());
    crate::object::class_decl_prototype_object_root_store(class_id, proto);
    assert!(
        super::test_class_chain_may_have_to_json(class_id),
        "the first reflective `C.prototype` / `instanceof` / getPrototypeOf \
         materializes a prototype OBJECT and must retire the cached verdict"
    );
}

#[test]
fn a_late_generic_origin_edge_retires_the_cached_chain_verdict() {
    let generic = probe_test_class_id(0x51);
    let specialization = probe_test_class_id(0x52);
    let proto = crate::object::js_object_alloc(0, 0);
    assert!(!proto.is_null());
    crate::object::class_decl_prototype_object_root_store(generic, proto);
    assert!(super::test_class_chain_may_have_to_json(generic));
    assert!(!super::test_class_chain_may_have_to_json(specialization));
    crate::object::js_register_class_generic_origin(specialization, generic);
    assert!(
        super::test_class_chain_may_have_to_json(specialization),
        "a generic-origin edge redirects both prototype-object readers and \
         `lookup_prototype_method`'s chain hop, so it must retire the entry"
    );
}

#[test]
fn un_marking_a_deleted_prototype_key_retires_the_cached_chain_verdict() {
    let class_id = probe_test_class_id(0x61);
    crate::object::class_prototype_method_root_store(
        class_id,
        "toJSON".to_string(),
        probe_test_method_bits(),
    );
    assert!(super::test_class_chain_may_have_to_json(class_id));
    crate::object::class_mark_key_deleted(class_id, "toJSON");
    // `delete C.prototype.toJSON` reaches `class_mark_key_deleted` through
    // `js_object_delete_field`, which bumps the semantic epoch; stand in for
    // that here so the memo holds the post-delete `false` the runtime would.
    crate::object::prop_plan::prop_plan_epoch_bump();
    assert!(!super::test_class_chain_may_have_to_json(class_id));
    // `CLASS_DELETED_KEYS` is shared between a class's prototype keys and its
    // STATIC field keys (`class_registry/state.rs`), so a later `C.toJSON = 1`
    // static store un-marks the key IN PLACE inside
    // `class_dynamic_prop_root_store` and re-exposes the prototype method to
    // `lookup_prototype_method` — with no vtable write and no descriptor
    // install to move either of the other two generations.
    crate::object::class_dynamic_prop_root_store(
        class_id,
        "toJSON",
        f64::from_bits(probe_test_method_bits()),
    );
    assert!(
        super::test_class_chain_may_have_to_json(class_id),
        "un-marking a deleted key re-exposes the prototype method and must \
         retire the cached verdict"
    );
}

// ─── #10696: the cheap `Object.prototype` signature comparison ───────────────

#[test]
fn object_proto_signature_fast_match_agrees_with_the_validated_builder() {
    unsafe {
        super::invalidate_object_proto_tojson_state();
        let first = super::object_proto_may_have_to_json();
        let cached = super::OBJECT_PROTO_TOJSON_SIGNATURE
            .with(std::cell::Cell::get)
            .expect("a verdict was recorded under a signature");
        assert_eq!(
            super::object_proto_tojson_signature(),
            Some(cached),
            "the validated builder must reproduce the recorded signature"
        );
        assert!(
            super::object_proto_tojson_signature_matches(&cached),
            "the cheap comparison must agree with the validated builder"
        );

        super::test_reset_object_proto_tojson_recomputes();
        for _ in 0..64 {
            assert_eq!(super::object_proto_may_have_to_json(), first);
        }
        assert_eq!(
            super::test_object_proto_tojson_recomputes(),
            0,
            "an unchanged signature must not recompute"
        );

        crate::object::prop_plan::prop_plan_epoch_bump();
        assert!(
            !super::object_proto_tojson_signature_matches(&cached),
            "a semantic property mutation must be a signature miss"
        );
        assert_eq!(super::object_proto_may_have_to_json(), first);
        assert_eq!(super::test_object_proto_tojson_recomputes(), 1);
    }
}

#[test]
fn object_proto_signature_fast_match_rejects_a_replaced_prototype_address() {
    unsafe {
        super::invalidate_object_proto_tojson_state();
        let _ = super::object_proto_may_have_to_json();
        let cached = super::OBJECT_PROTO_TOJSON_SIGNATURE
            .with(std::cell::Cell::get)
            .expect("a verdict was recorded under a signature");
        // A relocation rewrites `CACHED_OBJECT_PROTO_BITS` (a GC mutable
        // root), which is exactly what the comparison re-reads. Simulate the
        // rewritten root and require a miss rather than a stale hit.
        let saved = CACHED_OBJECT_PROTO_BITS.with(|c| c.get());
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(saved ^ 0x40));
        assert!(!super::object_proto_tojson_signature_matches(&cached));
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(0));
        assert!(!super::object_proto_tojson_signature_matches(&cached));
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(saved));
        assert!(super::object_proto_tojson_signature_matches(&cached));
    }
}

/// Sabotage proof that the magnitude guard on the two header reads is live,
/// not decoration. Every other miss in this comparison is decided by an
/// equality test, so a root that is not an address AT ALL — a `POINTER_TAG`
/// payload carrying a registry handle id — is the one shape that reaches the
/// dereference. `try_read_gc_header` declines it before
/// `addr - GC_HEADER_SIZE` is formed; the bare cast this replaced read
/// unmapped low memory instead (segfaults on Linux, masked on macOS by
/// mimalloc page retention — #4665/#4800).
#[test]
fn object_proto_signature_fast_match_declines_a_handle_band_root() {
    unsafe {
        let saved = CACHED_OBJECT_PROTO_BITS.with(|c| c.get());
        let handle = crate::value::addr_class::HANDLE_BAND_MAX as u64 - 0x40;
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(crate::value::POINTER_TAG | handle));
        // Everything else in the signature is arranged to MATCH, so the
        // guard is the only thing left that can decline it.
        let cached = super::ObjectProtoToJsonSignature {
            proto_addr: handle as usize,
            keys_addr: 0,
            keys_len: 0,
            obj_flags: 0,
            class_id: 0,
            semantic_epoch: crate::object::prop_plan::prop_plan_semantic_epoch(),
        };
        assert!(
            !crate::value::addr_class::is_plausible_heap_addr(cached.proto_addr),
            "fixture must start with a NON-address root, or the verdict below is vacuous"
        );
        assert!(
            !super::object_proto_tojson_signature_matches(&cached),
            "a handle-band root must be declined by the magnitude guard"
        );
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(saved));
        super::invalidate_object_proto_tojson_state();
    }
}

#[test]
fn class_chain_tojson_memo_holds_every_shape_of_one_nested_literal() {
    // `{a:{b:{c:{d:{e:1}}}}}` is FIVE distinct object-literal shapes, hence
    // five consecutive anon shape class ids, all live within a single
    // serialization walk. A table that evicts any pair of them pays a full
    // 448-instruction registry walk on EVERY operation — which is what a
    // 16-slot table indexed by `>> 12` did (#10696).
    let ids: Vec<u32> = (0..5).map(|n| probe_test_class_id(0x70 + n)).collect();
    super::test_reset_class_chain_tojson_recomputes();
    for _ in 0..50 {
        for &id in &ids {
            let _ = super::test_class_chain_may_have_to_json(id);
        }
    }
    assert_eq!(
        super::test_class_chain_tojson_recomputes(),
        ids.len() as u64,
        "consecutive shape ids of one walk must each keep their own slot"
    );
}

// ─── plain-record admission for anon shape classes (#10529) ──────────────────

#[test]
fn only_a_surface_free_anon_shape_is_a_plain_record() {
    assert!(super::class_is_plain_record(0));

    let never_registered = probe_test_class_id(0x81);
    assert!(
        !super::class_is_plain_record(never_registered),
        "a class id that is not a registered anon shape is a real class"
    );

    let anon = probe_test_class_id(0x82);
    unsafe { crate::object::js_register_anon_shape_class_id(anon) };
    assert!(super::class_is_plain_record(anon));

    // Class ids are per module, so an anon shape id can collide with a declared
    // class. A registered name is that class's evidence.
    let named = probe_test_class_id(0x83);
    unsafe {
        crate::object::js_register_anon_shape_class_id(named);
        crate::object::js_register_class_name(named, b"Declared".as_ptr(), 8);
    }
    assert!(!super::class_is_plain_record(named));

    let with_proto_object = probe_test_class_id(0x84);
    unsafe { crate::object::js_register_anon_shape_class_id(with_proto_object) };
    let proto = crate::object::js_object_alloc(0, 0);
    crate::object::class_prototype_object_root_store(with_proto_object, proto);
    assert!(
        !super::class_is_plain_record(with_proto_object),
        "a materialized prototype object can carry arbitrary properties"
    );
}

#[test]
fn a_late_prototype_to_json_retires_a_cached_plain_record_verdict() {
    let anon = probe_test_class_id(0x91);
    unsafe { crate::object::js_register_anon_shape_class_id(anon) };
    assert!(super::class_is_plain_record(anon));
    crate::object::class_prototype_method_root_store(
        anon,
        "toJSON".to_string(),
        probe_test_method_bits(),
    );
    assert!(
        !super::class_is_plain_record(anon),
        "a cached plain-record verdict must not outlive a prototype `toJSON`"
    );
}

#[test]
fn object_literal_shapes_reach_the_flat_emitter() {
    fn output(value: JSValue) -> Vec<u8> {
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let (ptr, len) =
            crate::string::str_bytes_from_jsvalue(f64::from_bits(value.bits()), &mut scratch)
                .unwrap();
        unsafe { std::slice::from_raw_parts(ptr, len as usize).to_vec() }
    }
    unsafe {
        let text = "{\"a\":1,\"b\":\"x\"}";
        let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = super::super::test_json_parse_direct(source);
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_u64(value.bits());
        let obj = || (value.get_nanbox_f64().to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
        // Warm the default-prototype lookup through the rooted general probe;
        // the flat emitter declines the allocating first lookup by design.
        assert!(super::to_json_definitely_absent(obj() as *const u8));

        // What HIR does to a closed-shape literal: an anon shape class.
        let anon = probe_test_class_id(0xA1);
        crate::object::js_register_anon_shape_class_id(anon);
        (*obj()).class_id = anon;
        let bits = value.get_nanbox_f64().to_bits();
        let result = super::super::stringify_flat::try_object(bits)
            .expect("an anon-shape literal is a plain record");
        assert_eq!(output(result), text.as_bytes());

        // A real class with the same own fields must keep the general walk.
        let declared = probe_test_class_id(0xA2);
        (*obj()).class_id = declared;
        let bits = value.get_nanbox_f64().to_bits();
        assert!(super::super::stringify_flat::try_object(bits).is_none());
        assert!(super::super::stringify_record_output::try_object(bits).is_none());

        // And the anon shape stops qualifying once its class grows a `toJSON`.
        (*obj()).class_id = anon;
        crate::object::class_prototype_method_root_store(
            anon,
            "toJSON".to_string(),
            probe_test_method_bits(),
        );
        let bits = value.get_nanbox_f64().to_bits();
        assert!(super::super::stringify_flat::try_object(bits).is_none());
        assert!(super::super::stringify_record_output::try_object(bits).is_none());
    }
}
