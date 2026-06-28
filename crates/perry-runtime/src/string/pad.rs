//! `padStart`, `padEnd`, `repeat`, and the default-pad space allocator.

use super::*;

/// Allocate a string containing a single space character " "
/// Used as default pad string for padStart/padEnd
#[no_mangle]
pub extern "C" fn js_string_alloc_space() -> *mut StringHeader {
    js_string_from_bytes(" ".as_ptr(), 1)
}

/// Coerce a `padStart`/`padEnd` `fillString` argument (ECMA-262 §22.1.3.16
/// `StringPad`): `undefined` keeps the default — returned as a null pointer so
/// `js_string_pad_*` substitutes `" "` — while any other value is
/// `ToString`-coerced (a number/boolean/null/`{ toString }` object renders to
/// its string form, may run user code and throw). A raw `unbox_str_handle` of
/// such an arg bit-cast it as a string handle, dropping non-string fills.
#[no_mangle]
pub extern "C" fn js_string_pad_fill(value: f64) -> *mut StringHeader {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return std::ptr::null_mut();
    }
    // ToString(fillString): a Symbol fill throws a TypeError (§7.1.17), it does
    // NOT stringify to "Symbol(...)" the way the lenient `String()` path would.
    crate::builtins::reject_symbol_to_string(value);
    crate::builtins::js_string_coerce(value)
}

// `#[used]` keepalive: `js_string_pad_fill` is reached only from generated
// `.o`, so the whole-program auto-optimize bitcode rebuild would dead-strip it
// without an anchor (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_PAD_FILL: extern "C" fn(f64) -> *mut StringHeader = js_string_pad_fill;

/// Maximum string length Perry/V8 supports as a single `String`. This
/// mirrors the value Node v25 reports via `buffer.constants.MAX_STRING_LENGTH`
/// (536_870_888 = `(1 << 29) - 24` on this V8 build). `padStart`/`padEnd`
/// throw `RangeError: Invalid string length` when the requested length
/// exceeds this, instead of silently capping. (#2786 / #2880)
const MAX_STRING_LENGTH: usize = 536_870_888;

/// ToLength coercion (ECMA-262 §7.1.21) for `padStart`/`padEnd`'s target
/// length: NaN/negative → 0, fractional values truncate, `+Infinity` →
/// `2^53 - 1`. Per the spec's `StringPad`, ToLength itself never throws —
/// the `RangeError: Invalid string length` is raised later (at allocation
/// time) only when a result string longer than `MAX_STRING_LENGTH` would
/// actually be produced. That means `"x".padStart(Infinity, "")` (empty
/// filler) and `"hi".padStart(Infinity)` (already long enough) return the
/// receiver unchanged, while `"x".padStart(Infinity, "0")` throws. See
/// `js_string_pad_start` / `_pad_end` for the deferred-throw call order.
///
/// The NaN/negative → 0 branch also preserves the pre-#2786 protection
/// against the codegen `fptosi(NaN)`-then-`u32`-cast path that produced
/// `0xFFFFFFFF` from a literal `-1` / `NaN`.
fn to_length(target_length: f64) -> usize {
    if target_length.is_nan() || target_length <= 0.0 {
        0
    } else if target_length.is_infinite() {
        // 2^53 - 1, the spec ToLength maximum. Stored as usize so the
        // later `> MAX_STRING_LENGTH` allocation guard fires.
        (1u64 << 53).wrapping_sub(1) as usize
    } else {
        // ToLength truncates the fractional part (e.g. 5.9 → 5).
        target_length.trunc() as usize
    }
}

