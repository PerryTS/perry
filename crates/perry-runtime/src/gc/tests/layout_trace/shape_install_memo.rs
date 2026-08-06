//! #7510 item 1 — the construction-side shape-install memo.
//!
//! The memo lets `js_gc_init_typed_shape_layout` skip the `SHAPE_LAYOUTS`
//! round-trip for a shape whose canonical descriptor is already installed, and
//! reduce the construction to the two header bits that install would have set.
//! Three things therefore have to be true, and each has a test here:
//!
//! 1. **It fires.** #7525's first commit was worth nothing because its fast
//!    path hit once in 40 million calls, and that was found by counting, not by
//!    reasoning. [`memo_fires_on_every_repeat_construction_of_one_shape`]
//!    counts.
//! 2. **It decides nothing.** The header declaration is re-derived from the
//!    mask words and the object's own field bits on every construction, hit or
//!    miss — [`a_memo_hit_produces_the_same_header_state_as_the_install`] and
//!    [`a_contradicting_field_is_refused_even_with_the_memo_warm`].
//! 3. **It heals.** The one transition that can falsify an entry —
//!    `SHAPE_LAYOUTS` poisoning a shape to ambiguous — drops the table:
//!    [`ambiguity_poison_invalidates_the_memo`].
//!
//! And because this is collector-facing metadata, a witness that objects built
//! through the fast path survive an evacuating minor *with their children*
//! ([`memo_installed_objects_survive_a_copying_minor_with_their_children`]),
//! next to the sabotage arm that shows what a wrong declaration would cost
//! ([`a_pointer_free_declaration_on_this_shape_strands_the_child`]).

use super::*;
use crate::gc::shape_install;

/// Distinct class ids per test: `js_build_class_keys_array` caches by class id
/// and anchors the result for the process lifetime, so sharing one id across
/// tests would share one shape — and the `SHAPE_LAYOUTS` entry that goes with
/// it, which one test's ambiguity poison would then leak into another's.
const CLASS_REPEAT: u32 = 0x7510_01;
const CLASS_STATE: u32 = 0x7510_02;
const CLASS_DIVERGE: u32 = 0x7510_03;
const CLASS_AMBIGUOUS: u32 = 0x7510_04;
const CLASS_WITNESS: u32 = 0x7510_05;

/// A two-field shape, `{ n: number; s: string }`: slot 0 raw-f64, slot 1
/// pointer-bearing. The pointer half is what makes the GC witness meaningful —
/// a pointer-free shape would survive any declaration.
const RAW_MASK: [u64; 1] = [0b01];
const POINTER_MASK_WORDS: [u64; 1] = [0b10];

fn keys_for(class_id: u32) -> *mut crate::array::ArrayHeader {
    let packed: &[u8] = b"n\0s\0";
    crate::object::js_build_class_keys_array(class_id, 2, packed.as_ptr(), packed.len() as u32)
}

/// Allocate one instance of the shape and fill it: a plain double in slot 0, a
/// fresh heap string in slot 1. Returns the object and its child string.
fn build_instance(
    class_id: u32,
    keys: *mut crate::array::ArrayHeader,
    n: f64,
    bytes: &[u8],
) -> (*mut crate::object::ObjectHeader, usize) {
    let obj = crate::object::js_object_alloc_class_inline_keys(class_id, 0, 2, keys);
    let child = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize;
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(n));
    crate::object::js_object_set_field(
        obj,
        1,
        crate::value::JSValue::from_bits(string_bits(child)),
    );
    (obj, child)
}

/// The install call every test makes, with the shape's canonical mask globals.
/// Passing the SAME two slices every time is the point: their addresses are
/// what the memo keys on, exactly as codegen's per-class `private unnamed_addr
/// constant` mask globals are.
fn install(obj: *mut crate::object::ObjectHeader) {
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        RAW_MASK.as_ptr(),
        RAW_MASK.len() as u32,
        POINTER_MASK_WORDS.as_ptr(),
        POINTER_MASK_WORDS.len() as u32,
    );
}

unsafe fn layout_state(obj: *mut crate::object::ObjectHeader) -> u16 {
    let header = header_from_user_ptr(obj as *const u8);
    (*header)._reserved & (GC_LAYOUT_STATE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT)
}

