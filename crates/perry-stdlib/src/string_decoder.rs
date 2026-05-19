//! StringDecoder — `node:string_decoder` real implementation.
//!
//! Issue #848. Pre-fix, `import { StringDecoder } from "node:string_decoder"`
//! plus `new StringDecoder("utf8")` flowed through the generic
//! `lower_new` placeholder (`js_object_alloc(0, 0)`) — `typeof dec === "object"`
//! held, but `typeof dec.write` was `"undefined"` because the placeholder
//! ObjectHeader had no method or property slots. This module supplies:
//!
//!   * `js_string_decoder_new(encoding_ptr)` — allocates a real
//!     `StringDecoderHandle` (incremental UTF-8 decoder with `lastNeed` /
//!     `lastTotal` / `lastChar` state) and returns the registry id.
//!     `lower_call/builtin.rs` NaN-boxes the result with `POINTER_TAG`.
//!   * `dispatch_string_decoder` (`write` / `end`) — wired into
//!     `common/dispatch.rs::js_handle_method_dispatch` so that
//!     `dec.write(buf)` / `dec.end(buf?)` on an any-typed receiver hits
//!     the runtime impl.
//!   * `dispatch_string_decoder_property` (`lastNeed` / `lastTotal` /
//!     `lastChar`) — wired into `js_handle_property_dispatch` so the
//!     state fields read as Node returns them.
//!
//! Each non-UTF-8 mode has its own incremental state: `utf16le` buffers
//! the odd trailing byte so a 2-byte code unit split across writes
//! still decodes correctly; `base64` buffers up to 2 unencoded bytes
//! so a chunk that isn't a multiple of 3 carries the leftover into
//! the next write. `hex` / `latin1` / `ascii` are stateless.

use crate::common::handle::{get_handle_mut, register_handle, with_handle, Handle};
use perry_runtime::buffer::{is_registered_buffer, BufferHeader};
use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

/// Which textual encoding the StringDecoder was constructed with.
/// Determines how `write`/`end` interpret the incoming bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DecodingMode {
    Utf8,
    Utf16Le,
    Base64,
    Hex,
    Latin1,
    Ascii,
}

/// Incremental decoder state, generalised across encodings. UTF-8 uses
/// `last_*`; UTF-16LE uses `utf16_partial`; Base64 uses `base64_partial`.
/// Each mode only touches its own fields, so they don't interact.
pub struct StringDecoderHandle {
    mode: DecodingMode,
    /// UTF-8: number of bytes still needed to complete the current code
    /// point (0 when no partial point is buffered).
    last_need: u8,
    /// UTF-8: total byte length of the in-progress code point (2, 3, or 4).
    last_total: u8,
    /// UTF-8: up to 4 bytes of partial code point captured from prior writes.
    last_char: [u8; 4],
    /// UTF-8: how many bytes of `last_char` are valid (= last_total -
    /// last_need at the time the partial was captured; never larger than 4).
    last_char_len: u8,
    /// UTF-16LE: at most 1 trailing byte buffered for the next write.
    /// `Some(b)` means an odd-length write ended with `b` as the low byte
    /// of an unfinished code unit. `None` means clean state.
    utf16_partial: Option<u8>,
    /// Base64: 0..=2 buffered bytes that didn't fit into the last 3-byte
    /// chunk. Re-prefixed onto the next `write` before encoding.
    base64_partial: Vec<u8>,
}

impl Default for StringDecoderHandle {
    fn default() -> Self {
        Self::with_mode(DecodingMode::Utf8)
    }
}

impl StringDecoderHandle {
    pub fn new() -> Self {
        Self::with_mode(DecodingMode::Utf8)
    }

    pub fn with_mode(mode: DecodingMode) -> Self {
        StringDecoderHandle {
            mode,
            last_need: 0,
            last_total: 0,
            last_char: [0; 4],
            last_char_len: 0,
            utf16_partial: None,
            base64_partial: Vec::new(),
        }
    }
}

