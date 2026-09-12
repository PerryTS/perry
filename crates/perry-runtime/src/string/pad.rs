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
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_PAD_FILL: extern "C" fn(f64) -> *mut StringHeader = js_string_pad_fill;

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

/// Wrap fully assembled result bytes (receiver + padding, in either order) in
/// the constructor matching the WTF-8/clean split, then canonicalize any
/// surrogate pair that now straddles the receiver/padding boundary.
/// `pad_has_lone_surrogate` reports the padding alone; the receiver's own
/// `STRING_FLAG_HAS_LONE_SURROGATES` is read here so every call site doesn't
/// have to.
fn wrap_pad_result(
    s: *const StringHeader,
    bytes: &[u8],
    pad_has_lone_surrogate: bool,
) -> *mut StringHeader {
    let receiver_has_lone_surrogate = unsafe { (*s).flags & STRING_FLAG_HAS_LONE_SURROGATES != 0 };
    let result = if pad_has_lone_surrogate || receiver_has_lone_surrogate {
        js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
    } else {
        js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    };
    super::concat::canonicalize_surrogate_pairs(result)
}

/// Assemble the padded result from the receiver's raw bytes and a padding
/// chunk built by the general per-code-unit `build_pad_chunk` path (used only
/// when the pad string itself contains a surrogate code unit — see
/// `pad_units_surrogate_free`).
fn finish_pad_result(
    s: *const StringHeader,
    str_data: &str,
    pad_chunk: &[u8],
    pad_has_lone_surrogate: bool,
    prepend_pad: bool,
) -> *mut StringHeader {
    let mut bytes = Vec::with_capacity(str_data.len() + pad_chunk.len());
    if prepend_pad {
        bytes.extend_from_slice(pad_chunk);
        bytes.extend_from_slice(str_data.as_bytes());
    } else {
        bytes.extend_from_slice(str_data.as_bytes());
        bytes.extend_from_slice(pad_chunk);
    }
    wrap_pad_result(s, &bytes, pad_has_lone_surrogate)
}

/// True when no code unit in a decoded pad string is a UTF-16 surrogate
/// (high or low). Such a pad string can never straddle a surrogate pair
/// across a tile-cycle boundary and truncating it can never produce a lone
/// surrogate, so every per-unit decision `build_pad_chunk` makes (the
/// lookahead, the two moduli, the lone-surrogate bookkeeping) is dead weight
/// — this is also the overwhelmingly common case (issue #10091: a plain
/// ASCII pad string, including the default single space).
fn pad_units_surrogate_free(pad_units: &[u16]) -> bool {
    !pad_units.iter().any(|&u| (0xD800..=0xDFFF).contains(&u))
}

/// Grow `bytes[start..start + total_len]` from an already-written
/// `unit_len`-byte prefix by repeatedly doubling the written region. Source
/// and destination ranges never overlap: each step copies at most as many
/// bytes as are already written, so the copied range always ends at or
/// before where it's copied to. Returns the number of bulk copies performed
/// — O(log(total_len / unit_len)), never one per output unit (issue #10091).
fn tile_by_doubling(bytes: &mut [u8], start: usize, unit_len: usize, total_len: usize) -> u32 {
    let mut written = unit_len;
    let mut steps = 0u32;
    while written < total_len {
        let chunk = written.min(total_len - written);
        bytes.copy_within(start..start + chunk, start + written);
        written += chunk;
        steps += 1;
    }
    steps
}

