use super::*;

struct Suppressed;
impl Suppressed {
    fn new() -> Self {
        crate::gc::gc_suppress();
        Self
    }
}
impl Drop for Suppressed {
    fn drop(&mut self) {
        crate::gc::gc_unsuppress();
    }
}

unsafe fn parse(text: &str) -> *mut ArrayHeader {
    let mut parser = DirectParser::new(text.as_bytes());
    let value = parser.parse_value();
    assert!(parser.finish(), "{text}");
    value.as_pointer::<ArrayHeader>() as *mut ArrayHeader
}

#[test]
fn json_short_arrays_preserve_values_and_use_exact_completed_width() {
    let _suppressed = Suppressed::new();
    unsafe {
        for item in [
            "17",
            "1.25",
            "null",
            "true",
            "\"short\"",
            "\"long heap string\"",
            "[1,2]",
            "{\"x\":1}",
        ] {
            for length in 0..=17 {
                let source = format!("[{}]", vec![item; length].join(","));
                let array = parse(&source);
                assert_eq!((*array).length as usize, length);
                if length <= 8 && (length == 0 || !item.starts_with('{')) {
                    assert_eq!((*array).capacity as usize, length, "{source}");
                }
                let output = crate::json::js_json_stringify(
                    f64::from_bits(JSValue::array_ptr(array).bits()),
                    0,
                );
                let rendered = std::slice::from_raw_parts(
                    crate::string::string_data(output),
                    (*output).byte_len as usize,
                );
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(rendered).unwrap(),
                    serde_json::from_str::<serde_json::Value>(&source).unwrap(),
                    "{source}",
                );
            }
        }
        let array = parse(r#"[0,{"x":"heap child"},[true,false],"tail"]"#);
        assert_eq!((*array).capacity, 4);
        assert_eq!((*array).length, 4);
    }
}

#[test]
fn json_short_arrays_allocate_no_intermediate_array() {
    let _suppressed = Suppressed::new();
    for length in 0..=8 {
        let source = format!("[{}]", vec!["17"; length].join(","));
        let roots = parse_root_save_len();
        let before = crate::arena::arena_in_use_bytes();
        let array = unsafe { parse(&source) };
        let header = unsafe {
            &*((array as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
        };
        assert_eq!(
            crate::arena::arena_in_use_bytes() - before,
            header.size as usize
        );
        assert_eq!(parse_root_save_len(), roots);
    }
}

#[test]
fn json_short_arrays_keep_numeric_layout_and_negative_zero() {
    let _suppressed = Suppressed::new();
    unsafe {
        for source in [
            "[]",
            "[1]",
            "[1,2,3,4,5,6,7,8]",
            "[1,2,3,4,5,6,7,8,9]",
            "[-0,1.25]",
        ] {
            let array = parse(source);
            assert_ne!(
                crate::array::js_array_is_numeric_f64_layout(array),
                0,
                "{source}"
            );
        }
        let array = parse("[-0,1.25]");
        assert_eq!(
            crate::array::js_array_get(array, 0).as_number().to_bits(),
            (-0.0f64).to_bits()
        );
        assert_eq!(crate::array::js_array_get(array, 1).as_number(), 1.25);
        for source in ["[null]", "[true]", "[\"a\"]", "[1,\"a\",3]", "[[1]]"] {
            let array = parse(source);
            assert_eq!(
                crate::array::js_array_is_numeric_f64_layout(array),
                0,
                "{source}"
            );
        }
    }
}

#[test]
fn json_short_array_growth_preserves_the_original_alias() {
    let _suppressed = Suppressed::new();
    unsafe {
        for length in 0..=9 {
            let source = format!(
                "[{}]",
                (0..length)
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let original = parse(&source);
            let mut array = original;
            for value in length..65 {
                array = crate::array::js_array_push(array, JSValue::number(value as f64));
                assert_eq!(crate::array::js_array_length(original), value + 1);
                assert_eq!(
                    crate::array::js_array_get(original, value).as_number(),
                    value as f64
                );
            }
            for index in 0..65 {
                assert_eq!(
                    crate::array::js_array_get(original, index).as_number(),
                    index as f64
                );
            }
        }
    }
}

#[test]
fn json_short_array_errors_and_spills_restore_roots() {
    let _suppressed = Suppressed::new();
    for source in [
        "[",
        "[1",
        "[1,]",
        "[,1]",
        "[1,,2]",
        "[1 2]",
        "[01]",
        "[1e]",
        "[1]x",
        "[1][2]",
        "[[0],]",
        "[1,2,3,4,5,6,7,8,]",
        "[1,2,3,4,5,6,7,8,9,]",
        "[\"long heap string\",]",
    ] {
        let roots = parse_root_save_len();
        let mut parser = DirectParser::new(source.as_bytes());
        unsafe {
            parser.parse_value();
        }
        assert!(!parser.finish(), "{source}");
        assert_eq!(parse_root_save_len(), roots, "{source}");
    }
}
