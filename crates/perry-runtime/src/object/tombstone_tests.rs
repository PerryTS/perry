//! Tombstone-delete (#9029) unit pins: the template-build SEGV, the
//! structured-clone hole skip, and the hole-count accounting that keeps the
//! squeeze threshold honest under delete/re-add churn. Split from
//! `object/tests.rs` for the file-size gate.

use super::super::{js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name};

/// Restores the per-thread tombstone-flag override on scope exit (panic
/// included) so a failing tombstone test cannot leak flag-on deletes into
/// unrelated tests on the same thread.
fn scopeguard_tombstone_flag() -> impl Drop {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::object::delete_rest::test_set_tombstone_deletes(None);
        }
    }
    Restore
}

/// Tombstone-delete (#9029) end-to-end at the unit level: the flag-on delete
/// leaves a TAG_HOLE key slot, and the JSON array-of-objects prefix template
/// (`build_shape_prefix_template`) must not treat that slot as a key — the
/// hole's bits are NOT a string header and dereferencing them is UB. Run
/// filtered (`tombstone_hole`) so the enable-flag OnceLock is primed by this
/// test's own env write, not an earlier delete from an unrelated test.
#[test]
fn tombstone_hole_never_reaches_template_prefixes() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("key_number_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        let victim_ptr = crate::string::js_string_from_bytes(b"key_number_03".as_ptr(), 13)
            as *const crate::StringHeader;
        // First delete: the keys array is transition-cache-shared, so this
        // clones + compacts (ownership transfer). Only the SECOND delete can
        // tombstone — that is the intended amortization for shared shapes.
        let first = crate::string::js_string_from_bytes(b"key_number_11".as_ptr(), 13)
            as *const crate::StringHeader;
        assert_eq!(super::delete_rest::js_object_delete_field(obj, first), 1);
        assert_eq!(super::shapes::object_shape_hole_count(obj), 0);
        let pre_tombstone_shape = super::shapes::object_shape_stamp(obj);
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, victim_ptr),
            1
        );
        // #9064 kept the ShapeId here and made every emitted read compare the
        // loaded slot against TAG_HOLE instead. A delete is now a SHAPE
        // TRANSITION, so the id moves and the predecessor is retired — see
        // `delete_transition_retires_the_shape_a_cache_was_primed_on` for the
        // property that buys.
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            pre_tombstone_shape,
            "owned ordinary tombstone delete must transition the receiver ShapeId"
        );
        let obj_gc = crate::value::addr_class::try_read_gc_header(obj as usize)
            .expect("a freshly allocated object must carry a readable GcHeader");
        assert_ne!(
            obj_gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES,
            0,
            "stable tombstone receiver must advertise per-slot IC validation"
        );
        assert_eq!(
            super::shapes::object_shape_hole_count(obj),
            1,
            "flag-on delete of an owned 19-key object must tombstone, not compact"
        );
        let bits = crate::value::POINTER_TAG | (obj as u64 & crate::value::POINTER_MASK);
        // The dangerous call: pre-fix this dereferenced the hole bits as a
        // StringHeader. Surviving it AND not templating the deleted key is
        // the contract.
        if let Some(t) = crate::json::stringify_shape_template::build_shape_prefix_template(bits) {
            assert!(
                !t.prefixes.iter().any(|p| p.contains("key_number_03")),
                "template must not resurrect a tombstoned key"
            );
        }
        // Structured clone must skip the hole too: round-trip and check the
        // rebuilt object has exactly the 18 live keys, neither deleted one.
        let payload = crate::child_process::v8_serde::v8_serialize(f64::from_bits(bits));
        let back = crate::child_process::v8_serde::v8_deserialize(&payload);
        let back_obj =
            crate::value::js_nanbox_get_pointer(back) as *const crate::object::ObjectHeader;
        let back_keys = crate::object::object_keys(back_obj).arr();
        assert_eq!(
            crate::array::keys_array_len_capped_to_capacity(back_keys),
            18,
            "structured clone must not serialize tombstoned slots"
        );
    }
}

