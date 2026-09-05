//! Descriptor-table GC root scanning.
//!
//! Split out of `descriptor_state.rs` to keep that file under the 2000-line
//! size gate. `scan_descriptor_roots_mut` is registered in `gc/mod.rs`.

use super::*;

/// GC scanner for the string-keyed descriptor side tables (2026-07-02 audit
/// P0; ported from the stranded be73b4f8d): `ACCESSOR_DESCRIPTORS` holds the
/// ONLY reference to `Object.defineProperty` getter/setter closures (the
/// accessor install path stores no field-slot copy), so without visiting
/// them a minor GC sweeps or moves the closure out from under the next
/// property read. Owner keys are `(obj_addr, key)` — rekeyed when the owning
/// object moves, exactly like the symbol-keyed twins, so frozen/non-writable
/// attrs and accessors don't silently detach (or fire on a new tenant at a
/// reused address).
pub(crate) fn scan_descriptor_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let st = state();
    // #9754: a minor-scoped pass visits only the young-logged owners; the
    // full walk below rebuilds the log from what it finds.
    if visitor.young_scope() {
        scan_descriptor_roots_young(visitor, st);
        return;
    }
    let table_len = st.descriptors.attr_keys_by_owner.borrow().len() as u64
        + st.descriptors.accessor_keys_by_owner.borrow().len() as u64;
    {
        // Probe DISTINCT OWNERS via the index, not every `(owner, key)` pair.
        // This runs on every GC cycle, and since the moving young-gen scavenge
        // became the default (#7019) that is often — so an O(total descriptors)
        // probe here was a per-collection tax proportional to the whole
        // program's descriptor count rather than to what actually moved.
        let needs_rebuild = st
            .descriptors
            .attr_keys_by_owner
            .borrow()
            .keys()
            .any(|owner| rewrite_descriptor_owner(visitor, *owner) != *owner);
        let mut descriptors = st.descriptors.property_descriptors.borrow_mut();
        if needs_rebuild {
            let old = std::mem::take(&mut *descriptors);
            for ((owner, key), attrs) in old {
                let owner = rewrite_descriptor_owner(visitor, owner);
                descriptors.insert((owner, key), attrs);
            }
        }
    }

    {
        let needs_rebuild = st
            .descriptors
            .accessor_keys_by_owner
            .borrow()
            .keys()
            .any(|owner| rewrite_descriptor_owner(visitor, *owner) != *owner);
        let mut descriptors = st.descriptors.accessor_descriptors.borrow_mut();
        if needs_rebuild {
            let old = std::mem::take(&mut *descriptors);
            for ((owner, key), mut acc) in old {
                if acc.get != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.get);
                }
                if acc.set != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.set);
                }
                let owner = rewrite_descriptor_owner(visitor, owner);
                descriptors.insert((owner, key), acc);
            }
        } else {
            for acc in descriptors.values_mut() {
                if acc.get != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.get);
                }
                if acc.set != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.set);
                }
            }
        }
    }

    // Rekey the owner index itself. Evacuation moved the owning objects, so
    // the tables above were rebuilt under new addresses; an index still keyed
    // by the OLD addresses would report no keys for the moved object (silently
    // dropping its accessors from `Object.keys`) and would keep a dead address
    // alive in every later scan. Merge on collision: an address freed by one
    // object can be reused by another in the same cycle.
    for index in [
        &st.descriptors.attr_keys_by_owner,
        &st.descriptors.accessor_keys_by_owner,
    ] {
        let mut idx = index.borrow_mut();
        if idx.is_empty() {
            continue;
        }
        let needs_rekey = idx
            .keys()
            .any(|owner| rewrite_descriptor_owner(visitor, *owner) != *owner);
        if !needs_rekey {
            continue;
        }
        let old = std::mem::take(&mut *idx);
        for (owner, keys) in old {
            let owner = rewrite_descriptor_owner(visitor, owner);
            let dest = idx.entry(owner).or_default();
            for k in keys {
                if !dest.iter().any(|existing| *existing == k) {
                    dest.push(k);
                }
            }
        }
    }

    // A full walk is authoritative: rebuild the young log from the tables.
    let kept = relevant_descriptor_owners(st);
    let kept_len = kept.len() as u64;
    {
        let mut log = st.descriptors.young_owners.borrow_mut();
        let _ = log.take_sorted();
        log.extend(kept);
    }
    crate::gc::young_log::note_walk(
        DESCRIPTOR_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: table_len,
            visited: table_len,
            kept: kept_len,
            table_len,
        },
    );
}

