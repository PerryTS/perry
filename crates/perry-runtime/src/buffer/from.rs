use super::*;

fn throw_invalid_buffer_from_first_arg() -> ! {
    let message = b"The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object.";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Create a Buffer from a string
/// encoding: 0 = utf8 (default), 1 = hex, 2 = base64, 3 = base64url,
/// 4 = latin1/binary, 5 = ascii, 6 = utf16le/ucs2.
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
            6 => buffer_from_vec(utf16le_string_bytes(str_bytes)),
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

pub(crate) fn buffer_string_bytes_for_encoding(str_bytes: &[u8], encoding: i32) -> Vec<u8> {
    match encoding {
        1 => decode_hex(str_bytes),
        2 | 3 => decode_base64(str_bytes),
        4 | 5 => latin1_string_bytes(str_bytes),
        6 => utf16le_string_bytes(str_bytes),
        _ => str_bytes.to_vec(),
    }
}

fn buffer_from_vec(out: Vec<u8>) -> *mut BufferHeader {
    unsafe {
        let buf = buffer_alloc(out.len() as u32);
        (*buf).length = out.len() as u32;
        if !out.is_empty() {
            ptr::copy_nonoverlapping(out.as_ptr(), buffer_data_mut(buf), out.len());
        }
        buf
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
    buffer_from_vec(latin1_string_bytes(str_bytes))
}

fn latin1_string_bytes(str_bytes: &[u8]) -> Vec<u8> {
    let decoded = String::from_utf8_lossy(str_bytes);
    let mut out = Vec::with_capacity(decoded.chars().count());
    for ch in decoded.chars() {
        out.push((ch as u32 & 0xFF) as u8);
    }
    out
}

fn utf16le_string_bytes(str_bytes: &[u8]) -> Vec<u8> {
    let decoded = String::from_utf8_lossy(str_bytes);
    let mut out = Vec::with_capacity(decoded.len() * 2);
    for unit in decoded.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
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
/// - 6 = utf16le / utf-16le / ucs2 / ucs-2
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
        } else if eq_ascii_lower(bytes, b"utf16le")
            || eq_ascii_lower(bytes, b"utf-16le")
            || eq_ascii_lower(bytes, b"ucs2")
            || eq_ascii_lower(bytes, b"ucs-2")
        {
            6
        } else {
            // utf8, utf-8, and unknown fall through to UTF-8.
            // Matches the runtime's `_ =>` arm in js_buffer_from_string/js_buffer_to_string.
            0
        }
    }
}

fn buffer_byte_from_js_value(value: f64) -> u8 {
    let number = crate::builtins::js_number_coerce(value);
    if number.is_finite() {
        ((number as i64) & 0xFF) as u8
    } else {
        0
    }
}

unsafe fn buffer_from_object_value_of(
    ptr: usize,
    original_value: f64,
    encoding: i32,
) -> Option<*mut BufferHeader> {
    if ptr < 0x1000 {
        return None;
    }
    let gc_type = *((ptr - crate::gc::GC_HEADER_SIZE) as *const u8);
    if gc_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }

    let value_of_key = js_string_from_bytes(b"valueOf".as_ptr(), 7);
    let value_of = crate::object::js_object_get_field_by_name(
        ptr as *const crate::object::ObjectHeader,
        value_of_key,
    );
    if !value_of.is_pointer() {
        return None;
    }
    let closure_ptr = value_of.as_pointer::<u8>() as usize;
    if !crate::closure::is_closure_ptr(closure_ptr) {
        return None;
    }

    let converted = crate::object::js_native_call_method(
        original_value,
        b"valueOf".as_ptr() as *const i8,
        7,
        ptr::null(),
        0,
    );
    if converted.to_bits() == original_value.to_bits() {
        return None;
    }
    let converted_value = crate::JSValue::from_bits(converted.to_bits());
    if converted_value.is_null() || converted_value.is_undefined() {
        return None;
    }
    if converted_value.is_any_string() || converted_value.is_pointer() {
        return Some(js_buffer_from_value(converted.to_bits() as i64, encoding));
    }
    None
}