/// The squeeze threshold reads `hole_count` off the CURRENT shape stamp, so
/// every publish that follows a tombstone — including the append publish a
/// re-add takes — must carry the count forward. A reset would mean
/// delete/re-add churn never squeezes and the keys array grows unbounded
/// (the 2x-live-size bound in the design doc, and the memory-parity rule).
#[test]
fn tombstone_hole_count_survives_readd_append() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("hc_key_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        // First delete clones the cache-shared array (ownership transfer),
        // second delete tombstones.
        for victim in [&b"hc_key_11"[..], &b"hc_key_03"[..]] {
            let vp = crate::string::js_string_from_bytes(victim.as_ptr(), victim.len() as u32)
                as *const crate::StringHeader;
            assert_eq!(super::delete_rest::js_object_delete_field(obj, vp), 1);
        }
        assert_eq!(super::shapes::object_shape_hole_count(obj), 1);
        let tombstoned_shape = super::shapes::object_shape_stamp(obj);
        // Re-add: appends (enumeration order moves the key to the end) and
        // must NOT reset the hole accounting. The append changes the layout,
        // so it publishes a successor shape: a shape id names exactly one
        // layout (step 2.5), the same rule a delete follows. Only the hole
        // accounting is carried across the publish.
        let readd = crate::string::js_string_from_bytes(b"hc_key_03".as_ptr(), 9);
        js_object_set_field_by_name(obj, readd, 99.0);
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            tombstoned_shape,
            "re-add changed the layout but kept the tombstoned ShapeId"
        );
        assert_eq!(
            super::shapes::object_shape_hole_count(obj),
            1,
            "append publish dropped hole_count: squeeze threshold broken"
        );
        // Sustained churn: with the count carried, the threshold must trip
        // and physically squeeze — the array stays within 2x live size
        // (plus growth-capacity slack) instead of growing one slot per cycle.
        for c in 0..60 {
            let _ = c;
            let vp = crate::string::js_string_from_bytes(b"hc_key_05".as_ptr(), 9)
                as *const crate::StringHeader;
            assert_eq!(super::delete_rest::js_object_delete_field(obj, vp), 1);
            let k = crate::string::js_string_from_bytes(b"hc_key_05".as_ptr(), 9);
            js_object_set_field_by_name(obj, k, 5.0);
        }
        let keys = crate::object::object_keys(obj).arr();
        let stored = crate::array::keys_array_len_capped_to_capacity(keys);
        assert!(
            stored <= 40,
            "churned 20-key object stores {stored} key slots: squeeze never tripped"
        );
    }
}

/// Object literals are compiler-registered anonymous-shape classes rather
/// than `class_id == 0` allocations. They are ordinary receivers, not real
/// class instances, and are the exact representation used by #9064's repro.
///
/// What the marker certifies is the SLOT REPRESENTATION (inline slots may
/// hold `TAG_HOLE`, and a re-add appends into the private array in place), not
/// the shape identity: the delete still transitions the ShapeId.
#[test]
fn anonymous_shape_object_literal_uses_stable_tombstone_slots() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        const ANON_ID: u32 = 0x6E06_4001;
        crate::object::js_register_anon_shape_class_id(ANON_ID);
        let obj = js_object_alloc(ANON_ID, 0);
        for i in 0..20 {
            let name = format!("anon_key_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        for victim in [&b"anon_key_11"[..], &b"anon_key_03"[..]] {
            let key = crate::string::js_string_from_bytes(victim.as_ptr(), victim.len() as u32);
            let before = super::shapes::object_shape_stamp(obj);
            assert_eq!(super::delete_rest::js_object_delete_field(obj, key), 1);
            if victim == b"anon_key_03" {
                assert_ne!(
                    super::shapes::object_shape_stamp(obj),
                    before,
                    "registered anonymous-shape literal must transition its ShapeId on \
                     an owned delete"
                );
            }
        }
        let obj_gc = crate::value::addr_class::try_read_gc_header(obj as usize)
            .expect("a freshly allocated object must carry a readable GcHeader");
        assert_ne!(obj_gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES, 0);
    }
}

/// A descriptor installed after the receiver entered its stable tombstone
/// epoch must re-open the full delete checks. The marker is a proof about the
/// object at installation time, not permission to bypass a later
/// non-configurable attribute.
#[test]
fn stable_tombstone_marker_reopens_later_descriptor_checks() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("descriptor_key_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        for victim in [&b"descriptor_key_11"[..], &b"descriptor_key_03"[..]] {
            let key = crate::string::js_string_from_bytes(victim.as_ptr(), victim.len() as u32);
            assert_eq!(super::delete_rest::js_object_delete_field(obj, key), 1);
        }
        let obj_gc = crate::value::addr_class::try_read_gc_header(obj as usize)
            .expect("a freshly allocated object must carry a readable GcHeader");
        assert_ne!(obj_gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES, 0);

        super::descriptor_state::set_property_attrs(
            obj as usize,
            "descriptor_key_05".to_string(),
            super::descriptor_state::PropertyAttrs::new(true, true, false),
        );
        // Reacquire after the mutation: `try_read_gc_header` returns an
        // immutable view, so retaining it across the flag write would let an
        // optimized test reuse the pre-install value.
        let obj_gc = crate::value::addr_class::try_read_gc_header(obj as usize)
            .expect("the descriptor target must retain a readable GcHeader");
        assert_ne!(
            obj_gc._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
            0,
            "installing an attribute must invalidate the stable plain-data proof"
        );
        let guarded = crate::string::js_string_from_bytes(b"descriptor_key_05".as_ptr(), 17);
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, guarded),
            0,
            "stable receiver bypassed a later non-configurable descriptor"
        );
        assert_eq!(
            js_object_get_field_by_name(obj, guarded).bits(),
            5.0f64.to_bits()
        );
    }
}