/// Visit one owner's descriptors. Returns the post-visit owner address and
/// whether the entry can still matter to a minor.
pub(crate) fn scan_descriptor_owner(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    st: &crate::state::RuntimeState,
    owner: usize,
) -> (usize, bool) {
    use crate::gc::young_log::{addr_is_minor_relevant, bits_are_minor_relevant};
    let new_owner = rewrite_descriptor_owner(visitor, owner);
    let mut relevant = false;
    let accessor_keys = st
        .descriptors
        .accessor_keys_by_owner
        .borrow()
        .get(&owner)
        .cloned()
        .unwrap_or_default();
    if !accessor_keys.is_empty() {
        let mut accessors = st.descriptors.accessor_descriptors.borrow_mut();
        for key in &accessor_keys {
            if let Some(acc) = accessors.get_mut(&(owner, key.clone())) {
                if acc.get != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.get);
                }
                if acc.set != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.set);
                }
                relevant |= bits_are_minor_relevant(acc.get) || bits_are_minor_relevant(acc.set);
            }
        }
        if new_owner != owner {
            for key in accessor_keys {
                if let Some(acc) = accessors.remove(&(owner, key.clone())) {
                    accessors.insert((new_owner, key), acc);
                }
            }
        }
    }
    if new_owner != owner {
        let attr_keys = st
            .descriptors
            .attr_keys_by_owner
            .borrow()
            .get(&owner)
            .cloned()
            .unwrap_or_default();
        if !attr_keys.is_empty() {
            let mut attrs = st.descriptors.property_descriptors.borrow_mut();
            for key in attr_keys {
                if let Some(value) = attrs.remove(&(owner, key.clone())) {
                    attrs.insert((new_owner, key), value);
                }
            }
        }
        owner_index_transfer(&st.descriptors.attr_keys_by_owner, owner, new_owner);
        owner_index_transfer(&st.descriptors.accessor_keys_by_owner, owner, new_owner);
    }
    relevant |= addr_is_minor_relevant(new_owner);
    (new_owner, relevant)
}

/// The owner index (`attr_keys_by_owner` / `accessor_keys_by_owner`) exists
/// only to answer "which keys does this owner have?" without walking every
/// descriptor in the process. It is a mirror, so the one way it can break is
/// **drift** from the tables it mirrors — which would not crash, it would
/// silently drop keys from `Object.keys` or resurrect deleted ones.
///
/// These tests therefore assert the mirror invariant directly (index ==
/// what a full scan of the table would return) across install, redefine,
/// delete, bulk-clear and owner-transfer.
#[cfg(test)]
mod owner_index_tests {
    use super::*;
    use std::collections::BTreeSet;

    /// What the pre-index implementation would have computed: a full scan of
    /// the table filtered by owner. The index must always agree with this.
    fn scan_table_keys(accessor: bool, owner: usize) -> BTreeSet<String> {
        let st = state();
        if accessor {
            st.descriptors
                .accessor_descriptors
                .borrow()
                .keys()
                .filter(|(o, _)| *o == owner)
                .map(|(_, k)| k.clone())
                .collect()
        } else {
            st.descriptors
                .property_descriptors
                .borrow()
                .keys()
                .filter(|(o, _)| *o == owner)
                .map(|(_, k)| k.clone())
                .collect()
        }
    }

