//! #8112 — the ordered-keys edge is rooted and rewritten from the ShapeId
//! descriptor, not from `ObjectHeader::keys_array`.
//!
//! Invariant under test:
//!
//! > A keys array is marked, and its pointer rewritten after a move, through
//! > the boxed `ShapeDescriptor` record named by a live receiver's ShapeId.
//! > #8047 deleted the header mirror; the descriptor edge alone must keep the
//! > keys array live and current.
//!
//! Why this needs a fixture rather than an argument. An unrooted-but-reachable
//! keys array is the quietest failure the collector has: nothing is missing at
//! the collection, and the damage surfaces cycles later as a wrong property
//! name or a `TypeError` at an unrelated call site. So every test below runs a
//! REAL copying minor, gates on `copied_objects > 0` (a cycle that moved
//! nothing exercises none of this) and on the receiver's address having
//! actually changed, and then asserts a discriminating quantity — the
//! descriptor's `keys` word before versus after.
//!
//! And the detector is sabotage-tested. `keys_edge_sabotage_is_detected`
//! re-runs the identical workload with the descriptor edge ALSO suppressed and
//! asserts the record comes back stale or pruned. Without that arm, a green run
//! here could not distinguish "the edge works" from "the keys array was kept
//! alive by something else entirely".

use super::super::*;
use super::support::{
    collect_minor_trace, complete_budgeted_gc_cycle, init_test_closure, ptr_bits,
    CopyingNurseryTestGuard, GcTriggerThresholdTestGuard,
};
use crate::object::shapes;

/// Facts the assertions compare, read exclusively through the descriptor.
#[derive(Clone, Copy, Debug)]
struct KeysEdge {
    keys: u64,
    forwarded: bool,
    is_array: bool,
    logical_key_count: u32,
    old_carrier: bool,
}

unsafe fn keys_edge_of(obj: *const crate::ObjectHeader) -> Option<KeysEdge> {
    let descriptor = shapes::object_shape_descriptor(obj)?;
    let keys = descriptor.keys;
    let (forwarded, is_array) = if keys == 0 {
        (false, false)
    } else {
        match crate::value::addr_class::try_read_tracked_gc_header(keys as usize) {
            Some(header) => (
                (*header.as_ptr()).gc_flags & GC_FLAG_FORWARDED != 0,
                (*header.as_ptr()).obj_type == GC_TYPE_ARRAY,
            ),
            None => (false, false),
        }
    };
    Some(KeysEdge {
        keys,
        forwarded,
        is_array,
        logical_key_count: descriptor.logical_key_count,
        old_carrier: descriptor.old_carrier,
    })
}

/// Build a two-key object in shadow slot `slot`, returning nothing: every later
/// read re-derives the receiver from the slot, because each store below can
/// allocate and therefore collect.
fn build_two_key_object(slot: u32, prefix: &[u8]) {
    js_shadow_slot_set(
        slot,
        ptr_bits(crate::object::js_object_alloc(0, 2) as usize),
    );
    for suffix in [b'a', b'b'] {
        let mut name = prefix.to_vec();
        name.push(suffix);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let obj = (js_shadow_slot_get(slot) & POINTER_MASK) as *mut crate::ObjectHeader;
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            crate::value::js_nanbox_pointer(key as i64),
        );
    }
}

/// The collector's own rewrite enumeration for `user_ptr`.
fn rewrite_slots(user_ptr: usize) -> Vec<usize> {
    crate::gc::test_gc_rewrite_slot_addresses(user_ptr)
        .expect("a live object has a rewrite-slot enumeration")
}