/// Flag-off compaction mutates an owned keys array in place. Its post-delete
/// count can equal a historical prefix count from growth, but the shifted key
/// order makes that old ShapeId semantically stale (#9064 differential pin).
#[test]
fn tombstone_off_compaction_does_not_reuse_growth_prefix_shape() {
    super::delete_rest::test_set_tombstone_deletes(Some(false));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..99 {
            let name = format!("compact_key_{i:03}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        let growth_prefix = super::shapes::object_shape_stamp(obj);
        let last = crate::string::js_string_from_bytes(b"compact_key_099".as_ptr(), 15);
        js_object_set_field_by_name(obj, last, 99.0);

        let victim = crate::string::js_string_from_bytes(b"compact_key_050".as_ptr(), 15)
            as *const crate::StringHeader;
        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            growth_prefix,
            "in-place compaction reused a growth-era prefix ShapeId"
        );
        let shifted = crate::string::js_string_from_bytes(b"compact_key_098".as_ptr(), 15);
        assert_eq!(
            js_object_get_field_by_name(obj, shifted).bits(),
            98.0f64.to_bits()
        );
    }
}

/// A one-live-key receiver used to miss tombstones entirely: transition-cache
/// insertion eagerly marked every freshly appended keys array shared, so each
/// delete cloned+compacted it and the next append repeated the cycle. The first
/// delete now forks one owned tombstone so the stable-token re-add path can
/// keep that private layout out of the transition cache.
#[test]
fn small_churn_first_delete_forks_owned_tombstone() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let mut obj = js_object_alloc(0, 0);
        let first = crate::string::js_string_from_bytes(b"small_0".as_ptr(), 7);
        js_object_set_field_by_name(obj, first, 0.0);

        let first_delete = crate::string::js_string_from_bytes(b"small_0".as_ptr(), 7);
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, first_delete),
            1
        );
        let owned_keys = crate::object::object_keys(obj).arr();
        assert_eq!(
            crate::array::keys_array_len_capped_to_capacity(owned_keys),
            1
        );
        assert_eq!(super::shapes::object_shape_hole_count(obj), 1);
        let keys_gc = crate::value::addr_class::try_read_gc_header(owned_keys as usize).unwrap();
        assert_eq!(
            keys_gc.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED,
            0,
            "first small-object delete must leave a private tombstone layout"
        );

        let sso = crate::value::JSValue::try_short_string(b"k0").unwrap();
        assert!(
            crate::object::try_readd_stable_tombstone(obj, f64::from_bits(sso.bits()), 1.0,)
                .is_some()
        );
        let stable_shape = super::shapes::object_shape_stamp(obj);
        assert_eq!(
            super::delete_rest::js_object_delete_dynamic(obj, f64::from_bits(sso.bits())),
            1
        );
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            stable_shape,
            "the dynamic-key delete must transition the ShapeId too"
        );
        assert_eq!(super::shapes::object_shape_hole_count(obj), 2);

        for n in 1..=14 {
            let name = format!("k{n}");
            let next = crate::value::JSValue::try_short_string(name.as_bytes()).unwrap();
            let (_, next_obj, _) =
                crate::object::try_readd_stable_tombstone(obj, f64::from_bits(next.bits()), 1.0)
                    .expect("small stable receiver must re-add its next SSO key");
            obj = next_obj;
            assert_eq!(
                super::delete_rest::js_object_delete_dynamic(obj, f64::from_bits(next.bits())),
                1
            );
        }
        let squeezed_keys = crate::object::object_keys(obj).arr();
        assert_eq!(
            crate::array::keys_array_len_capped_to_capacity(squeezed_keys),
            0,
            "the all-holes small epoch must squeeze back to logical length zero"
        );
        assert_eq!(super::shapes::object_shape_hole_count(obj), 0);
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            stable_shape,
            "slot reuse after squeeze must retire the previous IC token"
        );
    }
}