/// Build the fully assembled padded result (receiver + padding, in either
/// order) for a surrogate-free pad string, without a per-code-unit loop.
/// Encodes exactly one cycle of `pad_units` once, then bulk-tiles it to the
/// needed length by doubling (`tile_by_doubling`) and appends the leftover
/// partial-cycle remainder — a logarithmic number of bulk copies rather than
/// one modulo-and-push per output code unit.
fn build_pad_result_surrogate_free(
    str_data: &[u8],
    pad_units: &[u16],
    pad_needed: usize,
    prepend_pad: bool,
) -> Vec<u8> {
    let unit_count = pad_units.len();
    // One full cycle, plus each unit's cumulative byte offset so the
    // sub-one-cycle remainder (always the *first* `remainder_units` units —
    // cycling restarts at index 0 every time) can be sliced out below without
    // re-encoding it.
    let mut cycle = Vec::with_capacity(unit_count * 3);
    let mut offsets = Vec::with_capacity(unit_count + 1);
    offsets.push(0usize);
    for &unit in pad_units {
        // No unit here is a surrogate, so this never sets the lone-surrogate
        // flag — that's exactly what makes this path safe to skip.
        super::char_ops::push_code_unit_wtf8(&mut cycle, unit);
        offsets.push(cycle.len());
    }
    let cycle_len = cycle.len();

    let full_cycles = pad_needed / unit_count;
    let remainder_units = pad_needed % unit_count;
    let remainder_len = offsets[remainder_units];
    let full_bytes = cycle_len * full_cycles;
    let pad_len = full_bytes + remainder_len;

    let mut bytes = vec![0u8; str_data.len() + pad_len];
    let (pad_start, str_start) = if prepend_pad {
        (0, pad_len)
    } else {
        (str_data.len(), 0)
    };
    bytes[str_start..str_start + str_data.len()].copy_from_slice(str_data);

    if full_bytes > 0 {
        bytes[pad_start..pad_start + cycle_len].copy_from_slice(&cycle);
        tile_by_doubling(&mut bytes, pad_start, cycle_len, full_bytes);
    }
    if remainder_len > 0 {
        bytes[pad_start + full_bytes..pad_start + full_bytes + remainder_len]
            .copy_from_slice(&cycle[..remainder_len]);
    }
    bytes
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
    if pad_units_surrogate_free(&pad_units) {
        let bytes =
            build_pad_result_surrogate_free(str_data.as_bytes(), &pad_units, pad_needed, true);
        return wrap_pad_result(s, &bytes, false);
    }
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
    if pad_units_surrogate_free(&pad_units) {
        let bytes =
            build_pad_result_surrogate_free(str_data.as_bytes(), &pad_units, pad_needed, false);
        return wrap_pad_result(s, &bytes, false);
    }
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

    // `count` may be an object, so ToNumber runs its
    // `valueOf`/`Symbol.toPrimitive` — arbitrary user JS, whose loop back-edge
    // polls are moving-GC safepoints (default-on since #7721). Two distinct
    // hazards follow, and the ordering below is what closes both (#8427):
    //
    //   * The receiver's WTF-8 payload must NOT be borrowed across the
    //     coercion. `string_as_str` materializes a `&str` at the *pre-move*
    //     address; rooting rewrites slots, never an already-live borrow, so no
    //     root can repair one (`HeapKeyBytes`, `object/field_get_set.rs`).
    //     Pre-fix, an evacuating minor inside `valueOf` left `str_data`
    //     pointing at retired from-space and `repeat` copied garbage.
    //   * `s` itself is a raw pointer in a native Rust frame, which the
    //     collector does not scan by default — so deferring the borrow is not
    //     enough on its own. Park it in a transient root and take the
    //     *post-collection* address back out of the handle.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_root = scope.root_string_ptr(s);
    let (count_number, _) = receiver_root
        .across_const::<StringHeader, _>(|| crate::builtins::js_number_coerce(count_value));

    let count_integer = to_integer_or_infinity(count_number);
    if count_integer < 0.0 || count_integer.is_infinite() {
        throw_repeat_range_error(count_number);
    }

    // ECMA-262 §22.1.3.17 step 4 returns "" for n = 0 before the receiver is
    // consulted at all; an empty receiver repeats to "" for every remaining n.
    // Both are checked here so the payload is borrowed only on the path that
    // actually reads it — and only once no user code is left to run.
    if count_integer == 0.0 {
        return js_string_from_bytes("".as_ptr(), 0);
    }
    let (source_byte_len, source_utf16_len, source_flags) =
        receiver_root.with_const_ptr(|s_before: *const StringHeader| unsafe {
            (
                (*s_before).byte_len as usize,
                (*s_before).utf16_len as usize,
                (*s_before).flags,
            )
        });
    if source_byte_len == 0 {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    if count_integer > usize::MAX as f64 {
        throw_invalid_string_length();
    }
    let count = count_integer as usize;
    let result_utf16_len = source_utf16_len
        .checked_mul(count)
        .unwrap_or_else(|| throw_invalid_string_length());
    let result_byte_len = source_byte_len
        .checked_mul(count)
        .unwrap_or_else(|| throw_invalid_string_length());
    if result_utf16_len > MAX_STRING_LENGTH || result_byte_len > u32::MAX as usize {
        throw_invalid_string_length();
    }

    let (result, result_data) = string_storage_alloc(result_byte_len as u32);
    unsafe {
        init_string_header(
            result,
            result_utf16_len as u32,
            result_byte_len as u32,
            result_byte_len as u32,
            0,
            source_flags,
        );

        // The allocation above can move `s`; only now re-read its address.
        receiver_root.with_const_ptr(|s_now: *const StringHeader| {
            ptr::copy_nonoverlapping(string_data(s_now), result_data, source_byte_len);
        });

        // Grow from the already-written destination prefix. This takes O(log n)
        // bulk copies rather than one tiny memcpy per repetition.
        let mut written = source_byte_len;
        while written < result_byte_len {
            let chunk = written.min(result_byte_len - written);
            ptr::copy_nonoverlapping(result_data, result_data.add(written), chunk);
            written += chunk;
        }
    }
    super::concat::canonicalize_surrogate_pairs(result)
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

#[cfg(test)]
mod builder_tests {
    use super::*;

    fn wtf8(bytes: &[u8]) -> *mut StringHeader {
        js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    fn payload(s: *const StringHeader) -> Vec<u8> {
        unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize).to_vec() }
    }

    #[test]
    fn repeat_writes_exact_payload_and_preserves_lone_surrogate_flag() {
        let source = wtf8(&[0xED, 0xA0, 0xBD]); // lone high surrogate D83D
        let result = js_string_repeat(source, 3.0);
        assert_eq!(
            payload(result),
            [0xED, 0xA0, 0xBD, 0xED, 0xA0, 0xBD, 0xED, 0xA0, 0xBD]
        );
        unsafe {
            assert_eq!((*result).utf16_len, 3);
            assert_ne!((*result).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        }
    }

    #[test]
    fn pad_boundaries_canonicalize_surrogate_pairs() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let high = scope.root_string_ptr(wtf8(&[0xED, 0xA0, 0xBD]));
        let low = scope.root_string_ptr(wtf8(&[0xED, 0xB8, 0x80]));

        let start = low.with_const_ptr(|low: *const StringHeader| {
            high.with_const_ptr(|high: *const StringHeader| js_string_pad_start(low, 2.0, high))
        });
        assert_eq!(payload(start), "😀".as_bytes());
        unsafe {
            assert_eq!((*start).utf16_len, 2);
            assert_eq!((*start).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        }

        let end = high.with_const_ptr(|high: *const StringHeader| {
            low.with_const_ptr(|low: *const StringHeader| js_string_pad_end(high, 2.0, low))
        });
        assert_eq!(payload(end), "😀".as_bytes());
        unsafe {
            assert_eq!((*end).utf16_len, 2);
            assert_eq!((*end).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        }
    }

    #[test]
    fn pad_cycles_and_truncates_by_utf16_units() {
        let source = js_string_from_str("x");
        let pad = js_string_from_str("😀a"); // three UTF-16 units
        let result = js_string_pad_start(source, 6.0, pad);
        assert_eq!(string_as_str(result), "😀a😀x");
        unsafe {
            assert_eq!((*result).utf16_len, 6);
            assert_eq!((*result).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        }
    }
}

/// Issue #10091: the surrogate-free fast path must (a) route ASCII/BMP pad
/// strings there at all, (b) produce byte-identical output to the general
/// per-code-unit `build_pad_chunk` path it replaces, across cycle-boundary
/// remainders and multi-byte-but-surrogate-free pad characters, and (c)
/// actually perform a logarithmic, not linear, number of bulk copies.
#[cfg(test)]
mod surrogate_free_fast_path_tests {
    use super::*;

    #[test]
    fn ascii_pad_string_is_surrogate_free() {
        assert!(pad_units_surrogate_free(&decode_wtf8_units(b"aBcD")));
        assert!(pad_units_surrogate_free(&decode_wtf8_units(b" ")));
    }

    #[test]
    fn astral_pad_string_is_not_surrogate_free() {
        // "😀" decodes to a high/low surrogate pair.
        assert!(!pad_units_surrogate_free(&decode_wtf8_units(
            "😀".as_bytes()
        )));
    }

    /// Cross-check the fast path against the general per-unit path it
    /// bypasses, across pad_needed values that land exactly on, one below,
    /// and one above a full-cycle boundary, plus pad strings whose units
    /// encode to 1, 2 and 3 WTF-8 bytes (still surrogate-free throughout).
    #[test]
    fn matches_general_path_across_cycle_boundaries_and_encodings() {
        for pad_str in ["a", "aBcD", "é", "€ab", " "] {
            let pad_units = decode_wtf8_units(pad_str.as_bytes());
            assert!(pad_units_surrogate_free(&pad_units));
            let cycle_len = pad_units.len();
            for pad_needed in 1..=(cycle_len * 3 + 2) {
                let (expected, expected_lone) = build_pad_chunk(&pad_units, pad_needed);
                assert!(!expected_lone, "surrogate-free pad must never set the flag");

                let start_bytes =
                    build_pad_result_surrogate_free(b"RECEIVER", &pad_units, pad_needed, true);
                assert_eq!(
                    &start_bytes[..expected.len()],
                    expected.as_slice(),
                    "padStart mismatch for pad={pad_str:?} pad_needed={pad_needed}"
                );
                assert_eq!(&start_bytes[expected.len()..], b"RECEIVER");

                let end_bytes =
                    build_pad_result_surrogate_free(b"RECEIVER", &pad_units, pad_needed, false);
                assert_eq!(&end_bytes[..b"RECEIVER".len()], b"RECEIVER");
                assert_eq!(&end_bytes[b"RECEIVER".len()..], expected.as_slice());
            }
        }
    }

    #[test]
    fn empty_receiver_and_single_unit_pad_needed() {
        let pad_units = decode_wtf8_units(b"x");
        let bytes = build_pad_result_surrogate_free(b"", &pad_units, 1, true);
        assert_eq!(bytes, b"x");
    }

    /// The whole point of issue #10091: padding to a million units must not
    /// take a million per-unit steps. log2(1_000_000) ~= 20; 32 leaves ample
    /// margin while still being nowhere near linear.
    #[test]
    fn tile_by_doubling_is_logarithmic_not_linear() {
        let mut buf = vec![0u8; 1_000_001];
        buf[0] = b'x';
        let total_len = buf.len();
        let steps = tile_by_doubling(&mut buf, 0, 1, total_len);
        assert!(
            steps <= 32,
            "expected O(log n) bulk copies for 1M units, got {steps}"
        );
        assert!(buf.iter().all(|&b| b == b'x'));
    }

    #[test]
    fn pad_start_and_pad_end_use_fast_path_for_large_ascii_target() {
        let source = js_string_from_str("!");
        let pad = js_string_from_str("aBcD");
        let target = 1_000_001.0;

        let start = js_string_pad_start(source, target, pad);
        unsafe { assert_eq!((*start).utf16_len, 1_000_001) };
        assert!(string_as_str(start).starts_with("aBcD"));
        assert!(string_as_str(start).ends_with('!'));

        let end = js_string_pad_end(source, target, pad);
        unsafe { assert_eq!((*end).utf16_len, 1_000_001) };
        assert!(string_as_str(end).starts_with('!'));
        assert!(string_as_str(end).ends_with("aBcD"));
    }
}
