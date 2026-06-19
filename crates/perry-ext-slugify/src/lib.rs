//! Native bindings for the npm `slugify` package.
//!
//! Functionally identical to `crates/perry-stdlib/src/slugify.rs`.
//! Depends only on [`perry_ffi`] — fourth wrapper port under
//! #466 Phase 5.

use perry_ffi::{alloc_string, read_string, JsString, StringHeader};

/// Mirror of the perry-stdlib accent-folding table. Kept in-line
/// rather than pulling a transliteration crate so the resulting
/// `.a` is the same ~30 KB size as the stdlib copy — predictable
/// for users measuring binary growth across the well-known flip.
fn replace_accents(c: char) -> Option<char> {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => {
            Some('a')
        }
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => Some('e'),
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => Some('i'),
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ø' => {
            Some('o')
        }
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => Some('u'),
        'ý' | 'ÿ' | 'Ý' | 'Ÿ' => Some('y'),
        'ñ' | 'Ñ' => Some('n'),
        'ç' | 'Ç' => Some('c'),
        'ß' => Some('s'),
        'æ' | 'Æ' => Some('a'),
        'œ' | 'Œ' => Some('o'),
        'ð' | 'Ð' => Some('d'),
        'þ' | 'Þ' => Some('t'),
        _ => None,
    }
}

/// `slugify(string)` — default URL-friendly slug with `-` separator.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_slugify(input_ptr: *const StringHeader) -> *mut StringHeader {
    js_slugify_with_options(input_ptr, std::ptr::null(), std::ptr::null())
}

/// `slugify(string, { replacement, lower })` — slug with a caller-
/// supplied replacement character. `_options_ptr` is reserved for
/// future option passing without a signature change.
///
/// # Safety
///
/// All three pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_slugify_with_options(
    input_ptr: *const StringHeader,
    replacement_ptr: *const StringHeader,
    _options_ptr: *const StringHeader,
) -> *mut StringHeader {
    let input_handle = JsString::from_raw(input_ptr as *mut StringHeader);
    let Some(input) = read_string(input_handle) else {
        return std::ptr::null_mut();
    };

    let replacement_handle = JsString::from_raw(replacement_ptr as *mut StringHeader);
    let replacement_char = read_string(replacement_handle)
        .and_then(|s| s.chars().next())
        .unwrap_or('-');

    alloc_string(&slugify_to_string(input, replacement_char, false)).as_raw()
}

/// `slugify(string, { strict: true })` — only alphanumeric output;
/// non-alphanumeric clusters collapse to a single `-`.
///
/// # Safety
///
/// `input_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_slugify_strict(input_ptr: *const StringHeader) -> *mut StringHeader {
    let handle = JsString::from_raw(input_ptr as *mut StringHeader);
    let Some(input) = read_string(handle) else {
        return std::ptr::null_mut();
    };
    alloc_string(&slugify_to_string(input, '-', true)).as_raw()
}

fn slugify_to_string(input: &str, replacement: char, strict: bool) -> String {
    let mut result = String::with_capacity(input.len());
    let mut last_was_separator = true; // Start true to trim leading separators

    for c in input.chars() {
        let c = replace_accents(c).unwrap_or(c);

        if c.is_ascii_alphanumeric() {
            result.push(c.to_ascii_lowercase());
            last_was_separator = false;
        } else if strict {
            // Non-alphanumeric → single replacement, regardless of
            // what character it actually was.
            if !last_was_separator {
                result.push(replacement);
                last_was_separator = true;
            }
        } else if (c.is_whitespace() || c == '_' || c == '-' || c == '/' || c == '\\')
            && !last_was_separator
        {
            result.push(replacement);
            last_was_separator = true;
        }
        // Otherwise the char is stripped silently.
    }

    if result.ends_with(replacement) {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_handle(handle: *mut StringHeader) -> String {
        read_string(unsafe { JsString::from_raw(handle) })
            .map(String::from)
            .unwrap_or_default()
    }

    #[test]
    fn lowercases_and_dashifies() {
        let input = alloc_string("Hello World!");
        let s = read_handle(unsafe { js_slugify(input.as_raw() as *const _) });
        assert_eq!(s, "hello-world");
    }

    #[test]
    fn folds_accents() {
        let input = alloc_string("Café au lait");
        let s = read_handle(unsafe { js_slugify(input.as_raw() as *const _) });
        assert_eq!(s, "cafe-au-lait");
    }

    #[test]
    fn strict_mode_drops_punctuation_to_single_dash() {
        let input = alloc_string("hello!! ___ world");
        let s = read_handle(unsafe { js_slugify_strict(input.as_raw() as *const _) });
        assert_eq!(s, "hello-world");
    }

    #[test]
    fn custom_replacement_char_is_first_char_of_string() {
        let input = alloc_string("hello world foo");
        let replacement = alloc_string("_");
        let s = read_handle(unsafe {
            js_slugify_with_options(
                input.as_raw() as *const _,
                replacement.as_raw() as *const _,
                std::ptr::null(),
            )
        });
        assert_eq!(s, "hello_world_foo");
    }
}