/// #9200 pin: a flag-on tombstone publish onto a receiver a minor will not
/// enumerate must arm the successor descriptor's old-carrier gate in the same
/// breath as the stamp.
///
/// The failure this pins, traced on the gap fixture
/// (`test_gap_repsel_pshape_tower_delete.ts` under `PERRY_OBJECT_TOMBSTONES=1
/// PERRY_GC_HEAP_LIMIT=8 PERRY_GC_FORCE_EVACUATE=1`):
/// `publish_object_shape_holes` minted a fresh (`old_carrier = false`)
/// descriptor for an already-promoted receiver, stamped it, and then retired
/// the ARMED predecessor in its keys-address sweep. The receiver is invisible
/// to a minor, and a non-carrier record is walked metadata-only
/// (`scan_shape_table_rekey_mut`), so the nursery-young owned keys array had
/// no root at all: the next evacuating minor swept it while live,
/// `prune_dead_shape_keys` dropped the descriptor, and the receiver came back
/// shapeless — `Object.keys()` empty, fixed-slot reads `undefined`, silently.
///
/// A LARGE allocation is born outside the nursery through the public
/// allocator — the same "no minor ever enumerates me" population the gap
/// fixture reaches by churn-promotion, with no synthetic promotion machinery.
#[test]
fn tombstone_publish_on_untraced_receiver_arms_old_carrier() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        // Born OLD through the arena's old-gen allocator — the same "no minor
        // ever enumerates me" population the gap fixture reaches by
        // churn-promotion. (`js_object_alloc` routes through the nursery, so
        // the public allocator cannot produce this receiver in a unit test.)
        // Initialization mirrors `js_object_alloc_with_parent` exactly.
        let slots = 24usize;
        let header_size = std::mem::size_of::<crate::object::ObjectHeader>();
        let obj = crate::arena::arena_alloc_gc_old(
            header_size + slots * std::mem::size_of::<u64>(),
            8,
            crate::gc::GC_TYPE_OBJECT,
        ) as *mut crate::object::ObjectHeader;
        (*obj).class_id = 0;
        (*obj).parent_class_id = 0;
        (*obj).meta = std::ptr::null_mut();
        let fields = (obj as *mut u8).add(header_size) as *mut u64;
        for i in 0..slots {
            // GC_STORE_AUDIT(INIT): fresh unpublished storage, pointer-free.
            std::ptr::write(fields.add(i), crate::value::TAG_UNDEFINED);
        }
        crate::gc::layout_init_pointer_free(obj as *mut u8);
        super::shapes::birth_publish_object_shape(obj, slots as u32);
        assert!(
            !crate::arena::pointer_in_nursery(obj as usize),
            "precondition: the receiver must be born outside the nursery"
        );
        for i in 0..20 {
            let name = format!("key_number_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        // First delete: the keys array is transition-cache-shared and 20 keys
        // wide, so this clones + compacts (ownership transfer, no tombstone).
        let first = crate::string::js_string_from_bytes(b"key_number_11".as_ptr(), 13)
            as *const crate::StringHeader;
        assert_eq!(super::delete_rest::js_object_delete_field(obj, first), 1);
        assert_eq!(super::shapes::object_shape_hole_count(obj), 0);
        // Second delete: owned keys array, 19 >= 16 keys, holes below the
        // squeeze threshold — the O(1) tombstone lane and its
        // `publish_object_shape_holes` mint-and-retire publish.
        let second = crate::string::js_string_from_bytes(b"key_number_03".as_ptr(), 13)
            as *const crate::StringHeader;
        assert_eq!(super::delete_rest::js_object_delete_field(obj, second), 1);
        let descriptor = super::shapes::object_shape_descriptor(obj)
            .expect("the tombstone publish must leave a resolvable descriptor");
        assert_eq!(
            descriptor.hole_count, 1,
            "the second delete must take the tombstone lane"
        );
        assert!(
            descriptor.old_carrier,
            "#9200: the tombstone publish stamped a fresh descriptor onto a \
             receiver no minor enumerates without arming the old-carrier \
             gate; its young keys array has no root the shape-table scan can \
             see, and the next evacuating minor sweeps it while live"
        );
    }
}

// ── `delete` as a SHAPE TRANSITION ──────────────────────────────────────────
//
// #9064 kept the receiver's ShapeId across a tombstone delete and made every
// emitted property read compare the loaded slot against `TAG_HOLE` instead —
// four instructions on EVERY read of EVERY object, forever, so that a `(shape,
// key)` cache entry primed for a key could be retired by the slot's contents
// rather than by the shape word. A delete is now a transition: the successor
// is a pure function of `(predecessor ShapeId, deleted key, vacated slot)` and
// is never the predecessor.

