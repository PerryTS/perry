//! TC39 Uint8Array base64/hex conversion APIs (issue #2901).
//!
//! Implements the instance methods `toBase64({alphabet, omitPadding})`,
//! `toHex()`, `setFromBase64(str, {alphabet, lastChunkHandling})`,
//! `setFromHex(str)` and the static factories `Uint8Array.fromBase64` /
//! `Uint8Array.fromHex`. Perry aliases `Uint8Array` to its `Buffer`
//! representation (#2447), so the receiver of the instance methods is a
//! `BufferHeader` and the static factories return a fresh `BufferHeader`.
//!
//! These differ from the existing permissive Buffer hex/base64 codecs
//! (`coding.rs`) in that TC39 mandates *strict* validation: an invalid
//! character or an odd-length hex string throws a `SyntaxError`, and the
//! base64 alphabet is selected explicitly (standard `+/` vs. url `-_`)
//! rather than accepting both. Interior/trailing ASCII whitespace is
//! tolerated in base64 input.

use super::header::{buffer_alloc, BufferHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::{js_string_alloc_ascii_uninit, js_string_from_ascii_bytes, StringHeader};
use crate::value::JSValue;

const STD_ENCODE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_ENCODE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const HEX_ENCODE: &[u8; 16] = b"0123456789abcdef";

/// 0..=63 valid, 255 invalid. Standard alphabet (`+` = 62, `/` = 63).
const STD_DECODE: [u8; 256] = build_b64_decode(false);
/// Url-safe alphabet (`-` = 62, `_` = 63).
const URL_DECODE: [u8; 256] = build_b64_decode(true);

const fn build_b64_decode(url: bool) -> [u8; 256] {
    let mut t = [255u8; 256];
    let mut i = 0u8;
    while i < 26 {
        t[(b'A' + i) as usize] = i;
        t[(b'a' + i) as usize] = i + 26;
        i += 1;
    }
    let mut i = 0u8;
    while i < 10 {
        t[(b'0' + i) as usize] = i + 52;
        i += 1;
    }
    if url {
        t[b'-' as usize] = 62;
        t[b'_' as usize] = 63;
    } else {
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
    }
    t
}

const HEX_DECODE: [u8; 256] = {
    let mut t = [255u8; 256];
    let mut i = 0u8;
    while i < 10 {
        t[(b'0' + i) as usize] = i;
        i += 1;
    }
    let mut i = 0u8;
    while i < 6 {
        t[(b'a' + i) as usize] = 10 + i;
        t[(b'A' + i) as usize] = 10 + i;
        i += 1;
    }
    t
};

#[inline]
fn is_b64_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

// ---------------------------------------------------------------------------
// Helpers: unbox pointers, read input strings, throw, build result objects.
// ---------------------------------------------------------------------------

#[inline]
fn unbox_ptr(raw: u64) -> usize {
    let top16 = raw >> 48;
    if top16 >= 0x7FF8 {
        (raw & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        raw as usize
    }
}

unsafe fn buffer_from_addr(addr: usize) -> *mut BufferHeader {
    unbox_ptr(addr as u64) as *mut BufferHeader
}

/// Copy the input string's bytes out; `None` when the handle is null. The
/// handle is a NaN-boxed string, heap or inline SSO (#11519:
/// `Uint8Array.fromHex("4" + "869")` masked an SSO value into an address), or
/// a raw `StringHeader` pointer. The copy also keeps the bytes valid across
/// the result buffer's allocation, which can move a heap string. Payloads up
/// to 64 bytes are copied without allocating.
unsafe fn string_bytes(str_handle: i64) -> Option<crate::string::OwnedStringBytes> {
    let raw = str_handle as u64;
    if let Some(copy) = crate::string::with_string_value_bytes(
        f64::from_bits(raw),
        crate::string::OwnedStringBytes::copy_from_slice,
    ) {
        return Some(copy);
    }
    let addr = unbox_ptr(raw);
    if addr < 0x1000 {
        return None;
    }
    Some(crate::string::OwnedStringBytes::copy_from_header(
        addr as *const StringHeader,
    ))
}

fn throw_syntax(message: &[u8]) -> ! {
    let s = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_syntaxerror_new(s);
    crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *mut u8).bits()))
}