/// Child slots the collector would visit **inside the object's fields**. The
/// generic count also emits the `keys_array` and `meta` header edges as
/// `Prefix` children, which every shaped object has regardless of its layout
/// declaration and which would therefore mask the very difference these tests
/// are about.
unsafe fn payload_child_count(obj: *mut crate::object::ObjectHeader) -> usize {
    test_heap_child_slots_for_user(obj as *mut u8)
        .into_iter()
        .filter(|slot| {
            matches!(
                slot,
                HeapChildSlot::Child(_, kind) if *kind != HeapChildSlotReadKind::Prefix
            )
        })
        .count()
}

/// The memo must fire on every construction after the first, or it is not
/// buying anything: a fast path that hits once per program is exactly the
/// #7525 failure this suite exists to prevent repeating.
#[test]
fn memo_fires_on_every_repeat_construction_of_one_shape() {
    let _guard = CopyingNurseryTestGuard::new(1);
    shape_install::test_clear();

    let keys = keys_for(CLASS_REPEAT);
    const INSTANCES: usize = 64;
    for i in 0..INSTANCES {
        let (obj, _) = build_instance(CLASS_REPEAT, keys, i as f64, b"repeat");
        install(obj);
        assert_eq!(
            unsafe { layout_state(obj) },
            GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT,
            "instance {i} must end up intact with a side mask, hit or miss"
        );
    }

    let (hits, records) = shape_install::counters::snapshot();
    assert_eq!(
        records, 1,
        "one shape must reach `shape_install_shared` exactly once — a second \
         record means the memo is not being consulted on the repeats"
    );
    assert_eq!(
        hits,
        (INSTANCES - 1) as u64,
        "every construction after the first must take the memo fast path"
    );
}

/// A hit and a miss must leave the object in the same state. The memo stores
/// no header bits: `POINTER_FREE` vs `SIDE_MASK` is recomputed from the
/// pointer mask on both paths, which is what keeps a stale entry from ever
/// becoming a wrong declaration.
#[test]
fn a_memo_hit_produces_the_same_header_state_as_the_install() {
    let _guard = CopyingNurseryTestGuard::new(1);
    shape_install::test_clear();

    let keys = keys_for(CLASS_STATE);

    // Miss: the table was just cleared.
    let (via_install, _) = build_instance(CLASS_STATE, keys, 1.5, b"install");
    install(via_install);
    let (hits_after_first, _) = shape_install::counters::snapshot();
    assert_eq!(hits_after_first, 0, "the first construction must be a miss");

    // Hit.
    let (via_memo, _) = build_instance(CLASS_STATE, keys, 2.5, b"memo");
    install(via_memo);
    let (hits_after_second, _) = shape_install::counters::snapshot();
    assert_eq!(hits_after_second, 1, "the second construction must be a hit");

    unsafe {
        assert_eq!(
            layout_state(via_memo),
            layout_state(via_install),
            "the memo path must publish the same layout state as the install path"
        );
    }
    for obj in [via_install, via_memo] {
        let user = obj as usize;
        assert!(layout_typed_intact_for_user(user));
        assert!(
            layout_typed_raw_f64_slot_for_user(user, 0),
            "slot 0 must resolve as raw-f64 through the shared shape descriptor"
        );
        assert!(!layout_typed_raw_f64_slot_for_user(user, 1));
        assert_eq!(
            test_layout_pointer_slot_count(user, 2),
            Some(1),
            "the collector must see exactly the one pointer slot"
        );
        assert_eq!(unsafe { payload_child_count(obj) }, 1);
    }
}

