//! #7753 C3C PIC regression tests, split out of `ic_miss.rs` to keep that
//! file under the 2000-line cap. Behaviour is unchanged.

/// Installing an accessor on one object must not permanently disable every
/// property-read PIC in the process. Descriptor ownership is recorded on
/// the owning object's GC header, so a different descriptor-free object's
/// own data field remains safe to cache.
#[test]
fn unrelated_accessor_does_not_poison_plain_receiver_pic() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let unrelated = crate::object::js_object_alloc(0, 1);
    let unrelated = scope.root_raw_mut_ptr(unrelated);
    crate::object::set_accessor_descriptor(
        unrelated.with_mut_ptr(|o: *mut crate::object::ObjectHeader| o as usize),
        "pic_unrelated_accessor".to_string(),
        crate::object::AccessorDescriptor::default(),
    );
    assert!(
        crate::state::state().descriptors.accessors_in_use.get(),
        "test premise: the process-wide accessor latch is active"
    );

    let obj = crate::object::js_object_alloc(0, 8);
    let obj = scope.root_raw_mut_ptr(obj);
    let key_bytes = b"pic_plain_data";
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    let key = scope.root_string_ptr(key);
    obj.with_mut_ptr(|o| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 42.0))
    });

    let mut cache = [0i64; super::PIC_CACHE_WORDS];
    assert_eq!(
        obj.with_mut_ptr(
            |o| key.with_const_ptr(|k| super::js_object_get_field_ic_miss(o, k, &mut cache))
        ),
        42.0
    );
    assert_ne!(
        cache[0], 0,
        "an accessor owned by an unrelated object must not prevent this \
         descriptor-free receiver from priming its read PIC"
    );
    assert_eq!(cache[1], 0, "the first own data field lives in slot 0");
}

/// The receiver-local half of the proof: an accessor-bearing object must
/// keep taking descriptor-aware lookup and must never seed a raw-slot hit.
#[test]
fn accessor_bearing_receiver_does_not_prime_plain_data_pic() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 8);
    let obj = scope.root_raw_mut_ptr(obj);
    let key_bytes = b"pic_guarded_data";
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    let key = scope.root_string_ptr(key);
    obj.with_mut_ptr(|o| {
        key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 17.0))
    });
    crate::object::set_accessor_descriptor(
        obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| o as usize),
        "pic_guarded_data".to_string(),
        crate::object::AccessorDescriptor::default(),
    );

    let mut cache = [0i64; super::PIC_CACHE_WORDS];
    let via_pic = obj.with_mut_ptr(|o| {
        key.with_const_ptr(|k| super::js_object_get_field_ic_miss(o, k, &mut cache))
    });
    let via_ladder =
        obj.with_mut_ptr(|o| key.with_const_ptr(|k| super::js_object_get_field_by_name_f64(o, k)));
    assert_eq!(
        via_pic.to_bits(),
        via_ladder.to_bits(),
        "the miss path must preserve this receiver's accessor semantics"
    );
    assert_eq!(
        cache[0], 0,
        "an accessor-bearing receiver must not prime a raw-slot PIC"
    );
}

/// A class instance primes the same authoritative ShapeId token that the
/// emitted guard reads from the receiver.
#[test]
fn a_class_instance_primes_an_id_token_after_rung1() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0x6080, 8);
        let key = crate::string::js_string_from_bytes(b"pic6080_x".as_ptr(), 9);
        crate::object::js_object_set_field_by_name(obj, key, 7.0);
        let keys = crate::object::object_keys_array(obj);
        assert!(!keys.is_null(), "test premise: field append built keys");
        assert_eq!((*obj).class_id, 0x6080, "test premise: a class instance");

        let mut cache = [0i64; super::PIC_CACHE_WORDS];
        let v = super::js_object_get_field_ic_miss(obj, key, &mut cache);
        assert_eq!(v, 7.0);

        let stamp = crate::object::shapes::object_shape_stamp(obj);
        assert!(
            stamp != 0,
            "the miss handler did not stamp a class instance — rung 1 is inert"
        );
        assert_eq!(
            cache[0] as u64,
            stamp as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT,
            "a stamped class instance must prime the ID token the emitted \
             PIC computes for it, not its keys pointer"
        );
        assert_ne!(
            cache[0], keys as i64,
            "primed the keys pointer for a stamped receiver — every hit at \
             this site would miss forever"
        );
        assert_eq!(cache[2], 0, "word 2 is non-identity scratch");
    }
}

