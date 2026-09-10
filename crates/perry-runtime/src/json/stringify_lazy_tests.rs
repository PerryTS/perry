use super::*;
use crate::{gc::RuntimeHandleScope, JSValue};

fn admits(source: &[u8], root: usize) -> bool {
    let tape = build_tape(source).expect("valid fixture");
    source_is_copyable(&tape.entries, source, root)
}

#[test]
fn json_lazy_copy_admission_checks_spelling_duplicates_and_key_order() {
    for source in [
        r#"[ {"x":1}]"#,
        r#"[{"x" :1}]"#,
        r#"[{"x": 1}]"#,
        r#"[{"x":1} ,{"x":2}]"#,
        r#"["\u0061"]"#,
        r#"["\/"]"#,
        r#"[{"x":1,"x":2}]"#,
        r#"[{"x":1,"\u0078":2}]"#,
        r#"[{"2":2,"1":1}]"#,
        r#"[{"a":0,"1":1}]"#,
        r#"[{"0":1,"0":2}]"#,
        r#"[{"child":{"x":1,"x":2}}]"#,
    ] {
        assert!(!admits(source.as_bytes(), 0), "{source}");
    }
    let mut raw_surrogate = b"[\"".to_vec();
    raw_surrogate.extend_from_slice(b"\xed\xa0\x80");
    raw_surrogate.extend_from_slice(b"\"]");
    assert!(!admits(&raw_surrogate, 0));
}

#[test]
fn json_lazy_copy_admission_preserves_canonical_nested_and_unicode_sources() {
    for source in [
        r#"[]"#,
        r#"[[],{},[{},[]],null,true,false]"#,
        r#"[{"x":1},{"x":2}]"#,
        r#"[{"x":{"x":1},"y":2}]"#,
        r#"[{"0":0,"2":2,"4294967294":3,"a":4,"01":5,"4294967295":6}]"#,
        r#"["a b","é"]"#,
        r#"["a\nb\t\"\\/é😀"]"#,
        r#"[1e-400,1.0,-0,9007199254740993]"#,
        " \n[1,2]\t ",
    ] {
        assert!(admits(source.as_bytes(), 0), "{source}");
    }
    assert!(admits(br#"[ {"x":0},[1,2] ]"#, 5));
}

#[test]
fn json_lazy_copy_admission_bounds_wide_object_work_and_rejects_bad_ranges() {
    let object = (0..33)
        .map(|i| format!("\"k{i}\":{i}"))
        .collect::<Vec<_>>()
        .join(",");
    assert!(!admits(format!("[{{{object}}}]").as_bytes(), 0));
    let mut tape = build_tape(b"[1]").unwrap().entries;
    tape[0].link = u32::MAX;
    assert!(!source_is_copyable(&tape, b"[1]", 0));
    assert!(!source_is_copyable(&[], b"[]", 0));
}

#[test]
fn json_lazy_copy_admission_creates_no_managed_intermediates() {
    let source = format!(
        "[{}]",
        vec![r#"{"x":1,"child":{"x":"é😀"}}"#; 128].join(",")
    );
    let tape = build_tape(source.as_bytes()).unwrap();
    let bytes = crate::arena::arena_total_bytes();
    let roots = RuntimeHandleScope::active_len_for_tests();
    for _ in 0..100 {
        assert!(source_is_copyable(&tape.entries, source.as_bytes(), 0));
    }
    assert_eq!(crate::arena::arena_total_bytes(), bytes);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
}

unsafe fn roundtrip(source: &str, expected: &str, remains_lazy: bool) {
    let source = source.as_bytes();
    let scope = RuntimeHandleScope::new();
    let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
    let lazy = with_built_tape(source, |tape| {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
    })
    .unwrap();
    let held = scope.root_raw_mut_ptr(lazy);
    let output = held.with_mut_ptr(|lazy: *mut LazyArrayHeader| {
        crate::json::js_json_stringify(f64::from_bits(JSValue::object_ptr(lazy.cast()).bits()), 0)
    });
    assert!(!output.is_null());
    assert_eq!(crate::string::header_str_checked(output).unwrap(), expected);
    held.with_mut_ptr(|lazy: *mut LazyArrayHeader| {
        assert_eq!((*lazy).materialized.is_null(), remains_lazy);
    });
}

#[test]
fn json_lazy_stringify_materializes_noncanonical_source_correctly() {
    unsafe {
        for (source, expected) in [
            (r#"[ {"x" : 1} , {"x":2} ]"#, r#"[{"x":1},{"x":2}]"#),
            (r#"["\u0061","\/"]"#, r#"["a","/"]"#),
            (r#"[{"x":1,"x":2}]"#, r#"[{"x":2}]"#),
            (r#"[{"2":2,"1":1}]"#, r#"[{"1":1,"2":2}]"#),
            (r#"[{"x":1,"\u0078":2}]"#, r#"[{"x":2}]"#),
        ] {
            roundtrip(source, expected, false);
        }
    }
}

#[test]
fn json_lazy_stringify_keeps_canonical_arrays_lazy_and_normalizes_numbers() {
    unsafe {
        roundtrip(
            r#"[{"x":"é😀"},{"x":"a\nb"}]"#,
            r#"[{"x":"é😀"},{"x":"a\nb"}]"#,
            true,
        );
        roundtrip(
            r#"[1e-400,1.0,-0,9007199254740993,1e400]"#,
            r#"[0,1,0,9007199254740992,null]"#,
            true,
        );
    }
}
