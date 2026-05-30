//! TextEncoder / TextDecoder runtime.
//!
//! `js_text_encoder_encode_llvm` returns a `BufferHeader*` (packed u8 bytes,
//! identical layout to `new Uint8Array([...])`) so the inline `bytes[i]`
//! Uint8ArrayGet path (which reads `i8` at `ptr+8+idx`) sees real byte
//! values. Previously this allocated an `ArrayHeader` with f64-per-byte
//! storage, which iteration paths after #578 read as packed u8 — yielding
//! the IEEE-754 byte pattern of the first byte instead of the byte itself
//! (issue #584).
//!
//! `TextEncoder` / `TextDecoder` are stateless wrappers — the encoder is
//! always UTF-8, so we return a small sentinel integer NaN-boxed as a
//! pointer on the codegen side. The runtime doesn't need per-instance state.

use crate::buffer::{buffer_alloc, buffer_data_mut, mark_as_uint8array, BufferHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};

fn throw_type_error(message: &[u8]) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    let bits = crate::value::JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

pub(crate) fn text_encoder_string_ptr(value: f64) -> *const StringHeader {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        return js_string_from_bytes(std::ptr::null(), 0) as *const StringHeader;
    }

    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        throw_type_error(b"Cannot convert a Symbol value to a string");
    }

    crate::value::js_jsvalue_to_string(value) as *const StringHeader
}

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

/// `encoder.encode(str)` — UTF-8 encode `value` into a `BufferHeader`.
///
/// Takes a NaN-boxed f64 string value. Returns an i64 pointer to a freshly
/// allocated `BufferHeader` with `len` packed u8 bytes (same shape as
/// `new Uint8Array([...])`). The buffer is registered + marked as Uint8Array
/// so `instanceof Uint8Array` returns true and the standard Uint8Array
/// indexed-access / iteration / decoder paths all work.
///
/// The returned i64 is the raw `BufferHeader*` — the codegen NaN-boxes it
/// with `POINTER_TAG` before handing it to user code.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode_llvm(value: f64) -> i64 {
    let str_ptr = text_encoder_string_ptr(value);
    let (data_ptr, len) = unsafe {
        let l = (*str_ptr).byte_len as usize;
        let d = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        (d, l)
    };

    let buf = buffer_alloc(len as u32);
    unsafe {
        (*buf).length = len as u32;
        if len > 0 {
            std::ptr::copy_nonoverlapping(data_ptr, buffer_data_mut(buf), len);
        }
    }
    mark_as_uint8array(buf as usize);

    buf as i64
}

fn text_encoder_result(read: usize, written: usize) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, 2);
    if obj.is_null() {
        return obj;
    }

    let read_key = js_string_from_bytes(b"read".as_ptr(), 4);
    let written_key = js_string_from_bytes(b"written".as_ptr(), 7);
    js_object_set_field_by_name(obj, read_key, read as f64);
    js_object_set_field_by_name(obj, written_key, written as f64);
    obj
}

fn text_encoder_prefix_len(src: &[u8], dest_len: usize) -> (usize, usize) {
    if src.is_empty() || dest_len == 0 {
        return (0, 0);
    }
    if src.is_ascii() {
        let written = src.len().min(dest_len);
        return (written, written);
    }

    match std::str::from_utf8(src) {
        Ok(s) => {
            let mut read = 0usize;
            let mut written = 0usize;
            for ch in s.chars() {
                let byte_len = ch.len_utf8();
                if written + byte_len > dest_len {
                    break;
                }
                written += byte_len;
                read += ch.len_utf16();
            }
            (read, written)
        }
        Err(_) => {
            let written = src.len().min(dest_len);
            let read = crate::string::compute_utf16_len(src.as_ptr(), written as u32) as usize;
            (read, written)
        }
    }
}

/// `encoder.encodeInto(str, dest)` — UTF-8 encode into an existing Uint8Array.
///
/// Returns an object with Node's `{ read, written }` shape. `read` counts UTF-16
/// code units consumed from the source string; `written` counts bytes copied to
/// the destination and never splits a UTF-8 sequence.
#[no_mangle]
pub extern "C" fn js_text_encoder_encode_into_llvm(source: f64, dest: f64) -> i64 {
    let str_ptr_i = crate::value::js_get_string_pointer_unified(source);
    let dest_ptr_i = crate::value::js_nanbox_get_pointer(dest);

    if str_ptr_i == 0 || dest_ptr_i == 0 || (dest_ptr_i as usize) < 0x1000 {
        return text_encoder_result(0, 0) as i64;
    }

    let str_ptr = str_ptr_i as *const StringHeader;
    let dest_ptr = dest_ptr_i as *mut BufferHeader;
    if str_ptr.is_null() || dest_ptr.is_null() {
        return text_encoder_result(0, 0) as i64;
    }

    unsafe {
        let src_len = (*str_ptr).byte_len as usize;
        let src_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let src = std::slice::from_raw_parts(src_data, src_len);
        let dest_len = (*dest_ptr).length as usize;
        let (read, written) = text_encoder_prefix_len(src, dest_len);

        for (idx, byte) in src.iter().copied().take(written).enumerate() {
            crate::buffer::js_buffer_set(dest_ptr, idx as i32, byte as i32);
        }

        text_encoder_result(read, written) as i64
    }
}

/// `decoder.decode(buf)` — UTF-8 decode a NaN-boxed `BufferHeader` value.
///
/// Returns a `*const StringHeader` as i64 — the codegen NaN-boxes with
/// `STRING_TAG`. Both TextEncoder output and `new Uint8Array([...])` share
/// the same packed-u8 BufferHeader layout, so a single read path covers both.
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

    unsafe {
        let buf = ptr_usize as *const BufferHeader;
        let len = (*buf).length as usize;
        let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
        js_string_from_bytes(data, len as u32) as i64
    }
}
