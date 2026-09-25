//! `ObjectMeta` / descriptor-meta / shape-record survival and rekey tests
//! across copied minors and full GCs. Split out of `dead_owner_side_tables.rs`
//! to keep it under the 2,000-line cap (#10750); pure relocation.

use super::*;

/// #6759 Phase B: a shaped object's recorded `[[Prototype]]` lives in its
/// per-object `ObjectMeta` record. A copied minor moves the owner, the meta
/// record, AND the prototype object; the header's meta edge and the
/// record's prototype slot must both be rewritten so the moved owner still
/// resolves the moved prototype.
/// #10868 step 2.5 stage 1: a dictionary-mode receiver's private ordered key
/// list is a traced, REWRITTEN child edge of its meta record, like `spill`.
///
/// The pin for the one `visit` in `layout_slot_visit`'s `ObjectMeta` arm —
/// the single enumerator the minor mark, the full mark, the copying-nursery
/// evacuation, the whole-heap rewrite and the dirty-slot rescan all drive.
/// Removing it is SILENT everywhere else: nothing enumerates `ObjectMeta`'s
/// fields (no derive, no registry; `validate_gc_type_info` pairs the type
/// KINDS, never the slot lists), which is how `expando` came to be missing
/// from the second enumerator in `gc/layout.rs`. Sabotage-verified: with the
/// `visit` removed the key list is never evacuated and the "must itself
/// move" assertion below reddens.
#[test]
fn test_object_meta_dictionary_keys_survive_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _restore = {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                crate::object::dictionary::test_arm_latch(None);
            }
        }
        Restore
    };

    // EIGHT slots, not zero. `alloc_nursery_test_object(0)` allocates a
    // payload of exactly `size_of::<ObjectHeader>()` with no inline slots and
    // leaves the header unstamped; every sibling fixture only ever sets a
    // PROTOTYPE on it, so nothing has written a named property to one before.
    // A named store lands in inline slot 0 or 1 — `alloc_limit` is
    // `max(live, INLINE_SLOT_FLOOR)` and the floor is 2 — which on a
    // zero-slot allocation is the NEXT CELL. That corrupted the heap and
    // SIGSEGV'd a later read, with a backtrace deep inside an unrelated
    // URLSearchParams shape probe.
    let (owner, _) = unsafe { alloc_nursery_test_object(8) };
    let old_owner = owner as usize;
    unsafe {
        for i in 0..6 {
            let name = format!("gcdict_{i:02}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(owner, key, i as f64);
        }
        assert!(
            crate::object::dictionary::latch_object_to_dictionary(owner),
            "test premise: the receiver must latch"
        );
    }
    let old_keys = unsafe { crate::object::object_keys(owner).arr() } as usize;
    assert_ne!(old_keys, 0, "test premise: the private key list exists");
    assert_eq!(
        crate::array::js_array_length(old_keys as *mut crate::array::ArrayHeader),
        6,
        "test premise: it holds the receiver's six keys"
    );

    // Read every value back BEFORE the collection. Without this the test
    // cannot tell "the move lost it" from "the latch never stored it", and
    // those need different fixes.
    for i in 0..6 {
        let name = format!("gcdict_{i:02}");
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = f64::from_bits(crate::object::js_object_get_field_by_name(owner, key).bits());
        assert_eq!(
            value, i as f64,
            "test premise: key {i} reads back after the latch"
        );
    }
    assert!(
        unsafe { crate::object::dictionary::is_dictionary(owner) },
        "test premise: READING a dictionary receiver must not un-latch it. \
         The by-name read path stamps the receiver's shape to key its field \
         cache, and for a dictionary receiver that republishes the private \
         key list as a shape — a mode that survives writes and reverts on \
         the first read."
    );

    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let new_owner = new_owner as *mut crate::object::ObjectHeader;

    assert!(
        unsafe { crate::object::dictionary::is_dictionary(new_owner) },
        "the moved receiver must still be in dictionary mode"
    );
    let new_keys = unsafe { crate::object::object_keys(new_owner).arr() } as usize;
    assert_ne!(
        new_keys, 0,
        "the meta record's dictionary_keys slot was not marked: the key list \
         was collected out from under a live object"
    );
    assert_ne!(
        new_keys, old_keys,
        "test premise: the key list must itself move, or this test cannot \
         distinguish a marked edge from a REWRITTEN one"
    );
    assert_eq!(
        crate::array::js_array_length(new_keys as *mut crate::array::ArrayHeader),
        6,
        "the rewritten key list must still hold the receiver's six keys"
    );
    // The names survived; so must the values they address.
    for i in 0..6 {
        let name = format!("gcdict_{i:02}");
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value =
            f64::from_bits(crate::object::js_object_get_field_by_name(new_owner, key).bits());
        assert_eq!(value, i as f64, "key {i} lost its value across the move");
    }

    js_shadow_slot_set(0, 0);
}

