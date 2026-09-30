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
//! # Why a unit test, when the end-to-end probe reported clean
//!
//! #7633 deferred the JSON materialiser's per-slot layout notes to one
//! finalize. Auditing it, #7635 sabotaged that finalize to
//! `(ptr, /* saw_pointer */ false)` — every parsed record claiming
//! `POINTER_FREE` while holding heap strings — and got **byte-identical correct
//! output** from a 4,000-record Perry-compiled workload under `PERRY_GC_SCHEDULE_RATE=1
//! PERRY_GC_PROTECT_FROMSPACE=1` and under `PERRY_GC_FORCE_EVACUATE=1`, with
//! copying minors and retired quarantine sets observed live.
//!
//! **The instruments were not at fault; the probe's subject never existed.**
//! `js_json_parse` routes a top-level array of 1 KB–16 MB through the LAZY TAPE
//! (`json_tape`, default since #179), so `parse_object` does not run at
//! `JSON.parse` time — it runs when an element is first read. The probe read
//! its records only *after* the churn, so every misdeclared record was
//! materialised after the last collection. A traced-object audit added to
//! `heap_payload_slot_selection` on the sabotaged build confirms it: **zero**
//! objects in `POINTER_FREE` state with pointer-bearing payload words were ever
//! handed to the collector. Nothing was stranded because nothing was there.
//!
//! Re-run so the misdeclared records actually live across a collection and
//! every instrument fires (measured on the sabotaged build, this branch):
//!
//! | arm | clean | sabotaged |
//! |---|---|---|
//! | default (lazy), read after churn | exit 0 | exit 0, byte-identical, `dangling=0` |
//! | `PERRY_JSON_TAPE=0`, read after churn | exit 0 | **SIGSEGV**; `dangling=8000 owners=4000` on the first scanned cycle; `PERRY_GC_PROTECT_FROMSPACE=1` prints `FAULT: signal 10 at 0x…` |
//! | default (lazy), records touched BEFORE the churn | exit 0 | 7,872 of 8,000 values read back wrong |
//!
//! So the end-to-end lesson is about *probe construction*, not instrument
//! capability — with one real exception. `PERRY_GC_VERIFY_EVACUATION` walks the
//! same enumeration the rewrite pass walks, i.e. it asks this very layout state
//! which slots exist, and is blind to a misdeclaration by construction;
//! `gc/fromspace_scan.rs`'s module header makes the same point about the
//! verifier generally. **`PERRY_GC_FROMSPACE_SCAN=1` is the layout-independent
//! one** — a whole-payload word scan that consults no root enumeration and no
//! layout state — and it is the knob to reach for on this hazard class.
//!
//! What this file adds on top is a detector that needs no workload at all, so
//! it cannot be defeated by a lazy path, a GC that did not happen to run, or
//! conservative-scan residue:
//!
//! 1. **The child-slot enumerator itself** — `gc_child_slots` is the single
//!    question every collector pass funnels through, so asking it directly is
//!    deterministic regardless of GC timing.
//! 2. **Relocation** — after a copying minor that actually moved things, a
//!    traced child has a NEW address and the holding slot says so. A stranded
//!    child's slot still holds its pre-cycle address, and that comparison never
//!    reads the stale memory.
//!
//! Charter step 5 retired the hazard itself: the collector traces an object
//! BY ITS SHAPE and no longer reads the layout state. A field whose shape lane
//! is `Any` is visited whatever the finalize declared.
//! [`a_misdeclared_pointer_free_record_no_longer_strands_its_child`] keeps
//! #7635's sabotage arm and asserts the opposite of what it used to: the
//! misdeclared record enumerates every field, exactly like the honest one.

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

/// What the collector enumerates is the shape's answer, not the finalize's.
/// A numeric record with `Any` lanes enumerates each of its fields (the tag
/// test rejects the numbers); the same record re-stamped with `F64` lanes
/// enumerates none, and its payload is one pointer-free range.
#[test]
fn the_shape_decides_what_a_finalized_record_enumerates() {
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
            test_heap_child_slot_count(numeric as *mut u8),
            2,
            "an `Any` lane is visited (and tag-tested) whatever the finalize said"
        );

        restamp_with_rep(numeric, f64_lanes(0..2));
        assert_eq!(
            test_heap_child_slot_count(numeric as *mut u8),
            0,
            "`F64` lanes are skipped: the collector enumerates zero payload slots"
        );
        assert!(
            test_heap_child_slots_for_user(numeric as *mut u8)
                .into_iter()
                .any(|slot| matches!(slot, HeapChildSlot::PointerFreeRange(_))),
            "an all-`F64` payload is one pointer-free range"
        );

        let pointered = materialise_record(/* honest_finalize = */ true);
        assert_eq!(
            test_heap_child_slot_count(pointered as *mut u8),
            FIELD_VALUES.len(),
            "a record holding strings enumerates every field"
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

    let obj = unsafe {
        materialise_record(/* honest_finalize = */ true)
    };
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
                crate::arena::pointer_in_nursery(child) || crate::arena::pointer_in_old_gen(child),
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
    let parsed = unsafe {
        crate::json::js_json_parse(crate::string::js_string_from_bytes(
            text.as_ptr(),
            text.len() as u32,
        ))
    };
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

/// #7635's exact mutation, made permanent — and now harmless.
///
/// The identical construction with the finalize's `saw_pointer` forced to
/// `false` leaves the record's layout state claiming `POINTER_FREE`. The
/// collector traces by the shape, whose lanes are `Any`, so it enumerates
/// every field exactly as it does for the honest record: the children are
/// visible to marking, to the evacuation rewrite and to the remembered-set
/// scan alike.
///
/// Asserted on the ENUMERATOR, not through a collection; the positive tests
/// above collect.
#[test]
fn a_misdeclared_pointer_free_record_no_longer_strands_its_child() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let honest = unsafe {
        materialise_record(/* honest_finalize = */ true)
    };
    let sabotaged = unsafe {
        materialise_record(/* honest_finalize = */ false)
    };

    unsafe {
        assert_eq!(
            layout_state_of(sabotaged as usize),
            GC_LAYOUT_POINTER_FREE,
            "the sabotage must actually leave the record misdeclared, or this \
             arm tests nothing"
        );
        // Same fields, same bits, same stores: only the finalize differed.
        for index in 0..FIELD_VALUES.len() {
            assert_eq!(
                field_bits(sabotaged, index) & TAG_MASK,
                STRING_TAG,
                "field {index} really does hold a heap string in both arms"
            );
        }
        assert_eq!(
            test_heap_child_slot_count(honest as *mut u8),
            FIELD_VALUES.len()
        );
        assert_eq!(
            test_heap_child_slot_count(sabotaged as *mut u8),
            FIELD_VALUES.len(),
            "the shape traces every `Any` field: a POINTER_FREE layout state no \
             longer hides the children"
        );
    }
}
