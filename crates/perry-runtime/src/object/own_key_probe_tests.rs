//! The receiver `[[Set]]` own-key probe (#9180), split out of `tests.rs` to
//! keep it under the 2000-line cap.

use super::*;

/// #9180: the receiver-based `[[Set]]` walk's "does the receiver already own
/// this key" probe must consult the shared key index, not walk the keys array
/// one `js_array_get` at a time.
///
/// The chain is `js_put_value_set_dyn_ic_miss` -> `ordinary_set_with_receiver`
/// -> `create_or_update_receiver_property` -> `own_set_descriptor` ->
/// `obj_value_has_own_key`, reached by every store the #5054 direct-store lane
/// declines — one `Object.defineProperty` on the receiver is enough, which is
/// the esbuild CJS-namespace shape. The old loop re-read every already-installed
/// key through the JS-facing element accessor plus a handle round-trip, so
/// building an object property-by-property was quadratic: measured 163 200
/// element reads for 400 stores, versus 0 now.
///
/// Counted, not timed: `test_element_accessor_calls` is the entry counter on
/// `js_array_get_f64` itself. The assertions below cover BOTH index tiers —
/// under `KEYS_INDEX_THRESHOLD` (raw dense-slot compare) and over it (the O(1)
/// shape index) — and the two ways the index declines to answer: a delete that
/// shrinks it back to `Unindexed`, and the `Absent` completeness verdict for a
/// key that was never installed.
#[test]
fn has_own_key_probe_never_uses_the_element_accessor() {
    {
        let obj = js_object_alloc(0, 0);
        let obj_value = crate::value::js_nanbox_pointer(obj as i64);

        let key_value = |name: &str| {
            let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::value::js_nanbox_string(s as i64)
        };
        let set = |name: &str, v: f64| {
            let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, s, v);
        };

        // --- below KEYS_INDEX_THRESHOLD (32): the dense raw-slot compare.
        for i in 0..8u32 {
            set(&format!("k{i}"), f64::from(i));
        }
        let before = crate::array::test_element_accessor_calls();
        assert!(obj_value_has_own_key(obj_value, key_value("k0")));
        assert!(obj_value_has_own_key(obj_value, key_value("k7")));
        assert!(!obj_value_has_own_key(obj_value, key_value("nope")));
        assert_eq!(
            crate::array::test_element_accessor_calls(),
            before,
            "the small-object tier must answer from the dense slots, with no \
             `js_array_get_f64` call at all"
        );

        // --- across the threshold: the O(1) shape index.
        for i in 8..48u32 {
            set(&format!("k{i}"), f64::from(i));
        }
        let before = crate::array::test_element_accessor_calls();
        assert!(obj_value_has_own_key(obj_value, key_value("k0")));
        assert!(obj_value_has_own_key(obj_value, key_value("k31")));
        assert!(obj_value_has_own_key(obj_value, key_value("k47")));
        // The `Absent` verdict — a complete index proves a key is missing
        // without any scan at all. This is the arm a wrong answer here would
        // turn into a silently duplicated property.
        assert!(!obj_value_has_own_key(obj_value, key_value("k48")));
        assert!(!obj_value_has_own_key(obj_value, key_value("")));
        assert_eq!(
            crate::array::test_element_accessor_calls(),
            before,
            "the wide tier must answer from the shape index, not a per-element \
             `js_array_get_f64` walk"
        );

        // --- the index-declines arm: a delete shrinks the keys array, so the
        // next probe must fall back rather than trust a stale `Absent`, and a
        // re-add must be found again.
        let k20 = crate::string::js_string_from_bytes(b"k20".as_ptr(), 3);
        crate::object::js_object_delete_field(obj, k20);
        assert!(
            !obj_value_has_own_key(obj_value, key_value("k20")),
            "a deleted key is not an own key"
        );
        assert!(
            obj_value_has_own_key(obj_value, key_value("k21")),
            "a delete must not lose its neighbours"
        );
        set("k20", 2020.0);
        let before = crate::array::test_element_accessor_calls();
        assert!(
            obj_value_has_own_key(obj_value, key_value("k20")),
            "a re-added key must be found again after the index was dropped"
        );
        assert!(!obj_value_has_own_key(obj_value, key_value("k48")));
        assert_eq!(
            crate::array::test_element_accessor_calls(),
            before,
            "post-delete re-entry must still stay off the element accessor"
        );
    }
}
