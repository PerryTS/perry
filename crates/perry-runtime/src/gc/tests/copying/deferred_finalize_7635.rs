//! #7635 — is a `POINTER_FREE` MISDECLARATION detectable at all?
//!
//! Invariant under test:
//!
//! > `layout_finish_deferred_boxed_object(ptr, saw_pointer)` is the ONLY thing
//! > that moves a materialiser-built record off its `GC_LAYOUT_POINTER_FREE`
//! > birth state. When any pointer was stored, that call is load-bearing for GC
//! > correctness: `heap_payload_slot_selection` short-circuits on `POINTER_FREE`
//! > and skips the WHOLE payload without consulting any mask, so a record left
//! > in that state neither keeps its children alive nor has its slots rewritten
//! > when they move.
//!
//! # Why this file exists rather than an end-to-end probe
//!
//! #7633 deferred the JSON materialiser's per-slot layout notes to one
//! finalize. Auditing it, #7635 sabotaged that finalize to
//! `(ptr, /* saw_pointer */ false)` — every parsed record claiming
//! `POINTER_FREE` while holding heap strings — and got **byte-identical correct
//! output** from a 4,000-record Perry-compiled workload under `PERRY_GC_ZEAL=1
//! PERRY_GC_PROTECT_FROMSPACE=1` and under `PERRY_GC_FORCE_EVACUATE=1`, with
//! copying minors and retired quarantine sets observed live. Those three knobs
//! do not discriminate this hazard, for a structural reason worth stating once:
//!
//! - `PERRY_GC_PROTECT_FROMSPACE` faults on a *deref of a retired page*. A
//!   stranded child is only dereferenced if the mutator happens to read that
//!   field again after the retirement, and only while the address is still
//!   inside the bounded quarantine (`…_DEPTH`, default 4).
//! - `PERRY_GC_VERIFY_EVACUATION` walks the same enumeration the rewrite pass
//!   walks — which is to say it asks this very layout state which slots exist.
//!   It is blind to a misdeclaration by construction. (`gc/fromspace_scan.rs`'s
//!   module header makes the same point about the verifier generally.)
//! - `PERRY_GC_FORCE_EVACUATE` only makes survivors MOVE; moving harder does
//!   not make an un-enumerated slot enumerable.
//!
//! Two things do discriminate it, and both are used here:
//!
//! 1. **The child-slot enumerator itself** — `gc_child_slots` is the single
//!    question every collector pass funnels through, so asking it directly is
//!    deterministic regardless of GC timing, conservative-scan residue, or
//!    whether a copying minor happened to run.
//! 2. **Relocation** — after a copying minor that actually moved things, a
//!    traced child has a NEW address and the holding slot says so. A stranded
//!    child's slot still holds its pre-cycle address. That comparison does not
//!    depend on reading the stale memory, which is why it is stable where a
//!    poison/deref instrument is not.
//!
//! The layout-independent end-to-end instrument is `PERRY_GC_FROMSPACE_SCAN=1`
//! (whole-payload word scan, no root enumeration), which reports a stranded
//! child as `dangling=`. It is the knob #7635's audit was missing.
//!
//! [`a_misdeclared_pointer_free_record_strands_its_child`] is the SABOTAGE ARM,
//! made permanent: it performs the identical construction with the finalize's
//! `saw_pointer` forced to `false` and asserts the child is stranded. A green
//! run of the positive test therefore means the finalize was load-bearing, not
//! that nothing was tried.

use super::*;

use crate::object::ObjectHeader;

const FIELD_VALUES: [&[u8]; 2] = [b"value_alpha", b"value_bravo"];

fn fresh_string(bytes: &[u8]) -> usize {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

unsafe fn field_bits(obj: *mut ObjectHeader, index: usize) -> u64 {
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    *fields.add(index)
}

unsafe fn layout_state_of(user_ptr: usize) -> u16 {
    (*header_from_user_ptr(user_ptr as *const u8))._reserved & GC_LAYOUT_STATE_MASK
}

/// Addresses of the slots the collector says it will visit inside `user_ptr`.
/// A `POINTER_FREE` payload contributes NOTHING here — that is the failure mode
/// this file guards.
unsafe fn enumerated_slot_addrs(user_ptr: usize) -> Vec<usize> {
    test_heap_child_slots_for_user(user_ptr as *mut u8)
        .into_iter()
        .filter_map(|slot| match slot {
            HeapChildSlot::Child(p, _) => Some(p as usize),
            HeapChildSlot::PointerFreeRange(_) => None,
        })
        .collect()
}

unsafe fn field_slot_addr(obj: *mut ObjectHeader, index: usize) -> usize {
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    fields.add(index) as usize
}

/// The materialiser's construction loop, byte for byte: allocate the record,
/// store each field through the layout-deferred slot helper (no per-slot note),
/// accumulate the one fact the elided notes were computing, and settle the
/// layout state once.
///
/// `honest_finalize == false` is #7635's sabotage — the exact
/// `(ptr, /* saw_pointer */ false)` mutation, expressed as an argument so both
/// arms run identical code up to that single boolean.
unsafe fn materialise_record(honest_finalize: bool) -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(0, FIELD_VALUES.len() as u32);
    assert_eq!(
        layout_state_of(obj as usize),
        GC_LAYOUT_POINTER_FREE,
        "test premise: a fresh record is born POINTER_FREE, which is why the \
         finalize is the only thing that can move it off that state"
    );
    let mut saw_pointer = false;
    for (index, bytes) in FIELD_VALUES.iter().enumerate() {
        let child = fresh_string(bytes);
        saw_pointer |=
            crate::object::store_object_field_slot_layout_deferred(obj, index, string_bits(child));
    }
    assert!(
        saw_pointer,
        "test premise: storing heap strings must be reported as pointer-bearing"
    );
    layout_finish_deferred_boxed_object(obj as usize, saw_pointer && honest_finalize);
    obj
}