/// The memo says nothing about the object, only about the map. An instance
/// whose slot 0 holds a string contradicts the raw-f64 mask and must be
/// refused with the table warm exactly as it would be cold — the per-slot
/// validation runs on both paths, ahead of the memo.
#[test]
fn a_contradicting_field_is_refused_even_with_the_memo_warm() {
    let _guard = CopyingNurseryTestGuard::new(1);
    shape_install::test_clear();

    let keys = keys_for(CLASS_DIVERGE);
    let (good, _) = build_instance(CLASS_DIVERGE, keys, 1.0, b"conforming");
    install(good);
    let (hits_before, _) = shape_install::counters::snapshot();

    // Same shape, but slot 0 — declared raw-f64 — holds a heap string.
    let (bad, _) = build_instance(CLASS_DIVERGE, keys, 0.0, b"child");
    let boxed = crate::string::js_string_from_bytes(b"not-a-double".as_ptr(), 12) as usize;
    crate::object::js_object_set_field(bad, 0, crate::value::JSValue::from_bits(string_bits(boxed)));
    install(bad);

    assert!(
        !layout_typed_intact_for_user(bad as usize),
        "an instance that contradicts its shape's mask must not be declared \
         intact — the per-slot validation runs on the fast path too"
    );
    let (hits_after, _) = shape_install::counters::snapshot();
    assert_eq!(
        hits_after, hits_before,
        "validation must reject the instance BEFORE the memo is consulted"
    );
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(bad as *const u8))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN,
            "a refused install must fall back to the conservative scan, which is \
             the only state that keeps BOTH heap fields traceable"
        );
        assert_eq!(
            payload_child_count(bad),
            2,
            "both the boxed slot-0 string and the slot-1 child must stay enumerable"
        );
    }
    // The conforming sibling is untouched: the poison is per-object.
    assert!(layout_typed_intact_for_user(good as usize));
}

/// `Some(descriptor)` → `None` on a shape is the ONE transition that falsifies
/// a memo entry, and `shape_install_shared` is the only place it happens. The
/// poison must drop the table, or every later construction of the poisoned
/// shape would keep taking a fast path whose premise is gone.
#[test]
fn ambiguity_poison_invalidates_the_memo() {
    let _guard = CopyingNurseryTestGuard::new(1);
    shape_install::test_clear();

    let keys = keys_for(CLASS_AMBIGUOUS);
    let (first, _) = build_instance(CLASS_AMBIGUOUS, keys, 1.0, b"first");
    install(first);
    let (second, _) = build_instance(CLASS_AMBIGUOUS, keys, 2.0, b"second");
    install(second);
    let (hits_before_poison, records_before_poison) = shape_install::counters::snapshot();
    assert_eq!((hits_before_poison, records_before_poison), (1, 1));

    // Same key names, a DIFFERENT value layout: `{n, s}` where both slots are
    // ordinary JSValues. `shape_install_shared` cannot describe both, so it
    // poisons the shape to ambiguous.
    let ambiguous = crate::object::js_object_alloc_class_inline_keys(CLASS_AMBIGUOUS, 0, 2, keys);
    let a = crate::string::js_string_from_bytes(b"a".as_ptr(), 1) as usize;
    let b = crate::string::js_string_from_bytes(b"b".as_ptr(), 1) as usize;
    crate::object::js_object_set_field(ambiguous, 0, crate::value::JSValue::from_bits(string_bits(a)));
    crate::object::js_object_set_field(ambiguous, 1, crate::value::JSValue::from_bits(string_bits(b)));
    let both_pointers: [u64; 1] = [0b11];
    js_gc_init_typed_shape_layout(
        ambiguous as u64,
        2,
        std::ptr::null(),
        0,
        both_pointers.as_ptr(),
        both_pointers.len() as u32,
    );

    // A third construction of the ORIGINAL layout must now miss, and land on
    // the per-object path the poison redirects it to.
    let (third, _) = build_instance(CLASS_AMBIGUOUS, keys, 3.0, b"third");
    install(third);
    let (hits_after, _) = shape_install::counters::snapshot();
    assert_eq!(
        hits_after, hits_before_poison,
        "the poison must have dropped the memo entry; a hit here would be a fast \
         path running on a premise that no longer holds"
    );

    // Correctness of the fallback, not just its shape: the object is still
    // fully described and its child still enumerable.
    assert!(layout_typed_intact_for_user(third as usize));
    assert!(layout_typed_raw_f64_slot_for_user(third as usize, 0));
    assert_eq!(
        test_layout_pointer_slot_count(third as usize, 2),
        Some(1),
        "the per-object fallback must carry the same pointer mask"
    );
    assert_eq!(unsafe { payload_child_count(third) }, 1);
}