unsafe fn buffer_from_array_like_object(ptr: usize) -> Option<*mut BufferHeader> {
    if ptr < 0x1000 {
        return None;
    }
    let gc_type = *((ptr - crate::gc::GC_HEADER_SIZE) as *const u8);
    if gc_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }

    let length_key = js_string_from_bytes(b"length".as_ptr(), 6);
    let length_value = crate::object::js_object_get_field_by_name(
        ptr as *const crate::object::ObjectHeader,
        length_key,
    );
    if length_value.is_undefined() {
        return None;
    }

    let length = if length_value.is_int32() {
        length_value.as_int32() as f64
    } else if length_value.is_number() {
        length_value.as_number()
    } else {
        return Some(buffer_alloc(0));
    };

    if !length.is_finite() || length <= 0.0 {
        return Some(buffer_alloc(0));
    }

    let len = length.trunc().min(u32::MAX as f64) as u32;
    let buf = buffer_alloc(len);
    (*buf).length = len;
    let buf_data = buffer_data_mut(buf);

    for i in 0..len {
        let value = crate::object::js_object_get_index_polymorphic(ptr as i64, i as f64);
        *buf_data.add(i as usize) = buffer_byte_from_js_value(value);
    }

    Some(buf)
}

unsafe fn buffer_from_object_to_primitive(value: f64, encoding: i32) -> Option<*mut BufferHeader> {
    let primitive = crate::symbol::js_to_primitive(value, 2);
    if primitive.to_bits() == value.to_bits() {
        return None;
    }
    let primitive_value = crate::JSValue::from_bits(primitive.to_bits());
    if primitive_value.is_any_string() {
        return Some(js_buffer_from_value(primitive.to_bits() as i64, encoding));
    }
    None
}

