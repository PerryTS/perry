//! All materializers must preserve the same escaped UTF-16 code units.
use super::*;

const CASES: &[(&str, &str)] = &[
    (r#"{"id":1,"s":"\ud800"}"#, r#"{"id":1,"s":"\ud800"}"#),
    (r#"{"id":1,"s":"\udfff"}"#, r#"{"id":1,"s":"\udfff"}"#),
    (
        r#"{"id":1,"s":"\ud800\u0061"}"#,
        r#"{"id":1,"s":"\ud800a"}"#,
    ),
    (
        r#"{"id":1,"s":"\ud800\ud800"}"#,
        r#"{"id":1,"s":"\ud800\ud800"}"#,
    ),
    (r#"{"id":1,"s":"\ud83d\ude42"}"#, r#"{"id":1,"s":"🙂"}"#),
    (
        r#"{"id":1,"s":"prefix\ud800\n\t"}"#,
        r#"{"id":1,"s":"prefix\ud800\n\t"}"#,
    ),
    (
        r#"{"a":1,"\u0061":2,"\ud800":3,"\ud800":4,"s":["\udc00","\ud800\udc00"]}"#,
        r#"{"a":2,"\ud800":4,"s":["\udc00","𐀀"]}"#,
    ),
];

unsafe fn render(value: JSValue) -> String {
    let output = crate::json::js_json_stringify(f64::from_bits(value.bits()), 0);
    crate::string::string_as_str(output).to_owned()
}

#[test]
fn json_tape_escaped_records_match_in_all_materializers() {
    let _suppress = crate::gc::GcSuppressScope::new();
    for &(text, expected) in CASES {
        let tape = build_tape(text.as_bytes()).unwrap();
        let source = TapeSource::Borrowed {
            tape: &tape.entries,
            bytes: text.as_bytes(),
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        unsafe {
            let value = materialize_from_idx_source(&source, &scope, 0);
            assert_eq!(render(value), expected, "tape: {text}");
            let value = record_materialize::try_small_record(&source, &scope, 0).unwrap();
            assert_eq!(render(value), expected, "batch: {text}");
            let value = materialize_iterative(&tape.entries, text.as_bytes()).unwrap();
            assert_eq!(render(value), expected, "iterative: {text}");
        }
    }
}

#[test]
fn json_tape_escaped_records_do_not_depend_on_access_order() {
    let _suppress = crate::gc::GcSuppressScope::new();
    for &(record, expected) in CASES {
        let input = format!("[{}]", vec![record; 128].join(","));
        let tape = build_tape(input.as_bytes()).unwrap();
        for order in [vec![0, 1, 2, 127], vec![127, 2, 1, 0], (0..128).collect()] {
            let scope = crate::gc::RuntimeHandleScope::new();
            let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
            let lazy =
                scope.root_raw_mut_ptr(unsafe { alloc_lazy_array(&tape.entries, 0, 128, text) });
            for index in order {
                let value = lazy.with_mut_ptr(|header| unsafe { lazy_get(header, index) });
                assert_eq!(
                    unsafe { render(value) },
                    expected,
                    "index {index}: {record}"
                );
            }
            // Exercise the final sparse-cache merge or adaptive reparse too.
            lazy.with_mut_ptr(|header| unsafe { force_materialize_lazy(header) });
            for index in 0..128 {
                let value = lazy.with_mut_ptr(|header| unsafe { lazy_get(header, index) });
                assert_eq!(
                    unsafe { render(value) },
                    expected,
                    "after force {index}: {record}"
                );
            }
        }
    }
}

#[test]
fn json_tape_lone_escape_uses_heap_wtf8_metadata() {
    let _suppress = crate::gc::GcSuppressScope::new();
    for (text, units) in [
        (r#""\ud800""#, 1),
        (r#""xx\ud800""#, 3),
        (r#""xxx\ud800""#, 4),
    ] {
        let tape = build_tape(text.as_bytes()).unwrap();
        let source = TapeSource::Borrowed {
            tape: &tape.entries,
            bytes: text.as_bytes(),
        };
        unsafe {
            let value = materialize_string_value(&source, 0);
            assert!(!value.is_short_string(), "lone surrogate must not use SSO");
            let string = value.as_string_ptr();
            assert_eq!((*string).utf16_len, units);
            assert_ne!(
                (*string).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES,
                0
            );
            assert_eq!(render(value), text);
        }
    }
}
