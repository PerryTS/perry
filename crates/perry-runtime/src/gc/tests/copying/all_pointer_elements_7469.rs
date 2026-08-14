//! #7469 — an array whose element layout codegen DECLARED at its allocation
//! site, then filled with stores that record nothing.
//!
//! Invariant under test:
//!
//! > When codegen elides the per-push `js_gc_note_slot_layout`, the
//! > `js_array_declare_all_pointer_elements` it emitted at the allocation site
//! > is the ONLY thing telling the collector those element slots exist. It must
//! > therefore hold across the whole fill — including growth through
//! > `js_array_push_f64` — and the elided store must be refused whenever it
//! > does not.
//!
//! Every heap object is published `GC_LAYOUT_POINTER_FREE`, and
//! `heap_payload_slot_selection` short-circuits on that state and skips the
//! WHOLE payload without consulting any mask. So an elided pointer store into
//! an array that is *not* declared is not conservative — it is invisible: the
//! evacuating minor neither keeps the element alive nor rewrites the slot when
//! it moves, and the next read returns a pointer into reclaimed nursery memory.
//!
//! [`undeclared_array_enumerates_no_child_slots_which_is_what_the_declaration_buys`]
//! is the sabotage arm, made permanent. It runs the *identical* fill sequence
//! with the one declaration call removed and asserts the collector enumerates
//! ZERO element slots — so a green run of the positive test below means the
//! declaration was load-bearing, not that nothing was tried. It interrogates
//! the child-slot enumerator rather than collecting, so it is deterministic
//! regardless of GC timing and never leaves a stale pointer behind.

use super::*;

use crate::array::ArrayHeader;

/// The push sequence codegen emits on the ELIDED path, byte for byte: a bare
/// `store` at `elements[length]` and then `length += 1`. No
/// `js_gc_note_slot_layout`, no `js_array_note_numeric_write`, no
/// `js_array_push_f64`.
///
/// Callers must have room (`length < capacity`); the emitted IR branches to the
/// runtime grow path otherwise, which is a different sequence with its own
/// notes.
unsafe fn elided_inline_push(arr: *mut ArrayHeader, value_bits: u64) {
    let length = (*arr).length as usize;
    assert!(
        length < (*arr).capacity as usize,
        "the elided inline push models the in-capacity arm only"
    );
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
    std::ptr::write(elements.add(length), value_bits);
    (*arr).length = length as u32 + 1;
}

unsafe fn element_bits(arr: *mut ArrayHeader, index: usize) -> u64 {
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    *elements.add(index)
}

fn fresh_string(bytes: &[u8]) -> usize {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

const ELEMENTS: [&[u8]; 4] = [b"elem_zero", b"elem_one", b"elem_two", b"elem_three"];

/// The exact predicate `expr/array_push.rs` emits before taking the elided
/// store: `(_reserved & 0xF487) == 0xA000`. Duplicated numerically on purpose —
/// [`codegen_admission_test_matches_the_runtime_bit_names`] pins the two
/// spellings together, so a bit that moves in `gc/types.rs` fails here instead
/// of silently un-gating generated code in another crate.
const CODEGEN_ADMISSION_MASK: u16 = 62599;
const CODEGEN_ADMISSION_EXPECT: u16 = 40960;

unsafe fn codegen_would_take_the_elided_store(arr: *mut ArrayHeader) -> bool {
    let header = header_from_user_ptr(arr as *const u8);
    (*header)._reserved & CODEGEN_ADMISSION_MASK == CODEGEN_ADMISSION_EXPECT
}

#[test]
fn codegen_admission_test_matches_the_runtime_bit_names() {
    assert_eq!(
        CODEGEN_ADMISSION_MASK,
        OBJ_FLAG_FROZEN
            | OBJ_FLAG_SEALED
            | OBJ_FLAG_NO_EXTEND
            | OBJ_FLAG_ARRAY_DESCRIPTORS
            | GC_ARRAY_RAW_F64_LAYOUT
            | GC_ARRAY_RAW_F64_HOLES
            | GC_LAYOUT_ALL_POINTERS
            | GC_LAYOUT_STATE_MASK,
        "the mask emitted in perry-codegen/src/expr/array_push.rs must be \
         exactly these bits"
    );
    assert_eq!(
        CODEGEN_ADMISSION_EXPECT,
        GC_LAYOUT_SIDE_MASK | GC_LAYOUT_ALL_POINTERS,
        "the expected value emitted in perry-codegen/src/expr/array_push.rs \
         must be exactly the all-pointer declaration"
    );
}

#[test]
fn declaring_at_allocation_clears_the_raw_f64_layout_the_allocator_published() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(4);
    unsafe {
        // `js_array_alloc` publishes RawF64 + POINTER_FREE, so the codegen
        // admission test refuses the elided store on a bare allocation.
        assert!(!codegen_would_take_the_elided_store(arr));
        crate::array::js_array_declare_all_pointer_elements(arr);
        assert!(
            codegen_would_take_the_elided_store(arr),
            "the declaration must clear both raw-f64 bits AND install \
             SIDE_MASK|ALL_POINTERS, or the emitted store gate can never pass"
        );
    }
}

