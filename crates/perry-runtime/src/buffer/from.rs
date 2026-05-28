use super::*;

/// Create a Buffer from a string
/// encoding: 0 = utf8 (default), 1 = hex, 2 = base64, 3 = base64url, 4 = latin1/binary, 5 = ascii
#[no_mangle]
pub extern "C" fn js_buffer_from_string(
    str_ptr: *const StringHeader,
    encoding: i32,
) -> *mut BufferHeader {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(data_ptr, len);
        buffer_from_str_bytes(str_bytes, encoding)
    }
}

/// Decode a raw string byte-slice into a Buffer according to the encoding
/// tag (see `js_buffer_from_string` for the tag legend). Shared by the heap
/// `StringHeader` path and the inline SSO short-string path so both honor
/// the same hex / base64 / latin1 / utf8 semantics.
fn buffer_from_str_bytes(str_bytes: &[u8], encoding: i32) -> *mut BufferHeader {
    unsafe {
        match encoding {
            // v0.5.772 perf: decode directly into the BufferHeader instead of
            // routing through `decode_hex(&[u8]) -> Vec<u8>` + a follow-up
            // copy_nonoverlapping. Each Vec push had a bounds + capacity check
            // and the final `to_vec` (legacy helper) was a 2nd allocation; the
            // in-place writer skips both. ~2× speedup on 4 KB hex round-trips.
            1 => hex_decode_into_buffer(str_bytes),
            2 | 3 => base64_decode_into_buffer(str_bytes),
            4 | 5 => latin1_string_to_buffer(str_bytes),
            _ => {
                // UTF-8 (default)
                let len = str_bytes.len();
                let buf = buffer_alloc(len as u32);
                (*buf).length = len as u32;
                ptr::copy_nonoverlapping(str_bytes.as_ptr(), buffer_data_mut(buf), len);
                buf
            }
        }
    }
}

/// Encode a JS string as Node's `latin1`/`binary` Buffer input encoding.
///
/// Perry strings are stored as UTF-8, while Node's latin1 encoder writes the
/// low byte of each JS code point. This keeps high-bit bytes in binary-over-
/// string payloads from being expanded into UTF-8 multibyte sequences.
/// `ascii` uses the same input-encoding behavior in modern Node, so it
/// shares this path.
fn latin1_string_to_buffer(str_bytes: &[u8]) -> *mut BufferHeader {
    let decoded = String::from_utf8_lossy(str_bytes);
    let mut out = Vec::with_capacity(decoded.chars().count());
    for ch in decoded.chars() {
        out.push((ch as u32 & 0xFF) as u8);
    }
    unsafe {
        let buf = buffer_alloc(out.len() as u32);
        (*buf).length = out.len() as u32;
        if !out.is_empty() {
            ptr::copy_nonoverlapping(out.as_ptr(), buffer_data_mut(buf), out.len());
        }
        buf
    }
}

/// Map a JS string value (NaN-boxed, pointer-tagged, or raw `*const StringHeader`
/// bitcast to f64) to the integer encoding tag expected by `js_buffer_from_string`
/// and `js_buffer_to_string`:
/// - 0 = utf8 / utf-8 (fallback default)
/// - 1 = hex
/// - 2 = base64 decode-compatible
/// - 3 = base64url encode tag (decode uses the same permissive table)
/// - 4 = latin1 / binary
/// - 5 = ascii
///
/// Used by codegen for non-literal encoding arguments to `Buffer.from(str, enc)`
/// and `buf.toString(enc)` where the encoding expression cannot be statically
/// resolved to a string literal.
#[no_mangle]
pub extern "C" fn js_encoding_tag_from_value(value: f64) -> i32 {
    let str_ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        // Cap at a reasonable upper bound to avoid pathological reads on garbage inputs.
        if len == 0 || len > 32 {
            return 0;
        }
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        // Case-insensitive compare against known encoding names.
        // Avoid heap allocation: compare byte-by-byte with ASCII lowercase fold.
        fn eq_ascii_lower(a: &[u8], b: &[u8]) -> bool {
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .zip(b.iter())
                .all(|(x, y)| x.to_ascii_lowercase() == *y)
        }
        if eq_ascii_lower(bytes, b"hex") {
            1
        } else if eq_ascii_lower(bytes, b"base64") {
            2
        } else if eq_ascii_lower(bytes, b"base64url") {
            3
        } else if eq_ascii_lower(bytes, b"latin1") || eq_ascii_lower(bytes, b"binary") {
            4
        } else if eq_ascii_lower(bytes, b"ascii") {
            5
        } else {
            // utf8, utf-8, and unknown fall through to UTF-8.
            // Matches the runtime's `_ =>` arm in js_buffer_from_string/js_buffer_to_string.
            0
        }
    }
}