    fn index_keys(accessor: bool, owner: usize) -> BTreeSet<String> {
        let st = state();
        let idx = if accessor {
            &st.descriptors.accessor_keys_by_owner
        } else {
            &st.descriptors.attr_keys_by_owner
        };
        idx.borrow()
            .get(&owner)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    fn assert_mirrors(owner: usize, ctx: &str) {
        for (accessor, label) in [(false, "property"), (true, "accessor")] {
            assert_eq!(
                index_keys(accessor, owner),
                scan_table_keys(accessor, owner),
                "{label} owner index drifted from the table it mirrors ({ctx}); \
                 a drift here silently corrupts Object.keys / for-in output"
            );
        }
    }

    #[test]
    fn index_mirrors_tables_across_install_redefine_and_delete() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = crate::object::js_object_alloc(0, 0);
        let addr = obj as usize;

        set_property_attrs(addr, "a".to_string(), PropertyAttrs::new(true, true, true));
        set_property_attrs(addr, "b".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(addr, "g".to_string(), AccessorDescriptor::default());
        assert_mirrors(addr, "after installs");

        // Redefining an existing key must not duplicate it — a duplicate would
        // make `Object.keys` report the key twice.
        set_property_attrs(addr, "a".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(addr, "g".to_string(), AccessorDescriptor::default());
        assert_eq!(
            state()
                .descriptors
                .attr_keys_by_owner
                .borrow()
                .get(&addr)
                .map(|v| v.len()),
            Some(2),
            "redefining an existing descriptor must not push a duplicate key"
        );
        assert_mirrors(addr, "after redefine");

        clear_property_attrs(addr, "a");
        clear_accessor_descriptor(addr, "g");
        assert_mirrors(addr, "after delete");

        // Deleting the last key must drop the owner entry entirely, so a dead
        // owner leaves nothing for later GC scans to walk.
        clear_property_attrs(addr, "b");
        assert!(
            !state()
                .descriptors
                .attr_keys_by_owner
                .borrow()
                .contains_key(&addr),
            "an owner with no remaining descriptors must be removed from the index"
        );
    }

    #[test]
    fn accessor_keys_for_obj_agrees_with_a_full_scan() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = crate::object::js_object_alloc(0, 0);
        let addr = obj as usize;
        // A second owner with its own accessors: the whole point of the index
        // is that this one's keys never leak into the first one's answer.
        let other = crate::object::js_object_alloc(0, 0);
        let other_addr = other as usize;

        for k in ["z", "m", "a"] {
            set_accessor_descriptor(addr, k.to_string(), AccessorDescriptor::default());
        }
        for k in ["zz", "mm"] {
            set_accessor_descriptor(other_addr, k.to_string(), AccessorDescriptor::default());
        }

        let got = accessor_descriptor_keys_for_obj(addr);
        assert_eq!(
            got,
            vec!["a".to_string(), "m".to_string(), "z".to_string()],
            "keys must be sorted and scoped to the requested owner only"
        );
        assert_eq!(
            got.into_iter().collect::<BTreeSet<_>>(),
            scan_table_keys(true, addr),
            "the index answer must equal what a full table scan would return"
        );
    }

    #[test]
    fn transfer_moves_both_tables_and_the_index() {
        let _lock = crate::gc::global_side_table_test_lock();
        let old = crate::object::js_object_alloc(0, 0) as usize;
        let new = crate::object::js_object_alloc(0, 0) as usize;

        set_property_attrs(old, "p".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(old, "acc".to_string(), AccessorDescriptor::default());

        transfer_descriptor_owner(old, new);

        assert_mirrors(old, "old owner after transfer");
        assert_mirrors(new, "new owner after transfer");
        assert!(
            scan_table_keys(false, old).is_empty() && scan_table_keys(true, old).is_empty(),
            "transfer must leave nothing behind under the old owner address"
        );
        assert_eq!(
            accessor_descriptor_keys_for_obj(new),
            vec!["acc".to_string()],
            "accessors must be readable through the new owner address after growth"
        );
    }

    #[test]
    fn clear_object_descriptors_empties_the_index_too() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = crate::object::js_object_alloc(0, 0) as usize;
        // `clear_object_descriptors` early-returns unless a handle-band owner
        // has ever taken a descriptor; set the latch so the body actually runs.
        HANDLE_HAS_DESCRIPTORS.store(true, Ordering::Relaxed);

        set_property_attrs(obj, "p".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(obj, "acc".to_string(), AccessorDescriptor::default());
        assert_mirrors(obj, "before clear");

        clear_object_descriptors(obj);
        assert_mirrors(obj, "after clear");
        assert!(
            accessor_descriptor_keys_for_obj(obj).is_empty(),
            "a cleared owner must report no accessor keys"
        );
    }
}

#[cfg(test)]
mod c5a_tests {
    use super::*;

    /// #6759 C5a: a prototype-level descriptor whose key names no declared
    /// instance field must NOT flip the process-wide inline-guard disable;
    /// one whose key IS a declared field must.
    #[test]
    fn inline_guard_disable_is_per_declared_field_key() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset_class_field_inline_guard();

        let proto = crate::object::js_object_alloc(0, 0);
        class_registry::class_prototype_object_root_store(0x0666_0001, proto);
        let proto_addr = proto as usize;

        // Method-style install (babel output): key declared by no class.
        set_accessor_descriptor(
            proto_addr,
            "c5a_render_method".to_string(),
            AccessorDescriptor::default(),
        );
        assert!(
            class_field_inline_guard_enabled(),
            "a prototype install keyed by a non-field name must not poison \
             the inline class-field fast path"
        );
        assert!(
            !class_registry::class_prototype_fast_guards_invalidated(),
            "a keyed prototype descriptor must not retire every method guard"
        );
        let render_slot = class_registry::class_prototype_method_guard_slot("c5a_render_method");
        assert!(
            class_registry::class_prototype_fast_guard_invalidated_for_method(render_slot),
            "a prototype descriptor must retire its matching method guard"
        );
        let other_slot = class_registry::class_prototype_method_guard_slot("c5a_other_method");
        assert!(
            !class_registry::class_prototype_fast_guard_invalidated_for_method(other_slot),
            "an unrelated method guard must remain valid"
        );

        // Field-style install: key declared by a registered class.
        note_declared_instance_field_name(b"c5a_field_x");
        assert!(
            class_field_inline_guard_enabled(),
            "declaring the field alone (no matching install) must not disable"
        );
        set_property_attrs(
            proto_addr,
            "c5a_field_x".to_string(),
            PropertyAttrs::new(false, true, true),
        );
        assert!(
            !class_field_inline_guard_enabled(),
            "a prototype install keyed by a DECLARED field must disable"
        );

        test_reset_class_field_inline_guard();
    }

    /// #6759 C5a ordering: an install that precedes the declaring class's
    /// registration is retro-checked when the class arrives.
    #[test]
    fn inline_guard_retro_disable_on_late_class_registration() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset_class_field_inline_guard();

        let proto = crate::object::js_object_alloc(0, 0);
        class_registry::class_prototype_object_root_store(0x0666_0002, proto);

        set_accessor_descriptor(
            proto as usize,
            "c5a_late_field".to_string(),
            AccessorDescriptor::default(),
        );
        assert!(
            class_field_inline_guard_enabled(),
            "no class declares the key yet — install must skip the disable"
        );

        // The declaring class registers AFTER the install.
        note_declared_instance_field_name(b"c5a_late_field");
        assert!(
            !class_field_inline_guard_enabled(),
            "late class registration must retro-trigger the disable for \
             prototype keys installed earlier"
        );

        test_reset_class_field_inline_guard();
    }
}