fn throw_invalid_string_length() -> ! {
    let message = "Invalid string length";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Pad the start of a string to reach target length (in UTF-16 code units).
/// str.padStart(targetLength, padString)
#[no_mangle]
pub extern "C" fn js_string_pad_start(
    s: *const StringHeader,
    target_length: f64,
    pad_string: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) {
        string_as_str(pad_string)
    } else {
        " "
    };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = to_length(target_length);

    // ToLength itself never throws; the receiver is returned unchanged when
    // it's already long enough or the filler is empty — even for an
    // unrepresentable target like Infinity (Node parity, #2786/#2880).
    if current_len >= target_len || pad_data.is_empty() {
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    // Only now, when a longer string must actually be produced, reject
    // lengths beyond the engine's max string length with a RangeError.
    if target_len > MAX_STRING_LENGTH {
        throw_invalid_string_length();
    }

    let pad_needed = target_len - current_len;
    let _pad_u16: Vec<u16> = pad_data.encode_utf16().collect();
    let mut result = String::with_capacity(target_len * 4);

    // Build padding by UTF-16 code units
    let mut u16_added = 0;
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    while u16_added < pad_needed {
        let ch = pad_chars[pad_idx % pad_chars.len()];
        let ch_u16_len = ch.len_utf16();
        if u16_added + ch_u16_len > pad_needed {
            break;
        }
        result.push(ch);
        u16_added += ch_u16_len;
        pad_idx += 1;
    }

    result.push_str(str_data);

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Pad the end of a string to reach target length (in UTF-16 code units).
/// str.padEnd(targetLength, padString) — see `to_length_clamped` above.
#[no_mangle]
pub extern "C" fn js_string_pad_end(
    s: *const StringHeader,
    target_length: f64,
    pad_string: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let pad_data = if is_valid_string_ptr(pad_string) {
        string_as_str(pad_string)
    } else {
        " "
    };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = to_length(target_length);

    // ToLength itself never throws; the receiver is returned unchanged when
    // it's already long enough or the filler is empty — even for an
    // unrepresentable target like Infinity (Node parity, #2786/#2880).
    if current_len >= target_len || pad_data.is_empty() {
        return js_string_from_bytes(str_data.as_ptr(), str_data.len() as u32);
    }

    // Only now, when a longer string must actually be produced, reject
    // lengths beyond the engine's max string length with a RangeError.
    if target_len > MAX_STRING_LENGTH {
        throw_invalid_string_length();
    }

    let pad_needed = target_len - current_len;
    let mut result = String::with_capacity(target_len * 4);

    result.push_str(str_data);

    // Build padding by UTF-16 code units
    let pad_chars: Vec<char> = pad_data.chars().collect();
    let mut pad_idx = 0;
    let mut u16_added = 0;
    while u16_added < pad_needed {
        let ch = pad_chars[pad_idx % pad_chars.len()];
        let ch_u16_len = ch.len_utf16();
        if u16_added + ch_u16_len > pad_needed {
            break;
        }
        result.push(ch);
        u16_added += ch_u16_len;
        pad_idx += 1;
    }

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Repeat a string a specified number of times
/// str.repeat(count)
#[no_mangle]
pub extern "C" fn js_string_repeat(s: *const StringHeader, count_value: f64) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let str_data = string_as_str(s);
    let count_number = crate::builtins::js_number_coerce(count_value);
    let count_integer = to_integer_or_infinity(count_number);
    if count_integer < 0.0 || count_integer.is_infinite() {
        throw_repeat_range_error(count_number);
    }

    if count_integer == 0.0 || str_data.is_empty() {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    let count = count_integer as usize;
    let result = str_data.repeat(count);
    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

fn to_integer_or_infinity(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 {
        0.0
    } else if value.is_infinite() {
        value
    } else {
        value.trunc()
    }
}

fn throw_repeat_range_error(count: f64) -> ! {
    let rendered = if count.is_infinite() {
        if count.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string()
    } else {
        format!("{}", count)
    };
    let message = format!("Invalid count value: {}", rendered);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(test)]
mod pad_length_tests {
    use super::{to_length, MAX_STRING_LENGTH};

    /// #2786/#2880: ToLength for pad targets — NaN/negative → 0, fractional
    /// truncates, +Infinity maps to the spec maximum (2^53 - 1) which the
    /// caller then rejects at allocation time.
    #[test]
    fn to_length_matches_node_coercion() {
        assert_eq!(to_length(0.0), 0);
        assert_eq!(to_length(-1.0), 0);
        assert_eq!(to_length(f64::NAN), 0);
        assert_eq!(to_length(5.0), 5);
        assert_eq!(to_length(5.9), 5); // truncates, not rounds
        assert_eq!(to_length(1_048_577.0), 1_048_577);
        // +Infinity → the ToLength maximum, which exceeds MAX_STRING_LENGTH
        // so the pad helpers raise RangeError when a longer string is needed.
        assert_eq!(to_length(f64::INFINITY), (1u64 << 53) as usize - 1);
        assert!(to_length(f64::INFINITY) > MAX_STRING_LENGTH);
        // MAX is representable; MAX+1 exceeds the engine limit.
        assert_eq!(to_length(MAX_STRING_LENGTH as f64), MAX_STRING_LENGTH);
        assert!(to_length((MAX_STRING_LENGTH + 1) as f64) > MAX_STRING_LENGTH);
        assert!(to_length(4_294_967_296.0) > MAX_STRING_LENGTH); // 2^32
    }
}
