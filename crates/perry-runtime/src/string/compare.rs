//! Equality / comparison / starts-with / ends-with / well-formedness /
//! normalization / locale-compare.

use super::*;

/// Compare two strings lexicographically.
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
#[no_mangle]
pub extern "C" fn js_string_compare(a: *const StringHeader, b: *const StringHeader) -> i32 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0;
    }
    if !a_valid {
        return -1;
    }
    if !b_valid {
        return 1;
    }

    unsafe {
        let len_a = (*a).byte_len as usize;
        let len_b = (*b).byte_len as usize;
        let data_a = string_data(a);
        let data_b = string_data(b);
        let a_bytes = std::slice::from_raw_parts(data_a, len_a);
        let b_bytes = std::slice::from_raw_parts(data_b, len_b);
        match a_bytes.cmp(b_bytes) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Compare two strings for equality
#[no_mangle]
pub extern "C" fn js_string_equals(a: *const StringHeader, b: *const StringHeader) -> i32 {
    // Pointer identity fast path
    if std::ptr::eq(a, b) {
        return 1;
    }

    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 1;
    }
    if !a_valid || !b_valid {
        return 0;
    }

    let blen_a = unsafe { (*a).byte_len };
    let blen_b = unsafe { (*b).byte_len };

    if blen_a != blen_b {
        return 0;
    }

    unsafe {
        let data_a = string_data(a);
        let data_b = string_data(b);
        let slice_a = std::slice::from_raw_parts(data_a, blen_a as usize);
        let slice_b = std::slice::from_raw_parts(data_b, blen_b as usize);
        if slice_a == slice_b {
            1
        } else {
            0
        }
    }
}

/// SSO-aware key match: compare a stored-key `JSValue` (which may be a
/// `STRING_TAG` heap pointer OR a `SHORT_STRING_TAG` inline SSO value)
/// against an incoming heap `*const StringHeader` key.
///
/// This is the safe replacement for the `key_val.is_string() && js_string_equals(key, key_val.as_string_ptr())`
/// pattern that recurs in `object/field_get_set.rs`, `object/object_ops.rs`,
/// `object/delete_rest.rs`, etc. — `is_string()` is STRING_TAG-only, so
/// any SSO-stored key is silently skipped, which makes `Object.keys`,
/// `key in obj`, `delete obj[k]`, `obj[k] = v`, and `Object.assign`
/// drop or duplicate keys whose name is ≤ 5 ASCII bytes (#1781).
///
/// Returns `true` iff the stored value is some kind of string AND its
/// byte contents are equal to the incoming heap key. Returns `false`
/// for non-string stored values or a null incoming key.
///
/// Inline byte comparison — no allocation, no heap materialization of
/// the SSO operand. Safe on the hot path.
#[inline]
pub(crate) unsafe fn js_string_key_matches(
    stored: crate::JSValue,
    incoming: *const StringHeader,
) -> bool {
    if incoming.is_null() {
        return false;
    }
    // Heap-stored key: defer to the existing equals routine.
    if stored.is_string() {
        return js_string_equals(incoming, stored.as_string_ptr()) != 0;
    }
    // SSO-stored key: compare the incoming heap bytes against the
    // inline SSO bytes without materializing the SSO to the heap.
    if stored.is_short_string() {
        let incoming_len = (*incoming).byte_len as usize;
        let sso_len = stored.short_string_len();
        if incoming_len != sso_len {
            return false;
        }
        let incoming_data = (incoming as *const u8).add(std::mem::size_of::<StringHeader>());
        let incoming_bytes = std::slice::from_raw_parts(incoming_data, incoming_len);
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = stored.short_string_to_buf(&mut sso_buf);
        return &sso_buf[..n] == incoming_bytes;
    }
    false
}