#[test]
fn test_object_meta_prototype_survives_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let (proto, _) = unsafe { alloc_nursery_test_object(0) };
    let old_owner = owner as usize;
    let old_proto = proto as usize;
    crate::object::prototype_chain::object_set_user_prototype(old_owner, ptr_bits(old_proto));
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(old_owner),
        Some(ptr_bits(old_proto)),
        "test premise: the meta-resident prototype reads back before the GC"
    );
    assert!(
        crate::object::prototype_chain::object_has_user_prototype_override(old_owner),
        "test premise: the per-instance override bit lives in the meta record"
    );
    js_shadow_slot_set(0, ptr_bits(old_owner));
    js_shadow_slot_set(1, ptr_bits(old_proto));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let new_proto = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    assert_ne!(new_proto, old_proto, "test premise: the proto must move");
    let recorded = crate::object::prototype_chain::object_static_prototype(new_owner)
        .expect("the moved owner must still resolve its recorded prototype via its meta record");
    assert_eq!(
        (recorded & POINTER_MASK) as usize,
        new_proto,
        "the meta record's prototype slot must be rewritten to the moved proto"
    );
    assert!(
        crate::object::prototype_chain::object_has_user_prototype_override(new_owner),
        "the non-pointer meta flags must travel with the copied record"
    );

    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
}

/// A class-evaluation object may be reachable only through an instance's
/// hidden ObjectMeta brand. The edge must retain and rewrite that class object
/// when a copied minor moves the owner, its metadata, and the brand together.
#[test]
fn test_object_meta_private_evaluation_brand_survives_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let (class, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        crate::object::js_object_mark_class(class as i64);
        let meta = crate::object::object_meta_ensure(owner);
        (*meta).private_evaluation_brand = ptr_bits(class as usize);
    }
    let old_owner = owner as usize;
    let old_class = class as usize;
    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let brand = unsafe {
        let meta = (*(new_owner as *mut crate::object::ObjectHeader)).meta;
        assert!(
            !meta.is_null(),
            "the moved owner must retain its meta record"
        );
        (*meta).private_evaluation_brand
    };
    let new_class = (brand & POINTER_MASK) as usize;
    assert_ne!(new_class, old_class, "test premise: the brand must move");
    assert!(
        crate::object::is_class_object_value(f64::from_bits(brand)),
        "the metadata edge must retain and rewrite the class evaluation brand"
    );

    js_shadow_slot_set(0, 0);
}

