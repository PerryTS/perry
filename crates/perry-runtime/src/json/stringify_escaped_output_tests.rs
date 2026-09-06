use super::*;

#[test]
fn json_escaped_expansion_count_matches_scalar_oracle_at_every_lane_and_tail() {
    let oracle = |bytes: &[u8]| -> u64 {
        bytes
            .iter()
            .map(|b| match b {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 1,
                0..=31 => 5,
                _ => 0,
            })
            .sum()
    };
    for len in 0..=80 {
        for value in 0..=255u8 {
            let bytes = vec![value; len];
            assert_eq!(count_expansion(&bytes), oracle(&bytes));
        }
    }
    let mut state = 0x27f1_065d_99a8_c31bu64;
    for offset in 0..32 {
        let mut bytes = vec![0; 4096 + offset];
        for b in &mut bytes {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *b = state as u8;
        }
        for tail in 0..32 {
            let slice = &bytes[offset..bytes.len() - tail];
            assert_eq!(count_expansion(slice), oracle(slice));
        }
    }
}

#[test]
fn json_escaped_output_matches_complete_oracle_with_bounded_writes() {
    let ascii: String = (0..=127).map(char::from).collect();
    for text in [
        ascii,
        "\"\\\n\r\t\u{8}\u{c}東京🙂\u{2028}\u{2029}".repeat(4096),
    ] {
        let expected = serde_json::to_string(&text).unwrap();
        let plan = Plan::new(text.as_bytes(), text.encode_utf16().count() as u32).unwrap();
        let mut output = vec![0xa5; plan.bytes as usize + 32];
        let written = unsafe { plan.write(text.as_ptr(), output.as_mut_ptr().add(16)) };
        assert_eq!(written, expected.len());
        assert_eq!(&output[16..16 + written], expected.as_bytes());
        assert!(output[..16]
            .iter()
            .chain(&output[16 + written..])
            .all(|&b| b == 0xa5));
        assert_eq!(plan.units as usize, expected.encode_utf16().count());
        unsafe {
            let source = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let result = quote(source).unwrap();
            assert_eq!(
                std::slice::from_raw_parts(string_data(result), (*result).byte_len as usize),
                expected.as_bytes()
            );
            assert_eq!((*result).utf16_len, plan.units);
            assert_eq!((*result).capacity, plan.bytes);
            assert_eq!((*result).flags, 0);
        }
    }
}

#[test]
fn json_escaped_output_declines_invalid_utf8_and_length_overflow() {
    for bytes in [
        b"\n\xed\xa0\x80".as_slice(),
        b"\n\xed\xa0\xbd\xed\xb1\x8d",
        b"\"\x80",
        b"\n\xc2",
        b"\n\xe2\x82",
        b"\n\xf0\x9f\x99",
        b"\n\xff",
    ] {
        assert!(Plan::new(bytes, bytes.len() as u32).is_none());
    }
    assert!(Plan::with_expansion(u32::MAX - 1, 0, 0).is_none());
    assert!(Plan::with_expansion(0, u32::MAX - 1, 0).is_none());
    assert!(Plan::with_expansion(0, 0, u64::MAX).is_none());
    assert!(Plan::with_expansion(0, 0, u32::MAX as u64).is_none());
    assert_eq!(
        Plan::with_expansion(u32::MAX - 7, 0, 5).unwrap().bytes,
        u32::MAX
    );
}

#[test]
fn json_escaped_output_handles_every_ascii_byte_in_keys_values_and_arrays() {
    unsafe {
        for byte in 0..=127u8 {
            let text = format!("prefix{}東京🙂suffix", char::from(byte));
            let quoted = serde_json::to_string(&text).unwrap();
            for array in [false, true] {
                let field = if array {
                    format!("[{quoted},1,true,null]")
                } else {
                    quoted.clone()
                };
                let expected = format!("{{{quoted}:{field},\"id\":1}}");
                let source =
                    crate::string::js_string_from_bytes(expected.as_ptr(), expected.len() as u32);
                let input = crate::json::test_json_parse_direct(source);
                let result = if array {
                    super::super::stringify_record_output::try_object(input.bits())
                } else {
                    super::super::stringify_flat::try_object(input.bits())
                }
                .expect("direct output must accept escaped keys and fields");
                let header = result.as_string_ptr();
                assert_eq!(
                    std::slice::from_raw_parts(string_data(header), (*header).byte_len as usize),
                    expected.as_bytes()
                );
                assert_eq!(
                    (*header).utf16_len as usize,
                    expected.encode_utf16().count()
                );
            }
        }
    }
}
