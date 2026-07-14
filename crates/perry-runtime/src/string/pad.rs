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

/// Decode raw WTF-8 bytes (as stored by a `StringHeader`, which may contain
/// 3-byte lone-surrogate sequences per `STRING_FLAG_HAS_LONE_SURROGATES`)
/// into UTF-16 code units. Operates on bytes directly rather than through
/// `str`/`char` — a lone surrogate is not a valid Unicode scalar value, so
/// `str::encode_utf16()` over a `str::from_utf8_unchecked` buffer containing
/// one is undefined behavior (the decoder assumes well-formed UTF-8), not
/// just wrong output.
fn decode_wtf8_units(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b0 = bytes[i];
        if b0 < 0x80 {
            units.push(b0 as u16);
            i += 1;
        } else if b0 < 0xE0 {
            // 2-byte sequence: U+0080..U+07FF, always one code unit.
            // #6085: a multi-byte lead truncated at the end of the buffer
            // has no continuation byte; emit the lead as a lone byte instead
            // of reading `bytes[i + 1]` one past the slice. Strings are
            // normally well-formed WTF-8 so this is a defensive bound, but an
            // unchecked read here is a real out-of-slice access.
            if i + 1 >= len {
                units.push(b0 as u16);
                i += 1;
                continue;
            }
            let b1 = bytes[i + 1];
            let cp = ((b0 as u32 & 0x1F) << 6) | (b1 as u32 & 0x3F);
            units.push(cp as u16);
            i += 2;
        } else if b0 < 0xF0 {
            // 3-byte sequence: U+0800..U+FFFF, including a WTF-8 lone
            // surrogate (U+D800..U+DFFF) — always one code unit either way.
            if i + 2 >= len {
                units.push(b0 as u16);
                i += 1;
                continue;
            }
            let b1 = bytes[i + 1];
            let b2 = bytes[i + 2];
            let cp = ((b0 as u32 & 0x0F) << 12) | ((b1 as u32 & 0x3F) << 6) | (b2 as u32 & 0x3F);
            units.push(cp as u16);
            i += 3;
        } else {
            // 4-byte sequence: an astral code point, encoded as a surrogate pair.
            if i + 3 >= len {
                units.push(b0 as u16);
                i += 1;
                continue;
            }
            let b1 = bytes[i + 1];
            let b2 = bytes[i + 2];
            let b3 = bytes[i + 3];
            let cp = ((b0 as u32 & 0x07) << 18)
                | ((b1 as u32 & 0x3F) << 12)
                | ((b2 as u32 & 0x3F) << 6)
                | (b3 as u32 & 0x3F);
            let astral = cp - 0x10000;
            units.push(0xD800 + (astral >> 10) as u16);
            units.push(0xDC00 + (astral & 0x3FF) as u16);
            i += 4;
        }
    }
    units
}

/// Build exactly `pad_needed` UTF-16 code units of padding by cycling through
/// `pad_units`, encoded as WTF-8. A complete high+low surrogate pair straddled
/// across a cycle boundary is combined into its astral code point (ordinary
/// 4-byte UTF-8); only a surrogate pair *truncated* by `pad_needed` (the spec
/// counts by code unit, not code point — ECMA-262 §22.1.3.16 `StringPad`)
/// survives as a genuinely lone surrogate, encoded as 3-byte WTF-8. Returns
/// whether any lone surrogate was emitted so the caller can pick the
/// WTF-8-flagged construction path.
fn build_pad_chunk(pad_units: &[u16], pad_needed: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(pad_needed * 3);
    let mut has_lone_surrogate = false;
    let mut produced = 0usize;
    let mut idx = 0usize;
    while produced < pad_needed {
        let unit = pad_units[idx % pad_units.len()];
        if (0xD800..=0xDBFF).contains(&unit) && produced + 2 <= pad_needed {
            let next = pad_units[(idx + 1) % pad_units.len()];
            if (0xDC00..=0xDFFF).contains(&next) {
                let astral = 0x10000 + (((unit as u32) - 0xD800) << 10) + ((next as u32) - 0xDC00);
                let ch = unsafe { char::from_u32_unchecked(astral) };
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                produced += 2;
                idx += 2;
                continue;
            }
        }
        if super::char_ops::push_code_unit_wtf8(&mut out, unit) {
            has_lone_surrogate = true;
        }
        produced += 1;
        idx += 1;
    }
    (out, has_lone_surrogate)
}

