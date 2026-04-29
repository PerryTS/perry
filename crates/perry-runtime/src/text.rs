//! TextEncoder / TextDecoder runtime.
//!
//! `js_text_encoder_encode` / `js_text_decoder_decode` return an ArrayHeader
//! (with `f64` elements) so the inline `encoded[i]` / `encoded.length`
//! fast path works. We allocate a proper `ArrayHeader`,
//! widen each UTF-8 byte to f64, AND register the pointer in the buffer
//! registry so `encoded instanceof Uint8Array` still returns true.
//!
//! `TextEncoder` / `TextDecoder` are stateless wrappers — the encoder is
//! always UTF-8, so we return a small sentinel integer NaN-boxed as a
//! pointer on the codegen side. The runtime doesn't need per-instance state.

use crate::array::{js_array_alloc, ArrayHeader};
use crate::buffer::{register_buffer, BufferHeader};
use crate::string::{js_string_from_bytes, StringHeader};

/// `new TextEncoder()` — returns a non-null sentinel integer pointer.
///
/// The returned value is a small integer (`1`) that the codegen NaN-boxes
/// with `POINTER_TAG`. TextEncoder has no state beyond "I encode UTF-8",
/// so any non-null sentinel works. We use a distinct value from the
/// decoder sentinel purely for debuggability.
#[no_mangle]
pub extern "C" fn js_text_encoder_new() -> i64 {
    1
}

/// `new TextDecoder()` — returns a non-null sentinel integer pointer.
#[no_mangle]
pub extern "C" fn js_text_decoder_new() -> i64 {
    2
}

/// `encoder.encode(str)` — UTF-8 encode `value` into an `ArrayHeader`.
///
/// Takes a NaN-boxed f64 string value. Returns an i64 pointer to a
/// freshly allocated `ArrayHeader` holding one `f64` per UTF-8 byte.
/// The pointer is ALSO registered in the buffer registry so downstream
/// `instanceof Uint8Array` checks return true.
///
/// The returned i64 is the raw `ArrayHeader*` — the codegen NaN-boxes it
/// with `POINTER_TAG` before handing it to user code.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode_llvm(value: f64) -> i64 {
    let str_ptr_i = crate::value::js_get_string_pointer_unified(value);
    let (data_ptr, len) = if str_ptr_i == 0 {
        (std::ptr::null::<u8>(), 0usize)
    } else {
        let str_ptr = str_ptr_i as *const StringHeader;
        unsafe {
            let l = (*str_ptr).byte_len as usize;
            let d = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            (d, l)
        }
    };

    // Allocate an ArrayHeader with `len` capacity. js_array_alloc enforces
    // a minimum of MIN_ARRAY_CAPACITY (16), but that's fine — the length
    // field is what matters for bounds checks and `.length`.
    let arr = js_array_alloc(len as u32);
    unsafe {
        (*arr).length = len as u32;
        if len > 0 {
            // Write each byte as an f64 at offset 8 + i*8.
            let elems = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for i in 0..len {
                let byte = *data_ptr.add(i);
                *elems.add(i) = byte as f64;
            }
        }
    }

    // Register so `instanceof Uint8Array` works.
    register_buffer(arr as *const BufferHeader);
    // Remember that this pointer holds f64-encoded bytes (not packed u8)
    // so the decoder knows how to read it.
    remember_text_encoder_result(arr as usize);

    arr as i64
}

/// `decoder.decode(buf)` — UTF-8 decode a NaN-boxed value holding either
/// a text-encoder-produced `ArrayHeader` (with f64 bytes) or a real
/// `BufferHeader` (with packed u8 bytes, e.g. from `new Uint8Array([...])`).
///
/// Returns a `*const StringHeader` as i64 — the codegen NaN-boxes with
/// `STRING_TAG`.
///
/// Dispatch strategy: check the buffer registry. If the pointer isn't
/// registered, treat it as an ArrayHeader. If registered, we need to
/// distinguish between a "real" Uint8Array (BufferHeader with u8 bytes)
/// and our TextEncoder output (ArrayHeader with f64 bytes that we also
/// registered). We disambiguate via the `capacity` field:
/// - BufferHeader capacity == byte length (packed bytes)
/// - ArrayHeader capacity is f64-count (rounded up to MIN_ARRAY_CAPACITY)
///
/// A cleaner disambiguation: check whether reading past the header yields
/// "looks like f64-encoded bytes (0..=255)" — but that's brittle. Instead,
/// we track the ArrayHeader-backed registrations in a separate thread-local
/// set (`TEXT_ENCODER_RESULTS`) maintained by `js_text_encoder_encode_llvm`.
#[no_mangle]
pub extern "C" fn js_text_decoder_decode_llvm(value: f64) -> i64 {
    let bits = value.to_bits();

    // Unbox the pointer. Accept both POINTER_TAG NaN-boxing and raw small
    // pointer fallback (covers both `encoded` values and `new Uint8Array(...)`
    // bitcast results).
    let ptr_usize: usize = {
        const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
        const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
        if (bits & TAG_MASK) == POINTER_TAG {
            (bits & POINTER_MASK) as usize
        } else if !value.is_nan() && bits != 0 && bits < 0x0001_0000_0000_0000 {
            bits as usize
        } else {
            0
        }
    };

    if ptr_usize == 0 || ptr_usize < 0x1000 {
        // Empty or invalid — return empty string.
        return js_string_from_bytes(std::ptr::null(), 0) as i64;
    }

    // Try the TextEncoder-allocated ArrayHeader path first. If this
    // pointer was produced by `js_text_encoder_encode_llvm`, the bytes
    // are stored as f64 elements at offset 8.
    if is_text_encoder_result(ptr_usize) {
        unsafe {
            let arr = ptr_usize as *const ArrayHeader;
            let len = (*arr).length as usize;
            let elems = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let mut bytes = Vec::with_capacity(len);
            for i in 0..len {
                let d = *elems.add(i);
                // Defensive clamp in case something weird gets stored.
                let b = (d as i64).clamp(0, 255) as u8;
                bytes.push(b);
            }
            return js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64;
        }
    }

    // Fallback: treat as BufferHeader (packed u8 bytes).
    unsafe {
        let buf = ptr_usize as *const BufferHeader;
        let len = (*buf).length as usize;
        let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
        js_string_from_bytes(data, len as u32) as i64
    }
}

// ---------------------------------------------------------------------
// Internal thread-local registry for TextEncoder-allocated arrays.
//
// We use this to distinguish "array-with-f64-bytes produced by
// TextEncoder.encode" from "Uint8Array buffer-header with packed u8
// bytes". Both may be registered in the buffer registry (so
// `instanceof Uint8Array` returns true for the former), but their
// decode read path is different.
// ---------------------------------------------------------------------

use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static TEXT_ENCODER_RESULTS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

fn remember_text_encoder_result(ptr: usize) {
    TEXT_ENCODER_RESULTS.with(|s| s.borrow_mut().insert(ptr));
}

fn is_text_encoder_result(ptr: usize) -> bool {
    TEXT_ENCODER_RESULTS.with(|s| s.borrow().contains(&ptr))
}