/// The witness. Objects whose layout was published by the memo fast path —
/// never by a `SHAPE_LAYOUTS` install — are relocated by an evacuating minor
/// together with the heap string each one holds, and their slots rewritten.
///
/// The `hits` assertion is the "subject was live" gate: without it a green run
/// would be compatible with every object having gone down the slow path.
#[test]
fn memo_installed_objects_survive_a_copying_minor_with_their_children() {
    const INSTANCES: usize = 6;
    let _guard = CopyingNurseryTestGuard::new(INSTANCES as u32);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    shape_install::test_clear();

    let keys = keys_for(CLASS_WITNESS);
    let payloads: Vec<Vec<u8>> = (0..INSTANCES)
        .map(|i| format!("witness_child_{i}").into_bytes())
        .collect();

    let mut objects = Vec::new();
    let mut children = Vec::new();
    for (i, bytes) in payloads.iter().enumerate() {
        let (obj, child) = build_instance(CLASS_WITNESS, keys, i as f64, bytes);
        install(obj);
        assert_eq!(
            unsafe { payload_child_count(obj) },
            1,
            "instance {i} must expose its one pointer field to the collector"
        );
        js_shadow_slot_set(i as u32, ptr_bits(obj as usize));
        objects.push(obj as usize);
        children.push(child);
    }

    let (hits, records) = shape_install::counters::snapshot();
    assert_eq!(records, 1);
    assert_eq!(
        hits,
        (INSTANCES - 1) as u64,
        "all but the first instance must have been published by the memo fast \
         path, or this witness is testing the slow path"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.copying_nursery.copied_objects >= INSTANCES * 2,
        "the cycle must actually have MOVED every object and every child, or the \
         relocation claim below is vacuous (copied_objects = {})",
        trace.copying_nursery.copied_objects
    );

    for i in 0..INSTANCES {
        let after = (js_shadow_slot_get(i as u32) & POINTER_MASK) as usize;
        assert_ne!(after, objects[i], "instance {i} must have been relocated");
        assert!(crate::arena::pointer_in_nursery(after));
        unsafe {
            let obj = after as *mut crate::object::ObjectHeader;
            let fields = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            assert_eq!(
                f64::from_bits(*fields),
                i as f64,
                "instance {i}'s raw-f64 slot must survive the copy verbatim"
            );
            let moved_child = (*fields.add(1) & POINTER_MASK) as usize;
            assert_ne!(
                moved_child, children[i],
                "instance {i}'s child must have been relocated and its slot rewritten"
            );
            assert!(
                crate::arena::pointer_in_nursery(moved_child),
                "instance {i}'s child must live in the to-space, not a stale address"
            );
            assert_string_bytes(moved_child as *const crate::StringHeader, &payloads[i]);
        }
        js_shadow_slot_set(i as u32, crate::value::TAG_UNDEFINED);
    }
}

/// SABOTAGE ARM, made permanent. Publishing this shape `POINTER_FREE` — the
/// other state the fast path could have chosen if it read the answer out of
/// the memo instead of recomputing it from the pointer mask — makes the
/// collector skip the whole payload and enumerate NOTHING. That is the
/// use-after-free the witness above is green against, and the reason the memo
/// deliberately stores no header state.
///
/// Asserted on the child-slot enumerator rather than by collecting, so no
/// dangling pointer is ever created; the object's state is restored before the
/// guard drops.
#[test]
fn a_pointer_free_declaration_on_this_shape_strands_the_child() {
    let _guard = CopyingNurseryTestGuard::new(1);
    shape_install::test_clear();

    let keys = keys_for(CLASS_WITNESS);
    let (obj, _) = build_instance(CLASS_WITNESS, keys, 9.0, b"sabotage_child");
    install(obj);
    unsafe {
        assert_eq!(
            payload_child_count(obj),
            1,
            "the honest declaration enumerates the pointer field"
        );

        let header = header_from_user_ptr(obj as *const u8);
        let saved = (*header)._reserved;
        set_layout_state(header, GC_LAYOUT_POINTER_FREE);
        assert_eq!(
            payload_child_count(obj),
            0,
            "a POINTER_FREE declaration skips the whole payload — the child is \
             invisible to marking AND to rewriting, which is what makes a wrong \
             state a use-after-free rather than a slowdown"
        );
        (*header)._reserved = saved;
        assert_eq!(payload_child_count(obj), 1);
    }
}
