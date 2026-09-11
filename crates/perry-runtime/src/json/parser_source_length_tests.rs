use super::source_token_utf16_len;
use crate::{string, StringHeader};

struct Suppressed;

impl Suppressed {
    fn new() -> Self {
        crate::gc::gc_suppress();
        crate::json::test_clear_parse_roots();
        Self
    }
}

impl Drop for Suppressed {
    fn drop(&mut self) {
        crate::json::test_clear_parse_roots();
        crate::gc::gc_unsuppress();
    }
}

unsafe fn source(bytes: &[u8]) -> *mut StringHeader {
    let ptr = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::json::parse_root_push(crate::JSValue::string_ptr(ptr));
    ptr
}

unsafe fn bytes<'a>(ptr: *const StringHeader) -> &'a [u8] {
    std::slice::from_raw_parts(string::string_data(ptr), (*ptr).byte_len as usize)
}

#[test]
fn json_source_length_requires_exact_payload_and_bounded_ascii_surround() {
    let _guard = Suppressed::new();
    unsafe {
        let payload = "東京🙂é".repeat(80);
        for outside in [2, 18, 255, 256, 257] {
            let text = format!("{}\"{payload}\"", " ".repeat(outside - 2));
            let ptr = source(text.as_bytes());
            let input = bytes(ptr);
            let start = outside - 2;
            let expected = (outside <= 256).then_some(payload.encode_utf16().count() as u32);
            assert_eq!(
                source_token_utf16_len(input, ptr, start, payload.len()),
                expected
            );
            assert_eq!(
                source_token_utf16_len(text.as_bytes(), ptr, start, payload.len()),
                None
            );
            assert_eq!(
                source_token_utf16_len(input, std::ptr::null(), start, payload.len()),
                None
            );
            if outside > 2 {
                assert_eq!(
                    source_token_utf16_len(&input[1..], ptr, start - 1, payload.len()),
                    None
                );
            }
        }
        for (prefix, suffix) in [("{\"é\":\"", "\"}"), ("{\"text\":\"", "\",\"é\":1}")] {
            let text = format!("{prefix}{payload}{suffix}");
            let ptr = source(text.as_bytes());
            assert_eq!(
                source_token_utf16_len(bytes(ptr), ptr, prefix.len() - 1, payload.len()),
                None
            );
        }
        let text = format!("\"{payload}\"");
        let ptr = source(text.as_bytes());
        for (start, len) in [
            (usize::MAX, payload.len()),
            (0, usize::MAX),
            (1, payload.len()),
            (0, 255),
        ] {
            assert_eq!(source_token_utf16_len(bytes(ptr), ptr, start, len), None);
        }
    }
}

#[test]
fn json_source_length_preserves_raw_wtf8_count_and_boundary_fallback() {
    let _guard = Suppressed::new();
    unsafe {
        let tails: &[&[u8]] = &[
            b"ascii",
            "é".as_bytes(),
            "🙂".as_bytes(),
            b"\xed\xa0\x80",
            b"\x80\x80",
            b"\xc0\x80",
            b"\xf5\x80\x80\x80",
            b"\xc2",
            b"\xe2\x82",
            b"\xf0\x9f\x99",
        ];
        for tail in tails {
            let mut payload = vec![b'a'; 300];
            payload.extend_from_slice(tail);
            let mut text = b"{\"text\":\"".to_vec();
            let start = text.len() - 1;
            text.extend_from_slice(&payload);
            text.extend_from_slice(b"\"}");
            let ptr = source(&text);
            let hint = source_token_utf16_len(bytes(ptr), ptr, start, payload.len());
            let truncated = matches!(*tail, b"\xc2" | b"\xe2\x82" | b"\xf0\x9f\x99");
            if truncated {
                assert_eq!(hint, None, "{tail:?}");
            } else {
                assert_eq!(
                    hint,
                    Some(string::compute_utf16_len_wtf8(&payload)),
                    "{tail:?}"
                );
            }
        }
    }
}

