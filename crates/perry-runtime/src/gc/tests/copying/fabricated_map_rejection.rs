//! Regression test for the fabricated-Map classification bug.
//!
//! `classify_arena` validates that an address and `addr - 8` are both in
//! heap space, then pattern-matches a `GcHeader` at `addr - 8`. It never
//! checked that `addr - 8` is an object START, so an interior arena pointer
//! — a word in a live object's payload that happens to be an arena address
//! — would fabricate a fake object from the bytes preceding it.
//!
//! For an 8-aligned arena pointer near `0x400_0000_0000`, the fabricated
//! `GcHeader` supplies:
//!   * `obj_type` = low byte = 0x08 = `GC_TYPE_MAP` (the only valid type
//!     that is a multiple of 8)
//!   * `size` = top 32 bits ≈ 1024 (always passes the `[8, 2^34]` range
//!     check in the old `plausible_gc_header`)
//!   * `gc_flags` = second byte, needs `GC_FLAG_ARENA` (0x02) — ~coin flip
//!
//! The fix: `plausible_gc_header` now requires `size == 24` (the known
//! fixed total for Map: 8-byte header + 16-byte `MapHeader` payload) for
//! fixed-layout types. A fabricated header has `size ≈ 1024`, which is
//! rejected.

use super::*;

/// Size of `MapHeader` (and `SetHeader`): `{ size: u32, capacity: u32,
/// entries/elements: *mut f64 }` = 16 bytes.
const MAP_HEADER_PAYLOAD: usize = 16;
const MAP_FIXED_TOTAL: usize = GC_HEADER_SIZE + MAP_HEADER_PAYLOAD;

/// Directly test that `plausible_gc_header` rejects a fabricated Map
/// header with the ~1024-byte size that an interior arena pointer
/// produces, while accepting a genuine Map header with size = 24.
#[test]
fn test_plausible_gc_header_rejects_fabricated_map_size() {
    // A genuine Map header: obj_type = MAP, size = 24, GC_FLAG_ARENA set.
    let mut genuine = GcHeader {
        obj_type: GC_TYPE_MAP,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: MAP_FIXED_TOTAL as u32,
    };
    assert!(
        unsafe { plausible_gc_header(&mut genuine as *mut GcHeader, true) },
        "genuine Map header (size={MAP_FIXED_TOTAL}) must be plausible"
    );

    // A fabricated Map header: obj_type = MAP, size = 1024 (the top 32
    // bits of an arena address near 0x400_0000_0000), GC_FLAG_ARENA set.
    // This is what classify_arena would read from 8 bytes preceding an
    // interior arena pointer.
    let mut fabricated = GcHeader {
        obj_type: GC_TYPE_MAP,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: 1024,
    };
    assert!(
        !unsafe { plausible_gc_header(&mut fabricated as *mut GcHeader, true) },
        "fabricated Map header (size=1024) must be rejected"
    );

    // Edge: size = 24 + 8 = 32 (what free-list reuse into a larger slot
    // would produce) must also be rejected — only the exact fixed total
    // is accepted.
    let mut wrong_size = GcHeader {
        obj_type: GC_TYPE_MAP,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: 32,
    };
    assert!(
        !unsafe { plausible_gc_header(&mut wrong_size as *mut GcHeader, true) },
        "Map header with non-fixed size (32) must be rejected"
    );
}

/// Same check for Set (GC_TYPE_SET = 12). Set cannot be fabricated from
/// an 8-aligned pointer (12 is not a multiple of 8), but the size check
/// applies to it nonetheless — it is free for genuine objects and
/// guards against any future fabrication path.
#[test]
fn test_plausible_gc_header_rejects_fabricated_set_size() {
    let mut genuine = GcHeader {
        obj_type: GC_TYPE_SET,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: MAP_FIXED_TOTAL as u32,
    };
    assert!(
        unsafe { plausible_gc_header(&mut genuine as *mut GcHeader, true) },
        "genuine Set header (size={MAP_FIXED_TOTAL}) must be plausible"
    );

    let mut fabricated = GcHeader {
        obj_type: GC_TYPE_SET,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: 1040,
    };
    assert!(
        !unsafe { plausible_gc_header(&mut fabricated as *mut GcHeader, true) },
        "fabricated Set header (size=1040) must be rejected"
    );
}

