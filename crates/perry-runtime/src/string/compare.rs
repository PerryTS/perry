//! Equality / comparison / starts-with / ends-with / well-formedness /
//! normalization / locale-compare.

use super::*;

/// Lexicographic comparison of two UTF-8 byte slices by **UTF-16 code unit**,
/// matching ECMAScript string relational comparison (`<`/`>` and the default
/// `Array.prototype.sort` order), which compares UTF-16 code units — NOT Unicode
/// code points. The two orders agree for BMP-only strings but diverge once an
/// astral character (code point > U+FFFF) is involved: its UTF-16 surrogate pair
/// leads with 0xD800–0xDBFF, sorting it *before* BMP characters in 0xE000–0xFFFF,
/// whereas raw UTF-8 byte order (= code-point order) sorts it after. Falls back
/// to raw byte order if either side is not valid UTF-8 (WTF-8 lone surrogates —
/// a known categorical gap).
///
/// # ASCII fast path
///
/// When **both** payloads are pure ASCII, every byte is its own UTF-16 code
/// unit (0x00–0x7F zero-extends to 0x0000–0x007F), so lexicographic byte order
/// and lexicographic UTF-16 code-unit order are the *same total order* — the
/// prefix tie-break included, since `<[u8]>::cmp` and `Iterator::cmp` both rank
/// a proper prefix `Less`. The fast path therefore returns `a.cmp(b)` (a
/// `memcmp` + length compare) and skips the two `from_utf8` validations and the
/// two scalar `encode_utf16` decoder iterators the general path runs.
///
/// The precondition is **checked, never assumed**. Perry heap-string payloads
/// are not guaranteed valid UTF-8 (WTF-8 lone surrogates, `Buffer.toString`
/// of arbitrary bytes, FFI blobs — #6085), so this must not lean on any
/// derived metadata:
///
/// * `<[u8]>::is_ascii` inspects the actual bytes word-at-a-time and is total
///   over arbitrary byte strings — no validity assumption at all.
/// * The header-cached predicate `is_ascii_string` (`utf16_len == byte_len`)
///   would be *cheaper* but is **not sound** here: `compute_utf16_len_wtf8`
///   charges a truncated multi-byte lead its full nominal unit count while the
///   payload holds fewer bytes, so e.g. `[0xC3]` records `utf16_len == 1 ==
///   byte_len` and `[0xF0, 0x41]` records `utf16_len == 2 == byte_len` — both
///   non-ASCII payloads that the cached predicate calls ASCII. Byte-scanning is
///   the only precondition that holds for non-UTF-8 payloads.
///
/// Mixed operands (one ASCII, one not) deliberately fall through unchanged
/// rather than reasoning about lead-byte ranges: the general path already
/// handles them, and this stays a decision about *both* operands.
pub(crate) fn utf16_cmp_bytes(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    match (std::str::from_utf8(a), std::str::from_utf8(b)) {
        (Ok(a_str), Ok(b_str)) => a_str.encode_utf16().cmp(b_str.encode_utf16()),
        _ => a.cmp(b),
    }
}

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
        match utf16_cmp_bytes(a_bytes, b_bytes) {
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

/// Repsel Phase 3a: relational comparison over NaN-boxed operands that may
/// be heap strings (`STRING_TAG`) or inline SSO values (`SHORT_STRING_TAG`)
/// in any mix — the boxed counterpart of `js_string_compare`, used by the
/// canonical-Str compare lowering's non-proven-heap arm. Decodes SSO
/// operands through a stack scratch buffer (no heap materialization).
///
/// Semantics for non-string operands mirror the legacy
/// `js_get_string_pointer_unified` → `js_string_compare` composition this
/// arm replaces: a plain number compares by its decimal string form; every
/// other non-string value ranks like `js_string_compare`'s invalid-pointer
/// handling (invalid < any valid string; two invalids compare equal).
/// Returns -1 / 0 / 1.
#[no_mangle]
pub extern "C" fn js_string_compare_value(a: f64, b: f64) -> i32 {
    // Phase 1 — ALLOCATING coercions only. `js_number_to_string` allocates,
    // and an allocation can run a GC cycle that MOVES the other operand's
    // heap string (evacuation); the decimal bytes are therefore copied into
    // an owned `Vec` immediately, and no raw heap-string pointer may exist
    // yet. Both operands' coercions complete before phase 2 takes any view.
    fn number_bytes(v: f64) -> Option<Vec<u8>> {
        if !crate::JSValue::from_bits(v.to_bits()).is_number() {
            return None;
        }
        // Mirror the unified helper's number → decimal-string coercion.
        let s = crate::string::js_number_to_string(v);
        if !crate::string::is_valid_string_ptr(s) {
            return None;
        }
        unsafe {
            let len = (*s).byte_len;
            let data = crate::string::string_data(s);
            Some(std::slice::from_raw_parts(data, len as usize).to_vec())
        }
    }
    let a_num = number_bytes(a);
    let b_num = number_bytes(b);

    // Phase 2 — NON-allocating views only (heap payload pointers, SSO
    // scratch decode, or the owned number buffers). Nothing below allocates,
    // so the raw `from_raw_parts` reads cannot observe a moved string.
    fn view_of<'s>(
        v: f64,
        scratch: &'s mut [u8; crate::value::SHORT_STRING_MAX_LEN],
        num_buf: &'s Option<Vec<u8>>,
    ) -> Option<(*const u8, u32)> {
        if let Some(view) = crate::string::str_bytes_from_jsvalue(v, scratch) {
            return Some(view);
        }
        num_buf.as_ref().map(|buf| (buf.as_ptr(), buf.len() as u32))
    }
    let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let a_view = view_of(a, &mut a_scratch, &a_num);
    let b_view = view_of(b, &mut b_scratch, &b_num);
    match (a_view, b_view) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some((a_ptr, a_len)), Some((b_ptr, b_len))) => unsafe {
            let a_bytes = std::slice::from_raw_parts(a_ptr, a_len as usize);
            let b_bytes = std::slice::from_raw_parts(b_ptr, b_len as usize);
            match utf16_cmp_bytes(a_bytes, b_bytes) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        },
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
    let numeric = {
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
mod utf16_cmp_ascii_fast_path_tests {
    use super::utf16_cmp_bytes;
    use std::cmp::Ordering;

    /// Independent restatement of the pre-fast-path semantics: validate both
    /// sides as UTF-8 and compare the UTF-16 code-unit sequences, falling back
    /// to raw byte order when either side is not valid UTF-8.
    ///
    /// Deliberately *not* the production fallthrough — it is the oracle the
    /// fast path is differentially checked against, so breaking either arm of
    /// `utf16_cmp_bytes` in production code turns these tests red.
    fn reference_cmp(a: &[u8], b: &[u8]) -> Ordering {
        match (std::str::from_utf8(a), std::str::from_utf8(b)) {
            (Ok(a_str), Ok(b_str)) => {
                let au: Vec<u16> = a_str.encode_utf16().collect();
                let bu: Vec<u16> = b_str.encode_utf16().collect();
                au.cmp(&bu)
            }
            _ => a.cmp(b),
        }
    }

    /// Every shape the fast path has to get right or fall through on.
    /// WTF-8 lone surrogates are raw byte literals — they are not
    /// representable as Rust `&str`.
    fn corpus() -> Vec<Vec<u8>> {
        let mut v: Vec<Vec<u8>> = Vec::new();
        // Pure ASCII, incl. empty, prefixes, NUL, and the 0x7F boundary.
        for s in [
            "",
            "a",
            "A",
            "ab",
            "abc",
            "abd",
            "abcd",
            "b",
            "z",
            "0",
            "9",
            "~",
            "\u{7f}",
            " ",
            "!",
            "Zebra",
            "apple",
            "Apple",
            "apple pie",
            "record-00001",
            "record-00002",
        ] {
            v.push(s.as_bytes().to_vec());
        }
        // Embedded NUL — must not terminate the comparison early.
        v.push(b"ab\0cd".to_vec());
        v.push(b"ab\0ce".to_vec());
        v.push(b"ab\0".to_vec());
        v.push(vec![0u8]);
        // Non-ASCII BMP, incl. the ASCII/non-ASCII 0x80 boundary and the
        // 0xE000..0xFFFF band that is where UTF-16 order diverges from
        // code-point order.
        for s in [
            "\u{80}",
            "\u{7ff}",
            "\u{800}",
            "café",
            "cafè",
            "caf\u{e9}x",
            "日本",
            "日本語",
            "\u{e000}",
            "\u{fffd}",
            "\u{ffff}",
        ] {
            v.push(s.as_bytes().to_vec());
        }
        // Astral (4-byte UTF-8 / surrogate pair in UTF-16).
        for s in ["\u{10000}", "\u{1f600}", "a\u{1f600}", "\u{10ffff}"] {
            v.push(s.as_bytes().to_vec());
        }
        // WTF-8 lone surrogates (invalid UTF-8) — the fallthrough's byte-order arm.
        v.push(vec![0xED, 0xA0, 0x80]); // lone high surrogate U+D800
        v.push(vec![0xED, 0xB0, 0x80]); // lone low surrogate U+DC00
        v.push(vec![b'a', 0xED, 0xA0, 0x80]);
        // Truncated / malformed sequences — exactly the payloads whose cached
        // `utf16_len == byte_len` would lie about being ASCII.
        v.push(vec![0xC3]);
        v.push(vec![0xF0, 0x41]);
        v.push(vec![0x80]);
        v
    }

    /// The fast path must agree with the reference on every ordered pair, and
    /// the corpus must actually exercise *both* arms (a green run that never
    /// entered the fast path would prove nothing).
    #[test]
    fn fast_path_agrees_with_reference_on_every_pair() {
        let corpus = corpus();
        let (mut fast_arm, mut slow_arm) = (0usize, 0usize);
        for a in &corpus {
            for b in &corpus {
                let got = utf16_cmp_bytes(a, b);
                let want = reference_cmp(a, b);
                assert_eq!(got, want, "utf16_cmp_bytes({a:?}, {b:?})");
                if a.is_ascii() && b.is_ascii() {
                    fast_arm += 1;
                } else {
                    slow_arm += 1;
                }
            }
        }
        assert!(fast_arm > 0, "corpus never entered the ASCII fast path");
        assert!(slow_arm > 0, "corpus never entered the fallthrough");
    }

    /// Sort-safety: the relation must stay a total order (antisymmetric,
    /// reflexive-equal) with the fast path spliced in — a comparator that
    /// disagrees with itself corrupts `Array.prototype.sort`.
    #[test]
    fn fast_path_keeps_a_total_order() {
        let corpus = corpus();
        for a in &corpus {
            assert_eq!(utf16_cmp_bytes(a, a), Ordering::Equal, "{a:?} != itself");
            for b in &corpus {
                assert_eq!(
                    utf16_cmp_bytes(a, b),
                    utf16_cmp_bytes(b, a).reverse(),
                    "antisymmetry broken for ({a:?}, {b:?})"
                );
            }
        }
    }

    /// Pure-ASCII orderings, spelled out (these are the pairs the fast path
    /// answers on its own).
    #[test]
    fn pure_ascii_orderings() {
        let c = |a: &str, b: &str| utf16_cmp_bytes(a.as_bytes(), b.as_bytes());
        assert_eq!(c("", ""), Ordering::Equal);
        assert_eq!(c("", "a"), Ordering::Less);
        assert_eq!(c("a", ""), Ordering::Greater);
        assert_eq!(c("abc", "abc"), Ordering::Equal);
        assert_eq!(c("abc", "abd"), Ordering::Less);
        assert_eq!(c("abd", "abc"), Ordering::Greater);
        assert_eq!(c("abc", "abcd"), Ordering::Less); // proper prefix sorts first
        assert_eq!(c("abcd", "abc"), Ordering::Greater);
        // Uppercase < lowercase in code-unit order (JS `<`, not localeCompare).
        assert_eq!(c("Zebra", "apple"), Ordering::Less);
        assert_eq!(c("apple", "Apple"), Ordering::Greater);
        // Embedded NUL is an ordinary code unit.
        assert_eq!(
            utf16_cmp_bytes(b"ab\0cd", b"ab\0ce"),
            Ordering::Less,
            "embedded NUL must not truncate the comparison"
        );
        assert_eq!(utf16_cmp_bytes(b"ab\0", b"ab"), Ordering::Greater);
    }

    /// Non-ASCII pairs must keep going through the general path — including
    /// the astral-vs-BMP case where UTF-16 order and byte/code-point order
    /// disagree, which is the whole reason this helper is not `a.cmp(b)`.
    #[test]
    fn non_ascii_keeps_utf16_code_unit_order() {
        let c = |a: &str, b: &str| utf16_cmp_bytes(a.as_bytes(), b.as_bytes());
        // U+FFFD (BMP) vs U+10000 (astral): code-point/byte order says Less,
        // UTF-16 order says Greater because the surrogate lead is 0xD800.
        assert_eq!(c("\u{fffd}", "\u{10000}"), Ordering::Greater);
        assert_eq!(c("\u{10000}", "\u{fffd}"), Ordering::Less);
        assert_eq!(c("\u{e000}", "\u{1f600}"), Ordering::Greater);
        // Same, but only reachable past a shared ASCII prefix.
        assert_eq!(c("a\u{ffff}", "a\u{10000}"), Ordering::Greater);
        // Strings differing only past the ASCII range.
        assert_eq!(c("café", "cafè"), Ordering::Greater); // U+00E9 > U+00E8
        assert_eq!(c("cafe", "café"), Ordering::Less);
        assert_eq!(c("日本", "日本語"), Ordering::Less);
        assert_eq!(c("\u{7f}", "\u{80}"), Ordering::Less);
    }

    /// Mixed ASCII / non-ASCII operands take the fallthrough (`b.is_ascii()`
    /// is false) and must still be ordered by UTF-16 code unit.
    #[test]
    fn mixed_ascii_and_non_ascii_operands() {
        let c = |a: &str, b: &str| utf16_cmp_bytes(a.as_bytes(), b.as_bytes());
        assert_eq!(c("z", "\u{80}"), Ordering::Less);
        assert_eq!(c("\u{80}", "z"), Ordering::Greater);
        assert_eq!(c("abc", "abc\u{e9}"), Ordering::Less);
        assert_eq!(c("abc\u{e9}", "abc"), Ordering::Greater);
        assert_eq!(c("", "\u{1f600}"), Ordering::Less);
        assert_eq!(c("\u{1f600}", ""), Ordering::Greater);
    }

    /// WTF-8 lone surrogates are not valid UTF-8, so the general path falls
    /// back to raw byte order. The fast path must not claim them (0xED > 0x7F)
    /// and must not change the answer.
    #[test]
    fn lone_surrogates_fall_back_to_byte_order() {
        let high: &[u8] = &[0xED, 0xA0, 0x80]; // U+D800
        let low: &[u8] = &[0xED, 0xB0, 0x80]; // U+DC00
        assert!(!high.is_ascii() && !low.is_ascii());
        assert_eq!(utf16_cmp_bytes(high, low), Ordering::Less);
        assert_eq!(utf16_cmp_bytes(low, high), Ordering::Greater);
        assert_eq!(utf16_cmp_bytes(high, high), Ordering::Equal);
        assert_eq!(utf16_cmp_bytes(high, b"a"), Ordering::Greater);
        assert_eq!(utf16_cmp_bytes(b"a", high), Ordering::Less);
        assert_eq!(utf16_cmp_bytes(high, b""), Ordering::Greater);
    }

    /// The header-cached `utf16_len == byte_len` predicate is unsound as an
    /// ASCII test for these payloads; the byte scan the fast path uses is not.
    /// This pins the reason the cheaper flag was rejected.
    #[test]
    fn cached_utf16_len_predicate_would_misclassify_these() {
        for payload in [vec![0xC3u8], vec![0xF0u8, 0x41]] {
            let cached_says_ascii =
                crate::string::compute_utf16_len(payload.as_ptr(), payload.len() as u32) as usize
                    == payload.len();
            assert!(
                cached_says_ascii,
                "expected the cached predicate to (wrongly) call {payload:?} ASCII"
            );
            assert!(
                !payload.is_ascii(),
                "{payload:?} is not ASCII — the byte scan must reject it"
            );
            // And the answer is unchanged either way for these.
            assert_eq!(
                utf16_cmp_bytes(&payload, b"a"),
                reference_cmp(&payload, b"a")
            );
        }
    }
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

            let incoming = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
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
        let incoming = js_string_from_bytes(b"id".as_ptr(), 2);
        let sso_other = JSValue::try_short_string(b"tag").expect("SSO");
        let heap_other_ptr = js_string_from_bytes(b"other".as_ptr(), 5);
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
        let incoming = js_string_from_bytes(b"id".as_ptr(), 2);
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
        let heap = JSValue::string_ptr(js_string_from_bytes(b"longish".as_ptr(), 7));
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
