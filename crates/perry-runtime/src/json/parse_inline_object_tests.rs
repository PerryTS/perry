use super::*;

fn as_json(value: JSValue) -> serde_json::Value {
    if value.is_short_string() {
        let mut bytes = [0; INLINE_BYTES];
        let len = value.short_string_to_buf(&mut bytes);
        return serde_json::Value::String(std::str::from_utf8(&bytes[..len]).unwrap().into());
    }
    if value.is_null() {
        return serde_json::Value::Null;
    }
    if value.is_bool() {
        return serde_json::Value::Bool(value.as_bool());
    }
    serde_json::json!(value.as_number())
}

fn expected_object(bytes: &[u8]) -> serde_json::Value {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    // Perry numbers are f64. Normalize the oracle's integer variants too.
    for field in value.as_object_mut().unwrap().values_mut() {
        if field.is_number() {
            *field = serde_json::json!(field.as_f64().unwrap());
        }
    }
    value
}

#[test]
fn json_inline_object_plan_matches_independent_parser_and_last_duplicate() {
    for key in ["", "a", "abcde", "é", "🙂", "0", "001"] {
        for value in [
            "null", "true", "false", "-0", "12345", "-12.3e-5", "\"\"", "\"é\"", "\"🙂\"",
        ] {
            for padding in ["", " \t\r\n"] {
                let input = format!("{padding}{{\"{key}\":{value},\"x\":0,\"x\":1}}{padding}");
                let plan = decode(input.as_bytes()).unwrap();
                let expected = expected_object(input.as_bytes());
                let mut actual = serde_json::Map::new();
                for &(key, value) in &plan.fields[..plan.len] {
                    actual.insert(as_json(key).as_str().unwrap().into(), as_json(value));
                }
                assert_eq!(serde_json::Value::Object(actual), expected, "{input}");
            }
        }
    }
    let plan = decode(br#"{"z":0,"a":1,"z":2}"#).unwrap();
    assert_eq!(as_json(plan.fields[0].0), "z");
    assert_eq!(plan.fields[0].1.as_number(), 2.0);
    let plan = decode(br#"{"x":-0}"#).unwrap();
    assert_eq!(plan.fields[0].1.as_number().to_bits(), (-0.0f64).to_bits());
    assert!(decode(br#"{"a":0,"b":1,"c":2,"d":3,"e":4,"f":5,"g":6,"h":7}"#).is_some());
    assert!(decode(br#"{"a":0,"b":1,"c":2,"d":3,"e":4,"f":5,"g":6,"h":7,"i":8}"#).is_none());
}

#[test]
fn json_inline_object_plan_declines_incomplete_invalid_and_unsupported_input() {
    for input in [
        "",
        "{}",
        "[]",
        "null",
        "{",
        "{\"a\"}",
        "{\"a\":}",
        "{\"a\":1,}",
        "{\"a\":1}x",
        "{\"a\":1}{}",
        "{\"a\":01}",
        "{\"a\":+1}",
        "{\"a\":.1}",
        "{\"a\":1.}",
        "{\"a\":1e}",
        "{\"a\":truex}",
        "{\"a\":NaN}",
        "{\"a\":1 2}",
        "{\"a\":\"x\"\"y\"}",
        "{\"a\":{}}",
        "{\"a\":[]}",
        "{\"abcdef\":1}",
        "{\"a\":\"abcdef\"}",
        "{\"a\":\"\\u0061\"}",
        "{\"\\u0061\":1}",
        "{\"__proto__\":null}",
        "{\"a\":undefined}",
    ] {
        assert!(decode(input.as_bytes()).is_none(), "{input:?}");
    }
    let valid = br#"{"a":true,"b":2}"#;
    for n in 0..valid.len() {
        assert!(decode(&valid[..n]).is_none());
    }
    let mut bounded = vec![b' '; MAX_BYTES - valid.len()];
    bounded.extend_from_slice(valid);
    assert!(decode(&bounded).is_some());
    bounded.push(b' ');
    assert!(decode(&bounded).is_none());
}

#[test]
fn json_inline_object_every_byte_mutation_is_valid_when_accepted() {
    for seed in [
        br#"{"a":1}"#.as_slice(),
        br#"{"x":"abc","b":false}"#.as_slice(),
    ] {
        for at in 0..seed.len() {
            for byte in 0..=255u8 {
                let mut bytes = seed.to_vec();
                bytes[at] = byte;
                if let Some(plan) = decode(&bytes) {
                    let expected = expected_object(&bytes);
                    let mut actual = serde_json::Map::new();
                    for &(key, value) in &plan.fields[..plan.len] {
                        actual.insert(as_json(key).as_str().unwrap().into(), as_json(value));
                    }
                    assert_eq!(serde_json::Value::Object(actual), expected, "{bytes:?}");
                }
            }
        }
    }
}

#[test]
fn json_inline_object_cold_cache_declines_without_managed_allocation() {
    crate::json::PARSE_SHAPE_CACHE.with(|cache| cache.borrow_mut().clear());
    let before = crate::arena::arena_in_use_bytes();
    let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
    for _ in 0..100 {
        let plan = decode(br#"{"a":1,"b":false}"#).unwrap();
        assert!(unsafe { allocate(&plan) }.is_none());
    }
    assert_eq!(crate::arena::arena_in_use_bytes(), before);
    assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
}
