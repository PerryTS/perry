use super::*;

/// Strip POINTER_TAG NaN-box bits from a buffer-pointer-like u64. Returns
/// the raw heap address as usize. Returns 0 if the input is below the heap.
pub fn unbox_buffer_ptr(bits: u64) -> usize {
    let top16 = bits >> 48;
    let raw = if top16 >= 0x7FF8 {
        bits & 0x0000_FFFF_FFFF_FFFF
    } else {
        bits
    };
    if raw < 0x1000 {
        0
    } else {
        raw as usize
    }
}

/// Compare two buffers for equality
#[no_mangle]
pub extern "C" fn js_buffer_equals(
    buf1_ptr: *const BufferHeader,
    buf2_ptr: *const BufferHeader,
) -> i32 {
    let p1 = unbox_buffer_ptr(buf1_ptr as u64) as *const BufferHeader;
    let p2 = unbox_buffer_ptr(buf2_ptr as u64) as *const BufferHeader;
    if p1.is_null() && p2.is_null() {
        return 1;
    }
    if p1.is_null() || p2.is_null() {
        return 0;
    }

    unsafe {
        let len1 = (*p1).length;
        let len2 = (*p2).length;

        if len1 != len2 {
            return 0;
        }

        let data1 = buffer_data(p1);
        let data2 = buffer_data(p2);

        for i in 0..len1 as usize {
            if *data1.add(i) != *data2.add(i) {
                return 0;
            }
        }

        1
    }
}