/// Create a Buffer from a value (auto-detects string vs array vs buffer)
/// This is used by Buffer.from() which accepts multiple input types.
#[no_mangle]
pub extern "C" fn js_buffer_from_value(value: i64, encoding: i32) -> *mut BufferHeader {
    let bits = value as u64;
    let jsval = crate::JSValue::from_bits(bits);

    // Check if it's a NaN-boxed heap string
    if jsval.is_string() {
        let str_ptr = jsval.as_string_ptr();
        return js_buffer_from_string(str_ptr as *const crate::string::StringHeader, encoding);
    }

    // Inline SSO short string (SHORT_STRING_TAG = 0x7FF9): strings of length
    // 0..=5 carry their bytes inside the NaN-box payload instead of a heap
    // `StringHeader`. `is_string()` above is STRING_TAG-only and rejects
    // these, so without this branch the SSO value would fall through to the
    // pointer-extraction path below, where its inline bytes (e.g. the ASCII
    // of a 5-char `apiKey`) are misread as an array/buffer pointer and
    // dereferenced — the #1767 SIGSEGV in `Buffer.from(shortString)` reached
    // from `@perryts/mysql`'s prepared-statement param encoder. Decode the
    // inline bytes and route through the shared encoder.
    if jsval.is_short_string() {
        let mut tmp = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let len = jsval.short_string_to_buf(&mut tmp);
        return buffer_from_str_bytes(&tmp[..len], encoding);
    }

    // Extract the raw pointer
    let ptr = if bits >> 48 >= 0x7FF8 {
        // NaN-boxed pointer
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };

    if ptr < 0x1000 {
        return buffer_alloc(0);
    }

    // ArrayBuffer / SharedArrayBuffer inputs create a Buffer view over the
    // same backing storage. TypedArray and Buffer inputs still copy below.
    if is_any_array_buffer(ptr) {
        return js_buffer_from_arraybuffer_slice(value, 0, -1);
    }

    // Check if it's a buffer (copy it)
    if is_registered_buffer(ptr) {
        let src = ptr as *const BufferHeader;
        unsafe {
            let len = (*src).length;
            let buf = buffer_alloc(len);
            (*buf).length = len;
            std::ptr::copy_nonoverlapping(buffer_data(src), buffer_data_mut(buf), len as usize);
            // Issue #1225: Node's `Buffer.from(src)` carves the copy out of the
            // shared 8 KiB pool slab so `src.buffer === cp.buffer`.  Perry has
            // no pool, but we still need that `===` identity to hold for
            // userland code that compares `.buffer` references.  Propagate
            // src's resolved alias onto the copy; chained copies collapse to
            // the same root so `Buffer.from(Buffer.from(src)).buffer ===
            // src.buffer` also holds.
            //
            // Skip when src is a plain `Uint8Array` — Node's
            // `Buffer.from(uint8Array)` allocates a fresh ArrayBuffer for the
            // copy and never shares with the source.  Only honest Buffers go
            // through the pool.
            if !is_uint8array_buffer(ptr) {
                let alias = resolve_buffer_ab_alias(ptr);
                set_buffer_ab_alias(buf as usize, alias);
            }
            buf
        }
    } else {
        // Assume it's an array of numbers
        js_buffer_from_array(ptr as *const ArrayHeader)
    }
}

/// Create a Buffer from an array of numbers
#[no_mangle]
pub extern "C" fn js_buffer_from_array(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    // Strip NaN-boxing tags: if upper 16 bits are nonzero, this is a NaN-boxed value.
    // Valid heap pointers on macOS ARM64 have upper 16 bits = 0.
    let arr_ptr = if (arr_ptr as u64) >> 48 != 0 {
        ((arr_ptr as u64) & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
    } else {
        arr_ptr
    };
    if arr_ptr.is_null() || (arr_ptr as usize) < 0x1000 {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*arr_ptr).length as usize;
        let buf = buffer_alloc(len as u32);
        (*buf).length = len as u32;

        let arr_data = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let buf_data = buffer_data_mut(buf);

        for i in 0..len {
            let val = *arr_data.add(i);
            // Array elements may be NaN-boxed INT32, raw f64 numbers, or
            // NaN-boxed pointers/strings (rare for byte literals). Decode
            // numeric kinds; non-numeric values become 0.
            let bits = val.to_bits();
            let top16 = bits >> 48;
            let byte = if top16 == 0x7FFE {
                // INT32_TAG: lower 32 bits are an i32
                ((bits as u32) & 0xFF) as u8
            } else if !val.is_nan() {
                // Raw double — convert via i64 to handle negatives correctly
                ((val as i64) & 0xFF) as u8
            } else {
                0
            };
            *buf_data.add(i) = byte;
        }

        buf
    }
}