/// Parse Node's encoding-name normalisation (case-insensitive, hyphens
/// optional). Returns `Utf8` for unknown/`undefined` to match the previous
/// default — that mirrors Node's `normalizeEncoding` returning undefined
/// (then `new StringDecoder` throws), but Perry's existing callers expect
/// utf-8 as a forgiving default; keep that for now.
fn parse_encoding(name: &str) -> DecodingMode {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "utf8" | "utf-8" => DecodingMode::Utf8,
        "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => DecodingMode::Utf16Le,
        "base64" | "base64url" => DecodingMode::Base64,
        "hex" => DecodingMode::Hex,
        "latin1" | "binary" => DecodingMode::Latin1,
        "ascii" => DecodingMode::Ascii,
        _ => DecodingMode::Utf8,
    }
}

/// Extract the encoding name from the NaN-boxed argument passed by
/// codegen. Codegen sends the raw bits as i64 (via `unbox_to_i64`) so we
/// reconstruct the string pointer from the low 48 bits. STRING_TAG and
/// POINTER_TAG both keep the address there; SHORT_STRING_TAG can be
/// detected from the top 16 bits.
unsafe fn encoding_name_from_bits(bits: i64) -> Option<String> {
    let u = bits as u64;
    let top16 = u >> 48;
    // SHORT_STRING_TAG = 0x7FFA. Payload is bytes inline in the
    // remaining 48 bits, length in bits 44..47 of the top 16.
    if top16 == 0x7FFA {
        let len = ((u >> 44) & 0xF) as usize;
        if len == 0 || len > 6 {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, b) in bytes.iter_mut().enumerate().take(len) {
            *b = ((u >> (i * 8)) & 0xFF) as u8;
        }
        return Some(String::from_utf8_lossy(&bytes[..len]).into_owned());
    }
    // STRING_TAG / POINTER_TAG / raw pointer — all keep the heap address
    // in the low 48 bits.
    let addr = (u & 0x0000_FFFF_FFFF_FFFF) as usize;
    if addr < 0x1000 {
        return None;
    }
    let hdr = addr as *const StringHeader;
    let len = (*hdr).byte_len as usize;
    if len == 0 || len > 32 {
        return None;
    }
    let data = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Detect a multi-byte UTF-8 lead in the final 0–3 bytes of `buf`.
/// Returns the number of bytes that should be buffered for the next
/// write (so they aren't returned as garbled output). Mirrors the
/// `utf8CheckIncomplete` function in Node's `lib/string_decoder.js`.
fn utf8_check_incomplete(state: &mut StringDecoderHandle, buf: &[u8]) -> usize {
    let mut i = buf.len();
    // Walk back from the end of the buffer up to 3 bytes — the longest
    // UTF-8 lead sequence the trailing bytes could need to wait for.
    let walk = if buf.len() >= 3 { 3 } else { buf.len() };
    let mut steps = 0usize;
    while steps < walk {
        i -= 1;
        steps += 1;
        let b = buf[i];
        // Continuation byte 10xxxxxx — keep walking.
        if (b & 0xC0) == 0x80 {
            continue;
        }
        // 4-byte lead 11110xxx.
        if (b & 0xF8) == 0xF0 {
            // We've already walked `steps - 1` continuation bytes plus
            // this lead; we need 4 total, so we still need
            // `4 - steps` bytes.
            if steps < 4 {
                state.last_need = (4 - steps) as u8;
                state.last_total = 4;
                let start = buf.len() - steps;
                state.last_char_len = steps as u8;
                state.last_char[..steps].copy_from_slice(&buf[start..]);
                return steps;
            }
            return 0;
        }
        // 3-byte lead 1110xxxx.
        if (b & 0xF0) == 0xE0 {
            if steps < 3 {
                state.last_need = (3 - steps) as u8;
                state.last_total = 3;
                let start = buf.len() - steps;
                state.last_char_len = steps as u8;
                state.last_char[..steps].copy_from_slice(&buf[start..]);
                return steps;
            }
            return 0;
        }
        // 2-byte lead 110xxxxx.
        if (b & 0xE0) == 0xC0 {
            if steps < 2 {
                state.last_need = (2 - steps) as u8;
                state.last_total = 2;
                let start = buf.len() - steps;
                state.last_char_len = steps as u8;
                state.last_char[..steps].copy_from_slice(&buf[start..]);
                return steps;
            }
            return 0;
        }
        // ASCII byte 0xxxxxxx — nothing to buffer.
        return 0;
    }
    0
}

/// Decode `bytes` against the existing partial-codepoint state, mutating
/// `state` to reflect any new trailing partial. Returns the decoded
/// string. UTF-8 invalid sequences are replaced with U+FFFD, matching
/// Node's `lossy` UTF-8 decoder behavior.
fn write_utf8(state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    let mut out = String::new();

    // Stitch the buffered partial together with the new input first.
    if state.last_need > 0 {
        let need = state.last_need as usize;
        if bytes.len() < need {
            // Still incomplete — append what we can and exit empty.
            let new_len = state.last_char_len as usize + bytes.len();
            if new_len <= 4 {
                state.last_char[state.last_char_len as usize..new_len].copy_from_slice(bytes);
                state.last_char_len = new_len as u8;
                state.last_need -= bytes.len() as u8;
            } else {
                // Defensive: should never happen given UTF-8 is at most 4
                // bytes, but if upstream feeds garbage we reset rather
                // than overrun.
                state.last_need = 0;
                state.last_total = 0;
                state.last_char_len = 0;
            }
            return out;
        }

        // We have enough new bytes to complete the buffered point.
        let total = state.last_total as usize;
        let buffered = state.last_char_len as usize;
        let take_new = total - buffered;
        let mut cp = Vec::with_capacity(total);
        cp.extend_from_slice(&state.last_char[..buffered]);
        cp.extend_from_slice(&bytes[..take_new]);

        match std::str::from_utf8(&cp) {
            Ok(s) => out.push_str(s),
            Err(_) => out.push('\u{FFFD}'),
        }
        state.last_need = 0;
        state.last_total = 0;
        state.last_char_len = 0;

        // The "rest" continues below — chop off the consumed prefix.
        let rest = &bytes[take_new..];
        // Recurse on the tail so trailing partials get caught.
        out.push_str(&write_utf8_tail(state, rest));
        return out;
    }

    out.push_str(&write_utf8_tail(state, bytes));
    out
}

/// Tail half of `write_utf8`: assumes `state.last_need == 0` on entry.
/// Splits a trailing incomplete code point off into `state`.
fn write_utf8_tail(state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let trail = utf8_check_incomplete(state, bytes);
    let head = &bytes[..bytes.len() - trail];
    String::from_utf8_lossy(head).into_owned()
}

/// `decoder.end([buf?])` — flush any incomplete state as U+FFFD, matching
/// Node's behavior.
fn end_utf8(state: &mut StringDecoderHandle, bytes: Option<&[u8]>) -> String {
    let mut out = match bytes {
        Some(b) => write_utf8(state, b),
        None => String::new(),
    };
    if state.last_need > 0 {
        out.push('\u{FFFD}');
        state.last_need = 0;
        state.last_total = 0;
        state.last_char_len = 0;
    }
    out
}

/// UTF-16LE write: pair bytes as little-endian u16 code units. The last
/// odd byte (if any) is buffered into `utf16_partial`.
fn write_utf16le(state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    let mut combined: Vec<u8> =
        Vec::with_capacity(bytes.len() + if state.utf16_partial.is_some() { 1 } else { 0 });
    if let Some(b) = state.utf16_partial.take() {
        combined.push(b);
    }
    combined.extend_from_slice(bytes);
    // Even number of bytes → consume all; odd → carry the last byte.
    let take = combined.len() & !1; // round down to even
    let trail = combined.len() - take;
    if trail == 1 {
        state.utf16_partial = Some(combined[take]);
    }
    let head = &combined[..take];
    let mut out = String::with_capacity(take / 2);
    let mut iter = head.chunks_exact(2);
    let mut high_surrogate: Option<u16> = None;
    for pair in iter.by_ref() {
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if let Some(h) = high_surrogate.take() {
            // Expecting a low surrogate to pair with the buffered high.
            if (0xDC00..=0xDFFF).contains(&unit) {
                let cp = 0x10000 + (((h - 0xD800) as u32) << 10) + ((unit - 0xDC00) as u32);
                if let Some(c) = char::from_u32(cp) {
                    out.push(c);
                } else {
                    out.push('\u{FFFD}');
                }
            } else {
                // Lone high → replacement, then reprocess this unit.
                out.push('\u{FFFD}');
                process_utf16_unit(&mut out, &mut high_surrogate, unit);
            }
        } else {
            process_utf16_unit(&mut out, &mut high_surrogate, unit);
        }
    }
    // A still-pending high surrogate is rare (would need an odd number of
    // surrogate pairs straddling writes) — Node lets it ride to the next
    // write/end too. We don't track it across writes; flush as replacement
    // here.
    if high_surrogate.is_some() {
        out.push('\u{FFFD}');
    }
    out
}

fn process_utf16_unit(out: &mut String, high: &mut Option<u16>, unit: u16) {
    match unit {
        0xD800..=0xDBFF => *high = Some(unit),
        0xDC00..=0xDFFF => out.push('\u{FFFD}'),
        _ => out.push(char::from_u32(unit as u32).unwrap_or('\u{FFFD}')),
    }
}

fn end_utf16le(state: &mut StringDecoderHandle, bytes: Option<&[u8]>) -> String {
    let out = match bytes {
        Some(b) => write_utf16le(state, b),
        None => String::new(),
    };
    // Any leftover lone byte at end is dropped (matches Node — the trailing
    // odd byte produces no character).
    state.utf16_partial = None;
    out
}

/// Base64 alphabet for `STANDARD` (RFC 4648), matching Node's
/// `base64` (not `base64url`).
const B64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64_chunk(triplet: &[u8], out: &mut String) {
    // Encodes exactly 3 bytes into 4 base64 chars (full chunk).
    let b0 = triplet[0];
    let b1 = triplet[1];
    let b2 = triplet[2];
    out.push(B64_ALPHABET[(b0 >> 2) as usize] as char);
    out.push(B64_ALPHABET[(((b0 & 0x3) << 4) | (b1 >> 4)) as usize] as char);
    out.push(B64_ALPHABET[(((b1 & 0xF) << 2) | (b2 >> 6)) as usize] as char);
    out.push(B64_ALPHABET[(b2 & 0x3F) as usize] as char);
}

fn encode_base64_tail(tail: &[u8], out: &mut String) {
    // Final 1 or 2 bytes — produces 2 or 3 base64 chars + `=` padding to 4.
    match tail.len() {
        1 => {
            let b0 = tail[0];
            out.push(B64_ALPHABET[(b0 >> 2) as usize] as char);
            out.push(B64_ALPHABET[((b0 & 0x3) << 4) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let b0 = tail[0];
            let b1 = tail[1];
            out.push(B64_ALPHABET[(b0 >> 2) as usize] as char);
            out.push(B64_ALPHABET[(((b0 & 0x3) << 4) | (b1 >> 4)) as usize] as char);
            out.push(B64_ALPHABET[((b1 & 0xF) << 2) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
}

/// Base64 write: encode the bytes as base64 (this is the *encode*
/// direction — Node's `StringDecoder('base64')` turns binary input into
/// base64 text). Buffer 0..2 bytes if the running total isn't a multiple
/// of 3 so the next `write` can resume encoding cleanly.
fn write_base64(state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    let mut combined: Vec<u8> = Vec::with_capacity(state.base64_partial.len() + bytes.len());
    combined.extend_from_slice(&state.base64_partial);
    combined.extend_from_slice(bytes);
    state.base64_partial.clear();

    let take = (combined.len() / 3) * 3;
    let trail = &combined[take..];
    state.base64_partial.extend_from_slice(trail);

    let mut out = String::with_capacity((take / 3) * 4);
    for chunk in combined[..take].chunks_exact(3) {
        encode_base64_chunk(chunk, &mut out);
    }
    out
}

fn end_base64(state: &mut StringDecoderHandle, bytes: Option<&[u8]>) -> String {
    let mut out = match bytes {
        Some(b) => write_base64(state, b),
        None => String::new(),
    };
    if !state.base64_partial.is_empty() {
        encode_base64_tail(&state.base64_partial, &mut out);
        state.base64_partial.clear();
    }
    out
}

/// Hex encoding: each byte → two lowercase hex chars. Stateless.
fn write_hex(_state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0xF) as u32, 16).unwrap());
    }
    out
}

/// Latin-1 / binary: each byte maps 1:1 to a Unicode codepoint in
/// 0..=255. UTF-8 encode each char individually so the resulting String
/// is valid UTF-8.
fn write_latin1(_state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        out.push(b as char);
    }
    out
}

/// ASCII: each byte masked to 7 bits, then mapped to a char. Anything
/// above 0x7F gets stripped to 0..=0x7F per Node's behaviour.
fn write_ascii(_state: &mut StringDecoderHandle, bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        out.push((b & 0x7F) as char);
    }
    out
}

/// Extract bytes from a NaN-boxed f64 that may carry either a BufferHeader
/// or a StringHeader pointer. Mirrors `bytes_from_ptr` in crypto.rs but
/// takes the NaN-boxed `f64` directly so dispatch arms can pass `args[0]`
/// without manual unboxing.
unsafe fn bytes_from_nanboxed(value: f64) -> Vec<u8> {
    let bits = value.to_bits();
    // POINTER_TAG / STRING_TAG both keep the address in the low 48 bits.
    let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if addr < 0x1000 {
        return Vec::new();
    }
    if is_registered_buffer(addr) {
        let buf = addr as *const BufferHeader;
        let len = (*buf).length as usize;
        let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
        return std::slice::from_raw_parts(data, len).to_vec();
    }
    // Fall back to StringHeader layout — calling `dec.write("abc")` with
    // a literal string is uncommon but valid (Node coerces strings to
    // Buffers via the encoding); the byte_len slot lines up here.
    let hdr = addr as *const StringHeader;
    let len = (*hdr).byte_len as usize;
    let data = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
    std::slice::from_raw_parts(data, len).to_vec()
}

/// `new StringDecoder(encoding)` — allocate a real StringDecoderHandle.
///
/// `encoding_bits` arrives as `i64` carrying the raw bits of the NaN-boxed
/// encoding argument (the codegen unboxed-to-i64 via `unbox_to_i64`).
/// Supported encodings are `utf8` / `utf-8`, `utf16le` / `ucs2`, `base64`,
/// `hex`, `latin1` / `binary`, and `ascii`. Anything else defaults to
/// UTF-8 (Node throws there, but the previous Perry behaviour was a
/// forgiving fallback — keep that until callers prove a stricter default
/// is wanted).
#[no_mangle]
pub unsafe extern "C" fn js_string_decoder_new(encoding_bits: i64) -> i64 {
    let mode = match encoding_name_from_bits(encoding_bits) {
        Some(name) => parse_encoding(&name),
        None => DecodingMode::Utf8,
    };
    register_handle(StringDecoderHandle::with_mode(mode))
}

/// Direct FFI for `decoder.write(buf)`. Used by the static
/// NATIVE_MODULE_TABLE dispatch arm (typed receiver path:
/// `const d = new StringDecoder("utf8"); d.write(buf)` where the HIR
/// captured `d`'s native-instance class). Receives a NaN-unboxed handle
/// (i64) for the receiver and a NaN-boxed (f64) buffer argument; the
/// return is a NaN-boxed (f64) string. Matches the
/// `(NA_F64) → NR_STR` shape declared in `NATIVE_MODULE_TABLE` — except
/// we return a String via STRING_TAG-NaN-boxed bits, which is what
/// `NR_F64` expects (NR_STR would do its own NaN-box on a raw pointer
/// and we'd double-box).
#[no_mangle]
pub unsafe extern "C" fn js_string_decoder_write(handle: i64, buf: f64) -> f64 {
    dispatch_string_decoder(handle, "write", &[buf])
}

/// Direct FFI for `decoder.end(buf?)`. See `js_string_decoder_write` for
/// the call shape rationale. `buf` defaults to `undefined` (NaN-boxed)
/// when the user calls `d.end()` with no args — the dispatch impl
/// interprets that as "no buffer, just flush partial state".
#[no_mangle]
pub unsafe extern "C" fn js_string_decoder_end(handle: i64, buf: f64) -> f64 {
    let bits = buf.to_bits();
    if bits == JSValue::undefined().bits() || bits == JSValue::null().bits() {
        dispatch_string_decoder(handle, "end", &[])
    } else {
        dispatch_string_decoder(handle, "end", &[buf])
    }
}

/// Detect whether `handle` belongs to the StringDecoder registry. Used by
/// `common/dispatch.rs` to gate the dispatch arms — the global HANDLES
/// space is shared across stdlib classes and we don't want to claim a
/// foreign handle id whose method name happens to overlap.
pub fn is_string_decoder_handle(handle: i64) -> bool {
    with_handle::<StringDecoderHandle, bool, _>(handle, |_| true).unwrap_or(false)
}

/// Dispatch `write` / `end` method calls. Called from
/// `common/dispatch.rs::js_handle_method_dispatch` after the handle is
/// confirmed to live in the StringDecoder registry.
///
/// Returns NaN-boxed string values (STRING_TAG); `end()` with no args
/// flushes any partial-codepoint state as U+FFFD per Node semantics.
pub unsafe fn dispatch_string_decoder(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<StringDecoderHandle>(handle) {
        Some(h) => h,
        // undefined — caller already gated on is_string_decoder_handle,
        // so this is a defensive return for race conditions.
        None => return f64::from_bits(JSValue::undefined().bits()),
    };

    match method {
        "write" => {
            let bytes = if args.is_empty() {
                Vec::new()
            } else {
                bytes_from_nanboxed(args[0])
            };
            let s = match h.mode {
                DecodingMode::Utf8 => write_utf8(h, &bytes),
                DecodingMode::Utf16Le => write_utf16le(h, &bytes),
                DecodingMode::Base64 => write_base64(h, &bytes),
                DecodingMode::Hex => write_hex(h, &bytes),
                DecodingMode::Latin1 => write_latin1(h, &bytes),
                DecodingMode::Ascii => write_ascii(h, &bytes),
            };
            let sh = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(0x7FFF_0000_0000_0000u64 | ((sh as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "end" => {
            let bytes_opt = if args.is_empty() {
                None
            } else {
                let bits = args[0].to_bits();
                // undefined / null → no buffer, just flush.
                if bits == JSValue::undefined().bits() || bits == JSValue::null().bits() {
                    None
                } else {
                    Some(bytes_from_nanboxed(args[0]))
                }
            };
            let bytes_ref = bytes_opt.as_deref();
            let s = match h.mode {
                DecodingMode::Utf8 => end_utf8(h, bytes_ref),
                DecodingMode::Utf16Le => end_utf16le(h, bytes_ref),
                DecodingMode::Base64 => end_base64(h, bytes_ref),
                // Hex / Latin1 / Ascii have no carry-over state — `end`
                // is just a `write` with no trailing flush.
                DecodingMode::Hex => match bytes_ref {
                    Some(b) => write_hex(h, b),
                    None => String::new(),
                },
                DecodingMode::Latin1 => match bytes_ref {
                    Some(b) => write_latin1(h, b),
                    None => String::new(),
                },
                DecodingMode::Ascii => match bytes_ref {
                    Some(b) => write_ascii(h, b),
                    None => String::new(),
                },
            };
            let sh = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(0x7FFF_0000_0000_0000u64 | ((sh as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

/// Dispatch property access for `write` / `end` (returns a bound-method
/// closure so `typeof dec.write === "function"`) and the state getters
/// `lastNeed` / `lastTotal` / `lastChar`. Called from
/// `common/dispatch.rs::js_handle_property_dispatch` after the handle is
/// confirmed to live in the StringDecoder registry.
///
/// `lastChar` returns a `Buffer` (BufferHeader pointer) holding the four
/// bytes of partial-codepoint storage, matching Node — its `last_char_len`
/// bytes are valid; the rest are zero. We always return a 4-byte buffer
/// so user code can index it without bounds checks, same as Node.
///
/// `write` / `end` reads return a bound-method closure built by
/// `js_class_method_bind`. When invoked the closure routes through
/// `js_native_call_method`, which strips the POINTER_TAG, sees a small
/// handle, and dispatches back to `dispatch_string_decoder` via
/// `HANDLE_METHOD_DISPATCH` — the exact path `dec.write(buf)` takes
/// when called inline. So `const w = dec.write; w(buf)` works too.
pub unsafe fn dispatch_string_decoder_property(handle: i64, property: &str) -> f64 {
    let h = match get_handle_mut::<StringDecoderHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(JSValue::undefined().bits()),
    };

    match property {
        "lastNeed" => f64::from(h.last_need as i32),
        "lastTotal" => f64::from(h.last_total as i32),
        "lastChar" => {
            let buf = perry_runtime::buffer::buffer_alloc(4);
            if buf.is_null() {
                return f64::from_bits(JSValue::undefined().bits());
            }
            (*buf).length = 4;
            let dst = perry_runtime::buffer::buffer_data_mut(buf);
            std::ptr::copy_nonoverlapping(h.last_char.as_ptr(), dst, 4);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "write" | "end" => {
            // Build a bound-method closure whose `this` is the
            // POINTER_TAG-NaN-boxed handle. The closure captures the
            // method-name byte pointer + length verbatim — we leak a
            // small static so the pointer stays valid for the closure's
            // lifetime. Two names total (`write`, `end`) so the leak
            // is bounded.
            let name_bytes: &'static [u8] = if property == "write" {
                b"write"
            } else {
                b"end"
            };
            let this_f64 = f64::from_bits(
                0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF),
            );
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
        }
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_euro_sign() {
        // U+20AC EURO SIGN = E2 82 AC in UTF-8.
        let mut s = StringDecoderHandle::new();
        let a = write_utf8(&mut s, &[0xE2, 0x82]);
        assert_eq!(a, "");
        assert_eq!(s.last_need, 1);
        assert_eq!(s.last_total, 3);
        let b = write_utf8(&mut s, &[0xAC]);
        assert_eq!(b, "\u{20AC}");
        assert_eq!(s.last_need, 0);
    }

    #[test]
    fn split_emoji() {
        // U+1F600 GRINNING FACE = F0 9F 98 80 in UTF-8 (4 bytes).
        let mut s = StringDecoderHandle::new();
        assert_eq!(write_utf8(&mut s, &[0xF0, 0x9F]), "");
        assert_eq!(write_utf8(&mut s, &[0x98]), "");
        assert_eq!(write_utf8(&mut s, &[0x80]), "\u{1F600}");
    }

    #[test]
    fn end_flushes_partial_as_replacement() {
        let mut s = StringDecoderHandle::new();
        write_utf8(&mut s, &[0xE2, 0x82]);
        let final_str = end_utf8(&mut s, None);
        assert_eq!(final_str, "\u{FFFD}");
    }

    #[test]
    fn complete_codepoint_round_trip() {
        let mut s = StringDecoderHandle::new();
        assert_eq!(write_utf8(&mut s, "hello".as_bytes()), "hello");
        assert_eq!(s.last_need, 0);
    }
}
