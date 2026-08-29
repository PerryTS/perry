//! Tombstone-delete (#9029) unit pins: the template-build SEGV, the
//! structured-clone hole skip, and the hole-count accounting that keeps the
//! squeeze threshold honest under delete/re-add churn. Split from
//! `object/tests.rs` for the file-size gate.

use super::super::{js_object_alloc, js_object_set_field_by_name};

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
        assert_eq!(
            super::delete_rest::js_object_delete_field(obj, victim_ptr),
            1
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
        // Re-add: appends (enumeration order moves the key to the end) and
        // must NOT reset the hole accounting.
        let readd = crate::string::js_string_from_bytes(b"hc_key_03".as_ptr(), 9);
        js_object_set_field_by_name(obj, readd, 99.0);
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
            stored <= 45,
            "churned 20-key object stores {stored} key slots: squeeze never tripped"
        );
    }
}
