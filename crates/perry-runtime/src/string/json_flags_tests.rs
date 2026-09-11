use super::*;
use crate::{gc::RuntimeHandleScope, JSValue};

unsafe fn quoted(ptr: *mut StringHeader) -> String {
    let output = crate::json::js_json_stringify(f64::from_bits(JSValue::string_ptr(ptr).bits()), 0);
    let bytes = std::slice::from_raw_parts(string_data(output), (*output).byte_len as usize);
    std::str::from_utf8(bytes)
        .expect("JSON output must be UTF-8")
        .to_owned()
}

#[test]
fn json_escape_flags_do_not_certify_unproven_concat_parts() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let prefix = scope.root_string_ptr(string_from_json_bytes(&mut None, b"plain heap"));
        let suffix = scope.root_string_ptr(js_string_from_bytes(b"\n\"tail".as_ptr(), 6));
        prefix.with_const_ptr::<StringHeader, _>(|p| {
            assert_ne!((*p).flags & STRING_FLAG_JSON_ESCAPE_FREE, 0)
        });
        for route in 0..6 {
            let result = prefix.with_const_ptr::<StringHeader, _>(|a| {
                suffix.with_const_ptr::<StringHeader, _>(|b| {
                    let av = f64::from_bits(JSValue::string_ptr(a as *mut StringHeader).bits());
                    let bv = f64::from_bits(JSValue::string_ptr(b as *mut StringHeader).bits());
                    match route {
                        0 => js_string_concat(a, b),
                        1 => JSValue::from_bits(js_string_concat_box(av, bv).to_bits())
                            .as_string_ptr() as *mut StringHeader,
                        2 => concat::js_string_concat_chain([av, bv].as_ptr(), 2),
                        3 => concat::js_string_concat_chain([av, 5.0, bv].as_ptr(), 3),
                        4 => concat::js_string_append_chain([av, bv].as_ptr(), 2),
                        _ => js_string_append(a as *mut StringHeader, b),
                    }
                })
            });
            assert_eq!(
                (*result).flags & STRING_FLAG_JSON_ESCAPE_FREE,
                0,
                "route {route}"
            );
            let expected = if route == 3 {
                "\"plain heap5\\n\\\"tail\""
            } else {
                "\"plain heap\\n\\\"tail\""
            };
            assert_eq!(quoted(result), expected, "route {route}");
        }
    }
}

#[test]
fn json_escape_flags_clear_on_unique_in_place_append() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let suffix = scope.root_string_ptr(js_string_from_bytes(b"\n\"tail".as_ptr(), 6));
        for chain in [false, true] {
            let p = js_string_from_bytes_with_capacity(b"plain heap".as_ptr(), 10, 64);
            // Valid proof for this payload, with capacity/ownership admitting
            // the actual in-place mutation arm rather than only fresh concat.
            (*p).flags |= STRING_FLAG_JSON_ESCAPE_FREE;
            (*p).refcount = 1;
            let prefix = scope.root_string_ptr(p);
            let result = prefix.with_const_ptr::<StringHeader, _>(|a| {
                suffix.with_const_ptr::<StringHeader, _>(|b| {
                    let result = if chain {
                        let parts = [
                            f64::from_bits(JSValue::string_ptr(a as *mut StringHeader).bits()),
                            f64::from_bits(JSValue::string_ptr(b as *mut StringHeader).bits()),
                        ];
                        concat::js_string_append_chain(parts.as_ptr(), 2)
                    } else {
                        js_string_append(a as *mut StringHeader, b)
                    };
                    assert_eq!(
                        result as *const StringHeader, a,
                        "must exercise in-place append"
                    );
                    result
                })
            });
            assert_eq!((*result).flags & STRING_FLAG_JSON_ESCAPE_FREE, 0);
            assert_eq!(quoted(result), "\"plain heap\\n\\\"tail\"");
        }
    }
}

#[test]
fn json_escape_flags_preserve_lone_surrogate_metadata() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let prefix = scope.root_string_ptr(string_from_json_bytes(&mut None, b"plain heap"));
        let suffix =
            scope.root_string_ptr(js_string_from_wtf8_bytes([0xed, 0xa0, 0x80].as_ptr(), 3));
        let result = prefix.with_const_ptr::<StringHeader, _>(|a| {
            suffix.with_const_ptr::<StringHeader, _>(|b| js_string_concat(a, b))
        });
        assert_ne!((*result).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        assert_eq!((*result).flags & STRING_FLAG_JSON_ESCAPE_FREE, 0);
        assert_eq!(quoted(result), "\"plain heap\\ud800\"");
    }
}