#[cfg(test)]
pub(crate) fn test_property_descriptor_entry_count(obj: usize) -> usize {
    state()
        .descriptors
        .property_descriptors
        .borrow()
        .keys()
        .filter(|(owner, _)| *owner == obj)
        .count()
}

#[cfg(test)]
mod string_wrapper_index_attrs_tests {
    use super::*;

    fn boxed(text: &str) -> usize {
        let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = f64::from_bits(crate::value::JSValue::string_ptr(s).bits());
        let boxed = crate::builtins::js_boxed_string_new(value, 1);
        crate::value::js_nanbox_get_pointer(boxed) as usize
    }

    /// The index descriptors of a `String` exotic object are answered from the
    /// wrapper's payload, not from `PROPERTY_DESCRIPTORS`. Both halves matter:
    /// the ANSWER must still be the spec's
    /// `{ writable: false, enumerable: true, configurable: false }` (delete
    /// this synthesis and `str[0] = "x"` starts mutating the wrapper), and the
    /// STORAGE must be one entry — `length` — however long the string is
    /// (that is the allocation this exists to remove).
    #[test]
    fn in_range_indices_are_synthesized_and_not_stored() {
        let obj = boxed("hello world");
        for index in ["0", "1", "10"] {
            let attrs = get_property_attrs(obj, index)
                .unwrap_or_else(|| panic!("index {index} must have a descriptor"));
            assert!(!attrs.writable(), "index {index} is not writable");
            assert!(attrs.enumerable(), "index {index} is enumerable");
            assert!(!attrs.configurable(), "index {index} is not configurable");
        }
        assert_eq!(
            test_property_descriptor_entry_count(obj),
            1,
            "only `length` is stored; the 11 index descriptors are synthesized"
        );
    }