#[test]
fn the_descriptor_record_is_enumerated_as_a_child_slot() {
    let _guard = CopyingNurseryTestGuard::new(1);
    build_two_key_object(0, b"e8112_enum_");
    let obj = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;

    let descriptor = unsafe { shapes::object_shape_descriptor(obj) }
        .expect("a published object has an authoritative descriptor");
    let keys_slot = descriptor
        .keys_slot()
        .expect("a table-resident descriptor exposes the address of its own keys word")
        as usize;

    let slots = rewrite_slots(obj as usize);
    assert!(
        slots.contains(&keys_slot),
        "#8112: the collector must enumerate the DESCRIPTOR's keys word as a \
         child slot; it reported {slots:?} and the record lives at {keys_slot:#x}"
    );
    assert!(
        keys_slot < obj as usize
            || keys_slot >= obj as usize + std::mem::size_of::<crate::ObjectHeader>(),
        "#8112: the enumerated edge must be the descriptor record, not the \
         removed header word inside the receiver"
    );

    // Siblings of one shape hand the collector the SAME edge. That is the
    // ephemeron relation stated structurally: one shape, one keys array, one
    // slot, marked exactly while some live receiver still carries the id.
    build_two_key_object(0, b"e8112_enum_");
    let sibling = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    let sibling_descriptor =
        unsafe { shapes::object_shape_descriptor(sibling) }.expect("the sibling is published too");
    assert_eq!(
        sibling_descriptor.keys_slot(),
        descriptor.keys_slot(),
        "two receivers of one shape must share one descriptor record"
    );
}

/// Run the shared workload under `suppression` and report what the descriptor
/// says afterwards. Returns `None` when the cycle was not discriminating (no
/// copy, or the receiver did not move), so a caller can fail loudly instead of
/// passing vacuously.
fn collect_and_report(suppress_edge: bool) -> Option<(KeysEdge, KeysEdge)> {
    build_two_key_object(0, b"e8112_move_");
    let obj_before = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let before = unsafe { keys_edge_of(obj_before as *const crate::ObjectHeader) }
        .expect("the receiver is published before the collection");
    assert!(
        crate::arena::pointer_in_nursery(before.keys as usize),
        "#8112 test setup: the keys array must be YOUNG so a copying minor \
         actually has to move it (keys at {:#x})",
        before.keys
    );

    assert!(
        !before.old_carrier,
        "#8112 test setup: this shape must have YOUNG carriers only. Once the \
         old-carrier gate is armed, the shape table roots the keys array on its \
         own and the sabotage arm below would stop discriminating — it would \
         pass because the TABLE kept the array alive, not because the edge \
         under test did."
    );

    // Rooted young canary: keeps `copied_objects > 0` true independently of the
    // subject, so the liveness gate below cannot be satisfied by the very edge
    // under test.
    js_shadow_slot_set(1, ptr_bits(crate::object::js_object_alloc(0, 0) as usize));

    let _suppression = suppress_edge.then(shapes::TestKeysEdgeSuppression::without_descriptor_edge);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "#8112 requires a COPYING minor; a cycle that moved nothing exercises \
         no rewrite at all (copied_objects=0)"
    );

    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    if obj_after == obj_before {
        return None;
    }
    let after = unsafe { keys_edge_of(obj_after as *const crate::ObjectHeader) };
    Some((
        before,
        after.unwrap_or(KeysEdge {
            keys: 0,
            forwarded: false,
            is_array: false,
            logical_key_count: 0,
            old_carrier: false,
        }),
    ))
}

#[test]
fn a_keys_array_reachable_only_through_the_descriptor_survives_and_is_rewritten() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let (before, after) = collect_and_report(false)
        .expect("#8112: the receiver must move for this cycle to be discriminating");

    assert_ne!(
        after.keys, before.keys,
        "#8112: the receiver moved but the descriptor still names the \
         from-space keys array at {:#x} — the record was not rewritten",
        before.keys
    );
    assert!(
        !after.forwarded,
        "#8112: the descriptor names a FORWARDED header at {:#x}; the rewrite \
         followed only one hop",
        after.keys
    );
    assert!(
        after.is_array,
        "#8112: the descriptor no longer names an array after the move \
         (keys {:#x})",
        after.keys
    );
    assert_eq!(
        after.logical_key_count, before.logical_key_count,
        "#8112: the rewrite must not disturb the descriptor's other facts"
    );
}

