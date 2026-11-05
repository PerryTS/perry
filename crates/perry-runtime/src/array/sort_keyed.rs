//! Numeric property ordering, conditional on each receiver's live own-data shape.
use super::{dense_writable, radix};
use crate::array::*;
use crate::closure::ClosureHeader;

#[derive(Clone, Copy, Default)]
struct Entry {
    key: u64,
    value: u64,
}

#[no_mangle]
pub extern "C" fn js_array_sort_key_order(
    arr: *mut ArrayHeader,
    comparator: *const ClosureHeader,
    order: i32,
    key: f64,
) -> *mut ArrayHeader {
    unsafe {
        if order == 1 || order == -1 {
            let live = clean_owned_arr_ptr(arr) as *mut ArrayHeader;
            if !live.is_null() && try_sort_keyed(live, key.to_bits(), order == -1) {
                return live;
            }
        }
    }
    js_array_sort_with_comparator(arr, comparator)
}

/// No JS calls, managed allocations or safepoints occur during key extraction,
/// distribution or barriered publication. Rust workspaces are call-local and
/// all original/final object values remain reachable from the rooted receiver.
unsafe fn try_sort_keyed(arr: *mut ArrayHeader, key_bits: u64, descending: bool) -> bool {
    if !dense_writable(arr) {
        return false;
    }
    let n = (*arr).length as usize;
    if n <= 1 {
        return true;
    }
    let mut key_scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) =
        crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_bits), &mut key_scratch)
    else {
        return false;
    };
    let mut entries = Vec::with_capacity(n);
    let mut counts = [[0usize; 256]; 8];
    for i in 0..n {
        let value = *array_elements_ptr(arr).add(i);
        let Some(number) = own_numeric_key(value, key_bits, bytes) else {
            return false;
        };
        let Some(key) = radix::float_key(number, descending) else {
            return false;
        };
        radix::histogram(&mut counts, key);
        entries.push(Entry { key, value });
    }
    // Every possible refusal precedes the first receiver write. In particular
    // no accessor/proxy/coercion is evaluated while trying the operation.
    let mut scratch = vec![Entry::default(); n];
    radix::radix(&mut entries, &mut scratch, &mut counts, |e| e.key);
    for (i, entry) in entries.into_iter().enumerate() {
        // GC_STORE_AUDIT(BARRIERED): shared array store updates per-slot layout,
        // element shape and old-to-young remembered edges for the new position.
        note_array_slot(arr, i, entry.value);
    }
    true
}

unsafe fn own_numeric_key(bits: u64, key_bits: u64, bytes: &[u8]) -> Option<f64> {
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let object = addr as *const crate::object::ObjectHeader;
    // A1_UNPROVEN: record-first: A-tested admission, meta read follows (kind-A/B question for S0f).
    let meta = crate::object::header::object_meta_if_present(object as *mut _, header.obj_type);
    if (!meta.is_null() && (*meta).elements != 0) || crate::object::overflow::has_overflow(object) {
        return None;
    }
    // This strong ordinary-receiver proof excludes native/exotic reads,
    // class surfaces, descriptors, per-object prototypes, proxies and brands.
    if !crate::object::shapes::plain_receiver_miss_is_final(object, bytes) {
        return None;
    }
    let record = crate::object::shapes::object_shape_record(object)?;
    let slot = record.own_data_slot_of_value(key_bits, bytes)??;
    let value = crate::object::field_get_set::object_field_at_with_live(
        object,
        slot,
        record.live_inline_slot_count(),
    );
    crate::array::header::value_bits_to_number(value.bits())
}

#[cfg(test)]
#[path = "sort_keyed_tests.rs"]
mod tests;
