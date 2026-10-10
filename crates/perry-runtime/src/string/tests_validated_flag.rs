//! One encoding/count proof is shared by string producers and regex binding.
//! Unknown input is the negative control for every combining writer.
use super::*;

fn heap(text: &str) -> *mut StringHeader {
    js_string_from_str(text)
}

fn carries(s: *const StringHeader) -> bool {
    unsafe { (*s).flags & STRING_FLAG_WTF8_VALIDATED != 0 }
}

#[test]
fn exact_string_transformations_preserve_encoding_and_count_proof() {
    let v = heap("abcdef");
    let w = heap("xyz");
    assert!(carries(v) && carries(w));
    for result in [
        js_string_slice(v, 1, 4),
        js_string_substring(v, 0, 2),
        js_string_concat(v, w),
        js_string_repeat(v, 3.0),
        crate::string::js_string_to_upper_case(v),
    ] {
        assert!(carries(result));
        let bytes =
            unsafe { slice::from_raw_parts(string_data(result), (*result).byte_len as usize) };
        #[cfg(feature = "regex-engine")]
        {
            let validated = perex::binding::BoundSubject::new(bytes).unwrap();
            let counted = perex::binding::BoundSubject::new_counted(bytes, unsafe {
                (*result).utf16_len as usize
            })
            .unwrap();
            assert_eq!(
                validated.with_view(|v| v.len_utf16()).unwrap(),
                counted.with_view(|v| v.len_utf16()).unwrap()
            );
        }
        #[cfg(not(feature = "regex-engine"))]
        assert_eq!(
            std::str::from_utf8(bytes).unwrap().encode_utf16().count(),
            unsafe { (*result).utf16_len as usize }
        );
    }
}

#[test]
fn append_and_append_chain_recompute_the_proof_for_the_written_payload() {
    for known in [false, true] {
        let dest = js_string_from_bytes_with_capacity(b"ab".as_ptr(), 2, 64);
        let piece = if known {
            heap("cd")
        } else {
            js_string_from_bytes([0xc3].as_ptr(), 1)
        };
        unsafe {
            (*dest).refcount = 1;
        }
        let appended = js_string_append(dest, piece);
        assert_eq!(appended, dest, "the test needs the in-place path");
        assert_eq!(carries(appended), known);

        let dest = js_string_from_bytes_with_capacity(b"ab".as_ptr(), 2, 64);
        unsafe {
            (*dest).refcount = 1;
        }
        let parts = [
            crate::value::js_nanbox_string(dest as i64),
            crate::value::js_nanbox_string(piece as i64),
            crate::value::js_nanbox_string(heap("ef") as i64),
        ];
        let chained = js_string_append_chain(parts.as_ptr(), 3);
        assert_eq!(chained, dest, "the test needs the in-place chain path");
        assert_eq!(carries(chained), known);
    }
}