/// #9706: the reverse indices are keyed by the keys ADDRESS. After a copying
/// minor moves the keys array, the metadata scan must re-key the family and
/// the exact-facts accelerator, so that interning the moved facts answers the
/// SAME id (a fresh id would be a duplicate descriptor per collection) and
/// the stale address answers nothing.
#[test]
fn the_reverse_indices_follow_a_moved_keys_array() {
    let _guard = CopyingNurseryTestGuard::new(2);
    // The record rewrite comes from the receiver's own edge; the re-keying
    // is the metadata scanner's job, which production registers at gc init
    // and a unit test must register itself (as the recycled-keys fixtures do).
    gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    let (before, after) = collect_and_report(false)
        .expect("#9706: the receiver must move for this cycle to be discriminating");
    assert_ne!(
        after.keys, before.keys,
        "test premise: the keys array moved"
    );
    let obj = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    let id = unsafe { shapes::object_shape_stamp(obj) };
    assert!(shapes::is_shape_id(id), "the receiver stays stamped");
    assert_eq!(
        shapes::test_shape_ids_for_keys(after.keys as usize),
        vec![id],
        "#9706: the family index must be re-keyed to the forwarded address \
         (before={:#x} after={:#x} under-before={:?} record-keys={:#x})",
        before.keys,
        after.keys,
        shapes::test_shape_ids_for_keys(before.keys as usize),
        unsafe { shapes::object_shape_descriptor(obj) }
            .map(|d| d.keys)
            .unwrap_or(0)
    );
    assert!(
        shapes::test_shape_ids_for_keys(before.keys as usize).is_empty(),
        "#9706: nothing may stay indexed under the from-space address"
    );
    let descriptor = unsafe { shapes::object_shape_descriptor(obj) }.expect("published");
    assert_eq!(descriptor.keys, after.keys);
    assert_eq!(
        shapes::shape_descriptor_ensure(
            after.keys as usize as *const crate::ArrayHeader,
            descriptor.logical_key_count,
            descriptor.live_inline_slot_count,
        ),
        Ok(id),
        "#9706: interning the moved facts must answer the existing id, not mint a duplicate"
    );
}

#[test]
fn keys_edge_sabotage_is_detected() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let (before, after) = collect_and_report(true)
        .expect("#8112: the receiver must move for this cycle to be discriminating");

    // With the descriptor edge gone, nothing marked the keys array and nothing rewrote
    // the record. The detector above must be able to SEE that: either the
    // descriptor still names the from-space address, or the dead-owner prune
    // removed the descriptor outright. What it must NOT be is a live, moved,
    // correctly-named array — that would mean the assertions in the test above
    // are satisfied by something other than the descriptor edge.
    let correctly_rewritten = after.keys != 0
        && after.keys != before.keys
        && !after.forwarded
        && after.is_array
        && after.logical_key_count == before.logical_key_count;
    assert!(
        !correctly_rewritten,
        "#8112 SABOTAGE ARM: with the descriptor edge suppressed, the keys array \
         was still rooted and rewritten \
         ({:#x} -> {:#x}). Something other than the edge under test is keeping \
         it alive, so a green run of \
         `a_keys_array_reachable_only_through_the_descriptor_survives_and_is_rewritten` \
         proves nothing.",
        before.keys, after.keys
    );
}

#[test]
fn a_keys_array_whose_last_carrier_died_is_still_reclaimed() {
    let _guard = CopyingNurseryTestGuard::new(2);
    build_two_key_object(0, b"e8112_dead_");
    let obj = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    let shape_id = unsafe { shapes::object_shape_stamp(obj) };
    assert!(shapes::shape_descriptor_by_id(shape_id).is_some());

    // Drop the only root. An immortality bug is as real as a use-after-free:
    // if the descriptor table ever became an UNCONDITIONAL root for `keys`,
    // this descriptor would outlive every object that ever carried it and the
    // dead-key prune would go circular ("is the keys array dead?" can never be
    // yes once the asker roots it).
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    js_shadow_slot_set(1, ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "#8112: a cycle that copied nothing cannot demonstrate reclamation"
    );

    assert!(
        shapes::shape_descriptor_by_id(shape_id).is_none(),
        "#8112: the descriptor for a shape no live object carries any more must \
         be reclaimed by the dead-key prune; it survived, which is what \
         unconditional table rooting would look like"
    );
}

#[derive(Clone, Copy, Debug)]
struct RecycledKeysReport {
    replacement_before: usize,
    replacement_after: usize,
    copied_objects: usize,
    old_index_exists: bool,
    new_index_exists: bool,
    descriptor_keys: Option<u64>,
}