/// #6759 Phase C2: the per-key descriptor summary in the meta record gates
/// table probes — exactly (no false negatives) for installed keys, and
/// authoritatively negative for a fresh owner and for keys whose Bloom bit
/// is clear. Non-meta-capable owners (handle-band ids) stay on the
/// conservative probe-always arm, so their installs still round-trip.
#[test]
fn test_descriptor_meta_summary_gates_probes() {
    // NOTE: the guard already takes the process-global side-table lock —
    // taking `global_side_table_test_lock()` here too self-deadlocks.
    let _guard = GcTestIsolationGuard::new();

    unsafe {
        let (owner, _) = alloc_nursery_test_object(0);
        let addr = owner as usize;

        // Fresh meta-capable owner: null meta is an authoritative miss.
        assert!(
            !crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", false),
            "fresh owner must report no possible attr entry"
        );
        assert!(
            !crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", true),
            "fresh owner must report no possible accessor entry"
        );
        assert!(
            !crate::object::owner_may_have_descriptor_entries(addr, false),
            "fresh owner must report no possible entries at all"
        );

        crate::object::set_property_attrs(
            addr,
            "x".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert!(
            crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", false),
            "installed key's bit must be set"
        );
        assert!(
            crate::object::get_property_attrs(addr, "x").is_some_and(|a| !a.writable()),
            "gated getter must still return the installed attrs"
        );
        assert!(
            crate::object::get_property_attrs(addr, "unrelated").is_none(),
            "un-installed key must miss through the gate"
        );
        // Exact negative when the bits don't collide; a collision only
        // costs a (missing) probe, which the getter assertion above covers.
        let x_bit = crate::object::descriptor_state::test_descriptor_key_bit("x");
        let other_bit = crate::object::descriptor_state::test_descriptor_key_bit("unrelated");
        if x_bit != other_bit {
            assert!(
                !crate::object::descriptor_state::may_have_descriptor_entry(
                    addr,
                    "unrelated",
                    false
                ),
                "non-colliding un-installed key must be a summary miss"
            );
        }
        // The attr install must not set the ACCESSOR word.
        if x_bit != 0 {
            assert!(
                !crate::object::descriptor_state::may_have_descriptor_entry(addr, "x", true),
                "attr install must not claim a possible accessor entry"
            );
        }

        // Handle-band owner (no GC header): conservative arm, still works.
        let handle = 0x400usize;
        assert!(
            crate::object::descriptor_state::may_have_descriptor_entry(handle, "x", false),
            "non-meta-capable owner must stay conservative"
        );
        crate::object::set_property_attrs(
            handle,
            "h".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        assert!(
            crate::object::get_property_attrs(handle, "h").is_some_and(|a| !a.writable()),
            "handle-band install must round-trip via the conservative arm"
        );
        crate::object::clear_property_attrs(handle, "h");
    }
}

/// #6759 Phase C2: the summary bits live in the meta record, which moves
/// WITH its owner on a copied minor while `scan_descriptor_roots_mut`
/// rekeys the table entry to the owner's new address — the gated getter
/// must still resolve the entry at the moved address.
#[test]
fn test_descriptor_meta_summary_survives_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);
    // The scoped registry starts empty — install the scanner that rekeys
    // descriptor-table owner addresses on evacuation.
    gc_register_mutable_root_scanner(crate::object::descriptor_state::scan_descriptor_roots_mut);

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let old_addr = owner as usize;
    crate::object::set_property_attrs(
        old_addr,
        "ro".to_string(),
        crate::object::PropertyAttrs::new(false, true, false),
    );
    js_shadow_slot_set(0, ptr_bits(old_addr));

    let _ = gc_collect_minor();

    let new_addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_addr, old_addr, "test premise: the owner must move");
    assert!(
        crate::object::descriptor_state::may_have_descriptor_entry(new_addr, "ro", false),
        "summary bits must travel with the moved owner's meta record"
    );
    let attrs = crate::object::get_property_attrs(new_addr, "ro")
        .expect("rekeyed descriptor entry must resolve at the moved address");
    assert!(
        !attrs.writable() && !attrs.configurable(),
        "moved owner must keep its installed attributes"
    );
    assert!(
        crate::object::get_property_attrs(new_addr, "other").is_none(),
        "un-installed key must still miss at the moved address"
    );

    crate::object::clear_property_attrs(new_addr, "ro");
    js_shadow_slot_set(0, 0);
}

