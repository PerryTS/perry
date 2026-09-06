use super::nesting_depth_exceeds;

// An intentionally scalar state machine serves as an independent oracle for
// malformed as well as valid input. The preflight does not validate JSON.
fn reference(bytes: &[u8], limit: usize) -> bool {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escape = false;
    for &b in bytes {
        if quoted {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > limit {
                        return true;
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

#[test]
fn quoted_runs_escapes_and_truncations_keep_depth_semantics() {
    for prefix in [0, 1, 7, 8, 15, 16, 17, 31, 32, 63, 64, 1024] {
        for escapes in 0..=9 {
            let mut bytes = b"{\"text\":\"".to_vec();
            bytes.extend(std::iter::repeat_n(b'[', prefix));
            bytes.extend(std::iter::repeat_n(b'\\', escapes));
            bytes.extend_from_slice(b"\"[[[{\"x\":1}]]]}");
            for end in [
                prefix,
                bytes.len().saturating_sub(2),
                bytes.len().saturating_sub(1),
                bytes.len(),
            ] {
                for limit in 0..6 {
                    assert_eq!(
                        nesting_depth_exceeds(&bytes[..end], limit),
                        reference(&bytes[..end], limit),
                        "prefix={prefix} escapes={escapes} end={end} limit={limit}"
                    );
                }
            }
        }
    }
}

#[test]
fn random_malformed_bytes_match_depth_oracle() {
    let alphabet = b"[]{}\"\\abc012, :\n\r\t\0\xED\xFF";
    let mut state = 0x8D14_0A35_BC72_690Fu64;
    for n in 0..10000 {
        let mut bytes = vec![0; n % 513];
        for byte in &mut bytes {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = alphabet[(state as usize) % alphabet.len()];
        }
        for limit in [0, 1, 3, 15, 64, 1000] {
            assert_eq!(
                nesting_depth_exceeds(&bytes, limit),
                reference(&bytes, limit),
                "limit={limit} bytes={bytes:?}"
            );
        }
    }
}

#[test]
fn flat_scalar_array_hint_preserves_depth_even_for_malformed_input() {
    for length in [0, 1, 15, 16, 254, 255, 256, 257, 1024] {
        for suffix in [
            "]",
            "",
            ",true,false,null]",
            "[[[[]]]]",
            "{\"x\":[[[[0]]]]}]",
            "\"[[[[\"]",
        ] {
            let bytes = format!("[{}{suffix}", "1".repeat(length)).into_bytes();
            for limit in 0..6 {
                assert_eq!(
                    nesting_depth_exceeds(&bytes, limit),
                    reference(&bytes, limit),
                    "length={length} suffix={suffix:?} limit={limit}"
                );
            }
        }
    }
}

#[test]
fn wide_object_duplicates_keep_first_position_and_last_value() {
    use crate::json::{js_json_parse, js_json_stringify, str_from_header, TYPE_UNKNOWN};
    for count in [7, 8, 9, 31, 32, 33, 127, 128, 129, 130, 1024] {
        let fields: Vec<String> = (0..count).map(|i| format!("\"k{i}\":{i}")).collect();
        let input = format!(
            "{{{},\"k0\":-1,\"k{}\":-3,\"\\u006b0\":-2}}",
            fields.join(","),
            count - 1
        );
        let mut expected = fields;
        expected[0] = "\"k0\":-2".into();
        expected[count - 1] = format!("\"k{}\":-3", count - 1);
        let expected = format!("{{{}}}", expected.join(","));
        let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        unsafe {
            let value = js_json_parse(text);
            let output = js_json_stringify(f64::from_bits(value.bits()), TYPE_UNKNOWN);
            assert_eq!(str_from_header(output).unwrap(), expected, "keys={count}");
        }
    }
}

#[test]
fn parse_shape_cache_bounds_retained_keys_and_keeps_small_shape_hits() {
    use crate::json::{parse_shape_keys_array, PARSE_SHAPE_CACHE, PARSE_SHAPE_CACHE_KEY_BUDGET};
    unsafe {
        crate::gc::gc_suppress();
        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        // Distinct medium shapes fill the key budget before the entry cap.
        for shape in 0..20 {
            let keys: Vec<_> = (0..257)
                .map(|field| {
                    let text = format!("shape_{shape}_{field}");
                    crate::js_string_from_bytes(text.as_ptr(), text.len() as u32) as *const _
                })
                .collect();
            let arr = parse_shape_keys_array(&keys);
            assert_eq!((*arr).length, 257);
        }
        PARSE_SHAPE_CACHE.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(cache.len(), 15);
            assert!(
                cache.iter().map(|entry| entry.keys.len()).sum::<usize>()
                    <= PARSE_SHAPE_CACHE_KEY_BUDGET
            );
        });
        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        let key = crate::js_string_from_bytes(b"small".as_ptr(), 5) as *const _;
        let first = parse_shape_keys_array(&[key]);
        assert_eq!(first, parse_shape_keys_array(&[key]));
        // Too-wide shapes belong to the returned graph, not to retained
        // parser metadata. A repeated request must not add cache entries.
        let wide = vec![key; PARSE_SHAPE_CACHE_KEY_BUDGET + 1];
        let wide_a = parse_shape_keys_array(&wide);
        let wide_b = parse_shape_keys_array(&wide);
        assert_ne!(wide_a, wide_b);
        assert_eq!((*wide_a).length as usize, wide.len());
        PARSE_SHAPE_CACHE.with(|cache| assert_eq!(cache.borrow().len(), 1));
        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        crate::gc::gc_unsuppress();
    }
}
