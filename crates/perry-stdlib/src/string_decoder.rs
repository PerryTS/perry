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
//! Only UTF-8 is implemented. Other encodings (utf16le, base64, hex,
//! latin1, ascii) fall back to UTF-8 — calling code that actually
//! depends on a non-UTF-8 mode would already need a bigger
//! `string_decoder` port. The byte-by-byte split-codepoint case
//! (`[0xE2, 0x82, 0xAC]` across two `write` calls) is the one Node
//! actually documents and that compile-as-package npm libraries
//! (readable-stream, undici, etc.) exercise — that's what `lastNeed` /
//! `lastTotal` / `lastChar` track, and that's the path verified in the
//! repro.

use crate::common::handle::{get_handle_mut, register_handle, with_handle, Handle};
use perry_runtime::buffer::{is_registered_buffer, BufferHeader};
use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

/// UTF-8 incremental decoder state. Mirrors Node's `StringDecoder`
/// (`lib/string_decoder.js`): `last_*` fields buffer a partial code point
/// across `write()` boundaries so a single emoji split across multiple
/// chunks decodes to one character on the final `write`/`end`.
pub struct StringDecoderHandle {
    /// Number of bytes still needed to complete the current code point
    /// (0 when no partial point is buffered).
    last_need: u8,
    /// Total byte length of the in-progress code point (2, 3, or 4).
    last_total: u8,
    /// Up to 4 bytes of partial code point captured from prior writes.
    last_char: [u8; 4],
    /// How many bytes of `last_char` are valid (= last_total - last_need
    /// at the time the partial was captured; never larger than 4).
    last_char_len: u8,
}

impl Default for StringDecoderHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl StringDecoderHandle {
    pub fn new() -> Self {
        StringDecoderHandle {
            last_need: 0,
            last_total: 0,
            last_char: [0; 4],
            last_char_len: 0,
        }
    }
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
/// `encoding_ptr` arrives as `i64` (NaN-unboxed by the codegen). Currently
/// only utf-8 state is tracked; other encodings would need their own
/// decoders. The arg is accepted (and consumed for side effects on the
/// codegen side) but doesn't affect behavior in this build.
#[no_mangle]
pub unsafe extern "C" fn js_string_decoder_new(_encoding_ptr: i64) -> i64 {
    register_handle(StringDecoderHandle::new())
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
            let s = write_utf8(h, &bytes);
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
            let s = end_utf8(h, bytes_opt.as_deref());
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