/// SSO-aware byte-slice match for cases where the incoming key already
/// lives as a `&[u8]` slice (typed-feedback guards, `js_object_get_own_field_or_undef`,
/// etc.) — same SSO blind-spot fix as [`js_string_key_matches`] but
/// without the round-trip through a heap `StringHeader` for the
/// incoming side. Returns `true` iff the stored value is some kind of
/// string and its bytes equal `incoming_bytes`.
#[inline]
pub(crate) unsafe fn js_string_key_matches_bytes(
    stored: crate::JSValue,
    incoming_bytes: &[u8],
) -> bool {
    if stored.is_string() {
        let stored_ptr = stored.as_string_ptr();
        if stored_ptr.is_null() {
            return false;
        }
        let stored_len = (*stored_ptr).byte_len as usize;
        if stored_len != incoming_bytes.len() {
            return false;
        }
        let stored_data = (stored_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let stored_slice = std::slice::from_raw_parts(stored_data, stored_len);
        return stored_slice == incoming_bytes;
    }
    if stored.is_short_string() {
        let sso_len = stored.short_string_len();
        if sso_len != incoming_bytes.len() {
            return false;
        }
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = stored.short_string_to_buf(&mut sso_buf);
        return &sso_buf[..n] == incoming_bytes;
    }
    false
}

/// Extract the bytes of a stored-key JSValue (STRING_TAG or SHORT_STRING_TAG)
/// into a caller-provided buffer + length. Returns `None` for non-string
/// stored values. The slice borrowing into either the SSO buffer
/// (`stored_buf`) or the heap pointer is the caller's responsibility.
///
/// Used by paths like `Object.keys` and `Object.assign` that need to
/// materialize the key string into a usable form regardless of which
/// representation it currently has.
#[inline]
pub(crate) unsafe fn js_string_key_bytes(
    stored: crate::JSValue,
    stored_buf: &mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<&[u8]> {
    if stored.is_string() {
        let stored_ptr = stored.as_string_ptr();
        if stored_ptr.is_null() {
            return None;
        }
        let len = (*stored_ptr).byte_len as usize;
        let data = (stored_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return Some(std::slice::from_raw_parts(data, len));
    }
    if stored.is_short_string() {
        let n = stored.short_string_to_buf(stored_buf);
        return Some(&stored_buf[..n]);
    }
    None
}

/// Validate and coerce the search string for String.prototype.includes,
/// startsWith, and endsWith.
///
/// The ECMAScript path is IsRegExp(searchString) before ToString(searchString):
/// a real RegExp or an object with truthy Symbol.match must throw, while
/// Symbol.match === false/null explicitly opts out and then stringifies.
#[no_mangle]
pub extern "C" fn js_string_search_value_to_string(
    value: f64,
    method_id: i32,
) -> *mut StringHeader {
    if string_search_is_regexp(value) {
        throw_regexp_search_type_error(method_id);
    }
    // ToString(searchString): a Symbol throws a TypeError (§7.1.17) rather than
    // stringifying to "Symbol(...)".
    crate::builtins::reject_symbol_to_string(value);
    crate::value::js_jsvalue_to_string(value)
}

fn string_search_is_regexp(value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return false;
    }

    let raw_ptr = jsval.as_pointer::<u8>() as usize;
    if raw_ptr < 0x10000 || crate::symbol::is_registered_symbol(raw_ptr) {
        return false;
    }

    let match_sym = crate::symbol::well_known_symbol("match");
    if !match_sym.is_null() {
        let match_sym_f64 =
            f64::from_bits(crate::value::JSValue::pointer(match_sym as *const u8).bits());
        let matcher = unsafe { crate::symbol::js_object_get_symbol_property(value, match_sym_f64) };
        if matcher.to_bits() != crate::value::TAG_UNDEFINED {
            return crate::value::js_is_truthy(matcher) != 0;
        }
    }

    crate::regex::is_regex_pointer(jsval.as_pointer::<u8>())
}

fn throw_regexp_search_type_error(method_id: i32) -> ! {
    let method = match method_id {
        1 => "startsWith",
        2 => "endsWith",
        _ => "includes",
    };
    let message =
        format!("First argument to String.prototype.{method} must not be a regular expression");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Check if a string starts with a prefix
#[no_mangle]
pub extern "C" fn js_string_starts_with(
    s: *const StringHeader,
    prefix: *const StringHeader,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let blen = unsafe { (*s).byte_len };
    let prefix_blen = unsafe { (*prefix).byte_len };

    if prefix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let prefix_data = string_data(prefix);

        for i in 0..prefix_blen as usize {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with a suffix
#[no_mangle]
pub extern "C" fn js_string_ends_with(s: *const StringHeader, suffix: *const StringHeader) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let blen = unsafe { (*s).byte_len };
    let suffix_blen = unsafe { (*suffix).byte_len };

    if suffix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s);
        let suffix_data = string_data(suffix);
        let start = blen - suffix_blen;

        for i in 0..suffix_blen as usize {
            if *data.add(start as usize + i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string starts with `prefix` at UTF-16 code-unit `position`.
/// Mirrors `String.prototype.startsWith(searchString, position)` — clamps
/// negative positions to 0 and positions past the end to length.
#[no_mangle]
pub extern "C" fn js_string_starts_with_at(
    s: *const StringHeader,
    prefix: *const StringHeader,
    position: i32,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(prefix) {
        return 0;
    }

    let u16len = unsafe { (*s).utf16_len } as i32;
    let pos = position.max(0).min(u16len) as usize;

    let prefix_blen = unsafe { (*prefix).byte_len } as usize;

    let byte_start = if is_ascii_string(s) {
        pos
    } else {
        utf16_offset_to_byte_offset(string_as_str(s), pos)
    };

    let blen = unsafe { (*s).byte_len } as usize;
    if byte_start + prefix_blen > blen {
        return 0;
    }

    unsafe {
        let data = string_data(s).add(byte_start);
        let prefix_data = string_data(prefix);
        for i in 0..prefix_blen {
            if *data.add(i) != *prefix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// Check if a string ends with `suffix` if truncated to UTF-16 code-unit
/// `end_position`. Mirrors `String.prototype.endsWith(searchString, endPosition)`
/// — clamps negative positions to 0 and positions past the end to length.
#[no_mangle]
pub extern "C" fn js_string_ends_with_at(
    s: *const StringHeader,
    suffix: *const StringHeader,
    end_position: i32,
) -> i32 {
    if !is_valid_string_ptr(s) || !is_valid_string_ptr(suffix) {
        return 0;
    }

    let u16len = unsafe { (*s).utf16_len } as i32;
    let end_u16 = end_position.max(0).min(u16len) as usize;

    let byte_end = if is_ascii_string(s) {
        end_u16
    } else {
        utf16_offset_to_byte_offset(string_as_str(s), end_u16)
    };

    let suffix_blen = unsafe { (*suffix).byte_len } as usize;
    if suffix_blen > byte_end {
        return 0;
    }

    let byte_start = byte_end - suffix_blen;

    unsafe {
        let data = string_data(s).add(byte_start);
        let suffix_data = string_data(suffix);
        for i in 0..suffix_blen {
            if *data.add(i) != *suffix_data.add(i) {
                return 0;
            }
        }
    }

    1
}

/// String.prototype.normalize(form) — Unicode normalization.
///
/// `form_value` is the raw NaN-boxed argument (or NaN-boxed `undefined`
/// when the call site omitted it). Per ECMA-262 §22.1.3.13: when `form` is
/// `undefined` the form defaults to `"NFC"`; otherwise the form is coerced
/// with `ToString` and must be exactly one of `"NFC"`, `"NFD"`, `"NFKC"`,
/// `"NFKD"` — anything else (including explicit `null` → `"null"`, the empty
/// string, or `"BAD"`) throws a `RangeError`. (#2782)
#[no_mangle]
pub extern "C" fn js_string_normalize(
    s: *const StringHeader,
    form_value: f64,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let str_data = string_as_str(s);

    // `undefined` (omitted argument) → default NFC. Note: explicit `null`
    // is NOT undefined — it stringifies to "null" and falls through to the
    // invalid-form error path below.
    let form_jsval = crate::value::JSValue::from_bits(form_value.to_bits());
    let form_owned: String = if form_jsval.is_undefined() {
        "NFC".to_string()
    } else {
        // ToString(form) runs before the form-validity check, so a Symbol form
        // throws a TypeError (§7.1.17) — not the RangeError of an invalid form.
        crate::builtins::reject_symbol_to_string(form_value);
        let form_ptr = crate::value::js_jsvalue_to_string(form_value);
        if is_valid_string_ptr(form_ptr) {
            string_as_str(form_ptr).to_string()
        } else {
            String::new()
        }
    };

    #[cfg(feature = "string-normalize")]
    let normalized: String = {
        use unicode_normalization::UnicodeNormalization;
        match form_owned.as_str() {
            "NFC" => str_data.nfc().collect(),
            "NFD" => str_data.nfd().collect(),
            "NFKC" => str_data.nfkc().collect(),
            "NFKD" => str_data.nfkd().collect(),
            _ => throw_invalid_normalize_form(),
        }
    };
    // Normalize engine gated off: still validate the form (so a bad form throws
    // the spec RangeError), but pass the string through unchanged for the four
    // valid forms (no Unicode decomposition tables linked).
    #[cfg(not(feature = "string-normalize"))]
    let normalized: String = match form_owned.as_str() {
        "NFC" | "NFD" | "NFKC" | "NFKD" => str_data.to_string(),
        _ => throw_invalid_normalize_form(),
    };
    let bytes = normalized.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn throw_invalid_normalize_form() -> ! {
    let message = "The normalization form should be one of NFC, NFD, NFKC, NFKD.";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// String.prototype.localeCompare(other) — returns negative/zero/positive number.
/// We don't ship a true ICU collator. We approximate the Unicode default
/// collation with a two-pass comparison: first case-insensitive (so the
/// character class wins) and then case-sensitive with lowercase < uppercase
/// (matching V8's default ICU behavior where 'a' < 'A').
#[no_mangle]
pub extern "C" fn js_string_locale_compare(a: *const StringHeader, b: *const StringHeader) -> f64 {
    let a_valid = is_valid_string_ptr(a);
    let b_valid = is_valid_string_ptr(b);
    if !a_valid && !b_valid {
        return 0.0;
    }
    if !a_valid {
        return -1.0;
    }
    if !b_valid {
        return 1.0;
    }
    let a_str = string_as_str(a);
    let b_str = string_as_str(b);
    // Case-insensitive primary comparison
    let a_lower = a_str.to_lowercase();
    let b_lower = b_str.to_lowercase();
    match a_lower.cmp(&b_lower) {
        std::cmp::Ordering::Less => return -1.0,
        std::cmp::Ordering::Greater => return 1.0,
        std::cmp::Ordering::Equal => {}
    }
    // Same letters ignoring case — order by case (lowercase < uppercase
    // per the default Unicode collation tertiary weight).
    let mut ai = a_str.chars();
    let mut bi = b_str.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return 0.0,
            (None, Some(_)) => return -1.0,
            (Some(_), None) => return 1.0,
            (Some(ca), Some(cb)) => {
                if ca == cb {
                    continue;
                }
                let a_lower = ca.is_lowercase();
                let b_lower = cb.is_lowercase();
                if a_lower && !b_lower {
                    return -1.0;
                }
                if !a_lower && b_lower {
                    return 1.0;
                }
                return if (ca as u32) < (cb as u32) { -1.0 } else { 1.0 };
            }
        }
    }
}

/// Natural-order collation for `localeCompare(other, locales, { numeric: true })`:
/// maximal runs of ASCII digits compare by numeric value (leading zeros
/// ignored, then by digit-count and lexicographically), and non-digit runs
/// compare with the same case-insensitive primary / case tertiary rule as
/// `js_string_locale_compare`. So `"10" > "9"` and `"file10" > "file9"`.
fn locale_compare_numeric(a: &str, b: &str) -> f64 {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return 0.0,
            (None, Some(_)) => return -1.0,
            (Some(_), None) => return 1.0,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut da = String::new();
                while let Some(&c) = ai.peek() {
                    if c.is_ascii_digit() {
                        da.push(c);
                        ai.next();
                    } else {
                        break;
                    }
                }
                let mut db = String::new();
                while let Some(&c) = bi.peek() {
                    if c.is_ascii_digit() {
                        db.push(c);
                        bi.next();
                    } else {
                        break;
                    }
                }
                // Compare by numeric value: strip leading zeros, then longer
                // run wins, then lexicographically among equal lengths.
                let na = da.trim_start_matches('0');
                let nb = db.trim_start_matches('0');
                match na.len().cmp(&nb.len()).then_with(|| na.cmp(nb)) {
                    std::cmp::Ordering::Less => return -1.0,
                    std::cmp::Ordering::Greater => return 1.0,
                    std::cmp::Ordering::Equal => {} // equal numeric value — keep going
                }
            }
            (Some(ca), Some(cb)) => {
                ai.next();
                bi.next();
                if ca == cb {
                    continue;
                }
                let la = ca.to_lowercase().next().unwrap_or(ca);
                let lb = cb.to_lowercase().next().unwrap_or(cb);
                if la != lb {
                    return if la < lb { -1.0 } else { 1.0 };
                }
                // Same letter, different case: lowercase sorts before uppercase.
                let a_lower = ca.is_lowercase();
                let b_lower = cb.is_lowercase();
                if a_lower != b_lower {
                    return if a_lower { -1.0 } else { 1.0 };
                }
                return if (ca as u32) < (cb as u32) { -1.0 } else { 1.0 };
            }
        }
    }
}

/// `String.prototype.localeCompare(other, locales, options)` — honors the
/// `{ numeric: true }` collation option (natural sort); `locales` is ignored
/// (no Intl/ICU). `options` arrives as a NaN-boxed JSValue (the options object,
/// or undefined when absent). Reads `options.numeric` (ToBoolean) and routes to
/// `locale_compare_numeric` when set, else to the default `js_string_locale_compare`.
#[no_mangle]
pub extern "C" fn js_string_locale_compare_opts(
    a: *const StringHeader,
    b: *const StringHeader,
    options: f64,
) -> f64 {
    let numeric = unsafe {
        let ptr =
            crate::value::js_nanbox_get_pointer(options) as *const crate::object::ObjectHeader;
        if ptr.is_null() || (ptr as usize) < 0x10000 {
            false
        } else {
            let key = crate::string::js_string_from_bytes(b"numeric".as_ptr(), 7);
            let v = crate::object::js_object_get_field_by_name_f64(ptr, key);
            crate::value::js_is_truthy(v) != 0
        }
    };
    if !numeric {
        return js_string_locale_compare(a, b);
    }
    if !is_valid_string_ptr(a) || !is_valid_string_ptr(b) {
        // Match the validity edge-cases of the default path.
        return js_string_locale_compare(a, b);
    }
    locale_compare_numeric(string_as_str(a), string_as_str(b))
}

/// String.prototype.isWellFormed() — returns NaN-boxed boolean.
/// A string is well-formed if it contains no lone surrogates.
/// Lone-surrogate strings are marked with STRING_FLAG_HAS_LONE_SURROGATES at construction.
#[no_mangle]
pub extern "C" fn js_string_is_well_formed(s: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if !is_valid_string_ptr(s) {
        return f64::from_bits(TAG_TRUE);
    }
    let flags = unsafe { (*s).flags };
    if flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 {
        return f64::from_bits(TAG_FALSE);
    }
    f64::from_bits(TAG_TRUE)
}

/// String.prototype.toWellFormed() — replaces lone surrogates with U+FFFD (U+FFFD = EF BF BD).
/// Works directly on WTF-8 bytes: replaces each 3-byte surrogate sequence
/// (ED A0..BF 80..BF) with the 3-byte U+FFFD encoding.
#[no_mangle]
pub extern "C" fn js_string_to_well_formed(s: *const StringHeader) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(std::ptr::null(), 0);
    }
    let flags = unsafe { (*s).flags };
    let blen = unsafe { (*s).byte_len } as usize;
    let data = string_data(s);
    if flags & STRING_FLAG_HAS_LONE_SURROGATES == 0 {
        // Well-formed UTF-8: return a copy without scanning
        return js_string_from_bytes(data, blen as u32);
    }
    // Scan raw bytes and replace every WTF-8 lone-surrogate sequence with U+FFFD.
    // WTF-8 surrogate: first byte = 0xED, second = 0xA0..=0xBF, third = 0x80..=0xBF.
    let bytes = unsafe { slice::from_raw_parts(data, blen) };
    let mut result: Vec<u8> = Vec::with_capacity(blen);
    let mut i = 0;
    while i < blen {
        let b = bytes[i];
        if b == 0xED
            && i + 2 < blen
            && (0xA0..=0xBF).contains(&bytes[i + 1])
            && (0x80..=0xBF).contains(&bytes[i + 2])
        {
            // Lone surrogate → U+FFFD (EF BF BD)
            result.extend_from_slice(&[0xEF, 0xBF, 0xBD]);
            i += 3;
        } else if b < 0x80 {
            result.push(b);
            i += 1;
        } else if b < 0xC0 {
            result.push(b);
            i += 1;
        } else if b < 0xE0 {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            i += 2;
        } else if b < 0xF0 {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            if i + 2 < blen {
                result.push(bytes[i + 2]);
            }
            i += 3;
        } else {
            result.push(b);
            if i + 1 < blen {
                result.push(bytes[i + 1]);
            }
            if i + 2 < blen {
                result.push(bytes[i + 2]);
            }
            if i + 3 < blen {
                result.push(bytes[i + 3]);
            }
            i += 4;
        }
    }
    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

#[cfg(test)]
mod numeric_collation_tests {
    use super::locale_compare_numeric;

    #[test]
    fn natural_order_compares_digit_runs_numerically() {
        // Numeric runs compare by value, not lexicographically.
        assert_eq!(locale_compare_numeric("10", "9"), 1.0);
        assert_eq!(locale_compare_numeric("9", "10"), -1.0);
        assert_eq!(locale_compare_numeric("file10", "file9"), 1.0);
        assert_eq!(locale_compare_numeric("file2", "file10"), -1.0);
        // Leading zeros: equal numeric value → equal.
        assert_eq!(locale_compare_numeric("08", "8"), 0.0);
        assert_eq!(locale_compare_numeric("100", "99"), 1.0);
        // Mixed runs and pure alpha.
        assert_eq!(locale_compare_numeric("a10b", "a9b"), 1.0);
        assert_eq!(locale_compare_numeric("a", "b"), -1.0);
        assert_eq!(locale_compare_numeric("abc", "abc"), 0.0);
        // A digit run vs the end of the shorter string.
        assert_eq!(locale_compare_numeric("x", "x10"), -1.0);
        assert_eq!(locale_compare_numeric("2foo", "10foo"), -1.0);
    }
}

#[cfg(test)]
mod tests_sso_helpers {
    use super::*;
    use crate::value::SHORT_STRING_MAX_LEN;
    use crate::{js_string_from_bytes, JSValue};

    /// #1781: a STRING_TAG heap key and a SHORT_STRING_TAG inline key
    /// with the same bytes must both match an incoming heap key.
    #[test]
    fn key_matches_heap_and_sso_for_same_bytes() {
        for name in ["a", "id", "tag", "name", "mango"] {
            let bytes = name.as_bytes();
            assert!(bytes.len() <= SHORT_STRING_MAX_LEN);

            let incoming = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) };
            let heap_stored = JSValue::string_ptr(incoming);
            let sso_stored = JSValue::try_short_string(bytes).expect("len<=5 encodes as SSO");
            assert!(sso_stored.is_short_string(), "{name:?} should be SSO");

            unsafe {
                assert!(
                    js_string_key_matches(heap_stored, incoming),
                    "heap match failed for {name:?}"
                );
                assert!(
                    js_string_key_matches(sso_stored, incoming),
                    "SSO match failed for {name:?}"
                );
                assert!(
                    js_string_key_matches_bytes(heap_stored, bytes),
                    "heap bytes-match failed for {name:?}"
                );
                assert!(
                    js_string_key_matches_bytes(sso_stored, bytes),
                    "SSO bytes-match failed for {name:?}"
                );
            }
        }
    }

    /// Different-length stored vs incoming must return false even when one
    /// is SSO and the other is heap.
    #[test]
    fn key_matches_rejects_different_bytes_across_reps() {
        let incoming = unsafe { js_string_from_bytes(b"id".as_ptr(), 2) };
        let sso_other = JSValue::try_short_string(b"tag").expect("SSO");
        let heap_other_ptr = unsafe { js_string_from_bytes(b"other".as_ptr(), 5) };
        let heap_other = JSValue::string_ptr(heap_other_ptr);

        unsafe {
            assert!(!js_string_key_matches(sso_other, incoming));
            assert!(!js_string_key_matches(heap_other, incoming));
        }
    }

    /// Non-string stored values (undefined / number / pointer) must return false
    /// without dereferencing the payload.
    #[test]
    fn key_matches_rejects_non_string_stored() {
        let incoming = unsafe { js_string_from_bytes(b"id".as_ptr(), 2) };
        for stored in [
            JSValue::undefined(),
            JSValue::null(),
            JSValue::int32(42),
            JSValue::bool(true),
        ] {
            unsafe {
                assert!(!js_string_key_matches(stored, incoming));
                assert!(!js_string_key_matches_bytes(stored, b"id"));
            }
        }
    }

    /// SSO key_bytes() round-trip: returns the inline bytes for SSO,
    /// the heap bytes for STRING_TAG, None for everything else.
    #[test]
    fn key_bytes_round_trips_sso_and_heap() {
        let sso = JSValue::try_short_string(b"path").expect("SSO");
        let heap = JSValue::string_ptr(unsafe { js_string_from_bytes(b"longish".as_ptr(), 7) });
        let mut buf = [0u8; SHORT_STRING_MAX_LEN];
        unsafe {
            assert_eq!(js_string_key_bytes(sso, &mut buf), Some(b"path".as_ref()));
            assert_eq!(
                js_string_key_bytes(heap, &mut buf),
                Some(b"longish".as_ref())
            );
            assert_eq!(js_string_key_bytes(JSValue::int32(7), &mut buf), None);
        }
    }
}
