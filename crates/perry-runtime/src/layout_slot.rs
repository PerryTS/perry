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
//! change. A hole stays `TAG_HOLE` (an at-rest Array never has one).
//!
//! The birth is the fused fill `new Array(k).fill(v)`; codegen calls
//! [`js_array_rest_encode`] on the filled Array, which encodes each word,
//! marks the header (`GC_ARRAY_I32_LAYOUT`, bit 10 of `_reserved`, free for
//! `GC_TYPE_ARRAY` in the bit map in `gc/types.rs`), clears the raw-f64
//! layout bits (an I32 word is no raw double, so no raw-f64 tier admits the
//! Array) and declares the payload pointer free. Proven code never reads the
//! header bit: the facts are the authority. The accessor reads it.
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

/// The cold read of an at-rest element site: an element by its index, any
/// other key through the generic `Get`, whose INT32 word (an element named
/// by a string key) is decoded.
#[no_mangle]
pub extern "C" fn js_array_rest_get(arr: f64, key: f64) -> f64 {
    let a = handle(arr);
    if !a.is_null() {
        unsafe {
            if let Some(i) = index_below(key, elem_len(a).min((*a).capacity)) {
                return f64::from_bits(elem_get(a, i));
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
/// encoded; any other key through the generic `Set` (no index at or past
/// the length reaches here on a proven program: the length is fixed).
#[no_mangle]
pub extern "C" fn js_array_rest_set(arr: f64, key: f64, value: f64) {
    let a = handle(arr);
    if !a.is_null() {
        unsafe {
            if let Some(i) = index_below(key, elem_len(a).min((*a).capacity)) {
                if !elem_set(a, i, value.to_bits()) {
                    false_proof(value.to_bits());
                }
                return;
            }
            if is_i32_layout(a) && key >= f64::from(elem_len(a)) {
                false_proof(key.to_bits());
            }
        }
    }
    crate::value::js_dyn_index_set(arr, key, value);
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
}
