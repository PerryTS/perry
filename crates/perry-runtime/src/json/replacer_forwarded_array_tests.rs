use super::*;

/// #7269: `JSON.stringify(v, null, 2)` on an array grown past its
/// initial inline capacity (`MIN_ARRAY_CAPACITY`, 16) returned
/// nondeterministic garbage — a different, wrong-length string on every
/// run of the SAME binary. Root cause: `js_array_grow` (issue #233)
/// reallocates and installs a `GC_FLAG_FORWARDED` stub at the OLD
/// address, whose first 8 bytes — exactly `ArrayHeader.length` +
/// `.capacity` — now hold the raw forwarding pointer to the new array.
/// The stub is retained specifically so a caller still holding the
/// pre-grow address (its comment: "an async function's caller still
/// holding the pre-grow pointer") keeps resolving correctly *through
/// `clean_arr_ptr`*. The plain (non-pretty) path always went through
/// `clean_arr_ptr` before every header read
/// (`json/stringify.rs::stringify_array_depth`); the pretty-print path
/// in this file read `(*arr).length`/`.capacity` directly, so it saw the
/// forwarding pointer's bytes reinterpreted as a bogus, run-to-run-
/// different array shape — "garbage" because the bytes are a real, live
/// pointer, just not the field values they're pretending to be.
///
/// This test needs no explicit GC cycle: `js_array_grow` unconditionally
/// installs the forwarding stub on every reallocating grow, independent
/// of any minor/major collection.
#[test]
fn pretty_stringify_resolves_array_grown_past_inline_capacity() {
    unsafe {
        let mut arr = crate::js_array_alloc(0);
        let initial_capacity = (*arr).capacity;
        assert!(
            initial_capacity > 0,
            "a freshly allocated array must report a real capacity"
        );
        // Fill to capacity so the NEXT push must reallocate.
        for i in 0..initial_capacity {
            arr = crate::js_array_push_f64(arr, i as f64);
        }
        assert_eq!((*arr).length, initial_capacity);

        // Capture the pre-grow address. No allocation happens between this
        // read and the growing push below, so nothing else can have moved
        // or reused this address in between.
        let stale_ptr = arr as *const crate::ArrayHeader;
        let grown = crate::js_array_push_f64(arr, initial_capacity as f64);
        assert_ne!(
            grown as *const crate::ArrayHeader, stale_ptr,
            "growth past capacity must reallocate to a new address \
             (otherwise this test exercises nothing)"
        );
        assert_eq!((*grown).length, initial_capacity + 1);

        // Sabotage precondition, mirroring `array/subclass_tests.rs`'s
        // style: prove the stale address really carries a raw forwarding
        // pointer reinterpreted as (length, capacity), not merely stale
        // or reused memory — reconstructing the u64 from the two u32
        // fields must recover the grown array's exact address.
        let raw_len_bits = (*stale_ptr).length as u64;
        let raw_cap_bits = (*stale_ptr).capacity as u64;
        let forwarded_as_ptr = raw_len_bits | (raw_cap_bits << 32);
        assert_eq!(
            forwarded_as_ptr, grown as u64,
            "the stale header's raw (length, capacity) bytes must be the \
             exact bit pattern of the new array's address"
        );
        assert_ne!(
            (*stale_ptr).length,
            initial_capacity + 1,
            "sabotage precondition: an unresolved read of the stale \
             header must not already report the real length by luck"
        );
        let resolved = crate::array::clean_arr_ptr(stale_ptr);
        assert_eq!(
            resolved, grown as *const crate::ArrayHeader,
            "clean_arr_ptr must resolve the stale pre-grow pointer to the grown array"
        );

        // Build the exact NaN-boxed value a caller still holding the
        // pre-grow reference would carry, and pretty-stringify it —
        // mirroring the issue's `JSON.stringify(v, null, 2)` repro.
        let stale_value = f64::from_bits(POINTER_TAG | (stale_ptr as u64 & POINTER_MASK));
        let result_bits = js_json_stringify_full(stale_value, f64::from_bits(TAG_NULL), 2.0);
        let result_ptr = (result_bits as u64 & POINTER_MASK) as *const StringHeader;
        let s = str_from_header(result_ptr).expect("must produce a string");

        let expected_body = (0..=initial_capacity)
            .map(|i| format!("  {}", i))
            .collect::<Vec<_>>()
            .join(",\n");
        let expected = format!("[\n{}\n]", expected_body);
        assert_eq!(
            s, expected,
            "pretty stringify of a stale forwarded array pointer must \
             equal the CURRENT (grown) array's real contents, not \
             garbage read from the forwarding stub"
        );
    }
}
