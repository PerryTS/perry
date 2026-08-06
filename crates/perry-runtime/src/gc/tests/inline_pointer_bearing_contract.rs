//! #7511 — the contract the codegen-side inline pointer-bearing test rests on.
//!
//! `perry-codegen`'s `expr::write_barrier::emit_may_carry_heap_pointer_check`
//! puts the three per-class-field-store GC bookkeeping calls
//! (`js_write_barrier_slot`, `js_gc_note_slot_layout`,
//! `js_string_addref_if_heap_string`) behind ONE inline test of the stored
//! bits' TOP 16 BITS:
//!
//! ```text
//! may_carry_heap_pointer(bits) :=
//!       (bits >> 48) ∈ { 0x7FFA, 0x7FFD, 0x7FFF }          // NaN-boxed heap tags
//!    || ((bits >> 48) == 0 && bits >= 0x1000)              // bare heap address
//! ```
//!
//! That is sound only if it is a **superset** of what the runtime itself would
//! resolve to a heap pointer. The codegen crate cannot call
//! `decode_heap_addr` / `layout_pointer_bearing_bits` (they are private to this
//! crate, and codegen runs in a different process from the program it emits),
//! so the two halves are pinned from opposite sides:
//!
//! - `perry-codegen::nanbox::inline_pointer_bearing_top16_set_covers_every_heap_tag`
//!   pins the comparand set against the NaN-box tag constants.
//! - **this file** pins the runtime predicates against the same set, by
//!   enumerating the entire 16-bit tag space.
//!
//! A future tag that starts resolving to a heap address — the only way this
//! elision can turn into a stranded child — makes THIS test fail, on the side
//! that knows about it.

use super::super::barrier::decode_heap_addr;
use super::super::layout::layout_pointer_bearing_bits;
use super::super::*;
use super::support::*;

/// The exact tag comparands emitted by `emit_may_carry_heap_pointer_check`.
/// Kept as literals so a drift in codegen has to be mirrored here by hand
/// rather than silently inherited.
const CODEGEN_HEAP_TAG_TOP16: [u64; 3] = [0x7FFA, 0x7FFD, 0x7FFF];
/// The bare-address floor codegen emits — `layout_pointer_bearing_bits`' own
/// `0x1000`, which is the LOWER of the two runtime floors (`decode_heap_addr`
/// uses `0x10000`). Using the lower one is what makes the codegen test a
/// superset of both.
const CODEGEN_BARE_ADDR_FLOOR: u64 = 0x1000;

fn codegen_may_carry_heap_pointer(bits: u64) -> bool {
    let top16 = bits >> 48;
    CODEGEN_HEAP_TAG_TOP16.contains(&top16) || (top16 == 0 && bits >= CODEGEN_BARE_ADDR_FLOOR)
}

/// Payload patterns exercised under every tag: a plausible heap address, an
/// 8-aligned address inside the handle band, a misaligned one, zero, and an
/// all-ones payload.
const PAYLOADS: [u64; 6] = [
    0x0000_0000_0000_0000,
    0x0000_0000_0000_0008,
    0x0000_0001_0000_0000,
    0x0000_0001_0000_0007,
    0x0000_1234_5678_9AB0,
    0x0000_FFFF_FFFF_FFFF,
];

/// **The load-bearing direction.** For every one of the 65,536 possible tags
/// and a spread of payloads, a value the runtime would resolve to a heap
/// pointer must also pass the codegen test. A violation here is a barrier that
/// codegen skips and the collector needed — a stranded child.
#[test]
fn codegen_top16_test_is_a_superset_of_decode_heap_addr() {
    for tag in 0u64..=0xFFFF {
        for payload in PAYLOADS {
            let bits = (tag << 48) | payload;
            if decode_heap_addr(bits) != 0 {
                assert!(
                    codegen_may_carry_heap_pointer(bits),
                    "decode_heap_addr resolved {bits:#018x} (tag {tag:#06x}) to a heap address, \
                     but the codegen inline test would have skipped the write barrier"
                );
            }
        }
    }
}

/// Same obligation for the layout note: `js_gc_note_slot_layout` is only
/// skippable when the value is not pointer-bearing in the layout machinery's
/// own sense.
#[test]
fn codegen_top16_test_is_a_superset_of_layout_pointer_bearing_bits() {
    for tag in 0u64..=0xFFFF {
        for payload in PAYLOADS {
            let bits = (tag << 48) | payload;
            if layout_pointer_bearing_bits(bits) {
                assert!(
                    codegen_may_carry_heap_pointer(bits),
                    "layout_pointer_bearing_bits accepted {bits:#018x} (tag {tag:#06x}), but the \
                     codegen inline test would have skipped the layout note"
                );
            }
        }
    }
}

/// **Sabotage check — proves the two tests above can fail.**
///
/// Drop `POINTER_TAG` from the comparand set (the single most likely way to
/// get this wrong) and assert that a real NaN-boxed object pointer is then
/// classified as "no bookkeeping needed" while the runtime still resolves it
/// to a heap address. Without this, a green run could mean "the enumeration
/// found nothing" *or* "the enumeration never looked at a pointer".
#[test]
fn dropping_pointer_tag_from_the_set_would_strand_a_child() {
    const SABOTAGED: [u64; 2] = [0x7FFA, 0x7FFF];
    let object_bits = crate::value::POINTER_TAG | 0x0000_1234_5678_9AB0;
    assert_ne!(
        decode_heap_addr(object_bits),
        0,
        "the witness must be a value the runtime really does resolve to a heap address"
    );
    assert!(
        codegen_may_carry_heap_pointer(object_bits),
        "the shipped comparand set must keep the barrier for an object pointer"
    );
    assert!(
        !SABOTAGED.contains(&(object_bits >> 48)),
        "a comparand set missing POINTER_TAG must classify an object pointer as barrier-free — \
         if this assertion fails the superset tests above cannot detect a dropped tag either"
    );
}

