//! Tests for `enumeration.rs`, split out to keep it under the 2000-line gate.

use super::enumeration::*;

#[cfg(test)]
mod lazy_shadow_tests {
    use super::*;

    fn s(bytes: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    fn obj_value(o: *mut ObjectHeader) -> f64 {
        f64::from_bits(JSValue::object_ptr(o as *mut u8).bits())
    }

    /// Read a key array back as owned strings, in order.
    fn keys_of(arr: *mut ArrayHeader) -> Vec<String> {
        let mut out = Vec::new();
        let n = crate::array::js_array_length(arr);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..n {
            let kv = crate::array::js_array_get(arr, i);
            if let Some(b) = unsafe { crate::string::js_string_key_bytes(kv, &mut scratch) } {
                if let Ok(t) = std::str::from_utf8(b) {
                    out.push(t.to_string());
                }
            }
        }
        out
    }

    /// The deferred shadow set must produce the SAME key sequence as the eager
    /// one, including the case the deferral exists to skip and the case it
    /// cannot skip.
    ///
    /// This is the assertion the optimisation lives or dies on: `for_in_keys_with`
    /// is run both ways over the same object graph and the two key sequences are
    /// compared element by element. Deleting the `build_shadow_set` call, or
    /// emitting level 0 through the set instead of directly, makes the
    /// shadowing case below disagree and fails this test by name.
    #[test]
    fn deferring_the_shadow_set_does_not_change_the_key_sequence() {
        // 1. Flat object, prototype contributes nothing enumerable — the case
        //    the deferral is FOR. Both paths must agree.
        let flat = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(flat, s("alpha"), 1.0);
        crate::object::js_object_set_field_by_name(flat, s("beta"), 2.0);
        let flat_v = obj_value(flat);
        let lazy = keys_of(for_in_keys_with(flat_v, true));
        let eager = keys_of(for_in_keys_with(flat_v, false));
        assert_eq!(
            lazy, eager,
            "a flat object's for-in keys must not depend on when the shadow set is built"
        );
        assert_eq!(lazy, vec!["alpha".to_string(), "beta".to_string()]);

        // 2. Prototype WITH enumerable keys, one of them shadowed by an own
        //    property. This is the case the shadow set exists for, so the
        //    deferred build must fire and produce the same answer.
        let proto = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(proto, s("beta"), 20.0);
        crate::object::js_object_set_field_by_name(proto, s("gamma"), 30.0);
        let child = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(child, s("alpha"), 1.0);
        crate::object::js_object_set_field_by_name(child, s("beta"), 2.0);
        let child_v = obj_value(child);
        crate::object::object_ops::js_object_set_prototype_of(child_v, obj_value(proto));

        let lazy = keys_of(for_in_keys_with(child_v, true));
        let eager = keys_of(for_in_keys_with(child_v, false));
        assert_eq!(
            lazy, eager,
            "an inherited enumerable key, and an own key shadowing one on the \
             prototype, must come out identically whether the shadow set was \
             built eagerly or on demand"
        );
        // `beta` is owned by the child, so it appears once, at the child's
        // position — never again from the prototype.
        assert_eq!(
            lazy,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            "own keys first in insertion order, then unshadowed inherited ones"
        );
    }

    /// A prototype chain deeper than `VISITED_INLINE` where the ONLY
    /// level that shadows the name lives PAST the inline array.
    ///
    /// This arm never runs on the measured workload (the shadow set was built 0
    /// times in 17,266 `for-in` calls), so a test is its only coverage.
    ///
    /// # Why this test is shaped the way it is — do not "simplify" it
    ///
    /// The obvious way to write it is to give EVERY level the shadowing
    /// property, which reads as a stronger test and is not one. The first
    /// version of this test did exactly that: `marker` was owned
    /// non-enumerably at every level *including the leaf*. Deleting
    /// `VisitedLevels`' spill arm — so a rebuild cannot see any level past
    /// `INLINE` — left that test still passing, because the LEAF's own
    /// `marker` was already in the set and shadowed the root's copy on its
    /// own. The assertion was true no matter what the spill did, so it could
    /// not fail, and it certified nothing.
    ///
    /// The fix is not more levels or more assertions, it is making the
    /// spilled level the *only* thing that can produce the expected answer:
    /// levels `0..INLINE` own nothing at all, exactly one level past the
    /// inline array (`INLINE + 2`) owns `marker` non-enumerably, and only the
    /// root owns it enumerably. Now dropping the spill leaks `marker` into the
    /// result and the test fails by name — verified by making that edit.
    ///
    /// The general rule this is an instance of: after writing a test for a
    /// rarely-taken path, delete the code it covers and check the test
    /// actually fails. A test whose expected value is reachable by a second
    /// route is measuring the second route.
    #[test]
    fn only_a_spilled_level_shadows_the_root_and_the_rebuild_must_see_it() {
        let shadow_level = VISITED_INLINE + 2;
        let depth = shadow_level + 2;

        // Root (deepest): the enumerable `marker` that must stay hidden.
        let root = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(root, s("marker"), 1.0);
        crate::object::js_object_set_field_by_name(root, s("deep_only"), 2.0);

        // Build downwards from the root; `chain[i]` is at prototype level
        // `depth - 1 - i` when walked from the leaf.
        let mut chain = vec![root];
        for _ in 1..depth {
            let o = crate::object::js_object_alloc(0, 0);
            let ov = obj_value(o);
            crate::object::object_ops::js_object_set_prototype_of(
                ov,
                obj_value(*chain.last().unwrap()),
            );
            chain.push(o);
        }
        // Exactly one level shadows `marker`, and it is past the inline array.
        let shadower = chain[depth - 1 - shadow_level];
        crate::object::js_object_set_field_by_name_nonenum(shadower, s("marker"), 0.0);

        let leaf_v = obj_value(*chain.last().unwrap());
        let lazy = keys_of(for_in_keys_with(leaf_v, true));
        let eager = keys_of(for_in_keys_with(leaf_v, false));
        assert_eq!(
            lazy, eager,
            "a chain whose only shadowing level spilled past the inline array \
             must give the same keys eagerly and on demand"
        );
        assert!(
            !lazy.contains(&"marker".to_string()),
            "the only level owning `marker` sits past VISITED_INLINE, so \
             a rebuild that cannot see the spilled levels would leak the root's \
             enumerable `marker` — got {lazy:?}"
        );
        assert_eq!(lazy, vec!["deep_only".to_string()]);
    }

    /// A NON-enumerable own property still shadows the same name on the
    /// prototype (12.6.4-2). The deferred set only marks all-own-names for a
    /// level once it goes live, so this is exactly where a wrong deferral would
    /// leak the prototype's copy through.
    #[test]
    fn a_non_enumerable_own_name_still_shadows_the_prototype_under_deferral() {
        let proto = crate::object::js_object_alloc(0, 0);
        crate::object::js_object_set_field_by_name(proto, s("hidden"), 9.0);
        crate::object::js_object_set_field_by_name(proto, s("shown"), 8.0);

        let child = crate::object::js_object_alloc(0, 0);
        // Own but NOT enumerable: must not be emitted, must still shadow.
        crate::object::js_object_set_field_by_name_nonenum(child, s("hidden"), 1.0);
        let child_v = obj_value(child);
        crate::object::object_ops::js_object_set_prototype_of(child_v, obj_value(proto));

        let lazy = keys_of(for_in_keys_with(child_v, true));
        let eager = keys_of(for_in_keys_with(child_v, false));
        assert_eq!(
            lazy, eager,
            "deferral must not change shadowing by a non-enumerable own name"
        );
        assert!(
            !lazy.contains(&"hidden".to_string()),
            "a non-enumerable own `hidden` must hide the prototype's enumerable \
             `hidden` rather than letting it through — got {lazy:?}"
        );
        assert_eq!(lazy, vec!["shown".to_string()]);
    }
}