/// The finalize's two exact outcomes, pinned so a refactor cannot quietly widen
/// either one. A record with no pointer stored KEEPS its `POINTER_FREE` birth
/// state (that is the state's whole value); any pointer stored lands in
/// `GC_LAYOUT_UNKNOWN`, the tag-checked scan-all-slots state — never in a mask.
#[test]
fn finalize_settles_pointer_free_or_unknown_and_nothing_else() {
    let _guard = CopyingNurseryTestGuard::new(1);
    unsafe {
        let numeric = crate::object::js_object_alloc(0, 2);
        for index in 0..2usize {
            assert!(
                !crate::object::store_object_field_slot_layout_deferred(
                    numeric,
                    index,
                    crate::value::JSValue::number(index as f64 + 1.0).bits(),
                ),
                "a number store must not be reported as pointer-bearing"
            );
        }
        layout_finish_deferred_boxed_object(numeric as usize, false);
        assert_eq!(
            layout_state_of(numeric as usize),
            GC_LAYOUT_POINTER_FREE,
            "a record that stored no pointer keeps the birth state — the \
             tracer skips its whole payload, which is what #7630 bought"
        );
        assert_eq!(
            test_heap_child_slot_count(numeric as *mut u8),
            0,
            "and the collector enumerates zero payload slots on it"
        );

        let pointered = materialise_record(/* honest_finalize = */ true);
        assert_eq!(
            layout_state_of(pointered as usize),
            GC_LAYOUT_UNKNOWN,
            "any pointer stored must settle in the conservative scan-all state"
        );
        assert!(
            !layout_has_typed_descriptor(pointered as usize),
            "the finalize routes through `layout_mark_unknown`, so a mask a \
             slow-path by-name store created mid-construction is REMOVED, not \
             stranded"
        );
    }
}