/// The complement, so the elision is not vacuous: the value classes that
/// dominate a numeric store loop must be classified barrier-free, and the
/// runtime must agree that skipping their bookkeeping changes nothing.
#[test]
fn plain_numbers_and_primitives_need_no_bookkeeping() {
    let barrier_free = [
        1234.5f64.to_bits(),
        (-1234.5f64).to_bits(),
        0f64.to_bits(),
        f64::NAN.to_bits(),
        f64::INFINITY.to_bits(),
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_TRUE,
        crate::value::TAG_FALSE,
        crate::value::INT32_TAG | 42,
    ];
    for bits in barrier_free {
        assert!(
            !codegen_may_carry_heap_pointer(bits),
            "{bits:#018x} should not force the bookkeeping call"
        );
        assert_eq!(
            decode_heap_addr(bits),
            0,
            "{bits:#018x} must carry no heap address"
        );
        assert!(
            !layout_pointer_bearing_bits(bits),
            "{bits:#018x} must not be layout-pointer-bearing"
        );
    }
}

/// Perform a field store the way the guarded codegen sequence does: the slot
/// write is unconditional, and the barrier runs only when `guard` accepts the
/// stored bits. Returns whether the barrier was called.
unsafe fn guarded_field_store(
    old_obj: *mut crate::object::ObjectHeader,
    fields: *mut u64,
    child_bits: u64,
    guard: fn(u64) -> bool,
) -> bool {
    *fields = child_bits;
    if guard(child_bits) {
        js_write_barrier_slot(ptr_bits(old_obj as usize), fields as u64, child_bits);
        return true;
    }
    false
}

/// **The stranding witness.** An OLD parent, a YOUNG child, and the exact store
/// sequence codegen now emits — assert three things in one place:
///
/// 1. with the SHIPPED guard, a heap-pointer child takes the bookkeeping branch,
///    the old→young edge verifier is satisfied, and a remembered-set scan marks
///    the child (it survives the minor);
/// 2. with a SABOTAGED guard (one that always answers "no pointer" — the shape
///    a wrong comparand set or an inverted branch produces), the same store
///    leaves the edge unrecorded and the verifier REJECTS it: that child is
///    stranded and the next minor frees it under a live reference;
/// 3. a NUMERIC child through the shipped guard skips the barrier and the
///    verifier is still satisfied — which is exactly the case the elision
///    exists for, and proves (2)'s failure is about the pointer, not about
///    skipping a barrier per se.
#[test]
fn sabotaged_guard_strands_a_young_child_the_shipped_guard_keeps() {
    let _guard = GcTestIsolationGuard::new();

    // (1) shipped guard, pointer child — barrier runs, edge covered, marked.
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let old_header = unsafe { header_from_user_ptr(old_obj as *const u8) };
    unsafe { (*old_header).gc_flags |= GC_FLAG_MARKED };
    let child_bits = ptr_bits(young);
    assert!(
        unsafe { guarded_field_store(old_obj, fields, child_bits, codegen_may_carry_heap_pointer) },
        "the shipped guard must take the bookkeeping branch for a heap-pointer child"
    );
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    let valid_ptrs = build_valid_pointer_set();
    let scan = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(scan.newly_marked, 1, "the child must survive the minor");
    unsafe { (*old_header).gc_flags &= !GC_FLAG_MARKED };
    clear_marks();
    remembered_set_clear();

    // (2) SABOTAGE: a guard that never accepts. Same store, same child.
    fn never_needs_bookkeeping(_bits: u64) -> bool {
        false
    }
    reset_remembered_set();
    clear_marks();
    let young2 = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj2, fields2) = unsafe { alloc_old_test_object(1) };
    let old_header2 = unsafe { header_from_user_ptr(old_obj2 as *const u8) };
    unsafe { (*old_header2).gc_flags |= GC_FLAG_MARKED };
    assert!(
        !unsafe { guarded_field_store(old_obj2, fields2, ptr_bits(young2), never_needs_bookkeeping) },
        "the sabotaged guard must skip the barrier — otherwise this arm proves nothing"
    );
    let rejected = std::panic::catch_unwind(verify_old_to_young_edges_covered);
    assert!(
        rejected.is_err(),
        "a skipped barrier on a heap-pointer child must leave the old->young edge \
         unrecorded — this is the stranded child the shipped guard must never produce"
    );
    unsafe { (*old_header2).gc_flags &= !GC_FLAG_MARKED };
    clear_marks();
    remembered_set_clear();

    // (3) shipped guard, NUMERIC child — barrier skipped, nothing stranded.
    reset_remembered_set();
    clear_marks();
    let (old_obj3, fields3) = unsafe { alloc_old_test_object(1) };
    let old_header3 = unsafe { header_from_user_ptr(old_obj3 as *const u8) };
    unsafe { (*old_header3).gc_flags |= GC_FLAG_MARKED };
    assert!(
        !unsafe {
            guarded_field_store(
                old_obj3,
                fields3,
                1234.5f64.to_bits(),
                codegen_may_carry_heap_pointer,
            )
        },
        "a plain double must skip the bookkeeping branch"
    );
    let numeric_stats = verify_old_to_young_edges_covered();
    assert_eq!(
        numeric_stats.missing_edges, 0,
        "a numeric store publishes no old->young edge, so skipping its barrier strands nothing"
    );
    unsafe { (*old_header3).gc_flags &= !GC_FLAG_MARKED };
    clear_marks();
    remembered_set_clear();
}