    /// Out of range, non-canonical, and non-index keys get the ordinary
    /// answer, so the synthesis cannot invent properties the object does not
    /// have. `"01"` and `"1.0"` are NOT canonical index strings.
    #[test]
    fn only_canonical_in_range_indices_are_synthesized() {
        let obj = boxed("abc");
        assert!(get_property_attrs(obj, "3").is_none(), "past the end");
        assert!(get_property_attrs(obj, "01").is_none(), "not canonical");
        assert!(get_property_attrs(obj, "1.0").is_none(), "not canonical");
        assert!(get_property_attrs(obj, "").is_none());
        assert!(get_property_attrs(obj, "toString").is_none());
        assert!(
            get_property_attrs(obj, "0").is_some(),
            "the positive control: the same call answers for a real index"
        );
    }

    /// Nothing but a String wrapper answers. A plain object with an index-named
    /// property keeps the JS default (writable, enumerable, configurable), which
    /// is what `None` means to every caller.
    #[test]
    fn a_plain_object_is_never_treated_as_a_string_wrapper() {
        let obj = crate::object::js_object_alloc(0, 1) as usize;
        let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
        crate::object::js_object_set_field_by_name(obj as *mut _, key, 1.0);
        assert!(get_property_attrs(obj, "0").is_none());
        assert!(get_property_attrs(0, "0").is_none(), "null address");
    }

    /// A REAL entry still wins: `Object.defineProperty` / `Object.freeze` on a
    /// wrapper installs one, and the synthesized default must not shadow it.
    #[test]
    fn a_stored_descriptor_overrides_the_synthesized_one() {
        let obj = boxed("xy");
        set_property_attrs(obj, "1".to_string(), PropertyAttrs::new(true, false, true));
        let attrs = get_property_attrs(obj, "1").expect("stored entry");
        assert!(attrs.writable() && !attrs.enumerable() && attrs.configurable());
        let other = get_property_attrs(obj, "0").expect("synthesized entry");
        assert!(!other.writable() && other.enumerable() && !other.configurable());
    }
}