/// Restores the per-thread delete-transition override on scope exit.
fn tombstone_receiver_20(prefix: &str) -> *mut crate::object::ObjectHeader {
    unsafe {
        // Inline slots for every key: the `(shape, key)` slot query these
        // tests prime through only answers when `live_inline_slot_count ==
        // logical_key_count`, so a spilled receiver would make the premise
        // assertions vacuous rather than failing them.
        let obj = js_object_alloc(0, 24);
        for i in 0..20 {
            let name = format!("{prefix}{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        // The keys array a 20-key builder lands on is transition-cache SHARED,
        // so the first delete clones + compacts (ownership transfer) and only
        // the SECOND can take the O(1) tombstone lane. Spend the transfer here
        // so each test's delete under study is the tombstoning one.
        let name = format!("{prefix}19");
        let warm = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        assert_eq!(super::delete_rest::js_object_delete_field(obj, warm), 1);
        assert_eq!(
            super::shapes::object_shape_hole_count(obj),
            0,
            "fixture premise: the ownership-transfer delete compacts, it does not tombstone"
        );
        obj
    }
}

/// THE property this lane exists to establish, and the one
/// `perry-codegen`'s read path may lean on when it drops the `TAG_HOLE`
/// compare: after a delete, the shape word the read guard compares against is
/// a DIFFERENT one, and the shape a cache entry was primed under no longer
/// resolves at all.
///
/// Asserted three ways, because a broken fast path here is silent: the stamp
/// moved, the predecessor descriptor is retired (a surviving token resolves to
/// nothing), and the `(shape, key)` slot query declines the deleted key under
/// the successor.
#[test]
fn delete_transition_retires_the_shape_a_cache_was_primed_on() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = tombstone_receiver_20("prime_key_");
        let victim = crate::string::js_string_from_bytes(b"prime_key_07".as_ptr(), 12);
        let victim_bits = crate::JSValue::string_ptr(victim).bits();

        // PRIME: the shape a per-site or global `(shape, key)` cache entry
        // would record, and the slot it would record for the victim.
        let primed_shape = super::shapes::object_shape_stamp(obj);
        assert!(
            super::shapes::is_shape_id(primed_shape),
            "fixture premise: the receiver is stamped"
        );
        assert_eq!(
            js_object_get_field_by_name(obj, victim).bits(),
            7.0f64.to_bits()
        );
        let primed_slot =
            super::shapes::js_shape_ordinary_inline_slot_for_key(primed_shape, victim_bits);
        assert_eq!(
            primed_slot, 7,
            "fixture premise: the victim resolves to a slot BEFORE the delete — \
             otherwise the negative below is vacuous"
        );

        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
        assert_eq!(
            super::shapes::object_shape_hole_count(obj),
            1,
            "fixture premise: this delete took the O(1) tombstone lane"
        );

        let successor = super::shapes::object_shape_stamp(obj);
        assert_ne!(
            successor, primed_shape,
            "a delete that keeps the ShapeId leaves every cache entry primed \
             for the deleted key matching the receiver — which is exactly why \
             the emitted read has to re-check the slot for TAG_HOLE"
        );
        assert!(
            super::shapes::shape_descriptor_by_id(primed_shape).is_none(),
            "the predecessor must be retired, so a token that outlives the \
             delete resolves to no descriptor at all"
        );
        assert_eq!(
            super::shapes::js_shape_ordinary_inline_slot_for_key(successor, victim_bits),
            -1,
            "the successor shape must not resolve the deleted key to a slot"
        );
        // ...and the JS-visible answers, which a broken fast path would keep
        // getting right while a broken SLOW path would not.
        assert!(js_object_get_field_by_name(obj, victim).is_undefined());
        let survivor = crate::string::js_string_from_bytes(b"prime_key_08".as_ptr(), 12);
        assert_eq!(
            js_object_get_field_by_name(obj, survivor).bits(),
            8.0f64.to_bits(),
            "the tombstone must not move a surviving key's slot"
        );
    }
}