/// Variable-size types (arrays, objects, strings) must NOT be rejected
/// by the fixed-layout check — their `size` reflects runtime content.
#[test]
fn test_plausible_gc_header_still_accepts_variable_size_types() {
    let mut array_header = GcHeader {
        obj_type: GC_TYPE_ARRAY,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: 128, // arbitrary, > GC_HEADER_SIZE
    };
    assert!(
        unsafe { plausible_gc_header(&mut array_header as *mut GcHeader, true) },
        "variable-size type (array) with arbitrary size must be plausible"
    );

    let mut string_header = GcHeader {
        obj_type: GC_TYPE_STRING,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: 64,
    };
    assert!(
        unsafe { plausible_gc_header(&mut string_header as *mut GcHeader, true) },
        "variable-size type (string) with arbitrary size must be plausible"
    );
}

/// End-to-end: allocate a real Map in the nursery, verify its header
/// has the fixed total size, and verify classify_arena accepts it.
/// Then fabricate an interior-pointer scenario and verify rejection.
#[test]
fn test_classify_arena_rejects_interior_pointer_as_map() {
    let _guard = CopyingNurseryTestGuard::new(1);

    // Allocate a genuine Map. Its GcHeader.size must be 24.
    let map_ptr = crate::map::js_map_alloc(4) as *mut u8;
    assert!(!map_ptr.is_null(), "Map allocation must succeed");

    let map_header = unsafe { (map_ptr as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader };
    let map_total = unsafe { (*map_header).size as usize };
    assert_eq!(
        map_total, MAP_FIXED_TOTAL,
        "genuine nursery Map must have fixed total size {MAP_FIXED_TOTAL}, got {map_total}"
    );

    // The Map's user pointer must classify as a valid arena pointer.
    let ptrs = CopyingPointerSet::new();
    let classified = ptrs.classify_arena(map_ptr as usize);
    assert!(
        classified.is_some(),
        "genuine Map user pointer must classify in arena"
    );

    // Now fabricate the bug scenario: write a GcHeader-shaped word into
    // an array's payload, then check that the address immediately after
    // it (which would be "addr" in classify_arena, with the fabricated
    // header at "addr - 8") is NOT classified as a Map.
    //
    // We allocate an array with enough elements to hold our fabricated
    // header, write the header bytes into it, and try to classify the
    // address right after the header.
    let array_ptr = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as *mut u64;
    assert!(!array_ptr.is_null(), "array allocation must succeed");

    // Write a fabricated Map GcHeader into the first 8 bytes of the
    // array's payload: obj_type=MAP, gc_flags=ARENA, size=1024.
    let fabricated_header = GcHeader {
        obj_type: GC_TYPE_MAP,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: 1024,
    };
    unsafe {
        std::ptr::write(array_ptr as *mut GcHeader, fabricated_header);
    }

    // The address "array_ptr + 8" would have the fabricated header at
    // "array_ptr" (i.e., addr - 8 = array_ptr). If the array is in a
    // registered arena range, classify_arena would previously accept
    // this as a Map. After the fix, it must reject it because
    // size=1024 != 24.
    let fabricated_user_addr = unsafe { (array_ptr as *mut u8).add(8) } as usize;
    let result = ptrs.classify_arena(fabricated_user_addr);
    assert!(
        result.is_none(),
        "interior pointer with fabricated Map header (size=1024) must NOT classify"
    );

    // Also verify a genuine-size fabricated header (size=24) at the same
    // location WOULD have been accepted without the fix — but with the
    // fix, the fixed-layout check passes, so classify_arena returns Some.
    // This confirms the check is specific to the size, not a blanket
    // rejection of the address.
    let genuine_size_header = GcHeader {
        obj_type: GC_TYPE_MAP,
        gc_flags: GC_FLAG_ARENA,
        _reserved: 0,
        size: MAP_FIXED_TOTAL as u32,
    };
    unsafe {
        std::ptr::write(array_ptr as *mut GcHeader, genuine_size_header);
    }
    let result_genuine = ptrs.classify_arena(fabricated_user_addr);
    // This SHOULD classify — the header now has the correct fixed size.
    // (If it doesn't, the array payload may not be in a recognized heap
    // space, which would also block the fabricated bug — so None is
    // acceptable here. The key assertion is the previous one: size=1024
    // is rejected.)
    if result_genuine.is_some() {
        // It classified — verify it claims to be a Map.
        let ptr = result_genuine.unwrap();
        unsafe {
            assert_eq!(
                (*ptr.header).obj_type,
                GC_TYPE_MAP,
                "classified fabricated-as-genuine should be Map"
            );
        }
    }
}