/// Lexicographic compare of two buffers (Buffer.compare semantics).
/// Returns -1, 0, or 1 (i32).
#[no_mangle]
pub extern "C" fn js_buffer_compare(a: *const BufferHeader, b: *const BufferHeader) -> i32 {
    let pa = unbox_buffer_ptr(a as u64) as *const BufferHeader;
    let pb = unbox_buffer_ptr(b as u64) as *const BufferHeader;
    if pa.is_null() && pb.is_null() {
        return 0;
    }
    if pa.is_null() {
        return -1;
    }
    if pb.is_null() {
        return 1;
    }
    unsafe {
        let la = (*pa).length as usize;
        let lb = (*pb).length as usize;
        let da = std::slice::from_raw_parts(buffer_data(pa), la);
        let db = std::slice::from_raw_parts(buffer_data(pb), lb);
        match da.cmp(db) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Lexicographic compare over Node's range-argument form:
/// `buf.compare(target, targetStart, targetEnd, sourceStart, sourceEnd)`.
#[no_mangle]
pub extern "C" fn js_buffer_compare_range(
    a: *const BufferHeader,
    b: *const BufferHeader,
    target_start: i32,
    target_end: i32,
    source_start: i32,
    source_end: i32,
) -> i32 {
    let pa = unbox_buffer_ptr(a as u64) as *const BufferHeader;
    let pb = unbox_buffer_ptr(b as u64) as *const BufferHeader;
    if pa.is_null() && pb.is_null() {
        return 0;
    }
    if pa.is_null() {
        return -1;
    }
    if pb.is_null() {
        return 1;
    }
    unsafe {
        let la = (*pa).length as i32;
        let lb = (*pb).length as i32;
        // Node throws ERR_OUT_OF_RANGE when any range arg is outside
        // [0, length] or when start > end. The previous silent-clamp
        // matched Perry's pre-error-shape convention; align with Node
        // now that the error helper exists.
        if target_start < 0
            || target_end < target_start
            || target_end > lb
            || source_start < 0
            || source_end < source_start
            || source_end > la
        {
            super::numeric::throw_out_of_range();
        }
        let ss = source_start;
        let se = source_end;
        let ts = target_start;
        let te = target_end;
        let da = std::slice::from_raw_parts(buffer_data(pa).add(ss as usize), (se - ss) as usize);
        let db = std::slice::from_raw_parts(buffer_data(pb).add(ts as usize), (te - ts) as usize);
        match da.cmp(db) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

/// Search for a byte sequence in a buffer.
fn buffer_index_of_bytes(buf: *const BufferHeader, needle: &[u8], start: i32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    unsafe {
        let len = (*buf).length as usize;
        let data = std::slice::from_raw_parts(buffer_data(buf), len);
        let from = if start < 0 {
            ((len as i32) + start).max(0) as usize
        } else {
            (start as usize).min(len)
        };
        if needle.is_empty() {
            return from as i32;
        }
        if needle.len() > len.saturating_sub(from) {
            return -1;
        }
        for i in from..=(len - needle.len()) {
            if &data[i..i + needle.len()] == needle {
                return i as i32;
            }
        }
        -1
    }
}

/// Reverse search for a byte sequence in a buffer.
fn buffer_last_index_of_bytes(buf: *const BufferHeader, needle: &[u8], start: i32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    unsafe {
        let len = (*buf).length as usize;
        let data = std::slice::from_raw_parts(buffer_data(buf), len);
        if needle.is_empty() {
            return if start < 0 {
                ((len as i32) + start).clamp(0, len as i32)
            } else {
                (start as usize).min(len) as i32
            };
        }
        if needle.len() > len {
            return -1;
        }
        let max_start = len - needle.len();
        let from = if start < 0 {
            ((len as i32) + start).max(0) as usize
        } else {
            (start as usize).min(max_start)
        };
        for i in (0..=from).rev() {
            if &data[i..i + needle.len()] == needle {
                return i as i32;
            }
        }
        -1
    }
}

fn buffer_search_needle_with_encoding(
    buf: *const BufferHeader,
    needle: f64,
    encoding: i32,
) -> Option<Vec<u8>> {
    if buf.is_null() {
        return None;
    }
    let needle_bits = needle.to_bits();
    let top16 = needle_bits >> 48;

    let raw_ptr = if top16 >= 0x7FF8 {
        (needle_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && needle_bits >= 0x1000 {
        needle_bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        let other = raw_ptr as *const BufferHeader;
        return unsafe {
            Some(std::slice::from_raw_parts(buffer_data(other), (*other).length as usize).to_vec())
        };
    }
    if top16 == 0x7FFF {
        let str_ptr = (needle_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !str_ptr.is_null() {
            return unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                let decoded = match encoding {
                    1 => decode_hex(bytes),
                    2 | 3 => decode_base64(bytes),
                    _ => bytes.to_vec(),
                };
                Some(decoded)
            };
        }
    }
    let byte_val = if top16 == 0x7FFE {
        (needle_bits as u32) & 0xFF
    } else if top16 < 0x7FF8 || (top16 == 0x7FF8 && needle_bits == 0x7FF8_0000_0000_0000) {
        ((needle as i64) & 0xFF) as u32
    } else {
        return None;
    };
    Some(vec![byte_val as u8])
}

fn buffer_search_needle(buf: *const BufferHeader, needle: f64) -> Option<Vec<u8>> {
    buffer_search_needle_with_encoding(buf, needle, 0)
}

/// `buf.indexOf(needle, start?)` where `needle` is a string, buffer,
/// or numeric byte value (NaN-boxed value).
#[no_mangle]
pub extern "C" fn js_buffer_index_of(buf_ptr: f64, needle: f64, start: i32) -> i32 {
    js_buffer_index_of_enc(buf_ptr, needle, start, 0)
}

#[no_mangle]
pub extern "C" fn js_buffer_index_of_enc(
    buf_ptr: f64,
    needle: f64,
    start: i32,
    encoding: i32,
) -> i32 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    if buf.is_null() {
        return -1;
    }
    let needle_bits = needle.to_bits();
    let top16 = needle_bits >> 48;

    // Buffer needle (POINTER_TAG-boxed or raw)
    let raw_ptr = if top16 >= 0x7FF8 {
        (needle_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && needle_bits >= 0x1000 {
        needle_bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        let other = raw_ptr as *const BufferHeader;
        let needle_slice =
            unsafe { std::slice::from_raw_parts(buffer_data(other), (*other).length as usize) };
        return buffer_index_of_bytes(buf, needle_slice, start);
    }
    // String needle (STRING_TAG-boxed)
    if top16 == 0x7FFF {
        let str_ptr = (needle_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !str_ptr.is_null() {
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                let decoded = match encoding {
                    1 => decode_hex(bytes),
                    2 | 3 => decode_base64(bytes),
                    _ => bytes.to_vec(),
                };
                return buffer_index_of_bytes(buf, &decoded, start);
            }
        }
    }
    // Numeric byte needle — INT32_TAG or plain double
    let byte_val = if top16 == 0x7FFE {
        // INT32_TAG: lower 32 bits are an i32
        (needle_bits as u32) & 0xFF
    } else if top16 < 0x7FF8 || (top16 == 0x7FF8 && needle_bits == 0x7FF8_0000_0000_0000) {
        // Raw double — convert to byte
        ((needle as i64) & 0xFF) as u32
    } else {
        return -1;
    };
    let byte = [byte_val as u8];
    buffer_index_of_bytes(buf, &byte, start)
}

/// `buf.lastIndexOf(needle, start?)` where `needle` is a string, buffer,
/// or numeric byte value (NaN-boxed value).
#[no_mangle]
pub extern "C" fn js_buffer_last_index_of(buf_ptr: f64, needle: f64, start: i32) -> i32 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let Some(bytes) = buffer_search_needle(buf, needle) else {
        return -1;
    };
    buffer_last_index_of_bytes(buf, &bytes, start)
}

#[no_mangle]
pub extern "C" fn js_buffer_last_index_of_enc(
    buf_ptr: f64,
    needle: f64,
    start: i32,
    encoding: i32,
) -> i32 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let Some(bytes) = buffer_search_needle_with_encoding(buf, needle, encoding) else {
        return -1;
    };
    buffer_last_index_of_bytes(buf, &bytes, start)
}

/// `buf.includes(needle, start?)` — boolean i32.
#[no_mangle]
pub extern "C" fn js_buffer_includes(buf_ptr: f64, needle: f64, start: i32) -> i32 {
    if js_buffer_index_of(buf_ptr, needle, start) >= 0 {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_includes_enc(
    buf_ptr: f64,
    needle: f64,
    start: i32,
    encoding: i32,
) -> i32 {
    if js_buffer_index_of_enc(buf_ptr, needle, start, encoding) >= 0 {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_to_json(buf_ptr: f64) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let obj = crate::object::js_object_alloc(0, 2);
    unsafe {
        let type_key = crate::string::js_string_from_bytes(b"type".as_ptr(), 4);
        let type_val = crate::string::js_string_from_bytes(b"Buffer".as_ptr(), 6);
        crate::object::js_object_set_field_by_name(
            obj,
            type_key,
            f64::from_bits(crate::JSValue::string_ptr(type_val).bits()),
        );

        let arr = crate::array::js_array_alloc(0);
        let mut arr_ptr = arr;
        if !buf.is_null() {
            let len = (*buf).length as usize;
            let data = buffer_data(buf);
            for i in 0..len {
                arr_ptr = crate::array::js_array_push_f64(arr_ptr, *data.add(i) as f64);
            }
        }
        let data_key = crate::string::js_string_from_bytes(b"data".as_ptr(), 4);
        crate::object::js_object_set_field_by_name(
            obj,
            data_key,
            f64::from_bits(crate::JSValue::pointer(arr_ptr as *mut u8).bits()),
        );
    }
    f64::from_bits(crate::JSValue::pointer(obj as *mut u8).bits())
}
