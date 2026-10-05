//! Numeric-key PutValue on an existing own Array element. The Array's live
//! header and slot supply the proof, through the ordinary dense setter's
//! guards; no key conversion, handle scope or descriptor walk on a hit.

use crate::value::{POINTER_MASK, POINTER_TAG, TAG_MASK};

#[inline]
pub(super) fn try_set(target: f64, key: f64, value: f64, receiver: f64) -> bool {
    let bits = target.to_bits();
    if bits != receiver.to_bits() || bits & TAG_MASK != POINTER_TAG {
        return false;
    }
    // Only a Number key: objects, strings, Symbols and BigInts retain their
    // complete ToPropertyKey path. This decoder also rejects a ClassRef that
    // shares INT32_TAG with a boxed integer.
    let Some(index) = crate::array::value_bits_to_number(key.to_bits()) else {
        return false;
    };
    if !(index >= 0.0 && index < u32::MAX as f64 && index.fract() == 0.0) {
        return false;
    }
    let addr = (bits & POINTER_MASK) as usize;
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }
    // This helper neither allocates managed objects nor calls user code. A
    // miss leaves the receiver untouched; frozen/descriptor-bearing Arrays,
    // holes, growth and prototype-sensitive writes keep PutValue's Throw flag.
    // A general value still gets the helper's layout note and write barrier.
    crate::array::try_strict_dense_index_set(
        addr as *mut crate::array::ArrayHeader,
        index as u32,
        value,
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::{js_array_from_f64, js_array_get_f64};

    fn array(values: &[f64]) -> (*mut crate::array::ArrayHeader, f64) {
        let ptr = js_array_from_f64(values.as_ptr(), values.len() as u32);
        (ptr, crate::value::js_nanbox_pointer(ptr as i64))
    }

    #[test]
    fn numeric_key_put_value_does_not_allocate_property_keys() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        let values = vec![0.0; 128];
        let (ptr, boxed) = array(&values);
        let allocated = crate::arena::arena_live_allocated_bytes();
        for index in 0..128 {
            let value = index as f64 + 1.0;
            assert_eq!(
                super::super::js_put_value_set(boxed, index as f64, value, boxed, 1),
                value
            );
            assert_eq!(js_array_get_f64(ptr, index), value);
        }
        assert_eq!(
            crate::arena::arena_live_allocated_bytes(),
            allocated,
            "existing numeric-key element overwrites must not materialize String keys"
        );
    }

    #[test]
    fn dense_put_value_guard_preserves_key_receiver_and_descriptor_semantics() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        let (ptr, boxed) = array(&[10.0, 20.0]);
        let (_, other) = array(&[30.0]);
        for key in [-1.0, 0.5, f64::NAN, f64::INFINITY, u32::MAX as f64] {
            assert!(!try_set(boxed, key, 99.0, boxed));
        }
        assert!(!try_set(boxed, 0.0, 99.0, other));
        assert_eq!(js_array_get_f64(ptr, 0), 10.0);
        assert!(try_set(boxed, -0.0, 11.0, boxed));
        let int_key = f64::from_bits(crate::value::INT32_TAG | 1);
        assert!(try_set(boxed, int_key, 22.0, boxed));
        assert_eq!(js_array_get_f64(ptr, 0), 11.0);
        assert_eq!(js_array_get_f64(ptr, 1), 22.0);
        assert!(
            !try_set(boxed, 2.0, 99.0, boxed),
            "growth keeps ordinary Set"
        );
        let hole =
            crate::value::js_nanbox_pointer(crate::array::js_array_alloc_with_length(1) as i64);
        assert!(
            !try_set(hole, 0.0, 99.0, hole),
            "a hole is not an own property"
        );
        unsafe {
            let header = (ptr as *mut u8)
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            for flag in [
                crate::gc::OBJ_FLAG_FROZEN,
                crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
            ] {
                let saved = (*header)._reserved;
                (*header)._reserved |= flag;
                assert!(!try_set(boxed, 0.0, 99.0, boxed));
                (*header)._reserved = saved;
            }
        }
        assert_eq!(
            js_array_get_f64(ptr, 0),
            11.0,
            "refusals leave the slot alone"
        );
    }
}