/// The runtime CAN revoke the declaration behind codegen's back, and this is
/// the cheapest live demonstration of it: `js_array_is_numeric_f64_layout` on a
/// still-EMPTY declared array verifies vacuously and re-publishes the array as
/// RawF64 + `POINTER_FREE`.
///
/// That is the whole reason the elided store is fronted by a test of the LIVE
/// header rather than by trusting the allocation-site declaration statically.
/// A static elision would, from here on, write pointers into a `POINTER_FREE`
/// array whose payload the collector skips entirely — a use-after-free factory
/// reachable from ordinary TypeScript (`arr.length`-shaped numeric fast paths
/// probe this on arrays codegen also pushes into). The header test instead
/// declines, the push takes `js_array_push_f64`, and the slot is recorded.
///
/// `rebuild_array_layout` (sort/splice) is a second such path; it installs a
/// PRECISE mask, which is likewise not a state the elided store may run in.
#[test]
fn a_numeric_layout_probe_revokes_the_declaration_and_the_header_test_catches_it() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(4);
    crate::array::js_array_declare_all_pointer_elements(arr);
    unsafe {
        assert!(codegen_would_take_the_elided_store(arr));
    }

    assert_eq!(
        crate::array::js_array_is_numeric_f64_layout(arr as *const _),
        1,
        "an empty array verifies as numeric vacuously — this is the revocation"
    );
    unsafe {
        assert!(
            !codegen_would_take_the_elided_store(arr),
            "codegen must refuse the elided store once the declaration is gone"
        );
    }

    // The refused push routes through the runtime, which records the slot.
    let child = fresh_string(b"after_revocation");
    let arr = crate::array::js_array_push_f64(arr, f64::from_bits(string_bits(child)));
    assert!(
        test_heap_child_slot_count(arr as *mut u8) >= 1,
        "the runtime push must have recorded the pointer the elided store \
         was refused for"
    );
}

/// The all-pointer claim covers `0..length`. #8102 widened the declaration to
/// non-empty arrays whose every existing slot IS a pointer, so the property
/// under test here is the one that always mattered: an existing **non-pointer**
/// element refuses it. (Before #8102 this was spelled "non-empty is refused",
/// which is why the fixture pushes a number — that push is what makes it a
/// refusal, not the length.)
#[test]
fn declaring_an_array_holding_a_non_pointer_element_is_refused() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(4);
    crate::array::js_array_push_f64(arr, 1.0);
    unsafe {
        crate::array::js_array_declare_all_pointer_elements(arr);
        assert!(
            !codegen_would_take_the_elided_store(arr),
            "the all-pointer claim is not true of what is already stored, so \
             it must be refused rather than asserted over existing elements"
        );
    }
}