/// Create a Buffer from a value (auto-detects string vs array vs buffer)
/// This is used by Buffer.from() which accepts multiple input types.
#[no_mangle]
pub extern "C" fn js_buffer_from_value(value: i64, encoding: i32) -> *mut BufferHeader {
    let bits = value as u64;
    let jsval = crate::JSValue::from_bits(bits);
    let value_f64 = f64::from_bits(bits);

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

    if jsval.is_undefined()
        || jsval.is_null()
        || jsval.is_bool()
        || jsval.is_int32()
        || jsval.is_number()
        || jsval.is_bigint()
        || unsafe { crate::symbol::js_is_symbol(f64::from_bits(bits)) != 0 }
    {
        throw_invalid_buffer_from_first_arg();
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
    } else if let Some(buf) = unsafe { buffer_from_object_value_of(ptr, value_f64, encoding) } {
        buf
    } else if let Some(buf) = unsafe { buffer_from_array_like_object(ptr) } {
        buf
    } else if let Some(buf) = unsafe { buffer_from_object_to_primitive(value_f64, encoding) } {
        buf
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
            *buf_data.add(i) = buffer_byte_from_js_value(*arr_data.add(i));
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

/// Materialize non-Array Uint8Array sources through the shared TypedArray path.
fn js_uint8array_from_source(source: f64) -> *mut BufferHeader {
    let raw = unsafe { crate::typedarray::typed_array_from_source_raw_values(source) };

    // Coercion can collect, so retain the snapshot in runtime roots.
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw_handles = scope.root_nanbox_f64_slice(&raw);
    let bytes: Vec<u8> = raw_handles
        .iter()
        .map(|value| buffer_byte_from_js_value(value.get_nanbox_f64()))
        .collect();

    unsafe {
        let buf = buffer_alloc(bytes.len() as u32);
        (*buf).length = bytes.len() as u32;
        if !bytes.is_empty() {
            ptr::copy_nonoverlapping(bytes.as_ptr(), buffer_data_mut(buf), bytes.len());
        }
        mark_as_uint8array(buf as usize);
        buf
    }
}

/// Validate a `new Uint8Array(length)` argument with `ToIndex` semantics and
/// throw the spec `RangeError: Invalid typed array length: <n>` on a negative
/// or out-of-range length, matching Node (#3662). `Uint8Array` is backed by a
/// `BufferHeader` in Perry, so this lives here rather than in `typedarray.rs`.
#[inline]
fn uint8array_length_or_throw(val: f64) -> u32 {
    let integer = if val.is_nan() { 0.0 } else { val.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        // Node reports the ORIGINAL argument, not the truncated integer
        // (`new Uint8Array(-1.5)` → "Invalid typed array length: -1.5"), with
        // integral values shown without a decimal point (#3146).
        let shown = if val.is_infinite() {
            if val > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
        } else if val.fract() == 0.0 && val.abs() < (i64::MAX as f64) {
            format!("{}", val as i64)
        } else {
            format!("{val}")
        };
        let msg = format!("Invalid typed array length: {shown}");
        let m = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_rangeerror_new(m);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    // #5067 — a `Uint8Array` is backed by an `i32`-capacity `BufferHeader`, so
    // a length above `i32::MAX` (~2 GiB) cannot be allocated. Node passes the
    // `<= 2**53-1` length check for these and then fails the real allocation,
    // so match its `RangeError: Array buffer allocation failed` rather than
    // silently capping the length (which produced a wrong-size buffer).
    if integer > i32::MAX as f64 {
        crate::error::throw_allocation_failed();
    }
    integer as u32
}

/// `new Uint8Array(length)` — zero-filled buffer marked as Uint8Array.
#[no_mangle]
pub extern "C" fn js_uint8array_alloc(length: i32) -> *mut BufferHeader {
    let length = uint8array_length_or_throw(length as f64);
    let buf = buffer_alloc(length);
    unsafe {
        (*buf).length = length;
        // `buffer_alloc` hands back old-arena memory that is NOT guaranteed
        // zeroed: the old generation reclaims and re-hands dirty blocks, and
        // only pristine mmap pages read as zero. `new Uint8Array(n)` (and the
        // bool / string / numeric-length sources that funnel here) is
        // spec-mandated zero-filled, so clear the payload explicitly instead of
        // relying on incidental zeroing. The sibling `js_buffer_alloc`
        // (Buffer.alloc) already does this; this path was the lone gap. Latent
        // on macOS (fresh pages read zero); leaked stale bytes on Linux once an
        // unrelated codegen change shifted these allocations onto reclaimed
        // blocks.
        if length > 0 {
            ptr::write_bytes(buffer_data_mut(buf), 0, length as usize);
        }
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
    // ToIndex rejects BigInt and Symbol.
    if top16 == 0x7FFA {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if top16 == 0x7FFD && unsafe { crate::symbol::js_is_symbol(val) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
    }
    // POINTER_TAG (0x7FFD) — an object/array/buffer pointer.
    if top16 == 0x7FFD {
        let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if is_registered_buffer(raw) {
            // Issue #579: ArrayBuffer / SharedArrayBuffer sources alias
            // the storage via a registered view so writes through the
            // Uint8Array are visible through the backing ArrayBuffer while
            // the original value still formats as ArrayBuffer.
            //
            // Issue #227 (the prior memcpy branch) was about avoiding
            // f64-misinterpretation; aliasing also avoids it.
            if is_any_array_buffer(raw) {
                if super::detach::is_detached_buffer(raw) {
                    crate::collection_iter::throw_type_error(
                        "Cannot perform Construct on a detached ArrayBuffer",
                    );
                }
                let src = raw as *const BufferHeader;
                unsafe {
                    let len = (*src).length as i32;
                    let view = js_buffer_slice(src, 0, len);
                    mark_as_uint8array(view as usize);
                    set_buffer_ab_alias(view as usize, resolve_buffer_ab_alias(raw));
                    return view;
                }
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
        if let Some(src_kind) = crate::typedarray::lookup_typed_array_kind(raw) {
            if crate::typedarray::bigint::is_bigint_kind(src_kind) {
                crate::typedarray::bigint::throw_bigint_number_mix();
            }
            let src = raw as *const crate::typedarray::TypedArrayHeader;
            unsafe {
                let len = crate::typedarray::js_typed_array_length(src).max(0) as u32;
                let dst = buffer_alloc(len);
                (*dst).length = len;
                let dst_data = buffer_data_mut(dst);
                for i in 0..len as usize {
                    let value = crate::typedarray::js_typed_array_get(src, i as i32);
                    *dst_data.add(i) = buffer_byte_from_js_value(value);
                }
                mark_as_uint8array(dst as usize);
                return dst;
            }
        }
        // The raw helper requires an actual ArrayHeader layout.
        if unsafe { crate::value::addr_class::try_read_gc_header(raw) }
            .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_ARRAY)
        {
            return js_uint8array_from_array(raw as *const ArrayHeader);
        }
        return js_uint8array_from_source(val);
    }
    // Plain IEEE double (upper16 < 0x7FFC or > 0x7FFF) — numeric length.
    // Node applies ToIndex (NaN → 0, truncate toward zero) and throws a
    // RangeError on a negative / out-of-range length (#3662).
    if !(0x7FFC..=0x7FFF).contains(&top16) {
        // `uint8array_length_or_throw` already throws for lengths above
        // `i32::MAX` (#5067), so the cast is in range here.
        let len = uint8array_length_or_throw(val);
        return js_uint8array_alloc(len as i32);
    }
    if bits == crate::value::TAG_UNDEFINED {
        return js_uint8array_alloc(0);
    }
    let len = uint8array_length_or_throw(crate::typedarray::jsvalue_to_f64(val));
    js_uint8array_alloc(len as i32)
}

/// `new Uint8Array(buffer, byteOffset, length)` — create a Uint8Array view
/// over ArrayBuffer-like storage. Perry's BufferHeader model represents the
/// view with a sliced buffer registered against the original backing store.
///
/// `offset_value` / `length_value` are the raw NaN-boxed arguments
/// (`undefined` when absent), matching the other typed-array view kinds:
/// ToIndex runs here — it can execute user code (`valueOf`) that detaches
/// the source — and per spec the IsDetachedBuffer check and all bounds
/// validation happen after both coercions.
#[no_mangle]
pub extern "C" fn js_uint8array_view(
    source: f64,
    offset_value: f64,
    length_value: f64,
) -> *mut BufferHeader {
    let bits = source.to_bits();
    if (bits >> 48) != 0x7FFD {
        return js_uint8array_new(source);
    }
    let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if !is_registered_buffer(raw) || !is_any_array_buffer(raw) {
        return js_uint8array_new(source);
    }
    let byte_offset = crate::typedarray_view::typed_array_view_to_index(offset_value);
    let length_jv = crate::value::JSValue::from_bits(length_value.to_bits());
    let requested = if length_jv.is_undefined() {
        None
    } else {
        Some(crate::typedarray_view::typed_array_view_to_index(
            length_value,
        ))
    };
    if super::detach::is_detached_buffer(raw) {
        crate::collection_iter::throw_type_error(
            "Cannot perform Construct on a detached ArrayBuffer",
        );
    }
    let src = raw as *const BufferHeader;
    unsafe {
        let total_len = (*src).length as i64;
        // #4103: spec `RangeError` for an out-of-range view. `BYTES_PER_ELEMENT`
        // is 1 for `Uint8Array`, so there is no alignment constraint — only the
        // offset and (offset + length) bounds against the backing buffer.
        if byte_offset > total_len {
            crate::fs::validate::throw_range_error_with_code(&format!(
                "Start offset {byte_offset} is outside the bounds of the buffer"
            ));
        }
        if let Some(requested) = requested {
            if byte_offset.saturating_add(requested) > total_len {
                crate::fs::validate::throw_range_error_with_code(&format!(
                    "Invalid typed array length: {requested}"
                ));
            }
        }
        let start = byte_offset.clamp(0, total_len) as i32;
        let len = match requested {
            None => (total_len - byte_offset).max(0) as i32,
            Some(requested) => requested.max(0) as i32,
        };
        let end = start.saturating_add(len).min(total_len as i32);
        let view = js_buffer_slice(src, start, end);
        mark_as_uint8array(view as usize);
        set_buffer_ab_alias(view as usize, resolve_buffer_ab_alias(raw));
        view
    }
}

pub(super) fn zeroed_array_buffer_storage(size: i32) -> *mut BufferHeader {
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

fn throw_array_buffer_range_error() -> ! {
    crate::fs::validate::throw_range_error_with_code("Invalid array buffer length")
}

pub(super) fn array_buffer_to_index(value: f64) -> i32 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return 0;
    }
    // ToIndex(length) → ToNumber: a BigInt throws TypeError; `js_number_coerce`
    // is the full ToNumber — it runs an object's `valueOf`/`toString` (so a
    // throwing one propagates) and throws TypeError on a Symbol. The bare
    // `JSValue::to_number()` previously produced NaN→0 for objects/symbols, so
    // `new ArrayBuffer({valueOf(){return 42}})` silently became length 0.
    if jv.is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    let n = crate::builtins::js_number_coerce(value);
    // ToIndex truncates toward zero (ToIntegerOrInfinity) BEFORE the range
    // check, so a fractional value in (-1, 0] like `-0.1` or `-0.99999` maps to
    // +0 rather than tripping the negativity guard (test262
    // SharedArrayBuffer/toindex-length). NaN → 0.
    let integer = if n.is_nan() { 0.0 } else { n.trunc() };
    if integer < 0.0 || integer > i32::MAX as f64 {
        throw_array_buffer_range_error();
    }
    integer as i32
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

#[no_mangle]
pub extern "C" fn js_array_buffer_new_value(size_value: f64) -> *mut BufferHeader {
    js_array_buffer_new(array_buffer_to_index(size_value))
}

/// `new SharedArrayBuffer(size)` — same `BufferHeader` shape as ArrayBuffer,
/// tracked in a distinct side registry for util.types predicates.
///
/// Unlike a plain ArrayBuffer, the backing store is allocated process-globally
/// (never freed, stable address valid from every thread — see
/// `crate::shared_sab`) instead of from the thread-local slab. That is what
/// lets the same SAB alias the same physical bytes across `perry/thread`
/// agents, so cross-agent `Atomics.wait`/`notify` actually coordinate (#4913).
#[no_mangle]
pub extern "C" fn js_shared_array_buffer_new(size: i32) -> *mut BufferHeader {
    let size = size.max(0) as u32;
    let buf = crate::shared_sab::alloc_shared_sab(size);
    let addr = buf as usize;
    // Register in the creating thread's tables too so local predicates
    // (`is_registered_buffer`, `is_shared_array_buffer`, views) work without a
    // round-trip through the process-global registry.
    register_buffer(buf);
    mark_as_shared_array_buffer(addr);
    buf
}

#[no_mangle]
pub extern "C" fn js_shared_array_buffer_new_value(size_value: f64) -> *mut BufferHeader {
    js_shared_array_buffer_new(array_buffer_to_index(size_value))
}

/// `Type(buffer) is not Object` → `TypeError` (DataView spec step 2). Also
/// raised when `buffer` is a non-buffer pointer (Symbol, plain object, …) —
/// Perry only models ArrayBuffer/SharedArrayBuffer storage as a registered
/// `BufferHeader`.
fn throw_dataview_buffer_not_object() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "First argument to DataView constructor must be an ArrayBuffer",
        "ERR_INVALID_ARG_TYPE",
    )
}

/// `ToIndex`/range validation failure → `RangeError` (DataView spec steps 6,
/// 9, 11, and the `ToIndex` abstract operation: negative or non-integral
/// indices, and offsets/lengths that escape the backing buffer).
fn throw_dataview_range_error(message: &str) -> ! {
    crate::fs::validate::throw_range_error_with_code(message)
}

/// `ToIndex(value)` for the `DataView` constructor's `byteOffset`/`byteLength`
/// arguments: `undefined` → 0; otherwise `ToIntegerOrInfinity` with a
/// `RangeError` for negative or out-of-`[[0, 2^53-1]]` results. `what` names
/// the argument for the thrown message.
fn dataview_to_index(value: f64, what: &str) -> i64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return 0;
    }
    // ToIndex → ToNumber: a BigInt throws a TypeError (ToNumber rejects it), and
    // `js_number_coerce` is the full ToNumber — it throws a TypeError on a Symbol
    // and runs an object's `valueOf`/`toString`. The bare `JSValue::to_number()`
    // previously produced `NaN`→0 for those, so `new DataView(buf, Symbol())`
    // didn't throw and a `valueOf` never ran.
    if jv.is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    let n = crate::builtins::js_number_coerce(value);
    if n.is_nan() {
        // ToIntegerOrInfinity(NaN) = 0.
        return 0;
    }
    // ToIndex truncates toward zero (ToIntegerOrInfinity) *before* the sign and
    // range checks, so e.g. `-0.1`/`-0.99999` become `-0` (== 0), not a
    // RangeError (test262 toindex-byteoffset/-bytelength).
    let int = n.trunc();
    if int < 0.0 {
        throw_dataview_range_error(&format!("Invalid DataView {what}"));
    }
    // ToIndex rejects values above 2^53-1 (and thus +Infinity).
    if int > 9_007_199_254_740_991.0 {
        throw_dataview_range_error(&format!("Invalid DataView {what}"));
    }
    int as i64
}

/// `new DataView(buffer, byteOffset?, byteLength?)` — Perry models a DataView
/// as a `BufferHeader` view over the backing `ArrayBuffer`. Even a full-span
/// `new DataView(buffer)` gets its own registered view header so brand checks
/// can distinguish the DataView receiver from the backing ArrayBuffer while
/// preserving write propagation through the shared view registry.
///
/// `offset_value` / `length_value` are NaN-boxed JS values (the raw arguments,
/// `undefined` when absent) so the spec's `ToIndex`/range validation can run
/// here (#3657): a non-object `buffer` throws `TypeError`; negative or
/// out-of-range `byteOffset`/`byteLength` throw `RangeError`.
#[no_mangle]
pub extern "C" fn js_data_view_new(value: f64, offset_value: f64, length_value: f64) -> f64 {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    // Step 2: Type(buffer) must be Object holding [[ArrayBufferData]].
    if !v.is_pointer() {
        throw_dataview_buffer_not_object();
    }
    let addr = v.as_pointer::<u8>() as usize;
    if addr == 0 || !is_registered_buffer(addr) {
        throw_dataview_buffer_not_object();
    }
    // Steps 4-6: offset = ToIndex(byteOffset) (RangeError if negative). Runs
    // BEFORE the detached check — ToIndex can execute user code (`valueOf`)
    // that detaches the buffer, and the spec checks IsDetachedBuffer after.
    let offset = dataview_to_index(offset_value, "byteOffset");
    if super::detach::is_detached_buffer(addr) {
        crate::collection_iter::throw_type_error(
            "Cannot perform Construct on a detached ArrayBuffer",
        );
    }

    let src = addr as *const BufferHeader;
    let total_len = unsafe { (*src).length as i64 };

    // Step 9: offset must not exceed the backing buffer's byteLength.
    if offset > total_len {
        throw_dataview_range_error("Start offset is outside the bounds of the buffer");
    }

    // Steps 10-12: byteLength defaults to the remainder; an explicit length is
    // ToIndex-validated and must not escape the buffer.
    let length_jv = crate::value::JSValue::from_bits(length_value.to_bits());
    let view_len = if length_jv.is_undefined() {
        total_len - offset
    } else {
        let requested = dataview_to_index(length_value, "byteLength");
        if offset + requested > total_len {
            throw_dataview_range_error("Invalid DataView length");
        }
        requested
    };

    // Spec step 12's second IsDetachedBuffer check: ToIndex(byteLength) above
    // can also run user code that detaches the buffer; re-check before
    // touching its bytes.
    if super::detach::is_detached_buffer(addr) {
        crate::collection_iter::throw_type_error(
            "Cannot perform Construct on a detached ArrayBuffer",
        );
    }

    // Build a registered view so the numeric accessors index
    // relative to the view start and `.byteOffset`/`.byteLength`/`.buffer`
    // report the right values — including the zero-length edge cases
    // (`offset == total_len`) that `js_buffer_slice` would otherwise collapse
    // to an unregistered empty buffer, losing offset and backing.
    unsafe {
        let start = offset as u32;
        let len = view_len as u32;
        let view = buffer_alloc(len);
        (*view).length = len;
        if len > 0 {
            let src_data = buffer_data(src).add(start as usize);
            ptr::copy_nonoverlapping(src_data, buffer_data_mut(view), len as usize);
        }
        super::view::register(view as usize, src as usize, start, len);
        mark_as_data_view(view as usize);
        set_buffer_ab_alias(view as usize, resolve_buffer_ab_alias(addr));
        f64::from_bits(crate::value::JSValue::pointer(view as *mut u8).bits())
    }
}

fn throw_buffer_alloc_size_out_of_range() -> ! {
    static REGISTER_RANGE_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_RANGE_ERROR.call_once(|| {
        crate::object::js_register_class_extends_error(crate::error::CLASS_ID_RANGE_ERROR);
    });
    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_RANGE_ERROR, 4);
    unsafe {
        let set = |key: &[u8], value: f64| {
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key_ptr, value);
        };
        let str_val = |s: &[u8]| -> f64 {
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(crate::JSValue::string_ptr(ptr).bits())
        };
        set(b"name", str_val(b"RangeError"));
        set(b"code", str_val(b"ERR_OUT_OF_RANGE"));
        set(
            b"message",
            str_val(b"The value of \"size\" is out of range"),
        );
    }
    crate::exception::js_throw(crate::value::js_nanbox_pointer(obj as i64))
}

