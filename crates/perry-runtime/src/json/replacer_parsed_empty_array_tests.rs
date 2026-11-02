use super::*;

fn parse(text: &str) -> f64 {
    let input = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    unsafe { f64::from_bits(crate::json::js_json_parse(input).bits()) }
}

fn text(bits: i64) -> String {
    let value = JSValue::from_bits(bits as u64);
    if value.is_short_string() {
        let mut bytes = [0; crate::value::SHORT_STRING_MAX_LEN];
        let n = value.short_string_to_buf(&mut bytes);
        return String::from_utf8(bytes[..n].to_vec()).unwrap();
    }
    let ptr = (bits as u64 & POINTER_MASK) as *const StringHeader;
    unsafe { str_from_header(ptr) }
        .expect("JSON.stringify must return a string")
        .to_string()
}

struct GcSuppressed;
impl Drop for GcSuppressed {
    fn drop(&mut self) {
        crate::gc::gc_unsuppress();
    }
}

/// #13709: `JSON.parse` builds `[]` with capacity 0. The pretty path's
/// structural fallback required `cap > 0`, so the empty array fell through
/// to the string fallback and `JSON.stringify(v, null, 2)` printed `""`.
#[test]
fn pretty_stringify_keeps_parsed_empty_array() {
    crate::gc::gc_suppress();
    let _guard = GcSuppressed;
    let null = f64::from_bits(TAG_NULL);
    unsafe {
        // The test only means something if the parser really hands out a
        // zero-capacity array (the shape the old probe rejected).
        let top = parse("[]");
        let arr = crate::array::clean_arr_ptr(
            (top.to_bits() & POINTER_MASK) as *const crate::ArrayHeader,
        );
        assert!(!arr.is_null(), "JSON.parse(\"[]\") must yield an array");
        assert_eq!((*arr).length, 0);
        assert_eq!(
            (*arr).capacity,
            0,
            "precondition: the parser's empty array has capacity 0"
        );

        for (source, expected) in [
            ("[]", "[]"),
            ("{\"kb\":[]}", "{\n  \"kb\": []\n}"),
            ("[[]]", "[\n  []\n]"),
            ("{\"a\":{\"b\":[]}}", "{\n  \"a\": {\n    \"b\": []\n  }\n}"),
        ] {
            let compact = js_json_stringify_full(parse(source), null, null);
            assert_eq!(text(compact), source, "compact form of {source}");
            let pretty = js_json_stringify_full(parse(source), null, 2.0);
            assert_eq!(text(pretty), expected, "indented form of {source}");
        }
    }
}

/// The replacer-array probe shared the same `cap > 0` gate: an empty parsed
/// array as the replacer (`JSON.stringify(o, JSON.parse("[]"))`) was not
/// recognised as a property list, so every key was emitted instead of none.
#[test]
fn parsed_empty_array_replacer_selects_no_keys() {
    crate::gc::gc_suppress();
    let _guard = GcSuppressed;
    let null = f64::from_bits(TAG_NULL);
    unsafe {
        let out = js_json_stringify_full(parse("{\"a\":1}"), parse("[]"), null);
        assert_eq!(text(out), "{}");
        let out = js_json_stringify_full(parse("{\"a\":1}"), parse("[]"), 2.0);
        assert_eq!(text(out), "{}");
    }
}