/// The witness: four heap children published by stores that recorded nothing,
/// kept alive and re-pointed by an evacuating minor purely because of the
/// allocation-site declaration.
#[test]
fn declared_elements_survive_relocation_by_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(ELEMENTS.len() as u32);
    crate::array::js_array_declare_all_pointer_elements(arr);

    let mut before = Vec::new();
    unsafe {
        for bytes in ELEMENTS {
            let child = fresh_string(bytes);
            before.push(child);
            elided_inline_push(arr, string_bits(child));
        }
        assert!(
            codegen_would_take_the_elided_store(arr),
            "the declaration must still be live after the fill; otherwise the \
             pushes modelled above would not have been emitted elided"
        );
        assert_eq!(
            test_heap_child_slot_count(arr as *mut u8),
            ELEMENTS.len(),
            "the collector must enumerate every filled element slot"
        );
    }

    js_shadow_slot_set(0, ptr_bits(arr as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.copying_nursery.copied_objects >= ELEMENTS.len() + 1,
        "the cycle must actually have MOVED the array and all {} elements, \
         or this test proves nothing about relocation (copied_objects = {})",
        ELEMENTS.len(),
        trace.copying_nursery.copied_objects
    );

    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize as *mut ArrayHeader;
    assert_ne!(
        arr_after as usize, arr as usize,
        "the array itself must move"
    );
    unsafe {
        assert_eq!((*arr_after).length as usize, ELEMENTS.len());
        for (index, bytes) in ELEMENTS.iter().enumerate() {
            let slot = element_bits(arr_after, index);
            let moved = (slot & POINTER_MASK) as usize;
            assert_ne!(
                moved, before[index],
                "element {index} must have been relocated and its slot rewritten"
            );
            assert!(
                crate::arena::pointer_in_nursery(moved),
                "element {index} must live in the to-space, not a stale address"
            );
            assert_string_bytes(moved as *const crate::StringHeader, bytes);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

/// SABOTAGE ARM. The identical fill with the one declaration call removed: the
/// array stays `POINTER_FREE`, `heap_payload_slot_selection` skips the whole
/// payload, and the collector enumerates NOTHING — which is precisely the
/// use-after-free the declaration prevents, and precisely why codegen must
/// re-test the header before eliding any store.
///
/// Asserted on the enumerator rather than through a collection, so nothing here
/// creates a dangling pointer: the array is never rooted and its length is
/// reset before the guard drops.
#[test]
fn undeclared_array_enumerates_no_child_slots_which_is_what_the_declaration_buys() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let declared = crate::array::js_array_alloc(ELEMENTS.len() as u32);
    crate::array::js_array_declare_all_pointer_elements(declared);
    let undeclared = crate::array::js_array_alloc(ELEMENTS.len() as u32);

    unsafe {
        for bytes in ELEMENTS {
            let child = fresh_string(bytes);
            elided_inline_push(declared, string_bits(child));
            elided_inline_push(undeclared, string_bits(child));
        }
        assert_eq!(
            test_heap_child_slot_count(declared as *mut u8),
            ELEMENTS.len()
        );
        assert_eq!(
            test_heap_child_slot_count(undeclared as *mut u8),
            0,
            "an undeclared array skips its whole payload — the sabotage this \
             file exists to prove the positive test can detect"
        );
        assert!(
            !codegen_would_take_the_elided_store(undeclared),
            "codegen's emitted header test must REFUSE the elided store on \
             exactly this array, which is why the shape above is unreachable \
             from generated code"
        );
        // Leave no stale-pointer landmine for a later cycle on this thread.
        (*undeclared).length = 0;
        (*declared).length = 0;
    }
}

/// The declaration must survive the array outgrowing its initial capacity.
///
/// Growth routes through `js_array_push_f64` → `note_array_slot` →
/// `layout_note_slot`, whose all-pointer arm used to downgrade unconditionally.
/// That would demote the array on its FIRST growth and drop every later push
/// off the elided path for the rest of its life.
#[test]
fn a_pointer_append_through_the_runtime_preserves_the_declaration_across_growth() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(0);
    crate::array::js_array_declare_all_pointer_elements(arr);
    let initial_capacity = unsafe { (*arr).capacity };

    let mut arr = arr;
    for i in 0..(initial_capacity as usize * 4 + 8) {
        let child = fresh_string(format!("grow_{i}").as_bytes());
        arr = crate::array::js_array_push_f64(arr, f64::from_bits(string_bits(child)));
        unsafe {
            assert!(
                codegen_would_take_the_elided_store(arr),
                "the declaration must survive push #{i} (capacity now {})",
                (*arr).capacity
            );
        }
    }
    unsafe {
        assert!(
            (*arr).capacity > initial_capacity,
            "the loop must actually have grown the array, or it proves nothing"
        );
    }
}

/// A store that is NOT an append cannot keep the "every live slot is a
/// pointer" claim exact, so it downgrades — to `GC_LAYOUT_UNKNOWN`, which
/// scans every slot, never to `POINTER_FREE`, which scans none.
#[test]
fn a_replace_or_a_numeric_append_downgrades_to_a_conservative_scan() {
    let _guard = CopyingNurseryTestGuard::new(1);

    for downgrade_by_replace in [true, false] {
        let arr = crate::array::js_array_alloc(4);
        crate::array::js_array_declare_all_pointer_elements(arr);
        let first = fresh_string(b"kept");
        let arr = crate::array::js_array_push_f64(arr, f64::from_bits(string_bits(first)));
        unsafe {
            assert!(codegen_would_take_the_elided_store(arr));
        }

        if downgrade_by_replace {
            // Overwrite slot 0 (slot_index < length): a replace, not an append.
            let other = fresh_string(b"replacement");
            crate::array::js_array_set_f64(arr, 0, f64::from_bits(string_bits(other)));
        } else {
            // Append a NUMBER: the slot it lands in is no longer a pointer.
            crate::array::js_array_push_f64(arr, 42.0);
        }

        unsafe {
            let header = header_from_user_ptr(arr as *const u8);
            assert!(
                !codegen_would_take_the_elided_store(arr),
                "the declaration must be revoked (downgrade_by_replace = {downgrade_by_replace})"
            );
            assert_ne!(
                (*header)._reserved & GC_LAYOUT_STATE_MASK,
                GC_LAYOUT_POINTER_FREE,
                "downgrading an all-pointer array to POINTER_FREE would strand \
                 the element already stored in slot 0"
            );
            assert!(
                test_heap_child_slot_count(arr as *mut u8) >= 1,
                "the still-live pointer in slot 0 must remain enumerable after \
                 the downgrade"
            );
        }
    }
}

/// #8102 — the array-LITERAL shape: `const a: C[] = [x, y]; a.push(…)`.
///
/// The literal's element stores run first and note each slot
/// (`expr/array_literal.rs` publishes the payload `POINTER_FREE` and notes per
/// slot), and only then does the `Stmt::Let` tail emit the declaration. While
/// `js_array_declare_all_pointer_elements` refused every `length != 0` array
/// the declaration was a silent no-op for exactly this shape, so every later
/// push failed the codegen header test and paid the per-store layout note
/// #7469 exists to delete — +33.9% instructions on a 4M-push loop.
#[test]
fn a_non_empty_all_pointer_literal_is_declared_and_admits_the_elided_store() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(8);
    unsafe {
        for (slot, name) in [&b"lit_zero"[..], &b"lit_one"[..]].iter().enumerate() {
            let bits = string_bits(fresh_string(name));
            elided_inline_push(arr, bits);
            crate::gc::js_gc_note_slot_layout(arr as u64, slot as u32, bits);
        }
        assert!(
            !codegen_would_take_the_elided_store(arr),
            "the literal's own stores leave a precise side mask, which is NOT \
             a state the elided push may run in — this is the pre-declaration \
             state the fixture exists to start from"
        );

        crate::array::js_array_declare_all_pointer_elements(arr);

        assert!(
            codegen_would_take_the_elided_store(arr),
            "a non-empty literal whose every slot is a pointer must still be \
             declared; refusing it is #8102"
        );
        assert_eq!(
            test_heap_child_slot_count(arr as *mut u8),
            2,
            "the two elements already stored must stay enumerable across the \
             declaration"
        );
    }
}