/// What remains IN the vacated slot, which is the other half of what codegen
/// needs to know: a stable-tombstone receiver keeps `TAG_HOLE` there (the
/// representation #9029 introduced and every walker already skips). The shape
/// transition is what makes that value unreachable; it does not replace it.
#[test]
fn delete_transition_leaves_tag_hole_in_the_vacated_slot() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = tombstone_receiver_20("holeslot_");
        let victim = crate::string::js_string_from_bytes(b"holeslot_04".as_ptr(), 11);
        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);

        let keys = crate::object::object_keys(obj).arr();
        let (slots, slot_len) = crate::object::keys_array_dense_slots(keys);
        assert!(slot_len > 4);
        assert_eq!(
            (*slots.add(4)).to_bits(),
            crate::value::TAG_HOLE,
            "the KEY slot must carry the tombstone the walkers skip"
        );
        let fields =
            (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64;
        assert_eq!(
            *fields.add(4),
            crate::value::TAG_HOLE,
            "a stable-tombstone receiver keeps TAG_HOLE in the VALUE slot too"
        );
    }
}

/// Delete / re-add / delete again. The re-add appends (JS enumeration order
/// moves a re-added key to the end), and each delete must land on its own
/// shape: a cycle that returned to an earlier id would let a cache entry from
/// before the first delete hit after the second.
#[test]
fn delete_readd_delete_never_returns_to_an_earlier_shape() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = tombstone_receiver_20("churn_key_");
        let mut seen = Vec::new();
        seen.push(super::shapes::object_shape_stamp(obj));
        for round in 0..6u32 {
            let victim = crate::string::js_string_from_bytes(b"churn_key_02".as_ptr(), 12);
            assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
            let after_delete = super::shapes::object_shape_stamp(obj);
            assert!(
                !seen.contains(&after_delete),
                "round {round}: the delete returned to an earlier ShapeId {after_delete:#x}"
            );
            seen.push(after_delete);
            let readd = crate::string::js_string_from_bytes(b"churn_key_02".as_ptr(), 12);
            js_object_set_field_by_name(obj, readd, 100.0 + f64::from(round));
            assert_eq!(
                js_object_get_field_by_name(obj, readd).bits(),
                (100.0 + f64::from(round)).to_bits(),
                "round {round}: the re-add must be readable"
            );
        }
    }
}

/// Two receivers deleting the SAME pair of keys in OPPOSITE orders converge on
/// one layout — a tombstone leaves every survivor's slot alone, so both end
/// with holes at the same two positions — and must still each carry their own
/// ShapeId, while agreeing on every JS-visible answer.
///
/// Convergent layouts are where a delete transition could most plausibly hand
/// two histories one identity; asserting they do not is what keeps a cache
/// entry primed on one receiver from resolving against the other. (Today the
/// keys ARRAY address alone would separate them; this pins the property, not
/// the mechanism that currently provides it.)
#[test]
fn opposite_delete_orders_converge_in_layout_but_not_in_identity() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let forward = tombstone_receiver_20("order_key_");
        let backward = tombstone_receiver_20("order_key_");
        for (obj, order) in [(forward, [3usize, 9]), (backward, [9usize, 3])] {
            for slot in order {
                let name = format!("order_key_{slot:02}");
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                assert_eq!(super::delete_rest::js_object_delete_field(obj, key), 1);
            }
            assert_eq!(super::shapes::object_shape_hole_count(obj), 2);
        }
        // Same key SET, same counts, same hole count: only the ORDER of the
        // two transitions differed. The intermediate layouts differ, so the
        // identities must differ all the way down.
        assert_ne!(
            super::shapes::object_shape_stamp(forward),
            super::shapes::object_shape_stamp(backward),
            "two delete orders collapsed onto one ShapeId"
        );
        for obj in [forward, backward] {
            for slot in [3usize, 9] {
                let name = format!("order_key_{slot:02}");
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                assert!(js_object_get_field_by_name(obj, key).is_undefined());
            }
            let survivor = crate::string::js_string_from_bytes(b"order_key_10".as_ptr(), 12);
            assert_eq!(
                js_object_get_field_by_name(obj, survivor).bits(),
                10.0f64.to_bits()
            );
        }
    }
}