/// ★ #6759 C3 rung 1 opens a NEW correctness surface, and this is it.
///
/// A delete-compacted class instance receives a semantic successor ShapeId,
/// so the emitted hit path can serve it without confusing it with a
/// pristine sibling. A token that failed to move across the
/// compaction would therefore be read as a pristine sibling's shape at a
/// site that has both — the one-slot shift the whole ladder is about.
///
/// Pins: the compacted instance's primed token differs from a pristine
/// sibling's, AND the slot it primes is the post-compaction slot.
#[test]
fn a_compacted_class_instance_primes_a_token_a_pristine_sibling_cannot_match() {
    // Preserve the compacted fixture under default-on tombstones. The
    // tombstone lane already produces a distinct class-instance token,
    // but it keeps `c` in slot 2 and therefore cannot exercise the
    // shifted-slot/token pairing this regression test owns.
    let _tombstones = crate::object::delete_rest::test_scope_tombstone_deletes(false);
    let _lock = crate::gc::global_side_table_test_lock();
    {
        let packed = b"picdel_a\0picdel_b\0picdel_c";
        let mk = || {
            crate::object::js_object_alloc_class_with_keys(
                0x6081,
                0,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            )
        };
        let key = |n: &str| crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
        let pristine = mk();
        let compacted = mk();
        for (i, v) in [1.0f64, 2.0, 3.0].iter().enumerate() {
            crate::object::js_object_set_field(
                pristine,
                i as u32,
                crate::JSValue::from_bits(v.to_bits()),
            );
            crate::object::js_object_set_field(
                compacted,
                i as u32,
                crate::JSValue::from_bits(v.to_bits()),
            );
        }
        assert_eq!(
            crate::object::js_object_delete_field(compacted, key("picdel_a")),
            1
        );

        let mut c_pristine = [0i64; super::PIC_CACHE_WORDS];
        let vp = super::js_object_get_field_ic_miss(pristine, key("picdel_c"), &mut c_pristine);
        assert_eq!(vp, 3.0, "pristine `c` is slot 2");

        let mut c_compacted = [0i64; super::PIC_CACHE_WORDS];
        let vc = super::js_object_get_field_ic_miss(compacted, key("picdel_c"), &mut c_compacted);
        assert_eq!(
            vc, 3.0,
            "compacted `c` shifted to slot 1 and must still read 3"
        );

        assert_ne!(
            c_compacted[0], 0,
            "the compacted instance primed nothing — rung 1's new surface is inert"
        );
        assert_ne!(
            c_compacted[0], c_pristine[0],
            "the compacted instance primed its pristine sibling's token — an \
             id-comparing PIC would read slot {} for a receiver whose `c` is \
             at slot {}",
            c_pristine[1], c_compacted[1]
        );
        assert_eq!(c_pristine[1], 2, "pristine `c` slot");
        assert_eq!(c_compacted[1], 1, "compacted `c` slot");
    }
}

/// The PIC cache token the EMITTED code computes for `obj`, transcribed
/// from `perry-codegen/src/expr/property_get/generic_dispatch.rs`:
///
/// ```text
/// token = valid_shape_id ? (shape_id | 1<<62) : 0
/// ```
///
/// The runtime never calls this; it exists so a test can compare what the
/// miss handler PRIMES against what the hit path will COMPUTE, which is
/// the only pair whose agreement decides whether a site can ever hit.
unsafe fn emitted_pic_token(obj: *const super::ObjectHeader) -> u64 {
    let shape_id = crate::object::shapes::object_shape_id(obj);
    shape_id as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT
}

/// ★ The invariant #6759 C3 rung 1 broke, asserted where it broke.
///
/// A shape's population must be UNIFORMLY stamped: the token the miss
/// handler primes from one instance is only useful if a DIFFERENT,
/// freshly-allocated instance of the same class computes the same token.
/// A prior implementation stamped class instances lazily, so instance #1
/// primed an id token while every newborn sibling computed a different
/// identity. `token_eq` then failed at every field-read site until the
/// sibling took the miss path itself.
///
/// This is deliberately NOT "the newborn carries a stamp" — that is a
/// presence check two different states satisfy (both-stamped and
/// both-unstamped are each fine; the mixture is the bug). Comparing the
/// primed token against a fresh sibling's COMPUTED token is what fails
/// under either half of the split.
#[test]
fn a_fresh_class_instance_computes_the_token_the_miss_handler_primed() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let packed = b"picbirth_x\0picbirth_y";
        let mk = || {
            crate::object::js_object_alloc_class_with_keys(
                0x6082,
                0,
                2,
                packed.as_ptr(),
                packed.len() as u32,
            )
        };
        let key = crate::string::js_string_from_bytes(b"picbirth_x".as_ptr(), 10);

        let primed_from = mk();
        crate::object::js_object_set_field(
            primed_from,
            0,
            crate::JSValue::from_bits(5.0f64.to_bits()),
        );
        assert_eq!(
            (*primed_from).class_id,
            0x6082,
            "test premise: the receiver is a class instance, not a literal"
        );

        let mut cache = [0i64; super::PIC_CACHE_WORDS];
        assert_eq!(
            super::js_object_get_field_ic_miss(primed_from, key, &mut cache),
            5.0,
            "test premise: the miss handler resolved the field"
        );
        assert_ne!(
            cache[0], 0,
            "test premise: the miss handler primed SOMETHING — a zero token \
             never hits, so the comparison below would be vacuous"
        );

        // The next `new C(...)`. Nothing has resolved a field on it.
        let fresh = mk();
        assert_eq!(
            emitted_pic_token(fresh),
            cache[0] as u64,
            "a freshly allocated instance of the SAME class computes a \
             different PIC token than the one primed from its sibling, so \
             every read of a newborn instance's field misses the cache and \
             takes the full miss handler — #7983's split population"
        );

        // And the same must hold once the fresh one has itself resolved:
        // priming from either instance is interchangeable.
        let mut cache2 = [0i64; super::PIC_CACHE_WORDS];
        super::js_object_get_field_ic_miss(fresh, key, &mut cache2);
        assert_eq!(
            cache2[0], cache[0],
            "two instances of one class primed two different tokens — the \
             site thrashes between them"
        );
    }
}