/// #8067: an owned keys array's append-reallocation may migrate its validated
/// slot-index accelerator, but must neither repoint nor eagerly delete the old
/// immutable descriptor. A sibling naming it remains valid; otherwise weak
/// post-trace pruning eventually retires it.
#[test]
fn test_shape_record_migrates_on_owned_grow() {
    let _global = global_side_table_test_lock();
    let old_addr: usize = 0xC3A0_0000_0000_1010;
    let new_addr: usize = 0xC3A0_0000_0000_2020;
    crate::object::shapes::test_seed_shape_entry(old_addr);
    let id = crate::object::shapes::test_shape_id_for_keys(old_addr)
        .expect("seeded entry must have an id");
    assert!(id != 0, "shape ids are 1-based");

    crate::object::shapes::shape_keys_grown(old_addr, new_addr as *const crate::array::ArrayHeader);

    assert!(
        !crate::object::shapes::test_shape_entry_exists(old_addr),
        "grown-away address must no longer hold the record"
    );
    assert_eq!(
        crate::object::shapes::test_shape_id_for_keys(new_addr),
        None,
        "an append-reallocation must not repoint the old immutable descriptor"
    );
    assert_eq!(
        crate::object::shapes::shape_descriptor_by_id(id)
            .expect("grow must not delete a potentially sibling-owned descriptor")
            .keys,
        old_addr as u64,
        "an append-reallocation repointed the old immutable descriptor"
    );
    // Cleanup so the seeded address can't leak into later tests.
    crate::object::shapes::shape_drop(new_addr as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(old_addr);
}

/// #6759 Phase C3a: GC evacuation MOVES a live keys array — the shape
/// table's metadata-rewrite scanner must rekey the record to the array's
/// to-space address (same pattern as the descriptor owner rekey), so a
/// wide object's slot map survives a copied minor.
#[test]
fn test_shape_record_rekeys_on_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(2);
    // The scoped registry starts empty — install the C3a rekey scanner.
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let keys = unsafe { alloc_nursery_test_array() };
    let old_addr = keys as usize;
    crate::object::shapes::test_seed_shape_entry(old_addr);
    let id = crate::object::shapes::test_shape_id_for_keys(old_addr)
        .expect("seeded entry must have an id");
    js_shadow_slot_set(0, ptr_bits(old_addr));

    let _ = gc_collect_minor();

    let new_addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_addr, old_addr, "test premise: the keys array must move");
    assert!(
        !crate::object::shapes::test_shape_entry_exists(old_addr),
        "from-space address must no longer key the record"
    );
    assert_eq!(
        crate::object::shapes::test_shape_id_for_keys(new_addr),
        Some(id),
        "the shape record must be rekeyed to the moved keys array"
    );

    crate::object::shapes::shape_drop(new_addr as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(new_addr);
    js_shadow_slot_set(0, 0);
}

/// #8067: the by-id table is weak. Churning shapes without any live object
/// owners must return the descriptor census to baseline after a full trace;
/// otherwise the descriptor's keys copy would make historical arrays immortal.
#[test]
fn test_dead_shape_descriptor_churn_returns_to_baseline_after_full_gc() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::shapes::test_clear_shape_table();
    let baseline = crate::object::shapes::test_shape_descriptor_count();
    let mut ids = Vec::new();
    for _ in 0..32 {
        let keys = unsafe { alloc_nursery_test_array() };
        ids.push(
            crate::object::shapes::shape_descriptor_ensure(keys, 0, 0)
                .expect("shape range unexpectedly exhausted"),
        );
    }
    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        baseline + ids.len(),
        "test premise: churn must publish one descriptor per distinct keys array"
    );

    full_gc_with_no_block_persistence();

    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        baseline,
        "weak descriptor table retained dead shape keys"
    );
    assert!(
        ids.into_iter()
            .all(|id| crate::object::shapes::shape_descriptor_by_id(id).is_none()),
        "a dead shape id still resolved after its keys array was reclaimed"
    );
}