/// A class INSTANCE (a real `class_id`, so outside the stable-tombstone lane)
/// and a class PROTOTYPE object both keep transitioning their shape across a
/// delete. Before this lane the instance already minted a fresh id per delete
/// and only the private-literal lane preserved one, so this pins that the two
/// populations now agree.
#[test]
fn delete_transitions_the_shape_for_class_instances_and_prototypes() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let prefix = "instT_";
        let proto_prefix = "protoT_";
        let instance = js_object_alloc(0x0004_2101, 0);
        for i in 0..20 {
            let name = format!("{prefix}{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(instance, key, i as f64);
        }
        for victim in [19u32, 6] {
            let name = format!("{prefix}{victim:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let before = super::shapes::object_shape_stamp(instance);
            assert_eq!(super::delete_rest::js_object_delete_field(instance, key), 1);
            assert_ne!(
                super::shapes::object_shape_stamp(instance),
                before,
                "a class instance must transition its ShapeId on delete"
            );
        }
        let survivor_name = format!("{prefix}07");
        let survivor =
            crate::string::js_string_from_bytes(survivor_name.as_ptr(), survivor_name.len() as u32);
        assert_eq!(
            js_object_get_field_by_name(instance, survivor).bits(),
            7.0f64.to_bits(),
            "the delete moved a SURVIVING key's value"
        );

        // A plain object used as a prototype can enter the stable-tombstone
        // lane, so its second delete must also move the shape word.
        let proto = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("{proto_prefix}{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(proto, key, i as f64);
        }
        let child = js_object_alloc(0, 0);
        crate::object::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(child as i64),
            crate::value::js_nanbox_pointer(proto as i64),
        );
        // The FIRST delete meets a 20-key transition-cache-SHARED array and
        // takes the compacting path, which has always minted a fresh id; only
        // the SECOND runs the O(1) tombstone lane this change is about.
        for victim in [19u32, 6] {
            let name = format!("{proto_prefix}{victim:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let before = super::shapes::object_shape_stamp(proto);
            assert_eq!(super::delete_rest::js_object_delete_field(proto, key), 1);
            let after = super::shapes::object_shape_stamp(proto);
            assert_ne!(
                after, before,
                "an object used as a prototype must transition its ShapeId on delete"
            );
        }
        let inherited_name = format!("{proto_prefix}06");
        let inherited = crate::string::js_string_from_bytes(
            inherited_name.as_ptr(),
            inherited_name.len() as u32,
        );
        assert!(
            js_object_get_field_by_name(child, inherited).is_undefined(),
            "the delete must be visible through the prototype chain"
        );
    }
}

/// A receiver carrying a DESCRIPTOR is outside the stable-tombstone lane
/// (`OBJ_FLAG_HAS_DESCRIPTORS` re-opens the full semantic checks). Its delete
/// must still transition, and a non-configurable key must still refuse.
#[test]
fn delete_with_descriptors_transitions_and_still_refuses_non_configurable() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("desc_key_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        super::descriptor_state::set_property_attrs(
            obj as usize,
            "desc_key_05".to_string(),
            super::descriptor_state::PropertyAttrs::new(true, true, false),
        );
        let locked = crate::string::js_string_from_bytes(b"desc_key_05".as_ptr(), 11);
        let before = super::shapes::object_shape_stamp(obj);
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, locked),
            0,
            "a non-configurable key must refuse"
        );
        assert_eq!(
            super::shapes::object_shape_stamp(obj),
            before,
            "a REFUSED delete must not move the shape word"
        );
        let victim = crate::string::js_string_from_bytes(b"desc_key_11".as_ptr(), 11);
        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
        assert_ne!(
            super::shapes::object_shape_stamp(obj),
            before,
            "a descriptor-carrying receiver must transition its ShapeId on delete"
        );
        assert_eq!(
            js_object_get_field_by_name(obj, locked).bits(),
            5.0f64.to_bits()
        );
    }
}

/// ShapeId CONSUMPTION, which no output ever reveals: ids come from a 2^30
/// counter that is never reused and fail-stops at the end, so a path minting
/// one per delete has a process LIFETIME, not just a memory cost.
///
/// Delete/re-add churn spends ShapeIds over the life of a process, because
/// each re-add reaches a layout that address has never held before.
#[test]
fn delete_shape_id_consumption_per_delete_is_measured() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    const CYCLES: u32 = 200;

    fn churn(prefix: &str, cycles: u32) -> u32 {
        let obj = js_object_alloc(0, 0);
        for i in 0..20 {
            let name = format!("{prefix}{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        let warm = format!("{prefix}19");
        let warm_key = crate::string::js_string_from_bytes(warm.as_ptr(), warm.len() as u32);
        assert_eq!(super::delete_rest::js_object_delete_field(obj, warm_key), 1);

        let before = super::shapes::test_shape_id_counter();
        for round in 0..cycles {
            let name = format!("{prefix}{:02}", round % 10);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            assert_eq!(super::delete_rest::js_object_delete_field(obj, key), 1);
            let readd = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, readd, f64::from(round));
        }
        super::shapes::test_shape_id_counter() - before
    }

    let transitioned = churn("idcount_", CYCLES);
    // The transition mints one id per delete, by construction.
    assert!(
        transitioned >= CYCLES,
        "the transition must mint at least one ShapeId per delete \
         ({transitioned} over {CYCLES} cycles) — fewer would mean some delete \
         kept the predecessor id"
    );
}

