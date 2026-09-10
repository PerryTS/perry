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
        r#"["x" ,1]"#,
        r#"["x", 1]"#,
        r#"["x" ]"#,
        r#"["x\"" ,""]"#,
        r#"[{"x" :1}]"#,
        r#"[{"x": 1}]"#,
        r#"[{"x":1} ,{"x":2}]"#,
        r#"["\u0061"]"#,
        r#"["\/"]"#,
        r#"[{"x":1,"x":2}]"#,
        r#"[{"":1,"":2}]"#,
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
fn json_lazy_copy_number_visitor_excludes_strings_and_other_subtrees() {
    let source = br#"[8,[1.0,"2e3",{"x":4}],9]"#;
    let tape = build_tape(source).unwrap();
    let mut visited = Vec::new();
    visit_copyable_numbers(&tape.entries, source, 2, |start, end, token| {
        assert_eq!(&source[start..end], token);
        visited.push(token.to_vec());
        Some(())
    })
    .unwrap();
    assert_eq!(visited, [b"1.0".to_vec(), b"4".to_vec()]);
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
fn json_lazy_copy_key_lengths_preserve_exact_duplicate_decisions() {
    let mut keys = vec![
        "é".to_owned(),
        "xy".to_owned(),
        "😀".to_owned(),
        "abcd".to_owned(),
    ];
    for len in [0, 1, 2, 7, 8, 15, 31, 32, 63, 64, 65, 127, 128, 129] {
        keys.push("a".repeat(len));
        keys.push("b".repeat(len));
    }
    // Includes equal byte lengths, different lengths in the same bucket, empty
    // keys and multi-byte Unicode. Expected uniqueness uses ordinary equality.
    for left in &keys {
        for right in &keys {
            let source = format!("[{{\"{left}\":1,\"{right}\":2}}]");
            assert_eq!(admits(source.as_bytes(), 0), left != right, "{source}");
        }
    }
}

#[test]
fn json_lazy_copy_key_lengths_preserve_frames_and_wide_fallback() {
    for count in [31, 32, 33] {
        let keys: Vec<_> = (0..count).map(|len| "a".repeat(len)).collect();
        let fields: Vec<_> = keys.iter().map(|key| format!("\"{key}\":0")).collect();
        let source = format!("[{{{}}}]", fields.join(","));
        assert_eq!(admits(source.as_bytes(), 0), count <= 32);
        for key in &keys {
            let source = format!("[{{{},\"{key}\":1}}]", fields.join(","));
            assert!(!admits(source.as_bytes(), 0), "{source}");
        }
    }
    for (source, expected) in [
        (r#"[{"x":{"x":1},"y":{"x":2}},{"x":3}]"#, true),
        (r#"[{"x":{"x":1},"x":2}]"#, false),
        (r#"[{"x":[{"y":1,"y":2}]}]"#, false),
        (r#"[{"0":0,"1":1,"10":2,"20":3,"a":4}]"#, true),
        (r#"[{"10":0,"2":1}]"#, false),
        (r#"[{"a":0,"10":1}]"#, false),
        (r#"[{"x":0,"\u0078":1}]"#, false),
    ] {
        assert_eq!(admits(source.as_bytes(), 0), expected, "{source}");
    }
}

#[test]
fn json_lazy_copy_unknown_string_metadata_keeps_the_checked_fallback() {
    for (source, expected) in [
        (r#"[{"name":"plain é😀"},"a\nb"]"#, true),
        (r#"["\u0061"]"#, false),
        (r#"[{"\u0061":"plain"}]"#, false),
        (r#"["\/"]"#, false),
        (r#"[{"name":1,"name":2}]"#, false),
        (r#"["x" ,"y"]"#, false),
    ] {
        let mut tape = build_tape(source.as_bytes()).unwrap().entries;
        assert_eq!(source_is_copyable(&tape, source.as_bytes(), 0), expected);
        for metadata in [0, u32::MAX] {
            for entry in &mut tape {
                if matches!(entry.kind, KIND_KEY | KIND_STRING) {
                    entry.link = metadata;
                }
            }
            assert_eq!(source_is_copyable(&tape, source.as_bytes(), 0), expected);
        }
    }
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
