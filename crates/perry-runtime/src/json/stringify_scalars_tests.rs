use super::{write_escaped_string, write_number};

#[test]
fn json_number_spelling_matches_ecmascript_boundaries() {
    // Exact f64 bits and expected JSON.stringify text, checked with both Node
    // 26.5.1 and Bun 1.3.14. Bits avoid depending on the JSON decimal parser.
    let cases = [
        (0x8000000000000000, "0"),
        (0x7ff8000000000000, "null"),
        (0x7ff0000000000000, "null"),
        (0xfff0000000000000, "null"),
        (0x0000000000000001, "5e-324"),
        (0x0010000000000000, "2.2250738585072014e-308"),
        (0x7fefffffffffffff, "1.7976931348623157e+308"),
        (0x3e7ad7f29abcaf48, "1e-7"),
        (0x3eb0c6f7a0b5ed8d, "0.000001"),
        (0x4415af1d78b58c40, "100000000000000000000"),
        (0x444b1ae4d6e2ef50, "1e+21"),
        (0x433fffffffffffff, "9007199254740991"),
        (0x4340000000000000, "9007199254740992"),
        (0x4390000000000000, "288230376151711740"),
        (0x43abc16d674ec801, "1000000000000000100"),
        (0x3fb999999999999a, "0.1"),
        (0x3fd3333333333333, "0.3"),
        (0x3fd5555555555555, "0.3333333333333333"),
        (0x400921fb54442d18, "3.141592653589793"),
        // Exact halfway values: Rust Display rounded the final digit up;
        // ECMAScript selects the even last digit of the shortest spelling.
        (0x4300000000000002, "562949953421312.2"),
        (0xc300000000000002, "-562949953421312.2"),
        (0x4310000000000001, "1125899906842624.2"),
        (0x42d6bcc41e900008, "100000000000000.12"),
        (0xc2b979595fd7dd90, "-28008981190621.562"),
    ];
    for (bits, expected) in cases {
        let value = f64::from_bits(bits);
        let mut output = String::from("prefix:");
        unsafe { write_number(&mut output, value) };
        assert_eq!(
            output.strip_prefix("prefix:"),
            Some(expected),
            "{bits:016x}"
        );
        unsafe {
            let output = crate::json::js_json_stringify_number(value);
            assert_eq!(crate::json::str_from_header(output), Some(expected));
        }
    }
}

#[test]
fn tagged_int32_json_numbers_keep_their_signed_value() {
    for value in [i32::MIN, -1, 0, 1, i32::MAX] {
        let bits = crate::value::INT32_TAG | value as u32 as u64;
        let mut output = String::new();
        unsafe { write_number(&mut output, f64::from_bits(bits)) };
        assert_eq!(output, value.to_string());
    }
}

#[test]
fn vector_escaping_matches_json_encoder_at_every_boundary() {
    let characters: Vec<char> = (0..=127)
        .filter_map(char::from_u32)
        .chain(['é', '東', '🙂', '\u{D000}', '\u{D7FF}', '\u{E000}'])
        .collect();
    for prefix_len in [0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 63, 64, 255, 4096] {
        let prefix = "a".repeat(prefix_len);
        for &ch in &characters {
            let text = format!("{prefix}{ch}last");
            let mut output = String::new();
            unsafe { write_escaped_string(&mut output, &text) };
            assert_eq!(
                output,
                serde_json::to_string(&text).unwrap(),
                "prefix={prefix_len} ch={ch:?}"
            );
        }
    }
}

#[test]
fn control_escapes_append_to_existing_output_without_temporary_formatting() {
    let text: String = (0..32).filter_map(char::from_u32).collect();
    let mut output = String::from("prefix:");
    unsafe { write_escaped_string(&mut output, &text) };
    assert_eq!(
        output,
        format!("prefix:{}", serde_json::to_string(&text).unwrap())
    );
}
