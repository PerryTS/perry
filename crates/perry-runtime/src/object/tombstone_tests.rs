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
        assert_eq!(
            super::shapes::object_shape_stamp(obj),
            pre_tombstone_shape,
            "owned ordinary tombstone delete must keep the receiver ShapeId stable"
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
        let back_keys = crate::object::object_keys_array(back_obj);
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
        // must NOT reset the hole accounting.
        let readd = crate::string::js_string_from_bytes(b"hc_key_03".as_ptr(), 9);
        js_object_set_field_by_name(obj, readd, 99.0);
        assert_eq!(
            super::shapes::object_shape_stamp(obj),
            tombstoned_shape,
            "same-allocation re-add must not retire the stable tombstone ShapeId"
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
        let keys = crate::object::object_keys_array(obj);
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
#[test]
fn anonymous_shape_object_literal_uses_stable_tombstone_identity() {
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
                assert_eq!(
                    super::shapes::object_shape_stamp(obj),
                    before,
                    "registered anonymous-shape literal must keep its ShapeId on owned delete"
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
