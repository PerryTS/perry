//! Test-only bounds and receiver checks split out of `indexing.rs` to keep
//! it under the 2000-line cap. The modules are unchanged; only their home
//! file moved.

#[cfg(test)]
mod keys_len_cap_tests {
    use crate::array::js_array_length;
    use crate::array::keys_array_len_capped_to_capacity;

    #[test]
    fn keys_len_capped_bounds_bogus_length_to_capacity() {
        // Freshly-allocated array: well-formed (length 0 <= capacity), so the
        // cap is a no-op and returns the real length.
        let arr = crate::array::js_array_alloc(8);
        let capacity = unsafe { (*arr).capacity } as usize;
        assert!(capacity >= 8);
        assert_eq!(unsafe { keys_array_len_capped_to_capacity(arr) }, 0);

        // Simulate a malformed keys array whose length field reports a bogus,
        // pointer-sized value — the pathology the object property walks guard
        // against. Un-capped, callers would iterate/allocate ~645M slots.
        unsafe {
            (*arr).length = 645_115_168;
        }
        assert_eq!(
            js_array_length(arr) as usize,
            645_115_168,
            "sanity: js_array_length reflects the forged length"
        );
        assert_eq!(
            unsafe { keys_array_len_capped_to_capacity(arr) },
            capacity,
            "cap must bound a bogus oversized length to the array's capacity"
        );
    }
}

#[cfg(test)]
mod claimed_array_string_receiver_tests {
    use crate::array::indexing::array_get_property_by_key;

    #[test]
    fn numeric_string_key_reads_a_heap_string_before_by_name_fallback() {
        let receiver = crate::string::js_string_from_bytes(b"ss".as_ptr(), 2);
        let zero = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
        let indexed = array_get_property_by_key(receiver.cast(), zero);
        assert_eq!(
            crate::builtins::jsvalue_string_content(indexed).as_deref(),
            Some("s")
        );

        let length = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        assert_eq!(array_get_property_by_key(receiver.cast(), length), 2.0);
    }
}
