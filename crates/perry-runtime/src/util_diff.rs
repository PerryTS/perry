//! `util.diff(actual, expected)` edit tuples for strings and string arrays.

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::string::{js_string_from_bytes, js_string_from_wtf8_bytes, str_bytes_from_jsvalue};
use crate::value::{JSValue, POINTER_MASK};

#[derive(Clone)]
struct DiffToken {
    key: Vec<u16>,
    value: f64,
}

struct DiffInput {
    tokens: Vec<DiffToken>,
    source: Option<String>,
}

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn array_ptr_from_value(value: f64) -> Option<*const crate::array::ArrayHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        match gc_type_for_ptr(raw) {
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY) => {
                Some(raw as *const crate::array::ArrayHeader)
            }
            _ => None,
        }
    }
}

fn read_js_string(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn string_value_from_unit(unit: u16) -> f64 {
    if (0xD800..=0xDFFF).contains(&unit) {
        let bytes = [
            (0xE0 | ((unit >> 12) & 0x0F)) as u8,
            (0x80 | ((unit >> 6) & 0x3F)) as u8,
            (0x80 | (unit & 0x3F)) as u8,
        ];
        let ptr = js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(JSValue::string_ptr(ptr).bits());
    }

    let ch = char::from_u32(unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn invalid_arg(name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type string. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn collect_input(value: f64, name: &str) -> DiffInput {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_any_string() {
        let text = read_js_string(value).unwrap_or_default();
        let tokens = text
            .encode_utf16()
            .map(|unit| DiffToken {
                key: vec![unit],
                value: string_value_from_unit(unit),
            })
            .collect();
        return DiffInput {
            tokens,
            source: Some(text),
        };
    }

    if let Some(arr) = array_ptr_from_value(value) {
        let len = js_array_length(arr);
        let mut tokens = Vec::with_capacity(len as usize);
        for i in 0..len {
            let element = js_array_get_f64(arr, i);
            let Some(text) = read_js_string(element) else {
                invalid_arg(&format!("{}[{}]", name, i), element);
            };
            tokens.push(DiffToken {
                key: text.encode_utf16().collect(),
                value: element,
            });
        }
        return DiffInput {
            tokens,
            source: None,
        };
    }

    invalid_arg(name, value)
}

fn tuple_value(kind: i32, value: f64) -> f64 {
    let mut tuple = js_array_alloc(2);
    tuple = js_array_push_f64(tuple, kind as f64);
    tuple = js_array_push_f64(tuple, value);
    crate::value::js_nanbox_pointer(tuple as i64)
}

fn diff_to_array(actual: &[DiffToken], expected: &[DiffToken]) -> f64 {
    let n = actual.len();
    let m = expected.len();
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if actual[i].key == expected[j].key {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = js_array_alloc((n + m) as u32);
    let mut i = 0usize;
    let mut j = 0usize;
    while i < n && j < m {
        if actual[i].key == expected[j].key {
            out = js_array_push_f64(out, tuple_value(0, actual[i].value));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out = js_array_push_f64(out, tuple_value(1, actual[i].value));
            i += 1;
        } else {
            out = js_array_push_f64(out, tuple_value(-1, expected[j].value));
            j += 1;
        }
    }
    while i < n {
        out = js_array_push_f64(out, tuple_value(1, actual[i].value));
        i += 1;
    }
    while j < m {
        out = js_array_push_f64(out, tuple_value(-1, expected[j].value));
        j += 1;
    }

    crate::value::js_nanbox_pointer(out as i64)
}

#[no_mangle]
pub extern "C" fn js_util_diff(actual: f64, expected: f64) -> f64 {
    let actual_input = collect_input(actual, "actual");
    let expected_input = collect_input(expected, "expected");
    if actual_input.source.is_some()
        && expected_input.source.is_some()
        && actual_input.source == expected_input.source
    {
        return crate::value::js_nanbox_pointer(js_array_alloc(0) as i64);
    }
    diff_to_array(&actual_input.tokens, &expected_input.tokens)
}