/// Reproduce the two-cycle stale-key shape: a dead keys-array address is
/// reused by a closure, then that closure moves on the next copying minor.
/// Seeding after exact reuse isolates the cleanup under test from the earlier
/// collection's ordinary dead-owner prune.
fn collect_recycled_keys_report(suppress_type_check: bool) -> RecycledKeysReport {
    let _guard = CopyingNurseryTestGuard::new(1);
    shapes::test_clear_shape_table();
    gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead_keys = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::array::ArrayHeader>(),
        std::mem::align_of::<crate::array::ArrayHeader>(),
        GC_TYPE_ARRAY,
    ) as *mut crate::array::ArrayHeader;
    unsafe {
        (*dead_keys).length = 0;
        (*dead_keys).capacity = 0;
    }
    let recycled_addr = dead_keys as usize;

    // No roots: the array dies and the copied-minor Eden reset makes its first
    // allocation address available again.
    let _ = collect_minor_trace(GcTriggerKind::Direct);

    let replacement = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe { init_test_closure(replacement) };
    let replacement_before = replacement as usize;
    assert_eq!(
        replacement_before, recycled_addr,
        "test premise: Eden must reuse the dead keys array's exact address"
    );
    js_shadow_slot_set(0, ptr_bits(replacement_before));

    // This is the stale table state observed in the original workload: the
    // keys address now names a live object of another type.
    shapes::test_seed_shape_entry(recycled_addr);
    let shape_id = shapes::test_shape_id_for_keys(recycled_addr)
        .expect("the stale shape entry must include a descriptor");

    let trace = {
        let _sabotage = suppress_type_check.then(shapes::TestRecycledKeysCheckSuppression::new);
        collect_minor_trace(GcTriggerKind::Direct)
    };
    let replacement_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let report = RecycledKeysReport {
        replacement_before,
        replacement_after,
        copied_objects: trace.copying_nursery.copied_objects,
        old_index_exists: shapes::test_shape_entry_exists(recycled_addr),
        new_index_exists: shapes::test_shape_entry_exists(replacement_after),
        descriptor_keys: shapes::shape_descriptor_by_id(shape_id).map(|descriptor| descriptor.keys),
    };

    shapes::test_clear_shape_table();
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    report
}

#[test]
fn recycled_non_array_shape_keys_are_dropped_after_a_copying_minor() {
    let report = collect_recycled_keys_report(false);
    assert!(
        report.copied_objects > 0,
        "the fixture requires a copying minor; copied_objects was zero"
    );
    assert_ne!(
        report.replacement_after, report.replacement_before,
        "the replacement tenant must move for metadata rewrite to be exercised"
    );
    assert!(
        !report.old_index_exists && !report.new_index_exists,
        "a stale shape index must be neither retained at {:#x} nor rekeyed onto \
         the replacement closure at {:#x}",
        report.replacement_before,
        report.replacement_after
    );
    assert_eq!(
        report.descriptor_keys, None,
        "a descriptor whose keys address was recycled by a closure must be removed"
    );
}

#[test]
fn recycled_non_array_shape_keys_sabotage_is_detected() {
    let report = collect_recycled_keys_report(true);
    assert!(
        report.copied_objects > 0,
        "the sabotage fixture requires a copying minor; copied_objects was zero"
    );
    assert_ne!(
        report.replacement_after, report.replacement_before,
        "the replacement tenant must move for the sabotage to discriminate"
    );

    // Suppressing the new type check re-creates the bug: metadata follows the
    // closure's forwarding record and transfers both shape records onto it.
    assert!(
        report.new_index_exists && report.descriptor_keys == Some(report.replacement_after as u64),
        "SABOTAGE ARM: suppressing recycled-address validation did not preserve \
         and rekey the stale shape records, so the green fix arm proves nothing \
         (report: {report:?})"
    );
}

