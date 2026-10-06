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

    super::bytes::no_gc(|scope| {
        let a =
            super::bytes::bytes(crate::value::js_nanbox_pointer(p1 as i64), scope).unwrap_or(&[]);
        let b =
            super::bytes::bytes(crate::value::js_nanbox_pointer(p2 as i64), scope).unwrap_or(&[]);
        i32::from(a == b)
    })
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
    super::bytes::no_gc(|scope| {
        let a =
            super::bytes::bytes(crate::value::js_nanbox_pointer(pa as i64), scope).unwrap_or(&[]);
        let b =
            super::bytes::bytes(crate::value::js_nanbox_pointer(pb as i64), scope).unwrap_or(&[]);
        ordering(a.cmp(b))
    })
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
    let result = super::bytes::no_gc(|scope| {
        let a =
            super::bytes::bytes(crate::value::js_nanbox_pointer(pa as i64), scope).unwrap_or(&[]);
        let b =
            super::bytes::bytes(crate::value::js_nanbox_pointer(pb as i64), scope).unwrap_or(&[]);
        if target_start < 0
            || target_end < target_start
            || target_end as usize > b.len()
            || source_start < 0
            || source_end < source_start
            || source_end as usize > a.len()
        {
            return None;
        }
        Some(ordering(
            a[source_start as usize..source_end as usize]
                .cmp(&b[target_start as usize..target_end as usize]),
        ))
    });
    result.unwrap_or_else(|| super::numeric::throw_out_of_range())
}

/// Search for a byte sequence in a buffer.
fn buffer_index_of_bytes(buf: *const BufferHeader, needle: &[u8], start: i32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    super::bytes::no_gc(|scope| {
        let data =
            super::bytes::bytes(crate::value::js_nanbox_pointer(buf as i64), scope).unwrap_or(&[]);
        let len = data.len();
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
    })
}

/// Reverse search for a byte sequence in a buffer.
fn buffer_last_index_of_bytes(buf: *const BufferHeader, needle: &[u8], start: i32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    super::bytes::no_gc(|scope| {
        let data =
            super::bytes::bytes(crate::value::js_nanbox_pointer(buf as i64), scope).unwrap_or(&[]);
        let len = data.len();
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
    })
}

/// The needle bytes of an SSO (inline short string) needle under `encoding`,
/// or `None` when `needle` is not an SSO string.
fn sso_needle_bytes(needle: f64, encoding: i32) -> Option<Vec<u8>> {
    if !crate::value::JSValue::from_bits(needle.to_bits()).is_short_string() {
        return None;
    }
    let str_ptr =
        crate::value::js_get_string_pointer_unified(needle) as usize as *const StringHeader;
    if str_ptr.is_null() {
        return None;
    }
    let owned = unsafe { crate::string::OwnedStringBytes::copy_from_header(str_ptr) };
    Some(super::from::buffer_string_bytes_for_encoding(
        owned.as_bytes(),
        encoding,
    ))
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
    // #11430: an SSO short string carries its bytes inline (no heap header),
    // so it must be materialized before the heap-string branch can read it.
    if let Some(bytes) = sso_needle_bytes(needle, encoding) {
        return Some(bytes);
    }

    let raw_ptr = if top16 >= 0x7FF8 {
        (needle_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && needle_bits >= 0x1000 && super::header_is_owned(needle_bits as usize) {
        // Raw word: the allocator vouches before the brand read (#10694).
        needle_bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        return super::bytes::no_gc(|scope| {
            super::bytes::bytes(crate::value::js_nanbox_pointer(raw_ptr as i64), scope)
                .ok()
                .map(<[u8]>::to_vec)
        });
    }
    if top16 == 0x7FFF {
        let str_ptr = (needle_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !str_ptr.is_null() {
            return unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                let decoded = super::from::buffer_string_bytes_for_encoding(bytes, encoding);
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
    if let Some(bytes) = sso_needle_bytes(needle, encoding) {
        return buffer_index_of_bytes(buf, &bytes, start);
    }

    // Buffer needle (POINTER_TAG-boxed or raw)
    let raw_ptr = if top16 >= 0x7FF8 {
        (needle_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && needle_bits >= 0x1000 && super::header_is_owned(needle_bits as usize) {
        // Raw word: the allocator vouches before the brand read (#10694).
        needle_bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        return super::bytes::no_gc(|scope| {
            let needle =
                super::bytes::bytes(crate::value::js_nanbox_pointer(raw_ptr as i64), scope)
                    .unwrap_or(&[]);
            buffer_index_of_bytes(buf, needle, start)
        });
    }
    // String needle (STRING_TAG-boxed)
    if top16 == 0x7FFF {
        let str_ptr = (needle_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if !str_ptr.is_null() {
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                let decoded = super::from::buffer_string_bytes_for_encoding(bytes, encoding);
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
    let source = (!buf.is_null()).then(|| {
        super::bytes::ReadLease::new(crate::value::js_nanbox_pointer(buf as i64)).unwrap()
    });
    let handles = crate::gc::RuntimeHandleScope::new();
    let obj = handles.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    {
        let type_key = crate::string::js_string_from_bytes(b"type".as_ptr(), 4);
        let type_val = crate::string::js_string_from_bytes(b"Buffer".as_ptr(), 6);
        crate::object::js_object_set_field_by_name(
            obj.get_raw_mut_ptr(),
            type_key,
            f64::from_bits(crate::JSValue::string_ptr(type_val).bits()),
        );

        let arr = handles.root_raw_mut_ptr(crate::array::js_array_alloc(0));
        if let Some(source) = source.as_ref() {
            for byte in source.iter() {
                arr.set_raw_mut_ptr(crate::array::js_array_push_f64(
                    arr.get_raw_mut_ptr(),
                    *byte as f64,
                ));
            }
        }
        let data_key = crate::string::js_string_from_bytes(b"data".as_ptr(), 4);
        crate::object::js_object_set_field_by_name(
            obj.get_raw_mut_ptr(),
            data_key,
            f64::from_bits(
                crate::JSValue::pointer(arr.get_raw_mut_ptr::<ArrayHeader>() as *mut u8).bits(),
            ),
        );
    }
    f64::from_bits(
        crate::JSValue::pointer(obj.get_raw_mut_ptr::<crate::object::ObjectHeader>() as *mut u8)
            .bits(),
    )
}

fn ordering(order: std::cmp::Ordering) -> i32 {
    match order {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