/// `Buffer.from(arrayBuffer, byteOffset, length?)`.
///
/// Perry models ArrayBuffer and SharedArrayBuffer storage as BufferHeader
/// allocations. Node returns a Buffer view over that storage, not a detached
/// copy, so this mirrors the selected byte window into a fresh BufferHeader
/// and registers it with the shared view registry used by slice/subarray.
#[no_mangle]
pub extern "C" fn js_buffer_from_arraybuffer_slice(
    value_bits: i64,
    byte_offset: i32,
    length: i32,
) -> *mut BufferHeader {
    let bits = value_bits as u64;
    let raw = if bits >> 48 >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    if raw < 0x1000 || !is_registered_buffer(raw) {
        return buffer_alloc(0);
    }
    unsafe {
        let src = raw as *const BufferHeader;
        let src_len = (*src).length as i32;
        let start = byte_offset.max(0).min(src_len);
        let available = src_len - start;
        let take = if length < 0 {
            available
        } else {
            length.max(0).min(available)
        };
        let take = take as u32;
        let start = start as u32;
        let dst = buffer_alloc(take);
        (*dst).length = take;
        if take > 0 {
            ptr::copy_nonoverlapping(
                buffer_data(src).add(start as usize),
                buffer_data_mut(dst),
                take as usize,
            );
        }
        super::view::register(dst as usize, raw, start, take);
        set_buffer_ab_alias(dst as usize, resolve_buffer_ab_alias(raw));
        dst
    }
}

/// `new Uint8Array(arr)` — same as `js_buffer_from_array` but additionally
/// marks the resulting buffer so it formats as `Uint8Array(N) [ ... ]`.
#[no_mangle]
pub extern "C" fn js_uint8array_from_array(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    let buf = js_buffer_from_array(arr_ptr);
    mark_as_uint8array(buf as usize);
    buf
}

/// `new Uint8Array(length)` — zero-filled buffer marked as Uint8Array.
#[no_mangle]
pub extern "C" fn js_uint8array_alloc(length: i32) -> *mut BufferHeader {
    let length = length.max(0) as u32;
    let buf = buffer_alloc(length);
    unsafe {
        (*buf).length = length;
    }
    mark_as_uint8array(buf as usize);
    buf
}

/// `new Uint8Array(x)` runtime dispatch.
///
/// The codegen can't always statically distinguish `new Uint8Array(n)` (numeric
/// length) from `new Uint8Array(arr)` (source array) when `n` is not a literal,
/// so this entry point inspects the NaN-box tag on the incoming value and
/// routes accordingly. Before this helper the catch-all codegen arm always
/// called `js_uint8array_from_array`, which treated numeric lengths as
/// `ArrayHeader*` and silently produced a zero-length buffer (closes #38).
#[no_mangle]
pub extern "C" fn js_uint8array_new(val: f64) -> *mut BufferHeader {
    let bits = val.to_bits();
    let top16 = (bits >> 48) as u16;
    // POINTER_TAG (0x7FFD) — an object/array/buffer pointer.
    if top16 == 0x7FFD {
        let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if is_registered_buffer(raw) {
            // Issue #579: ArrayBuffer / SharedArrayBuffer sources alias
            // the storage — every `new Uint8Array(ab)` view shares the
            // same `BufferHeader` pointer so writes through one view are
            // visible through every other. The decision is gated on the
            // ArrayBuffer registries (set at constructor time) so
            // it survives the `mark_as_uint8array` side-effect — a second
            // view's `is_uint8array_buffer` would otherwise return true
            // and incorrectly fall into the spec-mandated COPY branch.
            //
            // Issue #227 (the prior memcpy branch) was about avoiding
            // f64-misinterpretation; aliasing also avoids it.
            if is_any_array_buffer(raw) {
                mark_as_uint8array(raw);
                return raw as *mut BufferHeader;
            }
            // Source is itself a Uint8Array → ECMAScript spec copies the
            // bytes into a fresh storage region.
            let src = raw as *const BufferHeader;
            unsafe {
                let len = (*src).length;
                let dst = buffer_alloc(len);
                (*dst).length = len;
                if len > 0 {
                    ptr::copy_nonoverlapping(buffer_data(src), buffer_data_mut(dst), len as usize);
                }
                mark_as_uint8array(dst as usize);
                return dst;
            }
        }
        // Otherwise treat as a numeric source array.
        return js_uint8array_from_array(raw as *const ArrayHeader);
    }
    // Plain IEEE double (upper16 < 0x7FFC or > 0x7FFF) — numeric length.
    if !(0x7FFC..=0x7FFF).contains(&top16) {
        let len = if val.is_finite() && val >= 0.0 {
            val as i32
        } else {
            0
        };
        return js_uint8array_alloc(len);
    }
    // Any other tag (undefined/null/bool/string/bigint) → empty buffer,
    // matching the JS semantics of `new Uint8Array(undefined)` et al.
    js_uint8array_alloc(0)
}