fn validate_buffer_alloc_size(size: i32) -> u32 {
    if size < 0 {
        throw_buffer_alloc_size_out_of_range();
    }
    size as u32
}

/// Allocate a zero-filled buffer
#[no_mangle]
pub extern "C" fn js_buffer_alloc(size: i32, fill: i32) -> *mut BufferHeader {
    let size = validate_buffer_alloc_size(size);
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
    let size = validate_buffer_alloc_size(size);
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

/// Fill an existing buffer by repeating/truncating a Node-compatible fill value.
#[no_mangle]
pub extern "C" fn js_buffer_fill_value_range(
    buf: *mut BufferHeader,
    fill_value: f64,
    start: i32,
    end: i32,
    encoding: i32,
) -> *mut BufferHeader {
    if buf.is_null() || (buf as u64) < 0x1000 {
        return buf;
    }
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
        let dst = data.add(start);
        let count = end - start;
        let bits = fill_value.to_bits();
        let jsval = crate::JSValue::from_bits(bits);

        let write_byte = |byte: u8| {
            ptr::write_bytes(dst, byte, count);
            super::view::propagate_written_range_from_receiver(
                buf as usize,
                start as u32,
                dst,
                count as u32,
            );
        };

        if jsval.is_number() {
            write_byte(fill_value as i64 as u8);
            return buf;
        }
        if jsval.is_int32() {
            write_byte(jsval.as_int32() as u8);
            return buf;
        }
        if jsval.is_bool() {
            write_byte(if jsval.as_bool() { 1 } else { 0 });
            return buf;
        }
        if jsval.is_undefined() || jsval.is_null() {
            write_byte(0);
            return buf;
        }

        let src = js_buffer_from_value(bits as i64, encoding);
        if src.is_null() || (*src).length == 0 {
            write_byte(0);
            return buf;
        }

        let src_len = (*src).length as usize;
        let src_data = buffer_data(src);
        for i in 0..count {
            *dst.add(i) = *src_data.add(i % src_len);
        }
        super::view::propagate_written_range_from_receiver(
            buf as usize,
            start as u32,
            dst,
            count as u32,
        );
    }
    buf
}