/// Assemble the padded result from the receiver's raw bytes and a padding
/// chunk, using the WTF-8-flagged constructor when either side may hold a
/// lone surrogate (the receiver's existing flag, or one newly introduced by
/// truncating the pad string mid-surrogate-pair).
fn finish_pad_result(
    s: *const StringHeader,
    str_data: &str,
    pad_chunk: &[u8],
    pad_has_lone_surrogate: bool,
    prepend_pad: bool,
) -> *mut StringHeader {
    let receiver_has_lone_surrogate = unsafe { (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 };
    let mut bytes = Vec::with_capacity(str_data.len() + pad_chunk.len());
    if prepend_pad {
        bytes.extend_from_slice(pad_chunk);
        bytes.extend_from_slice(str_data.as_bytes());
    } else {
        bytes.extend_from_slice(str_data.as_bytes());
        bytes.extend_from_slice(pad_chunk);
    }
    if pad_has_lone_surrogate || receiver_has_lone_surrogate {
        js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
    } else {
        js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
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
    let pad_bytes: &[u8] = if is_valid_string_ptr(pad_string) {
        unsafe { slice::from_raw_parts(string_data(pad_string), (*pad_string).byte_len as usize) }
    } else {
        b" "
    };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = to_length(target_length);

    // ToLength itself never throws; the receiver is returned unchanged when
    // it's already long enough or the filler is empty — even for an
    // unrepresentable target like Infinity (Node parity, #2786/#2880). Route
    // through `finish_pad_result` (empty pad chunk) rather than a bare
    // `js_string_from_bytes` so a receiver already flagged
    // `STRING_FLAG_HAS_LONE_SURROGATES` keeps that flag on the result.
    if current_len >= target_len || pad_bytes.is_empty() {
        return finish_pad_result(s, str_data, &[], false, true);
    }

    // Only now, when a longer string must actually be produced, reject
    // lengths beyond the engine's max string length with a RangeError.
    if target_len > MAX_STRING_LENGTH {
        throw_invalid_string_length();
    }

    let pad_needed = target_len - current_len;
    let pad_units = decode_wtf8_units(pad_bytes);
    let (pad_chunk, pad_has_lone_surrogate) = build_pad_chunk(&pad_units, pad_needed);
    finish_pad_result(s, str_data, &pad_chunk, pad_has_lone_surrogate, true)
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
    let pad_bytes: &[u8] = if is_valid_string_ptr(pad_string) {
        unsafe { slice::from_raw_parts(string_data(pad_string), (*pad_string).byte_len as usize) }
    } else {
        b" "
    };

    let current_len = unsafe { (*s).utf16_len } as usize;
    let target_len = to_length(target_length);

    // ToLength itself never throws; the receiver is returned unchanged when
    // it's already long enough or the filler is empty — even for an
    // unrepresentable target like Infinity (Node parity, #2786/#2880). Route
    // through `finish_pad_result` (empty pad chunk) rather than a bare
    // `js_string_from_bytes` so a receiver already flagged
    // `STRING_FLAG_HAS_LONE_SURROGATES` keeps that flag on the result.
    if current_len >= target_len || pad_bytes.is_empty() {
        return finish_pad_result(s, str_data, &[], false, false);
    }

    // Only now, when a longer string must actually be produced, reject
    // lengths beyond the engine's max string length with a RangeError.
    if target_len > MAX_STRING_LENGTH {
        throw_invalid_string_length();
    }

    let pad_needed = target_len - current_len;
    let pad_units = decode_wtf8_units(pad_bytes);
    let (pad_chunk, pad_has_lone_surrogate) = build_pad_chunk(&pad_units, pad_needed);
    finish_pad_result(s, str_data, &pad_chunk, pad_has_lone_surrogate, false)
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

#[cfg(test)]
mod decode_wtf8_tests {
    use super::decode_wtf8_units;

    #[test]
    fn decodes_well_formed_sequences() {
        // ASCII
        assert_eq!(decode_wtf8_units(b"AB"), vec![0x41, 0x42]);
        // 2-byte: U+00E9 (é) = 0xC3 0xA9
        assert_eq!(decode_wtf8_units(&[0xC3, 0xA9]), vec![0x00E9]);
        // 3-byte: U+20AC (€) = 0xE2 0x82 0xAC
        assert_eq!(decode_wtf8_units(&[0xE2, 0x82, 0xAC]), vec![0x20AC]);
        // 3-byte WTF-8 lone surrogate: U+D800 = 0xED 0xA0 0x80
        assert_eq!(decode_wtf8_units(&[0xED, 0xA0, 0x80]), vec![0xD800]);
        // 4-byte astral: U+1F600 (😀) = 0xF0 0x9F 0x98 0x80 -> surrogate pair
        assert_eq!(
            decode_wtf8_units(&[0xF0, 0x9F, 0x98, 0x80]),
            vec![0xD83D, 0xDE00]
        );
    }

    /// #6085: a multi-byte lead byte truncated at the end of the slice must not
    /// read past the buffer. Each case ends mid-sequence; the decoder must
    /// return without an out-of-bounds read (emitting the lead as a lone byte).
    #[test]
    fn truncated_trailing_lead_does_not_over_read() {
        // 2-byte lead with no continuation byte.
        assert_eq!(decode_wtf8_units(&[0xC3]), vec![0x00C3]);
        // 3-byte lead missing 1 and 2 continuation bytes.
        assert_eq!(decode_wtf8_units(&[0xE2]), vec![0x00E2]);
        assert_eq!(decode_wtf8_units(&[0xE2, 0x82]), vec![0x00E2, 0x0082]);
        // 4-byte lead with no continuation byte.
        assert_eq!(decode_wtf8_units(&[0xF0]), vec![0x00F0]);
        // Valid prefix followed by a truncated lead: prefix decodes, tail is safe.
        assert_eq!(decode_wtf8_units(&[0x41, 0xF0]), vec![0x0041, 0x00F0]);
        // Longer malformed tails must also complete without an out-of-slice
        // read (exact re-interpreted units are unspecified — only safety
        // matters), so just assert the call returns a bounded result.
        assert!(decode_wtf8_units(&[0xF0, 0x9F, 0x98]).len() <= 3);
        assert!(decode_wtf8_units(&[0xE2, 0x82]).len() <= 2);
    }
}