#[test]
fn json_source_length_parser_matches_standalone_allocation_and_escape_paths() {
    let _guard = Suppressed::new();
    unsafe {
        for (unit, repeat) in [("abc", 100), ("東京🙂é", 80), ("東京🙂é", 100_000)] {
            let payload = unit.repeat(repeat);
            for escaped in [false, true] {
                let token = if escaped {
                    format!("{payload}\\n\\uD800")
                } else {
                    payload.clone()
                };
                for (prefix, suffix) in [("{\"text\":\"", "\"}"), ("{\"é\":\"", "\"}")] {
                    let text = format!("{prefix}{token}{suffix}");
                    let ptr = source(text.as_bytes());
                    let mut candidate =
                        super::super::SpecializedDirectParser::<true>::new_batched_from_string(
                            bytes(ptr),
                            ptr,
                        );
                    candidate.pos = prefix.len() - 1;
                    let actual = candidate.parse_string_value().as_string_ptr();
                    let mut reference = super::super::DirectParser::new(text.as_bytes());
                    reference.pos = prefix.len() - 1;
                    let expected = reference.parse_string_value().as_string_ptr();
                    assert_eq!(candidate.pos, reference.pos);
                    assert_eq!((*actual).utf16_len, (*expected).utf16_len);
                    assert_eq!((*actual).byte_len, (*expected).byte_len);
                    assert_eq!((*actual).flags, (*expected).flags);
                    assert_eq!(bytes(actual), bytes(expected));
                    assert_eq!(
                        (*actual).utf16_len as usize,
                        payload.encode_utf16().count() + if escaped { 2 } else { 0 }
                    );
                }
            }
        }
    }
}

#[test]
fn json_source_length_dispatch_boundaries_match_scanned_fallback() {
    let _guard = Suppressed::new();
    unsafe {
        for total in [255usize, 256, 257, 258, 259, 4095, 4096, 4097, 4098, 8192] {
            for ending in [
                &b"plain"[..],
                "é".as_bytes(),
                "🙂".as_bytes(),
                &b"\xed\xa0\x80"[..],
            ] {
                let mut text = vec![b'"'];
                text.extend(std::iter::repeat_n(b'a', total - 2 - ending.len()));
                text.extend_from_slice(ending);
                text.push(b'"');
                assert_eq!(text.len(), total);
                let ptr = source(&text);
                let (value, valid) = super::super::parse_batched_from_source(bytes(ptr), ptr);
                let mut fallback =
                    super::super::DirectParser::new_batched_from_string(bytes(ptr), ptr);
                let expected = fallback.parse_value();
                assert_eq!(valid, fallback.finish());
                assert!(valid);
                let actual = value.as_string_ptr();
                let reference = expected.as_string_ptr();
                assert_eq!((*actual).utf16_len, (*reference).utf16_len);
                assert_eq!((*actual).flags, (*reference).flags);
                assert_eq!(bytes(actual), bytes(reference));
            }
        }
    }
}

#[test]
fn json_source_length_large_dispatch_preserves_complete_outputs_and_rejects_suffixes() {
    let _guard = Suppressed::new();
    let fields = (0..1000)
        .map(|i| format!("\"field_{i}\":{i}"))
        .collect::<Vec<_>>();
    let payload = "한🙂".repeat(1000);
    let cases = [
        (format!("{{{}}}", fields.join(",")), false),
        (
            format!("[{}]", vec![r#"{"id":1,"text":"tiny"}"#; 300].join(",")),
            false,
        ),
        (format!("\"{payload}\""), true),
        (format!(r#"{{"prefix":"a\"b","text":"{payload}"}}"#), true),
        (format!(r#""\n{payload}""#), true),
        (format!("\"short\"{}", " ".repeat(4096)), true),
        (
            format!("{}{{\"crossing\":1}}{}", " ".repeat(248), " ".repeat(4096)),
            false,
        ),
    ];
    unsafe {
        for (text, selected) in cases {
            assert!(text.len() > 4096);
            assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
            assert_eq!(super::use_source_length_parser(text.as_bytes()), selected);
            let ptr = source(text.as_bytes());
            let (value, valid) = super::super::parse_batched_from_source(bytes(ptr), ptr);
            assert!(valid);
            crate::json::parse_root_push(value);
            let output = crate::json::js_json_stringify(
                f64::from_bits(value.bits()),
                crate::json::TYPE_UNKNOWN,
            );
            assert_eq!(crate::json::str_from_header(output), Some(text.trim()));
            let invalid = text + "]";
            let ptr = source(invalid.as_bytes());
            let (_, valid) = super::super::parse_batched_from_source(bytes(ptr), ptr);
            assert!(
                !valid,
                "dispatch must not bypass complete syntax validation"
            );
        }
    }
}

#[test]
fn json_source_length_large_dispatch_keeps_every_bounded_surround() {
    for payload in ["a".repeat(4097), "한🙂".repeat(1000)] {
        for outside in 2..=256 {
            for before in 0..=outside - 2 {
                let text = format!(
                    "{}\"{payload}\"{}",
                    " ".repeat(before),
                    " ".repeat(outside - before - 2)
                );
                assert!(super::use_source_length_parser(text.as_bytes()));
            }
        }
    }
}
