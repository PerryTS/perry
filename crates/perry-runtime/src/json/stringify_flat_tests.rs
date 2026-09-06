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
        // A cold prototype lookup belongs to the rooted general serializer.
        let warm = parse("{}");
        let scope = crate::gc::RuntimeHandleScope::new();
        let warm = scope.root_nanbox_u64(warm.bits());
        assert!(
            super::super::stringify_tojson_probe::to_json_definitely_absent(
                (warm.get_nanbox_f64().to_bits() & POINTER_MASK) as *const u8
            )
        );
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
fn flat_empty_output_declines_cold_lookup_and_stays_allocation_free_when_warm() {
    unsafe {
        let value = parse("{}");
        let scope = crate::gc::RuntimeHandleScope::new();
        let input = scope.root_nanbox_u64(value.bits());
        let saved_proto = scope.root_nanbox_u64(CACHED_OBJECT_PROTO_BITS.with(|c| c.replace(0)));
        let saved_state = OBJECT_PROTO_TOJSON_STATE.with(|c| c.replace(0));
        let before = crate::arena::arena_total_bytes();
        let cold = try_object(input.get_nanbox_f64().to_bits());
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(saved_proto.get_nanbox_f64().to_bits()));
        OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(saved_state));
        assert!(cold.is_none());
        assert_eq!(crate::arena::arena_total_bytes(), before);

        super::super::invalidate_object_proto_tojson_state();
        assert!(
            super::super::stringify_tojson_probe::to_json_definitely_absent(
                (input.get_nanbox_f64().to_bits() & POINTER_MASK) as *const u8
            )
        );
        let before = crate::arena::arena_total_bytes();
        let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
        let mut collections_before = 0;
        crate::gc::js_gc_stats(
            &mut collections_before,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        for _ in 0..1000 {
            let result = try_object(input.get_nanbox_f64().to_bits()).unwrap();
            assert_eq!(result.bits(), JSValue::short_string_unchecked(b"{}").bits());
        }
        let mut collections_after = 0;
        crate::gc::js_gc_stats(
            &mut collections_after,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(crate::arena::arena_total_bytes(), before);
        assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
        assert_eq!(collections_after, collections_before);
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

#[test]
fn flat_direct_integer_boundaries_match_ecmascript_spelling() {
    unsafe {
        let check = |number: f64| {
            let bits = number.to_bits();
            let piece = scalar_piece(bits).expect("finite numeric value");
            let mut bytes = [0; 32];
            let len = emit_piece(piece, bits, bytes.as_mut_ptr());
            let mut oracle = ryu_js::Buffer::new();
            assert_eq!(
                &bytes[..len],
                oracle.format_finite(number).as_bytes(),
                "{bits:016x}"
            );
        };
        for number in [0.0, -0.0, 0.1, -0.1, 1.5, -1.5, 1e20, 1e21] {
            check(number);
        }
        // Include adjacent fractional floats and integers around powers of two,
        // especially 2^53 and the shortest-spelling boundary cases above it.
        for exponent in 0..=63 {
            let center = (1u64 << exponent) as f64;
            for bits in [center.to_bits() - 1, center.to_bits(), center.to_bits() + 1] {
                let number = f64::from_bits(bits);
                check(number);
                check(-number);
            }
        }
        let mut state = 0x9176_25bc_d803_4aefu64;
        for _ in 0..16384 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let integer = (state & ((1u64 << 53) - 1)) as f64;
            check(integer);
            check(-integer);
        }
    }
}