/// The interning key includes the keys array's ADDRESS, so two receivers that
/// delete the same key from the same predecessor shape do NOT share the
/// successor: each forks a private tombstone array first. This test states
/// that limit rather than leaving it a silent gap — it is the concrete reason
/// `delete` cannot be memoized into a shared transition today, and it FAILS
/// (correctly, asking to be rewritten as `assert_eq!`) the day shape identity
/// becomes address-free.
#[test]
fn two_receivers_do_not_share_a_delete_successor_because_facts_carry_the_address() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let a = js_object_alloc(0, 0);
        let b = js_object_alloc(0, 0);
        for obj in [a, b] {
            for i in 0..4 {
                let name = format!("share_key_{i:02}");
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                js_object_set_field_by_name(obj, key, i as f64);
            }
        }
        assert_eq!(
            super::shapes::object_shape_stamp(a),
            super::shapes::object_shape_stamp(b),
            "premise: two identically built receivers share ONE shape"
        );
        let mut successors = Vec::new();
        for obj in [a, b] {
            let key = crate::string::js_string_from_bytes(b"share_key_01".as_ptr(), 12);
            assert_eq!(super::delete_rest::js_object_delete_field(obj, key), 1);
            successors.push(super::shapes::object_shape_stamp(obj));
        }
        let descriptors: Vec<_> = successors
            .iter()
            .map(|&id| {
                super::shapes::shape_descriptor_by_id(id)
                    .expect("each successor must resolve to a descriptor")
            })
            .collect();
        assert_ne!(
            descriptors[0].keys, descriptors[1].keys,
            "premise: each receiver forked its OWN tombstone array — that fork \
             is what makes the successors distinct"
        );
        assert_ne!(
            successors[0], successors[1],
            "two receivers deleting the same key from the same predecessor do \
             not share a successor today. When shape facts stop carrying the \
             keys array's address, this becomes assert_eq! and `delete` gains a \
             memoized, shared transition."
        );
    }
}

/// The successor's identity is a PURE FUNCTION of the transition, not a draw
/// from the process-wide counter.
///
/// Nothing consumes that today — the successor descriptor is minted DETACHED
/// from exact-facts interning, because its facts name an owned keys array no
/// second receiver can ever present — so without this assertion the
/// determinism would be unfalsifiable and a future edit could quietly swap in
/// `SHAPE_SEMANTIC_NEXT` with no test noticing. The deterministic namespace is
/// bit 63; the counter starts at 1 and aborts long before it could reach 2^63,
/// so the bit separates them exactly.
#[test]
fn the_delete_successor_generation_is_deterministic_not_a_counter_draw() {
    super::delete_rest::test_set_tombstone_deletes(Some(true));
    let _restore = scopeguard_tombstone_flag();
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = tombstone_receiver_20("purefn_");
        let victim = crate::string::js_string_from_bytes(b"purefn_06".as_ptr(), 9);
        assert_eq!(super::delete_rest::js_object_delete_field(obj, victim), 1);
        assert_eq!(
            super::shapes::object_shape_hole_count(obj),
            1,
            "fixture premise: the O(1) tombstone lane"
        );
        let successor =
            super::shapes::shape_descriptor_by_id(super::shapes::object_shape_stamp(obj))
                .expect("the delete must publish a resolvable descriptor");
        assert_ne!(
            successor.semantic_generation & (1u64 << 63),
            0,
            "the delete successor drew a COUNTER generation ({:#x}): the \
             transition is no longer a pure function of (predecessor, key, \
             slot), so two receivers performing the same delete can never \
             agree on a successor",
            successor.semantic_generation
        );

        // A DIFFERENT key from the same receiver must land elsewhere: the key
        // and the vacated slot are both folded in, so the two successors
        // cannot collide.
        let obj2 = tombstone_receiver_20("purefn2_");
        let other = crate::string::js_string_from_bytes(b"purefn2_07".as_ptr(), 10);
        assert_eq!(super::delete_rest::js_object_delete_field(obj2, other), 1);
        let successor2 =
            super::shapes::shape_descriptor_by_id(super::shapes::object_shape_stamp(obj2))
                .expect("the delete must publish a resolvable descriptor");
        assert_ne!(
            successor.semantic_generation, successor2.semantic_generation,
            "two different (predecessor, key, slot) transitions folded to one \
             generation"
        );
    }
}
