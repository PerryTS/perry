use super::*;

/// Convert a buffer slice to a string. Honors the optional `start`/`end`
/// range (Node semantics: `start` clamped to `[0, len]`, `end` clamped to
/// `[start, len]`; defaults are `start=0, end=len`).
///
/// `encoding`: 0 = utf8 (default), 1 = hex, 2 = base64, 3 = base64url.
#[no_mangle]
pub extern "C" fn js_buffer_to_string_range(
    buf_ptr: *const BufferHeader,
    encoding: i32,
    start: i32,
    end: i32,
) -> *mut StringHeader {
    let buf_ptr = {
        let bits = buf_ptr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(ptr::null(), 0);
    }

    unsafe {
        let len = (*buf_ptr).length as i32;
        let s = start.max(0).min(len);
        let e = end.max(s).min(len);
        let slice_len = (e - s) as usize;
        let data = buffer_data(buf_ptr).add(s as usize);
        let bytes = std::slice::from_raw_parts(data, slice_len);

        match encoding {
            // v0.5.772 perf: encode directly into a fresh StringHeader without
            // an intermediate Vec<u8>. Hex/base64 outputs are pure ASCII so the
            // ASCII-only string allocator skips compute_utf16_len's byte-walk.
            1 => hex_encode_into_string(bytes),
            2 => base64_encode_into_string(bytes),
            3 => base64url_encode_into_string(bytes),
            _ => buf_bytes_to_utf8_string(bytes),
        }
    }
}

/// Convert a buffer to a string
/// encoding: 0 = utf8 (default), 1 = hex, 2 = base64, 3 = base64url.
#[no_mangle]
pub extern "C" fn js_buffer_to_string(
    buf_ptr: *const BufferHeader,
    encoding: i32,
) -> *mut StringHeader {
    // Strip NaN-boxing tags if present so callers can pass an i64 that came
    // from `bitcast double → i64` without unboxing first. The LLVM backend
    // NaN-boxes Buffer pointers with POINTER_TAG (0x7FFD), and the dispatch
    // path in `js_value_to_string_with_encoding` below passes the raw bits
    // straight through.
    let buf_ptr = {
        let bits = buf_ptr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return js_string_from_bytes(ptr::null(), 0);
    }

    unsafe {
        let len = (*buf_ptr).length as usize;
        let data = buffer_data(buf_ptr);
        let bytes = std::slice::from_raw_parts(data, len);

        match encoding {
            // v0.5.772 perf: hex/base64 outputs are pure ASCII — the in-place
            // encoder writes directly into a fresh StringHeader allocated via
            // `js_string_from_ascii_bytes` (no compute_utf16_len byte-walk).
            1 => hex_encode_into_string(bytes),
            2 => base64_encode_into_string(bytes),
            3 => base64url_encode_into_string(bytes),
            _ => {
                // UTF-8 (default) — Node spec: invalid UTF-8 sequences are
                // replaced with U+FFFD. Pre-fix this path passed the raw
                // bytes straight to `js_string_from_bytes`, whose downstream
                // `compute_utf16_len` ran `str::from_utf8_unchecked`
                // (UB on non-UTF-8) → SIGSEGV in the multi-byte counter on
                // random binary buffers. Issue #609.
                buf_bytes_to_utf8_string(bytes)
            }
        }
    }
}

/// Build a Perry string (validated UTF-8) from a buffer's raw bytes.
/// Invalid UTF-8 sequences are replaced with U+FFFD per the WHATWG / Node
/// `Buffer.toString('utf8')` contract. Issue #609.
///
/// v0.5.772 perf: hot path on networking/transcode workloads is ASCII-only
/// payloads (HTTP bodies, JSON, base64-decoded text). A single `is_ascii`
/// scan (vectorisable, ~1 ns/byte on AArch64) lets us bypass the
/// `from_utf8_lossy` validation pass + the downstream `compute_utf16_len`
/// scan in `js_string_from_bytes` (which itself does an ASCII scan, then
/// walks the bytes again for utf16 counting on non-ASCII). For pure-ASCII
/// inputs we land on `js_string_from_ascii_bytes` directly — one byte scan
/// total. Non-ASCII falls through to the spec-correct lossy path.
fn buf_bytes_to_utf8_string(bytes: &[u8]) -> *mut StringHeader {
    if bytes.is_ascii() {
        return js_string_from_ascii_bytes(bytes.as_ptr(), bytes.len() as u32);
    }
    let cow = String::from_utf8_lossy(bytes);
    js_string_from_bytes(cow.as_ptr(), cow.len() as u32)
}