/// #8112: the DESCRIPTOR record is the strong edge and the rewritten location;
/// Two siblings share one descriptor — and therefore one edge — so after
/// copied-minor evacuation that one record must be rewritten exactly once.
#[test]
fn test_shared_live_shape_descriptor_survives_and_rekeys_once() {
    let _guard = CopyingNurseryTestGuard::new(2);
    crate::object::shapes::test_clear_shape_table();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    let keys = unsafe { alloc_nursery_test_array() };
    let old_keys = keys as usize;
    let id = crate::object::shapes::shape_descriptor_ensure(keys, 0, 0)
        .expect("shape range unexpectedly exhausted");
    let (a, _) = unsafe { alloc_nursery_test_object(0) };
    let (b, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*a).parent_class_id = id;
        (*b).parent_class_id = id;
    }
    assert_eq!(
        crate::gc::test_gc_rewrite_slot_addresses(a as usize),
        Some(vec![crate::object::shapes::shape_descriptor_keys_slot(id)
            .expect("a table-resident descriptor exposes its keys word")
            as usize,]),
        "#8047: a stamped object enumerates only the authoritative descriptor edge"
    );
    assert_eq!(
        crate::gc::test_gc_rewrite_slot_addresses(b as usize).map(|slots| slots[0]),
        crate::gc::test_gc_rewrite_slot_addresses(a as usize).map(|slots| slots[0]),
        "#8112: siblings of one shape must share ONE keys edge"
    );
    js_shadow_slot_set(0, ptr_bits(a as usize));
    js_shadow_slot_set(1, ptr_bits(b as usize));

    let _ = gc_collect_minor();

    let a_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let b_after = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let descriptor = crate::object::shapes::shape_descriptor_by_id(id)
        .expect("live shared descriptor disappeared during evacuation");
    unsafe {
        assert_eq!((*a_after).parent_class_id, id);
        assert_eq!((*b_after).parent_class_id, id);
        assert_eq!(
            crate::object::object_keys(a_after).arr(),
            crate::object::object_keys(b_after).arr()
        );
        assert_ne!(crate::object::object_keys(a_after).arr() as usize, old_keys);
        assert_eq!(
            descriptor.keys,
            crate::object::object_keys(a_after).arr() as u64
        );
    }
    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        1,
        "two live siblings must retain exactly their one shared descriptor"
    );
    crate::object::shapes::shape_drop(descriptor.keys as usize as *const crate::array::ArrayHeader);
    crate::object::shapes::test_drop_shape_descriptors(descriptor.keys as usize);
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
}

/// #8074 review, retired by #8112 and kept as its regression guard.
///
/// The hazard was real under the old model: a forwarded array's from-space
/// payload holds its forwarding address, not a usable `(length, capacity)`
/// pair, and the post-visit callback CAPTURED descriptor facts from the header
/// edge — so a stale capacity word could truncate the logical count and make
/// the rewrite fail closed. There is no fact capture any more. The visitor
/// writes the record it was handed, reading no `ArrayHeader` field at all, so
/// this fixture now asserts the structural reason the hazard cannot recur:
/// rewriting through the enumerated descriptor slot lands exactly, with every
/// other fact untouched, while the array's own length/capacity words say
/// something impossible.
#[test]
fn test_forwarded_keys_capacity_cannot_disturb_the_descriptor_rewrite() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::shapes::test_clear_shape_table();

    let old_keys = unsafe { alloc_nursery_test_array() };
    let live_keys = unsafe { alloc_nursery_test_array() };
    // `set_forwarding_address` stores this pointer over the old length/capacity
    // pair. Pick a logical count one above the resulting stale capacity word,
    // so a re-derivation from the array would deterministically truncate it.
    let stale_capacity = ((live_keys as u64) >> 32) as u32;
    let logical_key_count = stale_capacity
        .checked_add(1)
        .expect("tracked array address unexpectedly fills the high u32");
    unsafe {
        (*old_keys).length = logical_key_count;
        (*old_keys).capacity = logical_key_count;
        (*live_keys).length = logical_key_count;
        (*live_keys).capacity = logical_key_count;
    }
    let id = crate::object::shapes::shape_descriptor_ensure(old_keys, logical_key_count, 0)
        .expect("shape range unexpectedly exhausted");
    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*owner).parent_class_id = id;
    }

    let old_header = unsafe { header_from_user_ptr(old_keys.cast()) } as *mut GcHeader;
    let old_flags = unsafe { (*old_header).gc_flags };
    let old_payload = unsafe { *(old_keys as *const u64) };
    unsafe {
        set_forwarding_address(old_header, live_keys.cast());
        assert_eq!((*old_keys).capacity, stale_capacity);
        assert!(
            (*old_keys).capacity < logical_key_count,
            "test premise: the stale capacity would truncate the live count"
        );
    }

    let owner_header = unsafe { header_from_user_ptr(owner.cast()) } as *mut GcHeader;
    let descriptor_keys_slot = crate::object::shapes::shape_descriptor_keys_slot(id)
        .expect("a table-resident descriptor exposes its keys word");
    let mut rewritten = 0usize;
    unsafe {
        visit_gc_layout_slot_descriptors(owner_header, &mut |descriptor| {
            descriptor.visit_slots(&mut |slot| {
                if slot.slot == descriptor_keys_slot {
                    *slot.slot = live_keys as u64;
                    rewritten += 1;
                }
            });
        });
    }
    assert_eq!(
        rewritten, 1,
        "the authoritative descriptor edge must be enumerated exactly once"
    );
    let descriptor = crate::object::shapes::shape_descriptor_by_id(id)
        .expect("rewritten live descriptor disappeared");
    assert_eq!(descriptor.keys, live_keys as u64);
    assert_eq!(
        descriptor.logical_key_count, logical_key_count,
        "#8112: nothing in the visit re-derives a count from the array, so the \
         stale capacity word cannot truncate it"
    );
    assert_eq!(descriptor.live_inline_slot_count, 0);

    unsafe {
        *(old_keys as *mut u64) = old_payload;
        (*old_header).gc_flags = old_flags;
    }
    crate::object::shapes::test_clear_shape_table();
}

