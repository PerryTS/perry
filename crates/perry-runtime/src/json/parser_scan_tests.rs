use super::nesting_depth_exceeds;

// An intentionally scalar state machine serves as an independent oracle for
// malformed as well as valid input. The preflight does not validate JSON.
pub(super) fn reference(bytes: &[u8], limit: usize) -> bool {
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
fn large_unescaped_parsed_strings_are_individually_tracked_leaves() {
    let payload_len = crate::string::JSON_MALLOC_OUTPUT_THRESHOLD as usize;
    let mut input = Vec::with_capacity(payload_len + 2);
    input.push(b'"');
    input.extend(std::iter::repeat_n(b'x', payload_len));
    input.push(b'"');
    let source = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    unsafe {
        let value = crate::json::js_json_parse(source);
        let string = value.as_string_ptr();
        let header = crate::value::addr_class::try_read_gc_header(string as usize)
            .expect("parsed string should have a tracked GC header");
        assert!(crate::gc::gc_malloc_header_is_tracked(header));
        assert_eq!((*string).byte_len as usize, payload_len);
        assert_eq!((*string).utf16_len as usize, payload_len);
        let bytes = std::slice::from_raw_parts(crate::string::string_data(string), payload_len);
        assert!(bytes.iter().all(|&byte| byte == b'x'));
    }
}

#[test]
fn repeated_parse_reuses_only_the_immutable_string_token() {
    let payload = "x".repeat(512);
    let input = format!(r#"{{"text":"{payload}"}}"#);
    let source = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    unsafe {
        let first = crate::json::js_json_parse(source);
        let scope = crate::gc::RuntimeHandleScope::new();
        let first = scope.root_nanbox_u64(first.bits());
        assert!(crate::json::test_parse_object_template_matches(
            source,
            input.len()
        ));
        let second = crate::json::js_json_parse(source);
        assert_ne!(
            first.get_nanbox_u64(),
            second.bits(),
            "mutable objects stay distinct"
        );

        let first_output = crate::json::js_json_stringify(
            f64::from_bits(first.get_nanbox_u64()),
            crate::json::TYPE_UNKNOWN,
        );
        let second_output = crate::json::js_json_stringify(
            f64::from_bits(second.bits()),
            crate::json::TYPE_UNKNOWN,
        );
        assert_eq!(
            crate::json::str_from_header(first_output),
            Some(input.as_str())
        );
        assert_eq!(
            crate::json::str_from_header(second_output),
            Some(input.as_str())
        );
    }

    let scalar = format!("\"{payload}\"");
    let scalar_source = crate::js_string_from_bytes(scalar.as_ptr(), scalar.len() as u32);
    unsafe {
        let first = crate::json::js_json_parse(scalar_source);
        let second = crate::json::js_json_parse(scalar_source);
        assert_eq!(
            first.bits(),
            second.bits(),
            "string primitives may be shared"
        );
    }
}

#[test]
fn repeated_small_object_template_rebuilds_nested_arrays() {
    let input = r#"{"id":42,"name":"long-enough-to-cross-the-small-template-threshold","tags":["alpha","beta"]}"#;
    let source = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    unsafe {
        let first = crate::json::js_json_parse(source);
        let scope = crate::gc::RuntimeHandleScope::new();
        let first = scope.root_nanbox_u64(first.bits());
        assert!(crate::json::test_parse_object_template_matches(
            source,
            input.len()
        ));
        let second = crate::json::js_json_parse(source);
        let first_object = crate::JSValue::from_bits(first.get_nanbox_u64())
            .as_pointer::<crate::object::ObjectHeader>();
        let second_object = second.as_pointer::<crate::object::ObjectHeader>();
        assert_ne!(first_object, second_object);
        let first_fields = first_object
            .cast::<u8>()
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            .cast::<crate::JSValue>();
        let second_fields = second_object
            .cast::<u8>()
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            .cast::<crate::JSValue>();
        assert_ne!(
            (*first_fields.add(2)).as_pointer::<crate::array::ArrayHeader>(),
            (*second_fields.add(2)).as_pointer::<crate::array::ArrayHeader>(),
            "nested mutable arrays must be reconstructed"
        );
        let output = crate::json::js_json_stringify(
            f64::from_bits(second.bits()),
            crate::json::TYPE_UNKNOWN,
        );
        assert_eq!(crate::json::str_from_header(output), Some(input));
    }
}

#[test]
fn nested_wide_object_does_not_break_outer_duplicate_identity() {
    use crate::json::{js_json_parse, js_json_stringify, str_from_header, TYPE_UNKNOWN};

    let nested_fields: Vec<String> = (0..129).map(|i| format!("\"k{i}\":{i}")).collect();
    let input = format!(
        "{{\"dup\":1,\"nested\":{{{}}},\"dup\":2}}",
        nested_fields.join(",")
    );
    let expected = format!("{{\"dup\":2,\"nested\":{{{}}}}}", nested_fields.join(","));
    let text = crate::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    unsafe {
        let value = js_json_parse(text);
        let output = js_json_stringify(f64::from_bits(value.bits()), TYPE_UNKNOWN);
        assert_eq!(str_from_header(output).unwrap(), expected);
    }
}

#[test]
fn wide_object_index_resolves_exact_bytes_inside_a_hash_collision() {
    unsafe {
        crate::gc::gc_suppress();
        let keys: Vec<*const crate::StringHeader> = [b"alpha".as_slice(), b"beta".as_slice()]
            .into_iter()
            .map(|bytes| {
                crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32).cast_const()
            })
            .collect();
        let mut index = super::ParsedObjectIndex {
            hash_state: ahash::RandomState::new(),
            primary: crate::fast_hash::new_ptr_hash_map(),
            collisions: Vec::new(),
        };
        let forced_hash = 0x51de_c011_1510_0001;
        index.insert_hash(forced_hash, 0);
        index.insert_hash(forced_hash, 1);
        assert_eq!(index.find_hashed(forced_hash, b"alpha", &keys), Some(0));
        assert_eq!(index.find_hashed(forced_hash, b"beta", &keys), Some(1));
        assert_eq!(index.find_hashed(forced_hash, b"gamma", &keys), None);
        assert_eq!(index.find_hashed(forced_hash ^ 1, b"alpha", &keys), None);
        crate::gc::gc_unsuppress();
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

#[test]
fn bounded_root_record_reuses_the_warm_shape_during_one_pass_parse() {
    use crate::json::{cached_parse_key_ptr, parse_shape_keys_array, PARSE_SHAPE_CACHE};

    let input = br#"{"id":42,"name":"user_42","email":"user_42@example.com","active":false,"score":63.0,"tags":["tag_2","tag_0"]}"#;
    unsafe {
        crate::gc::gc_suppress();
        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        let keys: Vec<_> = ["id", "name", "email", "active", "score", "tags"]
            .into_iter()
            .map(|key| cached_parse_key_ptr(key.as_bytes()))
            .collect();
        let keys_array = parse_shape_keys_array(&keys);
        let shape_id = PARSE_SHAPE_CACHE.with(|cache| cache.borrow().last().unwrap().shape_id);
        assert_ne!(shape_id, 0);

        let mut parser = super::DirectParser::new_batched(input);
        assert!(parser.warm_record_shape_pending);
        assert_eq!(parser.hot_shape_len, keys.len());
        assert_eq!(parser.hot_shape_id, shape_id);
        let value = parser.parse_value();
        assert!(parser.finish());
        let object = value.as_pointer::<crate::ObjectHeader>();
        assert_eq!(crate::object::object_keys_array(object), keys_array);
        assert_eq!((*object).parent_class_id, shape_id);

        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        crate::gc::gc_unsuppress();
    }
}

#[test]
fn warm_key_scan_only_claims_an_exact_unescaped_spelling() {
    use crate::json::cached_parse_key_ptr;

    unsafe {
        crate::gc::gc_suppress();
        let expected = cached_parse_key_ptr(b"name");

        let mut plain = super::DirectParser::new(br#""name":"#);
        let (key, matched) = plain.parse_string_bytes_expected(expected).unwrap();
        assert!(matched);
        assert_eq!(key.as_bytes(), b"name");
        assert_eq!(plain.pos, 6);

        let mut escaped = super::DirectParser::new(br#""na\u006de":"#);
        let (key, matched) = escaped.parse_string_bytes_expected(expected).unwrap();
        assert!(!matched);
        assert_eq!(key.as_bytes(), b"name");

        let escaped_backslash = cached_parse_key_ptr(b"\\q");
        let mut invalid_escape = super::DirectParser::new(br#""\q":"#);
        assert!(invalid_escape
            .parse_string_bytes_expected(escaped_backslash)
            .is_none());
        assert!(!invalid_escape.valid);

        let control = cached_parse_key_ptr(b"\n");
        let mut raw_control = super::DirectParser::new(b"\"\n\":");
        assert!(raw_control.parse_string_bytes_expected(control).is_none());
        assert!(!raw_control.valid);
        crate::gc::gc_unsuppress();
    }
}

#[test]
fn warm_shape_fallback_keeps_duplicate_semantics_after_key_cache_eviction() {
    use crate::json::{
        cached_parse_key_ptr, clear_parse_key_ring, parse_shape_keys_array, PARSE_KEY_CACHE,
        PARSE_SHAPE_CACHE,
    };

    let input = br#"{"id":1,"other":"padding keeps this record above sixty-four bytes","id":2,"name":"n","email":"e"}"#;
    unsafe {
        crate::gc::gc_suppress();
        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        let shape_keys: Vec<_> = ["id", "name", "email"]
            .into_iter()
            .map(|key| cached_parse_key_ptr(key.as_bytes()))
            .collect();
        parse_shape_keys_array(&shape_keys);
        PARSE_KEY_CACHE.with(|cache| cache.borrow_mut().clear());
        clear_parse_key_ring();

        let mut parser = super::DirectParser::new_batched(input);
        let value = parser.parse_value();
        assert!(parser.finish());
        let object = value.as_pointer::<crate::ObjectHeader>();
        let keys = crate::object::object_keys_array(object);
        assert_eq!((*keys).length, 4, "the second id must replace the first");
        let id = cached_parse_key_ptr(b"id");
        assert_eq!(
            crate::object::js_object_get_field_by_name(object, id).as_number(),
            2.0
        );

        PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
        crate::gc::gc_unsuppress();
    }
}

#[test]
fn depth_preflight_byte_bound_keeps_the_first_excess_opening() {
    use super::nesting_depth_exceeds;
    for limit in [0, 1, 31, super::MAX_RECURSIVE_NESTING_DEPTH] {
        // All opening bytes is the worst possible depth for a given length,
        // including malformed input that must still take the bounded path.
        assert!(!nesting_depth_exceeds(&vec![b'['; limit], limit));
        assert!(nesting_depth_exceeds(&vec![b'['; limit + 1], limit));
        assert!(!nesting_depth_exceeds(&vec![b'}'; limit + 1], limit));
    }
    assert!(!nesting_depth_exceeds(b"\"[[[[\"", 1));
    assert!(nesting_depth_exceeds(b"[\"[\",[]]", 1));
}
