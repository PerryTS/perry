//! The layout accessor (I32 at rest, representation RFC §10.7;
//! DESIGN-n1-at-rest §6): the one place a heap slot's representation is
//! decoded to, or encoded from, a JSValue.
//!
//! **Elements (PR B).** An Array of an at-rest template (wpfacts
//! `rest_elems`) holds every element as an I32 word `INT32_TAG | (u32)v`:
//! the low half is the int32 the proven code loads and stores, the high half
//! puts the word in the `0x7FFE` band, which the collector never takes for a
//! pointer (`gc::layout_pointer_bearing_bits`), so the scanner, the layout
//! notes, the copying collector and the conservative stack scan need no
//! change. A hole stays `TAG_HOLE`: an at-rest Array has none below its
//! floor, and one whose length may move (the capacity reservation below) has
//! them in `[length, capacity)` and in the gap a store past the length
//! leaves.
//!
//! The birth is the fused fill `new Array(k).fill(v)`; codegen calls
//! [`js_array_rest_encode`] on the filled Array, which encodes each word,
//! marks the header (`GC_ARRAY_I32_LAYOUT`, bit 10 of `_reserved`, free for
//! `GC_TYPE_ARRAY` in the bit map in `gc/types.rs`), clears the raw-f64
//! layout bits (an I32 word is no raw double, so no raw-f64 tier admits the
//! Array) and declares the payload pointer free. Proven code never reads the
//! header bit: the facts are the authority. The accessor reads it.
//!
//! **The capacity reservation** (wpfacts `rest_elems::reserve`). A birth
//! whose length may move inside a small proven hull `[lo, hi]` calls
//! [`js_array_rest_reserve`] instead: it gives the fresh Array `hi` slots
//! (lengthened where it stands, else one exact replacement before anyone
//! holds it), then encodes. Every store the facts allow has a key below
//! `hi`, so [`js_array_rest_set`] lengthens the Array in place: the slot is
//! already there, the gap is already holes, and only `length` moves. The
//! storage is never reallocated, so no at-rest head is ever forwarded and a
//! slot below the floor keeps its address. A store at or past the capacity
//! is a false proof ("never past capacity"). A hole below the length reads
//! through the shared cold read (`undefined`, or what a prototype answers).
//!
//! **Who reads an at-rest Array.** The proven element sites codegen lowers
//! with the encoding (`perry-codegen` `expr::rest_elems`), and the cold arms
//! of those sites, which call [`js_array_rest_get`] / [`js_array_rest_set`]:
//! an index at or past the length, a key that is no index. No other reader
//! can meet one: wpfacts admits a template only when no builtin, protocol,
//! spread or native touches its Arrays. Check mode (`PERRY_VERIFY_FACTS=1`)
//! asserts it in the JSON array writer (`generic_reader`).
//!
//! The field half (`slot_get`/`slot_set`, PR C) and VR-5's record element
//! accessor take the same entry shape (`len`, `get(i) -> JSValue bits`).

use crate::array::ArrayHeader;
use crate::value::{INT32_TAG, TAG_MASK};

/// `GcHeader::_reserved` bit 10 of a `GC_TYPE_ARRAY`: every element slot
/// holds an I32 word (module docs).
pub(crate) const GC_ARRAY_I32_LAYOUT: u16 = 0x400;