/// Sabotage arm for the test above, made permanent: the SAME sequence with one
/// element that is not a pointer. The declaration must refuse and must leave
/// the header exactly as it found it — otherwise the widening would be
/// declaring `0..length` all-pointer over a payload it had not checked, and a
/// green positive test above would mean nothing.
#[test]
fn a_non_pointer_element_in_the_literal_still_refuses_the_declaration() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(8);
    unsafe {
        let ptr_bits = string_bits(fresh_string(b"lit_zero"));
        elided_inline_push(arr, ptr_bits);
        crate::gc::js_gc_note_slot_layout(arr as u64, 0, ptr_bits);
        let number_bits = 42.0f64.to_bits();
        elided_inline_push(arr, number_bits);
        crate::gc::js_gc_note_slot_layout(arr as u64, 1, number_bits);

        let header = header_from_user_ptr(arr as *const u8);
        let before = (*header)._reserved;

        crate::array::js_array_declare_all_pointer_elements(arr);

        assert_eq!(
            (*header)._reserved,
            before,
            "a refused declaration must not touch the header — in particular \
             it must not clear the raw-f64 bits on its way out"
        );
        assert!(
            !codegen_would_take_the_elided_store(arr),
            "codegen must keep routing this array's pushes through \
             js_array_push_f64"
        );
    }
}

/// The empty-literal path — the one this function has always served — must be
/// bit-identical after the widening.
#[test]
fn an_empty_array_is_still_declared_vacuously() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(4);
    unsafe {
        assert!(!codegen_would_take_the_elided_store(arr));
        crate::array::js_array_declare_all_pointer_elements(arr);
        assert!(codegen_would_take_the_elided_store(arr));
    }
}
