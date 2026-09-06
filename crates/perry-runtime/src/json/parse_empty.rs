//! An empty ordinary object needs only its final allocation. Validate the
//! complete input before entering the allocator, so no source borrow or
//! intermediate managed value needs to survive collection.

use crate::JSValue;

#[inline(always)]
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

#[inline(always)]
pub(super) fn is_empty_object(bytes: &[u8]) -> bool {
    if bytes == b"{}" {
        return true;
    }
    // Reject nonempty containers before a whitespace walk or an outlined call.
    // The padded form can start with whitespace, or with '{' followed by
    // whitespace / '}'. Every other input stays on the general parser.
    if bytes.len() < 3
        || !(whitespace(bytes[0])
            || (bytes[0] == b'{' && (whitespace(bytes[1]) || bytes[1] == b'}')))
    {
        return false;
    }
    padded_empty_object(bytes)
}

#[inline(never)]
fn padded_empty_object(mut bytes: &[u8]) -> bool {
    while bytes.first().copied().is_some_and(whitespace) {
        bytes = &bytes[1..];
    }
    let Some(mut bytes) = bytes.strip_prefix(b"{") else {
        return false;
    };
    while bytes.first().copied().is_some_and(whitespace) {
        bytes = &bytes[1..];
    }
    let Some(bytes) = bytes.strip_prefix(b"}") else {
        return false;
    };
    bytes.iter().copied().all(whitespace)
}

/// Call only after the entire input has been validated and is no longer used.
/// The ordinary allocator services its own trigger before publishing the new
/// object. Cache cleanup and pressure scheduling below cannot collect.
#[inline(never)]
pub(super) unsafe fn allocate_empty_object() -> JSValue {
    crate::gc::gc_collect_pending_suppressed_parse();
    let object = crate::object::js_object_alloc(0, 0);
    crate::object::mark_object_plain_ordinary(object);
    super::parse_scalar::clear_oversized_key_cache();
    crate::gc::gc_schedule_parse_boundary_collection_if_pressure();
    JSValue::object_ptr(object as *mut u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_empty_object_recognizer_consumes_only_complete_json_grammar() {
        for prefix in ["", " ", "\t", "\r", "\n", " \t\r\n"] {
            for inside in ["", " ", "\t", "\r", "\n", " \t\r\n"] {
                for suffix in ["", " ", "\t", "\r", "\n", " \t\r\n"] {
                    let text = format!("{prefix}{{{inside}}}{suffix}");
                    assert!(is_empty_object(text.as_bytes()), "{text:?}");
                }
            }
        }
        for byte in 0..=255u8 {
            for input in [[byte, b'{', b'}'], [b'{', byte, b'}'], [b'{', b'}', byte]] {
                assert_eq!(is_empty_object(&input), whitespace(byte), "{input:?}");
            }
        }
        for text in [
            "",
            "{",
            "}",
            "{ }x",
            "{}{}",
            "{{}}",
            "{,}",
            "[]",
            "null",
            "{\"a\":1}",
            "{\u{a0}}",
            "{}\u{feff}",
        ] {
            assert!(!is_empty_object(text.as_bytes()), "{text:?}");
        }
        let padded = format!(
            "{}{{{}}}{}",
            " ".repeat(100_000),
            "\t".repeat(100_000),
            "\n".repeat(100_000)
        );
        assert!(is_empty_object(padded.as_bytes()));
        assert!(!is_empty_object(format!("{padded}x").as_bytes()));
    }
}