/// Universal `.toString(encoding?)` dispatch used by the LLVM backend's
/// `lower_call.rs` for chained `.toString(arg)` calls where the receiver
/// type is not statically known.
///
/// - If the receiver is a registered Buffer (POINTER_TAG-boxed or raw),
///   route to `js_buffer_to_string` with the encoding tag.
/// - Otherwise fall through to `js_jsvalue_to_string` (encoding ignored,
///   matches Node behavior for non-Buffer values like numbers/objects).
///
/// `enc_tag` is the i32 produced by `js_encoding_tag_from_value` (or a
/// compile-time-folded literal): 0 = utf8, 1 = hex, 2 = base64.
#[no_mangle]
pub extern "C" fn js_value_to_string_with_encoding(value: f64, enc_tag: i32) -> *mut StringHeader {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    // Extract the underlying pointer regardless of NaN-box presence:
    //   - POINTER_TAG (0x7FFD) → strip top 16 bits
    //   - raw pointer bitcast to f64 → use bits directly (top16 == 0)
    let ptr_addr = if top16 >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && bits >= 0x1000 {
        bits as usize
    } else {
        0
    };
    if ptr_addr != 0 && is_registered_buffer(ptr_addr) {
        return js_buffer_to_string(ptr_addr as *const BufferHeader, enc_tag);
    }
    crate::value::js_jsvalue_to_string(value)
}

/// Print a buffer in Node.js `<Buffer xx xx ...>` format to stdout
#[no_mangle]
pub extern "C" fn js_buffer_print(buf_ptr: *const BufferHeader) {
    if buf_ptr.is_null() {
        println!("<Buffer >");
        return;
    }
    unsafe {
        let len = (*buf_ptr).length as usize;
        let data = buffer_data(buf_ptr);
        let bytes = std::slice::from_raw_parts(data, len);
        let mut out = String::with_capacity(9 + len * 3);
        out.push_str("<Buffer");
        for (i, b) in bytes.iter().enumerate() {
            if i == 0 {
                out.push(' ');
            } else {
                out.push(' ');
            }
            out.push_str(&format!("{:02x}", b));
        }
        out.push('>');
        println!("{}", out);
    }
}

/// Get the length of a buffer
#[no_mangle]
pub extern "C" fn js_buffer_length(buf_ptr: *const BufferHeader) -> i32 {
    // Strip NaN-boxing tags if present (POINTER_TAG-boxed buffer pointers).
    let buf_ptr = {
        let bits = buf_ptr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe { (*buf_ptr).length as i32 }
}

/// Materialize a buffer (Uint8Array) as a regular Array of f64 byte values.
/// Used by `js_array_clone` / `js_array_concat` to give `Array.from(uint8arr)`,
/// `[...uint8arr]`, and the for-of `ArrayFrom`-wrapped iterable the byte
/// values rather than the byte buffer reinterpreted as f64s. Issue #578.
pub fn buffer_to_array(buf_ptr: *const BufferHeader) -> *mut ArrayHeader {
    // Strip NaN-box if present.
    let buf_ptr = {
        let bits = buf_ptr as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            buf_ptr
        }
    };
    if buf_ptr.is_null() || (buf_ptr as usize) < 0x1000 {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let len = (*buf_ptr).length as usize;
        let result = crate::array::js_array_alloc(len as u32);
        if len == 0 {
            return result;
        }
        let src = buffer_data(buf_ptr);
        let dst = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len {
            *dst.add(i) = (*src.add(i)) as f64;
        }
        (*result).length = len as u32;
        result
    }
}
