//! Validator module (validator compatible)
//!
//! Native implementation of the 'validator' npm package.
//! Provides string validation functions.

use perry_runtime::StringHeader;

use crate::common::string_from_header;

// These synchronous predicates perform no Perry allocation or callbacks, so
// the original string can remain borrowed for the complete operation.
unsafe fn validate_borrowed(input: *const StringHeader, check: impl FnOnce(&str) -> bool) -> f64 {
    if crate::common::map_string_header_bytes(input, |bytes| {
        std::str::from_utf8(bytes).is_ok_and(check)
    })
    .unwrap_or(false)
    {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is a valid email address
/// validator.isEmail(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_email(input_ptr: *const StringHeader) -> f64 {
    validate_borrowed(input_ptr, perry_validation::is_email)
}

/// Check if a string is a valid URL
/// validator.isURL(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_url(input_ptr: *const StringHeader) -> f64 {
    validate_borrowed(input_ptr, perry_validation::is_url)
}

/// Check if a string is a valid UUID
/// validator.isUUID(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_uuid(input_ptr: *const StringHeader) -> f64 {
    validate_borrowed(input_ptr, perry_validation::is_uuid)
}

/// Check if a string contains only alphabetic characters
/// validator.isAlpha(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_alpha(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.is_empty() {
        return 0.0;
    }

    if input.chars().all(|c| c.is_alphabetic()) {
        1.0
    } else {
        0.0
    }
}

/// Check if a string contains only alphanumeric characters
/// validator.isAlphanumeric(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_alphanumeric(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.is_empty() {
        return 0.0;
    }

    if input.chars().all(|c| c.is_alphanumeric()) {
        1.0
    } else {
        0.0
    }
}

/// Check if a string contains only numeric characters
/// validator.isNumeric(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_numeric(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.is_empty() {
        return 0.0;
    }

    // Allow optional leading minus sign
    let to_check = if input.starts_with('-') || input.starts_with('+') {
        &input[1..]
    } else {
        &input[..]
    };

    if to_check.is_empty() {
        return 0.0;
    }

    if to_check.chars().all(|c| c.is_ascii_digit()) {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is a valid integer
/// validator.isInt(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_int(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.parse::<i64>().is_ok() {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is a valid float
/// validator.isFloat(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_float(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.parse::<f64>().is_ok() {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is a valid hexadecimal
/// validator.isHexadecimal(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_hexadecimal(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.is_empty() {
        return 0.0;
    }

    // Remove optional 0x prefix
    let to_check = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(&input);

    if to_check.is_empty() {
        return 0.0;
    }

    if to_check.chars().all(|c| c.is_ascii_hexdigit()) {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is empty (after trimming whitespace)
/// validator.isEmpty(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_empty(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 1.0, // null/undefined is considered empty
    };

    if input.trim().is_empty() {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is valid JSON
/// validator.isJSON(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_json(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if serde_json::from_str::<serde_json::Value>(&input).is_ok() {
        1.0
    } else {
        0.0
    }
}

/// Check if a string has a minimum length
/// validator.isLength(str, { min }) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_length_min(
    input_ptr: *const StringHeader,
    min: f64,
) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.len() >= min as usize {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is within a length range
/// validator.isLength(str, { min, max }) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_length(
    input_ptr: *const StringHeader,
    min: f64,
    max: f64,
) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    let len = input.len();
    if len >= min as usize && len <= max as usize {
        1.0
    } else {
        0.0
    }
}

/// Check if a string contains a substring
/// validator.contains(str, seed) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_contains(
    input_ptr: *const StringHeader,
    seed_ptr: *const StringHeader,
) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    let seed = match string_from_header(seed_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input.contains(&seed) {
        1.0
    } else {
        0.0
    }
}

/// Check if strings are equal
/// validator.equals(str, comparison) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_equals(
    input_ptr: *const StringHeader,
    comparison_ptr: *const StringHeader,
) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    let comparison = match string_from_header(comparison_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input == comparison {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is lowercase
/// validator.isLowercase(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_lowercase(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_lowercase())
    {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is uppercase
/// validator.isUppercase(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_uppercase(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if input
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase())
    {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_runtime::gc::RuntimeHandleScope;

    #[test]
    fn validator_borrows_original_heap_payload_and_preserves_bad_input_results() {
        let scope = RuntimeHandleScope::new();
        let bytes = b"550e8400-e29b-41d4-a716-446655440000";
        let input = scope.root_string_ptr(perry_runtime::string::js_string_from_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        ));
        let ptr = input.get_raw_const_ptr::<StringHeader>();
        let original = unsafe { (ptr as *const u8).add(std::mem::size_of::<StringHeader>()) };
        assert_eq!(
            unsafe {
                validate_borrowed(ptr, |s| {
                    assert_eq!(
                        s.as_ptr(),
                        original,
                        "validation must not copy the heap subject"
                    );
                    s.as_bytes() == bytes
                })
            },
            1.0
        );
        assert_eq!(unsafe { js_validator_is_uuid(ptr) }, 1.0);
        let invalid = scope.root_string_ptr(perry_runtime::string::js_string_from_bytes(
            b"a\x80b".as_ptr(),
            3,
        ));
        for check in [
            js_validator_is_email,
            js_validator_is_url,
            js_validator_is_uuid,
        ] {
            for ptr in [
                std::ptr::null(),
                1usize as *const StringHeader,
                0x40000usize as *const StringHeader,
                invalid.get_raw_const_ptr(),
            ] {
                assert_eq!(unsafe { check(ptr) }, 0.0);
            }
        }
    }

    #[test]
    fn validator_bindings_use_shared_email_url_and_uuid_rules() {
        let scope = RuntimeHandleScope::new();
        for (text, expected) in [
            ("a@bücher.de", [1.0, 0.0, 0.0]),
            ("a@prefix[127.0.0.1]", [1.0, 0.0, 0.0]),
            ("a@b.com\n", [0.0, 0.0, 0.0]),
            ("https://example.com", [0.0, 1.0, 0.0]),
            ("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF", [0.0, 0.0, 1.0]),
        ] {
            let input = scope.root_string_ptr(perry_runtime::string::js_string_from_bytes(
                text.as_ptr(),
                text.len() as u32,
            ));
            for (check, expected) in [
                js_validator_is_email,
                js_validator_is_url,
                js_validator_is_uuid,
            ]
            .into_iter()
            .zip(expected)
            {
                assert_eq!(
                    unsafe { check(input.get_raw_const_ptr()) },
                    expected,
                    "{text:?}"
                );
            }
        }
    }
}