#[test]
fn dead_owner_prune_rejects_a_non_forwarded_non_array_tenant() {
    let _guard = CopyingNurseryTestGuard::new(0);
    shapes::test_clear_shape_table();

    let closure = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_CLOSURE) as usize;
    let header = unsafe { header_from_user_ptr(closure as *const u8) };
    assert_eq!(
        unsafe { (*header).gc_flags } & GC_FLAG_FORWARDED,
        0,
        "test premise: the replacement tenant must not be forwarded"
    );
    shapes::test_seed_shape_entry(closure);

    // The replacement is live according to the ordinary predicate. Its type,
    // not a forwarding flag, must still prove that it is not the keys array.
    shapes::prune_dead_shape_keys(&|_| false);

    assert!(!shapes::test_shape_entry_exists(closure));
    assert!(shapes::test_shape_id_for_keys(closure).is_none());
    shapes::test_clear_shape_table();
}

#[test]
fn metadata_rewrite_validates_the_post_visit_non_array_address() {
    let _guard = CopyingNurseryTestGuard::new(0);
    shapes::test_clear_shape_table();

    let from = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_CLOSURE) as usize;
    let valid_ptrs = build_valid_pointer_set();
    let to = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_CLOSURE) as usize;
    let from_header = unsafe { header_from_user_ptr(from as *const u8) as *mut GcHeader };
    unsafe { set_forwarding_address(from_header, to as *mut u8) };
    shapes::test_seed_shape_entry(from);

    shapes::scan_shape_table_rekey_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));

    unsafe { (*from_header).gc_flags &= !GC_FLAG_FORWARDED };
    assert!(
        !shapes::test_shape_entry_exists(from) && !shapes::test_shape_entry_exists(to),
        "the index must be dropped instead of following a closure's forwarding record"
    );
    assert!(
        shapes::test_shape_id_for_keys(from).is_none()
            && shapes::test_shape_id_for_keys(to).is_none(),
        "the descriptor must validate the rewritten address before publishing it"
    );
    shapes::test_clear_shape_table();
}

fn collect_synchronous_full_trace() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

fn build_unrooted_keyless_semantic_shape(slot: u32) -> u32 {
    js_shadow_slot_set(
        slot,
        ptr_bits(crate::object::js_object_alloc(0, 0) as usize),
    );
    let obj = (js_shadow_slot_get(slot) & POINTER_MASK) as *mut crate::ObjectHeader;
    let predecessor = unsafe { shapes::object_shape_stamp(obj) };
    let shape_id = unsafe { shapes::transition_object_shape_semantics(obj) };
    assert_ne!(shape_id, predecessor, "semantic transition must mint an id");
    assert_ne!(
        shapes::shape_descriptor_by_id(shape_id)
            .expect("semantic descriptor")
            .semantic_generation,
        0,
        "test premise: this must be a per-object semantic generation"
    );
    js_shadow_slot_set(slot, crate::value::TAG_UNDEFINED);
    shape_id
}

/// #9726: keyless semantic generations used to be immortal because dead-key
/// pruning asks about address zero, which is never a dead GC owner. A complete
/// receiver census must retire that descriptor while preserving the inverse:
/// the same kind of generation stays authoritative when its object is live.
#[test]
fn synchronous_full_trace_retires_only_uncarried_semantic_shapes() {
    let _guard = CopyingNurseryTestGuard::new(2);
    shapes::test_clear_shape_table();
    crate::arena::arena_reset_all_blocks_to_zero();
    gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);

    let dead_shape = build_unrooted_keyless_semantic_shape(0);
    collect_synchronous_full_trace();
    assert!(
        shapes::shape_descriptor_by_id(dead_shape).is_none(),
        "#9726: a keyless per-object generation with no live carrier survived a complete full trace"
    );

    build_two_key_object(0, b"e9726_live_");
    let live_before = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    let live_shape = unsafe { shapes::transition_object_shape_semantics(live_before) };
    assert_ne!(
        shapes::shape_descriptor_by_id(live_shape)
            .expect("live semantic descriptor before collection")
            .semantic_generation,
        0
    );

    collect_synchronous_full_trace();

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    assert_eq!(
        unsafe { shapes::object_shape_stamp(live_after) },
        live_shape
    );
    assert!(
        shapes::shape_descriptor_by_id(live_shape).is_some(),
        "#9726/#9200: pruning must not leave a live receiver stamped with an unresolved id"
    );
    let own_keys = crate::object::js_object_keys(live_after);
    assert_eq!(
        unsafe { (*own_keys).length },
        2,
        "#9726/#9200: Object.keys() lost the live receiver's descriptor facts"
    );

    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    shapes::test_clear_shape_table();
}

