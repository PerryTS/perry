//! Validator module (validator compatible)
//!
//! Native implementation of the 'validator' npm package.
//! Provides string validation functions.

use perry_runtime::StringHeader;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Check if a string is a valid email address
/// validator.isEmail(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_email(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if validator::ValidateEmail::validate_email(&input) {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is a valid URL
/// validator.isURL(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_url(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if validator::ValidateUrl::validate_url(&input) {
        1.0
    } else {
        0.0
    }
}

/// Check if a string is a valid UUID
/// validator.isUUID(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_uuid(input_ptr: *const StringHeader) -> f64 {
    let input = match string_from_header(input_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    // Simple UUID regex pattern
    let uuid_pattern = regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .unwrap();

    if uuid_pattern.is_match(&input) {
        1.0
    } else {
        0.0
    }
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