/// The positive arm. A record built exactly as the JSON materialiser builds it,
/// holding the ONLY reference to each of its children, must have every field
/// enumerated as a child edge and must survive a copying minor with both
/// children relocated and both slots rewritten.
#[test]
fn a_materialised_record_keeps_its_children_traced_and_rewritten_7635() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let obj = unsafe { materialise_record(/* honest_finalize = */ true) };
    let before: Vec<usize> = (0..FIELD_VALUES.len())
        .map(|index| unsafe { (field_bits(obj, index) & POINTER_MASK) as usize })
        .collect();
    assert_eq!(
        test_heap_child_slot_count(obj as *mut u8),
        FIELD_VALUES.len(),
        "the collector must enumerate every pointer-bearing field of a \
         finalized record; a POINTER_FREE record enumerates ZERO"
    );

    // The record's slots are now the sole path to each child.
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects >= FIELD_VALUES.len() + 1,
        "this test proves nothing unless the cycle actually MOVED the record \
         and both children (copied_objects = {})",
        trace.copying_nursery.copied_objects
    );

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ObjectHeader;
    assert_ne!(moved as usize, obj as usize, "the record itself must move");
    unsafe {
        for (index, bytes) in FIELD_VALUES.iter().enumerate() {
            let child = (field_bits(moved, index) & POINTER_MASK) as usize;
            assert_ne!(
                child, before[index],
                "field {index} must have been relocated and its slot rewritten"
            );
            assert!(
                crate::arena::pointer_in_nursery(child)
                    || crate::arena::pointer_in_old_gen(child),
                "field {index} must name a live heap object, not a stale address"
            );
            assert_string_bytes(child as *const crate::StringHeader, bytes);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

/// The same invariant driven through the REAL entry point, `js_json_parse`, so
/// the finalize CALL SITE in `json/parser.rs` is covered and not merely the
/// helper it calls. #7635's sabotage was applied at that call site, and the two
/// tests above would stay green through it.
///
/// The parsed record holds the only reference to each of its string values —
/// only the KEYS are interned into the (rooted) parse-key cache — so a
/// misdeclared record strands them.
#[test]
fn json_parse_record_keeps_its_string_values_traced_and_rewritten_7635() {
    let _guard = CopyingNurseryTestGuard::new(1);

    // Values are 11 bytes, well above `SHORT_STRING_MAX_LEN`, so they are real
    // heap `StringHeader`s in the nursery — collectable and movable — rather
    // than inline short strings that no layout state could strand.
    let text = br#"{"alpha":"value_alpha","bravo":"value_bravo"}"#;
    let parsed =
        unsafe { crate::json::js_json_parse(crate::string::js_string_from_bytes(
            text.as_ptr(),
            text.len() as u32,
        )) };
    js_shadow_slot_set(0, parsed.bits());

    let obj = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ObjectHeader;
    let before: Vec<usize> = (0..FIELD_VALUES.len())
        .map(|index| unsafe { (field_bits(obj, index) & POINTER_MASK) as usize })
        .collect();
    unsafe {
        assert_ne!(
            layout_state_of(obj as usize),
            GC_LAYOUT_POINTER_FREE,
            "a parsed record holding heap strings must NOT be left in the \
             birth state — that is #7635's sabotage"
        );
        for (index, bytes) in FIELD_VALUES.iter().enumerate() {
            assert_eq!(
                field_bits(obj, index) & TAG_MASK,
                STRING_TAG,
                "test premise: field {index} must hold a HEAP string"
            );
            assert_string_bytes(before[index] as *const crate::StringHeader, bytes);
        }
        let enumerated = enumerated_slot_addrs(obj as usize);
        for index in 0..FIELD_VALUES.len() {
            assert!(
                enumerated.contains(&field_slot_addr(obj, index)),
                "the collector must enumerate parsed field {index} as a child \
                 edge; it reported {enumerated:?}"
            );
        }
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects >= FIELD_VALUES.len() + 1,
        "this test proves nothing unless the cycle actually MOVED the record \
         and both values (copied_objects = {})",
        trace.copying_nursery.copied_objects
    );

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ObjectHeader;
    unsafe {
        for (index, bytes) in FIELD_VALUES.iter().enumerate() {
            let child = (field_bits(moved, index) & POINTER_MASK) as usize;
            assert_ne!(
                child, before[index],
                "parsed field {index} must have been relocated and its slot \
                 rewritten"
            );
            assert_string_bytes(child as *const crate::StringHeader, bytes);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

/// SABOTAGE ARM, made permanent — #7635's exact mutation.
///
/// The identical construction with the finalize's `saw_pointer` forced to
/// `false`: the record stays `POINTER_FREE`, `heap_payload_slot_selection`
/// skips the whole payload, and the collector enumerates NOTHING. That is the
/// stranded-live-child hazard, and asserting it here is what makes the positive
/// test above a detector rather than a formality.
///
/// Asserted on the ENUMERATOR, not through a collection, so nothing here leaves
/// a stale pointer behind for a later cycle on this thread.
///
/// If a future change makes the collector reach these children anyway — a
/// conservative payload sweep, a layout-independent rescue pass — this test
/// goes red. That is the intended signal, not a nuisance: it would mean the
/// `POINTER_FREE` trace-skip had stopped being load-bearing, and both this file
/// and the doc comment on `GC_LAYOUT_POINTER_FREE` would need rewriting.
#[test]
fn a_misdeclared_pointer_free_record_strands_its_child() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let honest = unsafe { materialise_record(/* honest_finalize = */ true) };
    let sabotaged = unsafe { materialise_record(/* honest_finalize = */ false) };

    unsafe {
        assert_eq!(
            layout_state_of(sabotaged as usize),
            GC_LAYOUT_POINTER_FREE,
            "the sabotage must actually leave the record misdeclared, or this \
             arm tests nothing"
        );
        assert_eq!(
            test_heap_child_slot_count(honest as *mut u8),
            FIELD_VALUES.len()
        );
        assert_eq!(
            test_heap_child_slot_count(sabotaged as *mut u8),
            0,
            "a POINTER_FREE record skips its whole payload — the children are \
             invisible to marking, to the evacuation rewrite, and to the \
             remembered-set scan alike"
        );

        // Same fields, same bits, same stores: only the finalize differed.
        for index in 0..FIELD_VALUES.len() {
            assert_eq!(
                field_bits(sabotaged, index) & TAG_MASK,
                STRING_TAG,
                "field {index} really does hold a heap string in both arms"
            );
        }

        // Leave no stale-pointer landmine: put the misdeclared record back into
        // the conservative state before the guard drops.
        layout_mark_unknown(sabotaged as *mut u8);
        assert_eq!(
            test_heap_child_slot_count(sabotaged as *mut u8),
            FIELD_VALUES.len(),
            "and the very same record becomes fully enumerable the moment its \
             layout state is corrected — the state is the ONLY difference"
        );
    }
}