/// Incremental full marking is sliced across mutator turns, so its receiver
/// notes are deliberately not an exact liveness census. It may rotate the
/// epoch, but it must leave uncarried retirement to a synchronous full trace.
#[test]
fn budgeted_full_trace_does_not_retire_from_a_partial_carrier_census() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    shapes::test_clear_shape_table();
    crate::arena::arena_reset_all_blocks_to_zero();

    let shape_id = build_unrooted_keyless_semantic_shape(0);
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    let mut first = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut first),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(first.collection_kind, GcCollectionKind::Full.ffi_code());
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        shapes::shape_descriptor_by_id(shape_id).is_some(),
        "#9726: a budgeted trace must not retire from its partial carrier notes"
    );

    collect_synchronous_full_trace();
    assert!(
        shapes::shape_descriptor_by_id(shape_id).is_none(),
        "the next complete full trace must retire the same uncarried descriptor"
    );
    shapes::test_clear_shape_table();
}

fn build_transition_cache_target_then_drop(slot: u32) -> (u32, u32) {
    // Model the process-lifetime module global that can birth the predecessor
    // again after no receiver currently carries it.
    let predecessor = shapes::js_object_shape_id_for_keys(0, 0);
    js_shadow_slot_set(
        slot,
        ptr_bits(crate::object::js_object_alloc(0, 0) as usize),
    );
    let key = crate::string::js_string_from_bytes(b"cache6".as_ptr(), 6);
    let obj = (js_shadow_slot_get(slot) & POINTER_MASK) as *mut crate::ObjectHeader;
    assert_eq!(unsafe { shapes::object_shape_stamp(obj) }, predecessor);
    crate::object::js_object_set_field_by_name(obj, key, 9726.0);
    let obj = (js_shadow_slot_get(slot) & POINTER_MASK) as *mut crate::ObjectHeader;
    let target = unsafe { shapes::object_shape_stamp(obj) };
    assert_ne!(target, predecessor);
    js_shadow_slot_set(slot, crate::value::TAG_UNDEFINED);
    (predecessor, target)
}

/// The transition table is a real ShapeId publisher: generated write sites
/// read its target id and stamp it directly. Park a target with no receiver,
/// collect, then prove a later receiver can still take the cached transition.
#[test]
fn transition_cache_target_survives_and_can_restamp_after_full_trace() {
    let _guard = CopyingNurseryTestGuard::new(1);
    shapes::test_clear_shape_table();
    crate::arena::arena_reset_all_blocks_to_zero();
    gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);

    let (predecessor, target) = build_transition_cache_target_then_drop(0);
    collect_synchronous_full_trace();
    assert!(
        shapes::shape_descriptor_by_id(predecessor).is_some(),
        "a process-lifetime generated-code id must remain installed"
    );
    let cached = shapes::shape_descriptor_by_id(target)
        .expect("a live transition-cache entry must retain its target descriptor");
    assert!(
        cached.cache_carrier,
        "the transition target must own its id"
    );

    js_shadow_slot_set(0, ptr_bits(crate::object::js_object_alloc(0, 0) as usize));
    let key = crate::string::js_string_from_bytes(b"cache6".as_ptr(), 6);
    let consumer = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    shapes::test_watch_cached_transition_stamps(consumer as usize);
    crate::object::js_object_set_field_by_name(consumer, key, 26.0);
    assert_eq!(
        shapes::test_cached_transition_stamps(),
        1,
        "the cache path must perform the stamp"
    );
    let consumer = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    assert_eq!(
        unsafe { shapes::object_shape_stamp(consumer) },
        target,
        "the post-GC transition hit must stamp the retained target id"
    );
    assert_eq!(
        unsafe { (*crate::object::js_object_keys(consumer)).length },
        1
    );

    shapes::test_reset_cached_transition_stamps();
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    shapes::test_clear_shape_table();
}
