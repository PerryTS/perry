use super::*;

#[test]
fn scalar_entries_preserve_oversized_key_cache_cleanup() {
    struct Suppressed;
    impl Drop for Suppressed {
        fn drop(&mut self) {
            crate::json::test_clear_parse_roots();
            crate::gc::gc_unsuppress();
        }
    }
    crate::gc::gc_suppress();
    let _suppressed = Suppressed;
    crate::json::test_clear_parse_roots();
    for (fallible, text) in [
        (false, b"null".as_slice()),
        (true, b"null".as_slice()),
        (false, b"{}".as_slice()),
        (true, b"{}".as_slice()),
    ] {
        // Lazy materialization can populate this cache between top-level
        // parses. The next parse must preserve the existing cleanup policy,
        // even when its result is an inline scalar.
        for i in 0..4097 {
            crate::json::cached_parse_key_ptr(format!("scalar-cache-key-{i}").as_bytes());
        }
        assert_eq!(
            crate::json::PARSE_KEY_CACHE.with(|c| c.borrow().len()),
            4097
        );
        assert!(!crate::json::PARSE_KEY_RING.with(|c| c.borrow().is_empty()));
        let input = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = unsafe {
            if fallible {
                crate::json::js_json_parse_result(input).unwrap()
            } else {
                crate::json::js_json_parse(input)
            }
        };
        assert!(value.is_null() || value.is_pointer());
        assert!(crate::json::PARSE_KEY_CACHE.with(|c| c.borrow().is_empty()));
        assert!(crate::json::PARSE_KEY_RING.with(|c| c.borrow().is_empty()));
    }
}

#[test]
fn scalar_literals_and_only_json_whitespace() {
    for (text, value) in [
        ("null", JSValue::null()),
        ("true", JSValue::bool(true)),
        ("false", JSValue::bool(false)),
    ] {
        for prefix in ["", " ", "\t", "\n", "\r", " \t\r\n"] {
            for suffix in ["", " ", "\t", "\n", "\r", " \t\r\n"] {
                let input = format!("{prefix}{text}{suffix}");
                assert_eq!(
                    try_parse_scalar(input.as_bytes()).unwrap().bits(),
                    value.bits()
                );
            }
        }
        for extra in ["x", "\0", "\u{b}", "\u{c}", "\u{a0}", "\u{feff}", " true"] {
            assert!(try_parse_scalar(format!("{text}{extra}").as_bytes()).is_none());
            assert!(try_parse_scalar(format!("{extra}{text}").as_bytes()).is_none());
        }
    }
}

#[test]
fn short_strings_validate_every_byte_and_preserve_utf8() {
    for len in 0..=crate::value::SHORT_STRING_MAX_LEN {
        let mut input = vec![b'a'; len + 2];
        input[0] = b'"';
        input[len + 1] = b'"';
        assert!(try_parse_scalar(&input).unwrap().is_short_string());
        for i in 1..=len {
            for byte in 0..=255u8 {
                input[i] = byte;
                let value = try_parse_scalar(&input);
                let eligible = (0x20..0x80).contains(&byte) && byte != b'"' && byte != b'\\';
                assert_eq!(value.is_some(), eligible, "{input:?}");
                if let Some(value) = value {
                    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
                    let n = value.short_string_to_buf(&mut scratch);
                    assert_eq!(&scratch[..n], &input[1..input.len() - 1]);
                }
            }
            input[i] = b'a';
        }
    }
    for text in [
        "é", "東京", /* six bytes: fallback */
        "🙂", "éé", "éabc",
    ] {
        let input = format!("\"{text}\"");
        let value = try_parse_scalar(input.as_bytes());
        assert_eq!(
            value.is_some(),
            text.len() <= crate::value::SHORT_STRING_MAX_LEN
        );
        if let Some(value) = value {
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let n = value.short_string_to_buf(&mut scratch);
            assert_eq!(&scratch[..n], text.as_bytes());
        }
    }
    for input in [
        b"\"\xed\xa0\x80\"".as_slice(),
        b"\"abcdef\"",
        b"\"\\n\"",
        b"\"\\u0061\"",
        b"\"a",
        b"\"a\"x",
    ] {
        assert!(try_parse_scalar(input).is_none(), "{input:?}");
    }
}

#[test]
fn scalar_numbers_keep_existing_rounding_and_reject_malformed_tokens() {
    for token in [
        "0",
        "-0",
        "42",
        "-123",
        "9007199254740993",
        "18446744073709551615",
        "260.75197",
        "0.000000001",
        "123456789012345678901234567890",
        "1e400",
        "-1e400",
        "1e-400",
        "-1e-400",
        "5e-324",
        "4.9406564584124654e-324",
        "1.7976931348623157e308",
    ] {
        let bytes = token.as_bytes();
        let mut reference = DirectParser::new(bytes);
        let expected = unsafe { reference.parse_value() };
        assert!(reference.finish());
        let value = try_parse_scalar(bytes).unwrap();
        assert_eq!(value.bits(), expected.bits(), "{token}");
        assert_eq!(
            value.as_number().to_bits(),
            token.parse::<f64>().unwrap().to_bits(),
            "{token}"
        );
    }
    for token in [
        "",
        " ",
        "-",
        "+1",
        "01",
        "-01",
        "1.",
        "1e",
        "1e+",
        "1e-",
        "1e+-1",
        "1x",
        "1.2.3",
        "0x1",
        "1e1.0",
        "1e00x",
        "NaN",
        "Infinity",
        "-Infinity",
        "1 2",
        "1\0",
        "true false",
        "{}",
        "[]",
        "[1]",
        "{\"a\":1}",
    ] {
        assert!(
            try_parse_scalar(token.as_bytes()).is_none(),
            "accepted {token:?}"
        );
    }
}

#[test]
fn mutated_scalar_tokens_never_accept_invalid_json() {
    let tokens = [
        "null",
        "true",
        "false",
        "-12.345e+6",
        "12345678901234567890",
        "0.1e-324",
    ];
    for token in tokens {
        for i in 0..token.len() {
            for byte in 0..=255u8 {
                let mut input = token.as_bytes().to_vec();
                input[i] = byte;
                if let Some(value) = try_parse_scalar(&input) {
                    let mut reference = DirectParser::new(&input);
                    let expected = unsafe { reference.parse_value() };
                    assert!(reference.finish(), "accepted {input:?}");
                    assert_eq!(value.bits(), expected.bits(), "{input:?}");
                }
            }
        }
    }
}