static VERIFY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Check mode is on (`perry_verify_facts_init` ran).
pub(crate) fn set_verify() {
    VERIFY.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn handle(v: f64) -> *mut ArrayHeader {
    crate::array::clean_arr_ptr_mut((v.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader)
}

/// The GC header of array head `arr`.
unsafe fn gc_header(arr: *const ArrayHeader) -> Option<*mut crate::gc::GcHeader> {
    if arr.is_null() || (arr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let h = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    (crate::gc_type_is_array!((*h).obj_type)).then_some(h)
}

/// The array's header says its slots are I32 words.
///
/// # Safety
/// `arr` is null or a live array head.
pub(crate) unsafe fn is_i32_layout(arr: *const ArrayHeader) -> bool {
    crate::array::array_object_flags(arr) & GC_ARRAY_I32_LAYOUT != 0
}

/// The element count.
///
/// # Safety
/// `arr` is a live array head.
pub(crate) unsafe fn elem_len(arr: *const ArrayHeader) -> u32 {
    (*arr).length
}

/// The JSValue bits of element `i < elem_len(arr)`: an I32 word as its
/// canonical double, any other word as it is.
///
/// # Safety
/// `arr` is a live array head and `i` below its length and capacity.
pub(crate) unsafe fn elem_get(arr: *const ArrayHeader, i: u32) -> u64 {
    let w = *crate::array::array_elements_ptr(arr).add(i as usize);
    decode(arr, w)
}

/// Store the JSValue `v` as element `i < elem_len(arr)`, encoded for the
/// array's layout. `false`: an I32 slot met a value that is no int32
/// Number (a false proof).
///
/// # Safety
/// `arr` is a live array head and `i` below its length and capacity.
pub(crate) unsafe fn elem_set(arr: *mut ArrayHeader, i: u32, v: u64) -> bool {
    let slot = crate::array::array_elements_ptr(arr).add(i as usize);
    if !is_i32_layout(arr) {
        *slot = v;
        return true;
    }
    match encode(v) {
        Some(w) => {
            *slot = w;
            true
        }
        None => false,
    }
}

unsafe fn decode(arr: *const ArrayHeader, w: u64) -> u64 {
    if w & TAG_MASK == INT32_TAG && is_i32_layout(arr) {
        f64::from(w as u32 as i32).to_bits()
    } else {
        w
    }
}

/// The I32 word of an int32 Number (a canonical double, or an INT32-boxed
/// word), never `-0`.
fn encode(v: u64) -> Option<u64> {
    if v & TAG_MASK == INT32_TAG {
        return Some(v);
    }
    let d = f64::from_bits(v);
    let i = d as i32;
    (f64::from(i) == d && v != (-0.0f64).to_bits()).then(|| INT32_TAG | u64::from(i as u32))
}

/// A proof the runtime found false: check mode reports it (exit 86).
fn false_proof(v: u64) {
    if VERIFY.load(std::sync::atomic::Ordering::Relaxed) {
        crate::wpfacts_check::perry_wpfacts_check_miss(f64::from_bits(v), 0);
    }
}

/// The key as an element index below `len`.
fn index_below(key: f64, len: u32) -> Option<u32> {
    let i = key as u32;
    (f64::from(i) == key && i < len).then_some(i)
}

/// The birth of an at-rest Array: encode the words a native fill wrote,
/// mark the header (module docs). Returns `arr`.
#[no_mangle]
pub extern "C" fn js_array_rest_encode(arr: f64) -> f64 {
    let a = handle(arr);
    if a.is_null() {
        return arr;
    }
    unsafe {
        let n = elem_len(a).min((*a).capacity);
        let slots = crate::array::array_elements_ptr(a);
        for i in 0..n as usize {
            let w = *slots.add(i);
            match encode(w) {
                Some(e) => *slots.add(i) = e,
                None => false_proof(w),
            }
        }
        crate::array::clear_array_numeric_layout(a);
        crate::gc::mark_payload_pointer_free(a as usize);
        if let Some(h) = gc_header(a) {
            (*h)._reserved |= GC_ARRAY_I32_LAYOUT;
        }
    }
    arr
}

/// The birth of an at-rest Array whose length may move (module docs):
/// reserve `cap` slots, then [`js_array_rest_encode`] on the head that has
/// them. Returns that head. May collect (the replacement arm allocates).
#[no_mangle]
pub extern "C" fn js_array_rest_reserve(arr: f64, cap: u32) -> f64 {
    let a = handle(arr);
    if a.is_null() {
        return arr;
    }
    let head = unsafe { reserve(a, cap) };
    if head == a {
        return js_array_rest_encode(arr);
    }
    js_array_rest_encode(crate::value::js_nanbox_pointer(head as i64))
}

/// The head of `a` with at least `cap` slots (`array::reserve_capacity`).
///
/// # Safety
/// `a` is a live array head.
unsafe fn reserve(a: *mut ArrayHeader, cap: u32) -> *mut ArrayHeader {
    if gc_header(a).is_none() || cap <= (*a).capacity {
        return a;
    }
    crate::array::reserve_capacity(a, cap)
}

/// The cold read of an at-rest element site: an element by its index (a
/// hole is the shared cold read's miss), any other key through the generic
/// `Get`, whose INT32 word (an element named by a string key) is decoded.
#[no_mangle]
pub extern "C" fn js_array_rest_get(arr: f64, key: f64) -> f64 {
    let a = handle(arr);
    if !a.is_null() {
        unsafe {
            let len = elem_len(a);
            if let Some(i) = index_below(key, len.min((*a).capacity)) {
                let w = elem_get(a, i);
                // A hole (the gap a store past the length left) is no own
                // element: the shared cold read's miss, as below.
                if w != crate::value::TAG_HOLE || !is_i32_layout(a) {
                    return f64::from_bits(w);
                }
                return crate::array::perry_array_read_cold_shared(arr, key, 4);
            }
            // I32 at rest B2: an integer index at or past the length names
            // no own slot, so it is the shared cold read's absent-slot miss
            // (control 4): `undefined` unless a prototype or an element
            // descriptor may answer.
            if is_i32_layout(a)
                && key >= f64::from(len)
                && key < f64::from(u32::MAX)
                && key == key.trunc()
            {
                return crate::array::perry_array_read_cold_shared(arr, key, 4);
            }
        }
    }
    let v = crate::value::js_dyn_index_get(arr, key);
    if a.is_null() {
        return v;
    }
    unsafe { f64::from_bits(decode(a, v.to_bits())) }
}

/// The cold store of an at-rest element site: an element by its index,
/// encoded; an index in `[length, capacity)` lengthens the Array where it
/// stands (the capacity reservation, module docs); any other key through
/// the generic `Set`. No index at or past the capacity reaches here on a
/// proven program.
#[no_mangle]
pub extern "C" fn js_array_rest_set(arr: f64, key: f64, value: f64) {
    let a = handle(arr);
    if !a.is_null() {
        unsafe {
            let len = elem_len(a);
            let cap = (*a).capacity;
            if let Some(i) = index_below(key, len.min(cap)) {
                if !elem_set(a, i, value.to_bits()) {
                    false_proof(value.to_bits());
                }
                return;
            }
            if is_i32_layout(a) {
                if let Some(i) = index_below(key, cap) {
                    if extend(a, i, value.to_bits()) {
                        return;
                    }
                    false_proof(value.to_bits());
                } else if key >= f64::from(len) {
                    false_proof(key.to_bits());
                }
            }
        }
    }
    crate::value::js_dyn_index_set(arr, key, value);
}

/// Store `v` as element `i` of the I32-layout array `arr`, `length <= i <
/// capacity`, and lengthen it to `i + 1`. The slots in `[length, i)` are
/// already holes (`array::storage`: `[length, capacity)` is hole-filled) and
/// stay holes. `false`, with nothing written: `v` is no int32 Number.
///
/// # Safety
/// `arr` is a live array head with `length <= i < capacity`.
unsafe fn extend(arr: *mut ArrayHeader, i: u32, v: u64) -> bool {
    let Some(w) = encode(v) else {
        return false;
    };
    // GC_STORE_AUDIT(POINTER_FREE): an I32 word is in the 0x7FFE band, which no collector takes for a pointer.
    *crate::array::array_elements_ptr(arr).add(i as usize) = w;
    (*arr).length = i + 1;
    crate::array::ek_note_published(arr, i as usize);
    true
}

/// Check mode: a generic reader (one that reads element words undecoded)
/// met an at-rest Array, which wpfacts says it never does.
///
/// # Safety
/// `arr` is null or a live array head.
pub(crate) unsafe fn generic_reader(arr: *const ArrayHeader) {
    if VERIFY.load(std::sync::atomic::Ordering::Relaxed) && is_i32_layout(arr) {
        crate::wpfacts_check::perry_wpfacts_check_miss(f64::from_bits(arr as u64), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_int32_number_encodes_in_the_int32_band() {
        assert_eq!(encode(5.0f64.to_bits()), Some(INT32_TAG | 5));
        assert_eq!(encode((-1.0f64).to_bits()), Some(INT32_TAG | 0xFFFF_FFFF));
        assert_eq!(encode(4096.0f64.to_bits()), Some(INT32_TAG | 4096));
        assert!(!crate::gc::layout_pointer_bearing_bits(INT32_TAG | 4096));
        assert!(!crate::gc::layout_pointer_bearing_bits(
            INT32_TAG | 0x7FFF_FFF8
        ));
    }

    #[test]
    fn no_other_number_encodes() {
        for v in [
            -0.0f64,
            0.5,
            2147483648.0,
            -2147483649.0,
            f64::NAN,
            f64::INFINITY,
        ] {
            assert_eq!(encode(v.to_bits()), None, "{v}");
        }
        assert_eq!(encode(crate::value::TAG_UNDEFINED), None);
    }

    #[test]
    fn an_index_is_an_integer_below_the_length() {
        assert_eq!(index_below(3.0, 4), Some(3));
        assert_eq!(index_below(-0.0, 4), Some(0));
        assert_eq!(index_below(4.0, 4), None);
        assert_eq!(index_below(1.5, 4), None);
        assert_eq!(index_below(-1.0, 4), None);
        assert_eq!(index_below(f64::NAN, 4), None);
    }

    /// A filled `new Array(len).fill(3)`, born at rest with `cap` slots.
    fn born(len: u32, cap: u32) -> f64 {
        let arr = crate::array::js_array_alloc_with_length(len);
        let arr = crate::array::js_array_fill(arr, 3.0);
        js_array_rest_reserve(crate::value::js_nanbox_pointer(arr as i64), cap)
    }

    unsafe fn word(a: *const ArrayHeader, i: usize) -> u64 {
        *crate::array::array_elements_ptr(a).add(i)
    }

    /// The reserved birth: `cap` slots behind the header, I32 words below
    /// the length, holes above it.
    unsafe fn assert_reserved(a: *const ArrayHeader, len: u32, cap: u32) {
        assert_eq!((elem_len(a), (*a).capacity), (len, cap));
        assert!(is_i32_layout(a));
        assert_eq!(
            crate::array::array_elements_ptr(a) as usize,
            a as usize + std::mem::size_of::<ArrayHeader>(),
            "the elements start behind the header"
        );
        for i in 0..len as usize {
            assert_eq!(word(a, i), INT32_TAG | 3);
        }
        for i in len as usize..cap as usize {
            assert_eq!(word(a, i), crate::value::TAG_HOLE);
        }
    }

    #[test]
    fn a_reserving_birth_has_its_capacity_where_it_stands_or_in_one_replacement() {
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        // Unit tests copy by default: one exact replacement.
        let v = born(8, 16);
        unsafe { assert_reserved(handle(v), 8, 16) };
        // The most recent birth is lengthened where it stands.
        let _in_place = crate::array::GrowInPlaceTestGuard::on();
        let arr = crate::array::js_array_alloc_with_length(8);
        let arr = crate::array::js_array_fill(arr, 3.0);
        let v = js_array_rest_reserve(crate::value::js_nanbox_pointer(arr as i64), 16);
        assert_eq!(handle(v), arr);
        unsafe { assert_reserved(arr, 8, 16) };
        // A capacity the birth already has is kept.
        let v = born(8, 8);
        unsafe { assert_reserved(handle(v), 8, 8) };
    }

    #[test]
    fn a_store_below_the_capacity_lengthens_the_array_where_it_stands() {
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let undefined = crate::value::TAG_UNDEFINED;
        let v = born(8, 16);
        let a = handle(v);
        unsafe {
            let base = crate::array::array_elements_ptr(a);
            js_array_rest_set(v, 11.0, 4096.0);
            assert_eq!((elem_len(a), (*a).capacity), (12, 16));
            assert_eq!(crate::array::array_elements_ptr(a), base);
            assert_eq!(crate::array::clean_arr_ptr(a), a as *const ArrayHeader);
            assert_eq!(word(a, 11), INT32_TAG | 4096);
            assert_eq!(js_array_rest_get(v, 11.0), 4096.0);
            // The gap stays holes, and a hole below the length reads
            // `undefined`, as a key at or past the length does.
            for k in [8.0, 9.0, 10.0, 12.0, 15.0, 16.0] {
                assert_eq!(js_array_rest_get(v, k).to_bits(), undefined, "{k}");
            }
            assert_eq!(js_array_rest_get(v, 7.0), 3.0);
            // A hole below the length is filled in place.
            js_array_rest_set(v, 9.0, -1.0);
            assert_eq!(elem_len(a), 12);
            assert_eq!(js_array_rest_get(v, 9.0), -1.0);
            // The last reserved slot.
            js_array_rest_set(v, 15.0, 7.0);
            assert_eq!((elem_len(a), (*a).capacity), (16, 16));
            assert_eq!(js_array_rest_get(v, 15.0), 7.0);
            assert_eq!(js_array_rest_get(v, 14.0).to_bits(), undefined);
            assert_eq!(crate::array::array_elements_ptr(a), base);
        }
    }

    #[test]
    fn a_store_that_is_no_int32_or_past_the_capacity_does_not_lengthen_in_place() {
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let v = born(8, 16);
        let a = handle(v);
        unsafe {
            assert!(!extend(a, 10, 0.5f64.to_bits()));
            assert_eq!(elem_len(a), 8);
            assert_eq!(word(a, 10), crate::value::TAG_HOLE);
            // Past the capacity: a false proof (reported in check mode), then
            // the generic store, which keeps JavaScript's length.
            js_array_rest_set(v, 16.0, 1.0);
            let live = crate::array::clean_arr_ptr(a);
            assert_eq!(elem_len(live), 17);
        }
    }
}