fn zeroed_array_buffer_storage(size: i32) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
        // Zero-fill the data region. `buffer_alloc` does not zero, but
        // ArrayBuffer per ECMAScript spec must observe zero-initialized
        // bytes. Small-slab path is bump-allocated and may carry stale
        // bytes from a prior allocation.
        if size > 0 {
            let data = buffer_data_mut(buf);
            ptr::write_bytes(data, 0, size as usize);
        }
    }
    buf
}

/// `new ArrayBuffer(size)` — allocate a zero-filled buffer of `size` bytes.
/// Issue #579: pre-fix, `ArrayBuffer` had no constructor handler so it fell
/// through to the empty-ObjectHeader placeholder path, and `new Uint8Array(ab)`
/// silently produced a 1-byte buffer with no aliasing. The runtime treats
/// ArrayBuffer and the Uint8Array's storage as the same `BufferHeader`
/// shape (see comment at `value.rs::js_object_get_field_by_name` —
/// "Perry doesn't separate ArrayBuffer"); this constructor allocates a
/// real buffer that subsequent `new Uint8Array(ab)` views can ALIAS by
/// sharing the same pointer.
#[no_mangle]
pub extern "C" fn js_array_buffer_new(size: i32) -> *mut BufferHeader {
    let buf = zeroed_array_buffer_storage(size);
    mark_as_array_buffer(buf as usize);
    buf
}

/// `new SharedArrayBuffer(size)` — same BufferHeader backing store as
/// ArrayBuffer, tracked in a distinct side registry for util.types predicates.
#[no_mangle]
pub extern "C" fn js_shared_array_buffer_new(size: i32) -> *mut BufferHeader {
    let buf = zeroed_array_buffer_storage(size);
    mark_as_shared_array_buffer(buf as usize);
    buf
}

/// `new DataView(buffer)` — Perry currently aliases the underlying buffer
/// storage, so mark that value as a DataView-backed ArrayBufferView.
#[no_mangle]
pub extern "C" fn js_data_view_new(value: f64) -> f64 {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    if v.is_pointer() {
        let addr = v.as_pointer::<u8>() as usize;
        if addr != 0 {
            mark_as_data_view(addr);
        }
    }
    value
}

/// Allocate a zero-filled buffer
#[no_mangle]
pub extern "C" fn js_buffer_alloc(size: i32, fill: i32) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
        let data = buffer_data_mut(buf);
        ptr::write_bytes(data, fill as u8, size as usize);
    }
    buf
}

/// Allocate a buffer and repeat/truncate a Node-compatible fill value.
/// Numeric fills use the low byte; string/Buffer/Uint8Array/array fills reuse
/// the same coercion path as `Buffer.from(value, encoding)`.
#[no_mangle]
pub extern "C" fn js_buffer_alloc_fill_value(
    size: i32,
    fill_value: f64,
    encoding: i32,
) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
        let data = buffer_data_mut(buf);
        if size == 0 {
            return buf;
        }

        let bits = fill_value.to_bits();
        let jsval = crate::JSValue::from_bits(bits);
        // Numeric fills — Node coerces the fill arg through ToUint32 and
        // writes the low byte. Raw f64, INT32-tagged, bool, undefined and
        // null all flow through this path (undefined/null → 0, true/false
        // → 1/0). Pre-fix only raw f64 was recognised, so a `Buffer.alloc(N, 65)`
        // whose argument propagated as INT32_TAG was misread as a pointer
        // and produced a zero-filled buffer.
        if jsval.is_number() {
            ptr::write_bytes(data, fill_value as i64 as u8, size as usize);
            return buf;
        }
        if jsval.is_int32() {
            ptr::write_bytes(data, jsval.as_int32() as u8, size as usize);
            return buf;
        }
        if jsval.is_bool() {
            let b = if jsval.as_bool() { 1u8 } else { 0u8 };
            ptr::write_bytes(data, b, size as usize);
            return buf;
        }
        if jsval.is_undefined() || jsval.is_null() {
            ptr::write_bytes(data, 0, size as usize);
            return buf;
        }

        let src = js_buffer_from_value(bits as i64, encoding);
        if src.is_null() || (*src).length == 0 {
            ptr::write_bytes(data, 0, size as usize);
            return buf;
        }

        let src_len = (*src).length as usize;
        let src_data = buffer_data(src);
        for i in 0..(size as usize) {
            *data.add(i) = *src_data.add(i % src_len);
        }
    }
    buf
}

