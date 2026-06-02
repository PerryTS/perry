//! Native bindings for the npm `validator` package.
//!
//! Sync, string-only — fits the perry-ffi v0.5 surface exactly.
//! Functionally identical to `crates/perry-stdlib/src/validator.rs`.
//! Eighth wrapper port under #466 Phase 5.
//!
//! Booleans cross the FFI as `f64` (`1.0` / `0.0`) per Perry's
//! existing convention for sync FFI booleans — same as the
//! perry-stdlib copy. No new perry-ffi surface needed.

use perry_ffi::{read_string, JsString, StringHeader};

unsafe fn read_str(ptr: *const StringHeader) -> Option<&'static str> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle)
}

unsafe fn read_string_owned(ptr: *const StringHeader) -> Option<String> {
    read_str(ptr).map(String::from)
}

#[inline]
fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

/// `validator.isEmail(str)`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_email(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(validator::ValidateEmail::validate_email(&input))
}

/// `validator.isURL(str)`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_url(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(validator::ValidateUrl::validate_url(&input))
}

/// `validator.isUUID(str)`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_uuid(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    // Cache the regex so repeated validate calls don't recompile it.
    use std::sync::OnceLock;
    static UUID_RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = UUID_RE.get_or_init(|| {
        regex::Regex::new(
            r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
        )
        .expect("static regex")
    });
    b(re.is_match(input))
}

/// `validator.isAlpha(str)`. Empty string is `false`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_alpha(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    if input.is_empty() {
        return 0.0;
    }
    b(input.chars().all(|c| c.is_alphabetic()))
}

/// `validator.isAlphanumeric(str)`. Empty string is `false`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_alphanumeric(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    if input.is_empty() {
        return 0.0;
    }
    b(input.chars().all(|c| c.is_alphanumeric()))
}

/// `validator.isNumeric(str)`. Allows a leading `+` / `-`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_numeric(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_string_owned(input_ptr) else {
        return 0.0;
    };
    if input.is_empty() {
        return 0.0;
    }
    let to_check = if input.starts_with('-') || input.starts_with('+') {
        &input[1..]
    } else {
        &input[..]
    };
    if to_check.is_empty() {
        return 0.0;
    }
    b(to_check.chars().all(|c| c.is_ascii_digit()))
}

/// `validator.isInt(str)`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_int(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(input.parse::<i64>().is_ok())
}

/// `validator.isFloat(str)`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_float(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(input.parse::<f64>().is_ok())
}

/// `validator.isHexadecimal(str)`. Strips an optional `0x`/`0X`
/// prefix before checking.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_hexadecimal(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    if input.is_empty() {
        return 0.0;
    }
    let to_check = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    if to_check.is_empty() {
        return 0.0;
    }
    b(to_check.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `validator.isEmpty(str)`. Returns `true` for null/undefined.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_empty(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 1.0;
    };
    b(input.trim().is_empty())
}

/// `validator.isJSON(str)`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_json(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(serde_json::from_str::<serde_json::Value>(input).is_ok())
}

/// `validator.isLength(str, { min })`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_length_min(
    input_ptr: *const StringHeader,
    min: f64,
) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(input.len() >= min as usize)
}

/// `validator.isLength(str, { min, max })`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_length(
    input_ptr: *const StringHeader,
    min: f64,
    max: f64,
) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    let len = input.len();
    b(len >= min as usize && len <= max as usize)
}

/// `validator.contains(str, seed)`.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_validator_contains(
    input_ptr: *const StringHeader,
    seed_ptr: *const StringHeader,
) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    let Some(seed) = read_str(seed_ptr) else {
        return 0.0;
    };
    b(input.contains(seed))
}

/// `validator.equals(str, comparison)`.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_validator_equals(
    input_ptr: *const StringHeader,
    comparison_ptr: *const StringHeader,
) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    let Some(comparison) = read_str(comparison_ptr) else {
        return 0.0;
    };
    b(input == comparison)
}

/// `validator.isLowercase(str)`. Letters must all be lowercase;
/// non-letter characters are ignored. Empty is `true`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_lowercase(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(input
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_lowercase()))
}

/// `validator.isUppercase(str)`. Letters must all be uppercase;
/// non-letter characters are ignored.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_validator_is_uppercase(input_ptr: *const StringHeader) -> f64 {
    let Some(input) = read_str(input_ptr) else {
        return 0.0;
    };
    b(input
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::alloc_string;

    fn p(s: &str) -> *const StringHeader {
        alloc_string(s).as_raw() as *const _
    }

    #[test]
    fn email_validation() {
        unsafe {
            assert_eq!(js_validator_is_email(p("foo@bar.com")), 1.0);
            assert_eq!(js_validator_is_email(p("not-an-email")), 0.0);
            assert_eq!(js_validator_is_email(std::ptr::null()), 0.0);
        }
    }

    #[test]
    fn uuid_validation() {
        unsafe {
            assert_eq!(
                js_validator_is_uuid(p("550e8400-e29b-41d4-a716-446655440000")),
                1.0
            );
            assert_eq!(js_validator_is_uuid(p("not-a-uuid")), 0.0);
        }
    }

    #[test]
    fn json_validation() {
        unsafe {
            assert_eq!(js_validator_is_json(p(r#"{"a":1}"#)), 1.0);
            assert_eq!(js_validator_is_json(p("[1,2,3]")), 1.0);
            assert_eq!(js_validator_is_json(p("not json")), 0.0);
        }
    }

    #[test]
    fn length_bounds() {
        unsafe {
            assert_eq!(js_validator_is_length(p("hello"), 3.0, 10.0), 1.0);
            assert_eq!(js_validator_is_length(p("hi"), 3.0, 10.0), 0.0);
            assert_eq!(
                js_validator_is_length(p("toolongtoolongtoolong"), 3.0, 10.0),
                0.0
            );
        }
    }

    #[test]
    fn contains_check() {
        unsafe {
            assert_eq!(js_validator_contains(p("hello world"), p("world")), 1.0);
            assert_eq!(js_validator_contains(p("hello world"), p("xyz")), 0.0);
        }
    }
}
