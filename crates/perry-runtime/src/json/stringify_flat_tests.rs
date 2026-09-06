use super::*;

unsafe fn parse(text: &str) -> JSValue {
    let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    super::super::test_json_parse_direct(source)
}

fn output(value: JSValue) -> Vec<u8> {
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) =
        crate::string::str_bytes_from_jsvalue(f64::from_bits(value.bits()), &mut scratch).unwrap();
    unsafe { std::slice::from_raw_parts(ptr, len as usize).to_vec() }
}

#[test]
fn flat_direct_output_preserves_complete_object_and_string_lengths() {
    unsafe {
        for text in [
            "{}",
            "{\"a\":1}",
            "{\"a\":true,\"b\":null}",
            "{\"x\":\"東京🙂\",\"y\":\"abc\"}",
        ] {
            let value = parse(text);
            let result = try_object(value.bits()).expect("plain inline object");
            assert_eq!(output(result), text.as_bytes());
        }
        for unit in ["a", "é", "東京", "🙂"] {
            let text = format!("{{\"id\":1,\"text\":\"{}\"}}", unit.repeat(8192));
            let value = parse(&text);
            let result = try_object(value.bits()).unwrap();
            assert_eq!(output(result), text.as_bytes());
            assert_eq!(
                (*result.as_string_ptr()).utf16_len as usize,
                text.encode_utf16().count()
            );
        }
    }
}

#[test]
fn flat_direct_output_declines_complex_fields_and_reordered_keys() {
    unsafe {
        for text in [
            "[]",
            "{\"a\":[]}",
            "{\"a\":{}}",
            "{\"a\":1,\"b\":2,\"c\":3,\"d\":4,\"e\":5}",
            "{\"2\":1,\"1\":2}",
            "{\"a\":\"\\n\"}",
            "{\"a\":\"\\ud800\"}",
        ] {
            let value = parse(text);
            assert!(try_object(value.bits()).is_none(), "{text}");
        }
    }
}

#[test]
fn flat_direct_number_spelling_matches_the_existing_emitter() {
    unsafe {
        let mut state = 0x1234_5678_9abc_def0u64;
        let check = |bits: u64| {
            let Some(piece) = scalar_piece(bits) else {
                return;
            };
            let mut expected = String::new();
            super::super::write_number(&mut expected, f64::from_bits(bits));
            let mut bytes = [0; 32];
            let len = emit_piece(piece, bits, bytes.as_mut_ptr());
            assert_eq!(&bytes[..len], expected.as_bytes(), "{bits:016x}");
        };
        for number in [
            0.0,
            -0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            5e-324,
            1e-7,
            1e-6,
            1e20,
            1e21,
            562949953421312.25,
        ] {
            check(number.to_bits());
        }
        for n in [i32::MIN, -1, 0, 1, i32::MAX] {
            check(INT32_TAG | n as u32 as u64);
        }
        for _ in 0..16384 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            if state & crate::value::TAG_MASK < 0x7ff8_0000_0000_0000 {
                check(state);
            }
        }
    }
}