/// Fill an existing buffer with a byte value. Returns the same buffer pointer.
/// Implements Uint8Array.prototype.fill(value)
#[no_mangle]
pub extern "C" fn js_buffer_fill(buf: *mut BufferHeader, value: i32) -> *mut BufferHeader {
    js_buffer_fill_range(buf, value, 0, i32::MAX)
}

/// Fill an existing buffer with a byte value over a clamped [start, end) range.
/// Implements the deterministic numeric subset of Buffer.prototype.fill.
#[no_mangle]
pub extern "C" fn js_buffer_fill_range(
    buf: *mut BufferHeader,
    value: i32,
    start: i32,
    end: i32,
) -> *mut BufferHeader {
    if buf.is_null() || (buf as u64) < 0x1000 {
        return buf;
    }
    // Strip NaN-boxing tags if present
    let buf = {
        let bits = buf as u64;
        let top16 = (bits >> 48) as u16;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *mut BufferHeader
        } else {
            buf
        }
    };
    unsafe {
        let len = (*buf).length as usize;
        let start = if start < 0 {
            ((len as i32) + start).max(0) as usize
        } else {
            (start as usize).min(len)
        };
        let end = if end < 0 {
            ((len as i32) + end).max(0) as usize
        } else {
            (end as usize).min(len)
        };
        if start >= end {
            return buf;
        }
        let data = buffer_data_mut(buf);
        ptr::write_bytes(data.add(start), value as u8, end - start);
        super::view::propagate_written_range_from_receiver(
            buf as usize,
            start as u32,
            data.add(start),
            (end - start) as u32,
        );
    }
    buf
}

/// Allocate an uninitialized buffer
#[no_mangle]
pub extern "C" fn js_buffer_alloc_unsafe(size: i32) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
    }
    buf
}

/// Concatenate multiple buffers
#[no_mangle]
pub extern "C" fn js_buffer_concat(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    // Strip NaN-boxing tags if present
    let arr_ptr = {
        let bits = arr_ptr as u64;
        let top16 = (bits >> 48) as u16;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr_ptr
        }
    };
    if arr_ptr.is_null() || (arr_ptr as u64) < 0x1000 {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*arr_ptr).length as usize;
        let arr_data = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Helper to strip NaN-boxing tags from buffer element pointers
        let strip_nanbox = |bits: u64| -> u64 {
            let top16 = (bits >> 48) as u16;
            if top16 >= 0x7FF8 {
                bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                bits
            }
        };

        // Calculate total size
        let mut total_size: usize = 0;
        for i in 0..len {
            let raw_bits = strip_nanbox((*arr_data.add(i)).to_bits());
            let buf_ptr = raw_bits as *const BufferHeader;
            if !buf_ptr.is_null() && raw_bits >= 0x1000 {
                total_size += (*buf_ptr).length as usize;
            }
        }

        // Allocate result buffer
        let result = buffer_alloc(total_size as u32);
        (*result).length = total_size as u32;

        // Copy data
        let mut offset: usize = 0;
        for i in 0..len {
            let raw_bits = strip_nanbox((*arr_data.add(i)).to_bits());
            let buf_ptr = raw_bits as *const BufferHeader;
            if !buf_ptr.is_null() && raw_bits >= 0x1000 {
                let buf_len = (*buf_ptr).length as usize;
                let src_data = buffer_data(buf_ptr);
                let dst_data = buffer_data_mut(result).add(offset);
                ptr::copy_nonoverlapping(src_data, dst_data, buf_len);
                offset += buf_len;
            }
        }

        result
    }
}