/// DirtyHeaderSlotScan retains enumerated raw slot pointers between budgeted
/// work units, and descriptor-table growth in that mutator window must not
/// invalidate any saved pointer. #8067 answered that by refusing to enumerate
/// the descriptor at all; #8112 answers it by BOXING the record, so the map
/// may rehash freely while every enumerated `keys` word stays put. This test
/// is the difference between those two answers: it saves the enumeration,
/// forces a thousand insertions, and demands the identical addresses back.
#[test]
fn test_deferred_shape_slot_enumeration_survives_descriptor_table_reallocation() {
    let _guard = GcTestIsolationGuard::new();
    crate::object::shapes::test_clear_shape_table();
    let frame = js_shadow_frame_push(1);

    let keys_before = unsafe { alloc_nursery_test_array() };
    js_shadow_slot_set(0, ptr_bits(keys_before as usize));
    let id = crate::object::shapes::shape_descriptor_ensure(keys_before, 0, 0)
        .expect("shape range unexpectedly exhausted");
    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*owner).parent_class_id = id;
    }
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let saved_slots = crate::gc::test_gc_rewrite_slot_addresses(owner as usize)
        .expect("tracked object must have a rewrite descriptor");
    let descriptor_keys_slot = crate::object::shapes::shape_descriptor_keys_slot(id)
        .expect("a table-resident descriptor exposes its keys word")
        as usize;
    assert_eq!(
        saved_slots,
        vec![descriptor_keys_slot],
        "#8047: the enumeration contains only the stable descriptor record"
    );

    for i in 0..1024usize {
        let fake_keys = 0x8067_1000_0000_0000usize + i * 0x1000;
        crate::object::shapes::shape_descriptor_ensure(fake_keys as *const _, 0, 0)
            .expect("shape range unexpectedly exhausted during reallocation fixture");
    }
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    let slots_after = crate::gc::test_gc_rewrite_slot_addresses(owner_after as usize)
        .expect("rooted object must remain enumerable after table growth");
    assert_eq!(
        slots_after,
        vec![descriptor_keys_slot],
        "descriptor-table growth moved a keys word that deferred dirty-page \
         work may still be holding — the box did not keep the record put"
    );
    assert_eq!(
        crate::object::shapes::shape_descriptor_keys_slot(id).map(|slot| slot as usize),
        Some(descriptor_keys_slot),
        "1024 insertions rehashed the map and moved the record with it"
    );

    js_shadow_slot_set(0, 0);
    js_shadow_frame_pop(frame);
    crate::object::shapes::test_clear_shape_table();
}

/// #6759 Phase B: the meta record is kept alive by its owner (the header
/// edge is a traced child slot) across a full non-moving collection, and an
/// explicit-null recording is preserved.
#[test]
fn test_object_meta_null_prototype_survives_full_gc_on_live_owner() {
    let _guard = GcTestIsolationGuard::new();

    let (owner, _) = unsafe { alloc_nursery_test_object(0) };
    let addr = owner as usize;
    crate::object::prototype_chain::object_set_static_prototype(addr, crate::value::TAG_NULL);
    js_shadow_slot_set(0, ptr_bits(addr));

    full_gc();

    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(addr),
        Some(crate::value::TAG_NULL),
        "a live (rooted) owner's meta record — and its explicit-null \
         prototype — must survive a full collection"
    );
    js_shadow_slot_set(0, 0);
}