/// Allocate an uninitialized buffer
#[no_mangle]
pub extern "C" fn js_buffer_alloc_unsafe(size: i32) -> *mut BufferHeader {
    let size = validate_buffer_alloc_size(size);
    let buf = buffer_alloc(size);
    unsafe {
        (*buf).length = size;
    }
    buf
}

fn throw_buffer_concat_invalid_arg_type(index: usize, element: f64) -> ! {
    static REGISTER_TYPE_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_TYPE_ERROR.call_once(|| {
        crate::object::js_register_class_extends_error(crate::error::CLASS_ID_TYPE_ERROR);
    });

    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_TYPE_ERROR, 4);
    unsafe {
        let set = |key: &[u8], value: f64| {
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key_ptr, value);
        };
        let str_val = |s: &[u8]| -> f64 {
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(crate::JSValue::string_ptr(ptr).bits())
        };
        let message = format!(
            "The \"list[{index}]\" argument must be an instance of Buffer or Uint8Array. Received {}",
            crate::fs::validate::describe_received(element)
        );
        set(b"name", str_val(b"TypeError"));
        set(b"code", str_val(b"ERR_INVALID_ARG_TYPE"));
        set(b"message", str_val(message.as_bytes()));
    }
    crate::exception::js_throw(crate::value::js_nanbox_pointer(obj as i64))
}