fn throw_type(message: &[u8]) -> ! {
    let s = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *mut u8).bits()))
}

/// Build a `{ read, written }` result object as Node returns for `setFrom*`.
unsafe fn read_written_object(read: usize, written: usize) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(js_object_alloc(0, 2));
    let read_key = crate::string::js_string_from_bytes(b"read".as_ptr(), 4);
    obj.with_mut_ptr(|obj| js_object_set_field_by_name(obj, read_key, read as f64));
    let written_key = crate::string::js_string_from_bytes(b"written".as_ptr(), 7);
    obj.with_mut_ptr(|obj| js_object_set_field_by_name(obj, written_key, written as f64));
    crate::value::js_nanbox_pointer(obj.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64)
}

/// Read the `alphabet` option ("base64" | "base64url") from an options object.
/// Returns true when base64url is requested. Non-object / missing → false.
unsafe fn opt_is_base64url(opts_bits: f64) -> bool {
    match opt_string_field(opts_bits, b"alphabet") {
        Some(s) => s == "base64url",
        None => false,
    }
}

unsafe fn opt_omit_padding(opts_bits: f64) -> bool {
    let raw = opts_bits.to_bits();
    if (raw >> 48) as u16 != 0x7FFD {
        return false;
    }
    let obj = (raw & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    if (obj as usize) < 0x1000 {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_const_ptr(obj);
    let key = crate::string::js_string_from_bytes(b"omitPadding".as_ptr(), 11);
    let val = obj.with_const_ptr(|obj| crate::object::js_object_get_field_by_name(obj, key));
    crate::value::js_is_truthy(f64::from_bits(val.bits())) != 0
}

/// `lastChunkHandling`: 0 = loose (default), 1 = strict, 2 = stop-before-partial.
unsafe fn opt_last_chunk_handling(opts_bits: f64) -> u8 {
    match opt_string_field(opts_bits, b"lastChunkHandling") {
        Some(s) if s == "strict" => 1,
        Some(s) if s == "stop-before-partial" => 2,
        _ => 0,
    }
}

unsafe fn opt_string_field(opts_bits: f64, name: &[u8]) -> Option<String> {
    let raw = opts_bits.to_bits();
    if (raw >> 48) as u16 != 0x7FFD {
        return None;
    }
    let obj = (raw & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    if (obj as usize) < 0x1000 {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_const_ptr(obj);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = obj.with_const_ptr(|obj| crate::object::js_object_get_field_by_name(obj, key));
    // Heap or inline SSO string (#11519): `"loose"` fits inline.
    crate::string::with_string_value_bytes(f64::from_bits(val.bits()), |bytes| {
        std::str::from_utf8(bytes).ok().map(str::to_string)
    })
    .flatten()
}

// ---------------------------------------------------------------------------
// Encoders.
// ---------------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> *mut StringHeader {
    let out_len = bytes.len() * 2;
    if out_len == 0 {
        return js_string_from_ascii_bytes(std::ptr::null(), 0);
    }
    let (hdr, dst) = js_string_alloc_ascii_uninit(out_len as u32);
    unsafe {
        for (i, &b) in bytes.iter().enumerate() {
            *dst.add(i * 2) = *HEX_ENCODE.get_unchecked((b >> 4) as usize);
            *dst.add(i * 2 + 1) = *HEX_ENCODE.get_unchecked((b & 0xF) as usize);
        }
    }
    hdr
}

fn base64_encode(bytes: &[u8], url: bool, omit_padding: bool) -> *mut StringHeader {
    let table = if url { URL_ENCODE } else { STD_ENCODE };
    let rem = bytes.len() % 3;
    let full_groups = bytes.len() / 3;
    let tail_len = match rem {
        0 => 0,
        _ => {
            if omit_padding {
                rem + 1 // 1 byte -> 2 chars, 2 bytes -> 3 chars
            } else {
                4
            }
        }
    };
    let out_len = full_groups * 4 + tail_len;
    if out_len == 0 {
        return js_string_from_ascii_bytes(std::ptr::null(), 0);
    }
    let (hdr, dst) = js_string_alloc_ascii_uninit(out_len as u32);
    unsafe {
        let mut i = 0usize;
        let mut o = 0usize;
        let triple_end = bytes.len() - rem;
        while i < triple_end {
            let a = *bytes.get_unchecked(i) as u32;
            let b = *bytes.get_unchecked(i + 1) as u32;
            let c = *bytes.get_unchecked(i + 2) as u32;
            let n = (a << 16) | (b << 8) | c;
            *dst.add(o) = *table.get_unchecked((n >> 18) as usize);
            *dst.add(o + 1) = *table.get_unchecked(((n >> 12) & 0x3F) as usize);
            *dst.add(o + 2) = *table.get_unchecked(((n >> 6) & 0x3F) as usize);
            *dst.add(o + 3) = *table.get_unchecked((n & 0x3F) as usize);
            i += 3;
            o += 4;
        }
        if rem == 1 {
            let a = *bytes.get_unchecked(i) as u32;
            let n = a << 16;
            *dst.add(o) = *table.get_unchecked((n >> 18) as usize);
            *dst.add(o + 1) = *table.get_unchecked(((n >> 12) & 0x3F) as usize);
            if !omit_padding {
                *dst.add(o + 2) = b'=';
                *dst.add(o + 3) = b'=';
            }
        } else if rem == 2 {
            let a = *bytes.get_unchecked(i) as u32;
            let b = *bytes.get_unchecked(i + 1) as u32;
            let n = (a << 16) | (b << 8);
            *dst.add(o) = *table.get_unchecked((n >> 18) as usize);
            *dst.add(o + 1) = *table.get_unchecked(((n >> 12) & 0x3F) as usize);
            *dst.add(o + 2) = *table.get_unchecked(((n >> 6) & 0x3F) as usize);
            if !omit_padding {
                *dst.add(o + 3) = b'=';
            }
        }
    }
    hdr
}

// ---------------------------------------------------------------------------
// Strict decoders (write into a caller-provided slice, bounded by `max`).
// ---------------------------------------------------------------------------

struct DecodeResult {
    read: usize,    // bytes consumed from the input string
    written: usize, // bytes written to the output
}

/// Strict TC39 hex decode. Throws SyntaxError on odd length or invalid char.
/// Writes up to `dst.len()` bytes; returns chars-read / bytes-written.
fn hex_decode_strict(input: &[u8], dst: &mut [u8]) -> Result<DecodeResult, &'static [u8]> {
    if !input.len().is_multiple_of(2) {
        return Err(b"Input string must contain hex characters in even length");
    }
    let mut written = 0usize;
    let mut read = 0usize;
    let mut i = 0usize;
    while i + 2 <= input.len() {
        let hi = HEX_DECODE[input[i] as usize];
        let lo = HEX_DECODE[input[i + 1] as usize];
        if hi == 255 || lo == 255 {
            return Err(b"Input string must contain only hex characters");
        }
        if written >= dst.len() {
            break;
        }
        dst[written] = (hi << 4) | lo;
        written += 1;
        read += 2;
        i += 2;
    }
    Ok(DecodeResult { read, written })
}

/// Strict TC39 base64 decode bounded by `dst.len()`.
///
/// `bounded` controls the partial-final-chunk behavior for `setFromBase64`:
/// when true, a final group whose decoded bytes would overflow `dst` is not
/// written at all (matching Node's chunk-boundary `{read, written}`).
fn base64_decode_strict(
    input: &[u8],
    url: bool,
    last_chunk: u8,
    dst: &mut [u8],
    bounded: bool,
) -> Result<DecodeResult, &'static [u8]> {
    let table = if url { &URL_DECODE } else { &STD_DECODE };
    // Collect the 6-bit values of significant characters, tracking input
    // offsets so `read` reflects characters consumed from the source string.
    let mut written = 0usize;
    // Sextet accumulator for the current 4-char group.
    let mut group: [u8; 4] = [0; 4];
    let mut group_len = 0usize;
    // Offset just past the last fully-consumed group.
    let mut last_group_end = 0usize;
    let mut i = 0usize;
    let mut saw_padding = false;
    let mut pad_count = 0usize;

    while i < input.len() {
        let b = input[i];
        i += 1;
        if is_b64_whitespace(b) {
            continue;
        }
        if b == b'=' {
            saw_padding = true;
            pad_count += 1;
            // Padding only valid in the last group (group_len 2 or 3).
            if group_len < 2 || pad_count > 2 {
                return Err(b"Invalid base64 padding");
            }
            if (group_len == 2 && pad_count == 2) || group_len == 3 {
                // Group complete with padding; flush the final partial group.
                break;
            }
            continue;
        }
        if saw_padding {
            return Err(b"Found a character after end of padding");
        }
        let v = table[b as usize];
        if v == 255 {
            return Err(b"Found a character that cannot be part of a valid base64 string.");
        }
        group[group_len] = v;
        group_len += 1;
        if group_len == 4 {
            // Emit a full 3-byte group if it fits.
            let out = [
                (group[0] << 2) | (group[1] >> 4),
                (group[1] << 4) | (group[2] >> 2),
                (group[2] << 6) | group[3],
            ];
            if written + 3 > dst.len() {
                if bounded {
                    // Stop before this group; do not consume it.
                    return Ok(DecodeResult {
                        read: last_group_end,
                        written,
                    });
                }
                // Unbounded (fromBase64): dst is sized exactly, so this is
                // unreachable, but be defensive.
                let room = dst.len() - written;
                dst[written..].copy_from_slice(&out[..room]);
                written += room;
                last_group_end = i;
                return Ok(DecodeResult {
                    read: last_group_end,
                    written,
                });
            }
            dst[written] = out[0];
            dst[written + 1] = out[1];
            dst[written + 2] = out[2];
            written += 3;
            group_len = 0;
            last_group_end = i;
        }
    }

    // Handle the trailing partial group (group_len 1..=3, or padded).
    if group_len == 0 {
        return Ok(DecodeResult {
            read: last_group_end,
            written,
        });
    }
    if group_len == 1 {
        // A single trailing sextet can't form a byte.
        if last_chunk == 1 {
            return Err(
                b"The base64 input terminates with a single character, excluding padding (=).",
            );
        }
        // loose / stop-before-partial: drop it.
        return Ok(DecodeResult {
            read: last_group_end,
            written,
        });
    }
    // group_len is 2 or 3.
    if last_chunk == 2 && !saw_padding {
        // stop-before-partial: leave the partial chunk unconsumed.
        return Ok(DecodeResult {
            read: last_group_end,
            written,
        });
    }
    if last_chunk == 1 && !saw_padding {
        return Err(b"Missing padding character in base64 string");
    }
    let produced = group_len - 1; // 2 sextets -> 1 byte, 3 sextets -> 2 bytes
    let out = [
        (group[0] << 2) | (group[1] >> 4),
        (group[1] << 4) | (group[2] >> 2),
    ];
    if last_chunk == 1 {
        // strict: the unused trailing bits must be zero.
        let extra_bits_zero = if group_len == 2 {
            (group[1] & 0x0F) == 0
        } else {
            (group[2] & 0x03) == 0
        };
        if !extra_bits_zero {
            return Err(b"The base64 input contains non-zero bits after the final character");
        }
    }
    let room = dst.len().saturating_sub(written);
    let to_write = produced.min(room);
    if bounded && to_write < produced {
        // Final partial group doesn't fully fit; write nothing more.
        return Ok(DecodeResult {
            read: last_group_end,
            written,
        });
    }
    dst[written..written + to_write].copy_from_slice(&out[..to_write]);
    written += to_write;
    Ok(DecodeResult { read: i, written })
}

/// Count the decoded byte length of a (validated) base64 string for the
/// unbounded `fromBase64` allocation. We over-allocate to the maximum and
/// trim afterwards, so a cheap upper bound suffices.
fn base64_max_bytes(input: &[u8]) -> usize {
    input.len().saturating_mul(3) / 4 + 3
}

// ---------------------------------------------------------------------------
// Public FFI entry points.
// ---------------------------------------------------------------------------

/// `Uint8Array.prototype.toBase64({ alphabet?, omitPadding? })`.
#[no_mangle]
pub extern "C" fn js_u8_to_base64(addr: i64, opts_bits: f64) -> *mut StringHeader {
    unsafe {
        let buf = buffer_from_addr(addr as usize);
        if buf.is_null() || (buf as usize) < 0x1000 {
            return js_string_from_ascii_bytes(std::ptr::null(), 0);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_u64(JSValue::pointer(buf as *mut u8).bits());
        let opts = scope.root_nanbox_u64(opts_bits.to_bits());
        let url = opt_is_base64url(f64::from_bits(opts.get_nanbox_u64()));
        let omit = opt_omit_padding(f64::from_bits(opts.get_nanbox_u64()));
        let bytes = super::bytes::ReadLease::new(f64::from_bits(receiver.get_nanbox_u64()))
            .unwrap_or_else(|_| throw_type(b"Invalid Uint8Array receiver"));
        base64_encode(&bytes, url, omit)
    }
}

/// `Uint8Array.prototype.toHex()`.
#[no_mangle]
pub extern "C" fn js_u8_to_hex(addr: i64) -> *mut StringHeader {
    unsafe {
        let buf = buffer_from_addr(addr as usize);
        if buf.is_null() || (buf as usize) < 0x1000 {
            return js_string_from_ascii_bytes(std::ptr::null(), 0);
        }
        let bytes = super::bytes::ReadLease::new(crate::value::js_nanbox_pointer(buf as i64))
            .unwrap_or_else(|_| throw_type(b"Invalid Uint8Array receiver"));
        hex_encode(&bytes)
    }
}

/// `Uint8Array.fromBase64(str, { alphabet?, lastChunkHandling? })`.
#[no_mangle]
pub extern "C" fn js_u8_from_base64(str_handle: i64, opts_bits: f64) -> *mut BufferHeader {
    unsafe {
        let Some(input) = string_bytes(str_handle) else {
            throw_type(b"input argument must be a string");
        };
        let input = input.as_bytes();
        let scope = crate::gc::RuntimeHandleScope::new();
        let opts = scope.root_nanbox_u64(opts_bits.to_bits());
        let url = opt_is_base64url(f64::from_bits(opts.get_nanbox_u64()));
        let last_chunk = opt_last_chunk_handling(f64::from_bits(opts.get_nanbox_u64()));
        let max = base64_max_bytes(input);
        let buf = buffer_alloc(max as u32);
        super::store::set_length(buf as usize, max as u32);
        let res = super::bytes::no_gc(|scope| {
            let dst = super::bytes::bytes_mut(crate::value::js_nanbox_pointer(buf as i64), scope)
                .unwrap();
            base64_decode_strict(input, url, last_chunk, dst, false)
        })
        .unwrap_or_else(|message| throw_syntax(message));
        super::store::set_length(buf as usize, res.written as u32);
        buf
    }
}

/// `Uint8Array.fromHex(str)`.
#[no_mangle]
pub extern "C" fn js_u8_from_hex(str_handle: i64) -> *mut BufferHeader {
    unsafe {
        let Some(input) = string_bytes(str_handle) else {
            throw_type(b"input argument must be a string");
        };
        let input = input.as_bytes();
        let max = input.len() / 2;
        let buf = buffer_alloc(max as u32);
        super::store::set_length(buf as usize, max as u32);
        let res = super::bytes::no_gc(|scope| {
            let dst = super::bytes::bytes_mut(crate::value::js_nanbox_pointer(buf as i64), scope)
                .unwrap();
            hex_decode_strict(input, dst)
        })
        .unwrap_or_else(|message| throw_syntax(message));
        super::store::set_length(buf as usize, res.written as u32);
        buf
    }
}

/// `Uint8Array.prototype.setFromBase64(str, { alphabet?, lastChunkHandling? })`.
/// Returns `{ read, written }`.
#[no_mangle]
pub extern "C" fn js_u8_set_from_base64(addr: i64, str_handle: i64, opts_bits: f64) -> f64 {
    unsafe {
        let buf = buffer_from_addr(addr as usize);
        if buf.is_null() || (buf as usize) < 0x1000 {
            return read_written_object(0, 0);
        }
        let receiver_scope = crate::gc::RuntimeHandleScope::new();
        let receiver = receiver_scope.root_nanbox_u64(JSValue::pointer(buf as *mut u8).bits());
        let Some(input) = string_bytes(str_handle) else {
            throw_type(b"input argument must be a string");
        };
        let input = input.as_bytes();
        let scope = crate::gc::RuntimeHandleScope::new();
        let opts = scope.root_nanbox_u64(opts_bits.to_bits());
        let url = opt_is_base64url(f64::from_bits(opts.get_nanbox_u64()));
        let last_chunk = opt_last_chunk_handling(f64::from_bits(opts.get_nanbox_u64()));
        let res = super::bytes::no_gc(|scope| {
            super::bytes::bytes_mut(f64::from_bits(receiver.get_nanbox_u64()), scope)
                .map(|dst| base64_decode_strict(input, url, last_chunk, dst, true))
        })
        .unwrap_or_else(|_| throw_type(b"Invalid Uint8Array receiver"))
        .unwrap_or_else(|message| throw_syntax(message));
        read_written_object(res.read, res.written)
    }
}

/// `Uint8Array.prototype.setFromHex(str)`. Returns `{ read, written }`.
#[no_mangle]
pub extern "C" fn js_u8_set_from_hex(addr: i64, str_handle: i64) -> f64 {
    unsafe {
        let buf = buffer_from_addr(addr as usize);
        if buf.is_null() || (buf as usize) < 0x1000 {
            return read_written_object(0, 0);
        }
        let receiver_scope = crate::gc::RuntimeHandleScope::new();
        let receiver = receiver_scope.root_nanbox_u64(JSValue::pointer(buf as *mut u8).bits());
        let Some(input) = string_bytes(str_handle) else {
            throw_type(b"input argument must be a string");
        };
        let input = input.as_bytes();
        let res = super::bytes::no_gc(|scope| {
            super::bytes::bytes_mut(f64::from_bits(receiver.get_nanbox_u64()), scope)
                .map(|dst| hex_decode_strict(input, dst))
        })
        .unwrap_or_else(|_| throw_type(b"Invalid Uint8Array receiver"))
        .unwrap_or_else(|message| throw_syntax(message));
        read_written_object(res.read, res.written)
    }
}

// Keepalive anchors: these `#[no_mangle]` symbols are only referenced from
// generated `.o` files, so the auto-optimize whole-program-LLVM bitcode
// rebuild would dead-strip them without an `#[used]` reference. See
// project_auto_optimize_keepalive_3320.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_U8_TO_BASE64: extern "C" fn(i64, f64) -> *mut StringHeader = js_u8_to_base64;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_U8_TO_HEX: extern "C" fn(i64) -> *mut StringHeader = js_u8_to_hex;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_U8_FROM_BASE64: extern "C" fn(i64, f64) -> *mut BufferHeader = js_u8_from_base64;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_U8_FROM_HEX: extern "C" fn(i64) -> *mut BufferHeader = js_u8_from_hex;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_U8_SET_FROM_BASE64: extern "C" fn(i64, i64, f64) -> f64 = js_u8_set_from_base64;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_U8_SET_FROM_HEX: extern "C" fn(i64, i64) -> f64 = js_u8_set_from_hex;