fn normalize_buffer_concat_total_length(total_length: f64) -> Option<usize> {
    let jsval = crate::JSValue::from_bits(total_length.to_bits());
    if jsval.is_undefined() {
        return None;
    }
    if jsval.is_int32() {
        return Some((jsval.as_int32().max(0)) as usize);
    }
    if jsval.is_bool() {
        return Some(if jsval.as_bool() { 1 } else { 0 });
    }
    if jsval.is_null() || total_length.is_nan() || !total_length.is_finite() || total_length <= 0.0
    {
        return Some(0);
    }
    Some((total_length.trunc() as usize).min(u32::MAX as usize))
}

fn js_buffer_concat_impl(
    arr_ptr: *const ArrayHeader,
    requested_total_length: Option<usize>,
) -> *mut BufferHeader {
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

        let mut actual_total_size: usize = 0;
        for i in 0..len {
            let element = *arr_data.add(i);
            let raw_bits = strip_nanbox(element.to_bits());
            if raw_bits < 0x1000 || !is_registered_buffer(raw_bits as usize) {
                throw_buffer_concat_invalid_arg_type(i, element);
            }
            let buf_ptr = raw_bits as *const BufferHeader;
            actual_total_size = actual_total_size.saturating_add((*buf_ptr).length as usize);
        }
        let total_size = requested_total_length.unwrap_or(actual_total_size);
        let total_size = total_size.min(u32::MAX as usize);

        // Allocate result buffer
        let result = buffer_alloc(total_size as u32);
        (*result).length = total_size as u32;
        ptr::write_bytes(buffer_data_mut(result), 0, total_size);

        // Copy data
        let mut offset: usize = 0;
        for i in 0..len {
            let raw_bits = strip_nanbox((*arr_data.add(i)).to_bits());
            let buf_ptr = raw_bits as *const BufferHeader;
            let buf_len = (*buf_ptr).length as usize;
            let remaining = total_size.saturating_sub(offset);
            if remaining == 0 {
                break;
            }
            let copy_len = buf_len.min(remaining);
            let src_data = buffer_data(buf_ptr);
            let dst_data = buffer_data_mut(result).add(offset);
            ptr::copy_nonoverlapping(src_data, dst_data, copy_len);
            offset += copy_len;
        }

        result
    }
}

/// Concatenate multiple buffers.
#[no_mangle]
pub extern "C" fn js_buffer_concat(arr_ptr: *const ArrayHeader) -> *mut BufferHeader {
    js_buffer_concat_impl(arr_ptr, None)
}

/// Concatenate multiple buffers using Node's optional totalLength semantics.
#[no_mangle]
pub extern "C" fn js_buffer_concat_with_length(
    arr_ptr: *const ArrayHeader,
    total_length: f64,
) -> *mut BufferHeader {
    // #2013: a provided `totalLength` must be a non-negative integer; Node
    // throws `ERR_INVALID_ARG_TYPE` / `ERR_OUT_OF_RANGE` otherwise.
    super::validate::validate_concat_length(total_length);
    js_buffer_concat_impl(arr_ptr, normalize_buffer_concat_total_length(total_length))
}
