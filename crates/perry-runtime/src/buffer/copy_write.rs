use super::*;

/// Copy data from source buffer to target buffer
/// Returns the number of bytes copied
#[no_mangle]
pub extern "C" fn js_buffer_copy(
    src_ptr: *const BufferHeader,
    dst_ptr: *mut BufferHeader,
    target_start: i32,
    source_start: i32,
    source_end: i32,
) -> i32 {
    if src_ptr.is_null() || dst_ptr.is_null() {
        return 0;
    }

    super::bytes::no_gc(|_scope| unsafe {
        let Ok(source) = super::bytes::span(crate::value::js_nanbox_pointer(src_ptr as i64), false)
        else {
            return 0;
        };
        let Ok(target) = super::bytes::span(crate::value::js_nanbox_pointer(dst_ptr as i64), true)
        else {
            return 0;
        };
        let src_len = source.len as i32;
        let dst_len = target.len as i32;
        let target_start = target_start.max(0).min(dst_len);
        let source_start = source_start.max(0).min(src_len);
        let source_end = if source_end < 0 {
            src_len
        } else {
            source_end.min(src_len)
        };
        let count = (source_end - source_start)
            .min(dst_len - target_start)
            .max(0);
        // Raw spans avoid creating aliased Rust references for overlapping JS views.
        ptr::copy(
            source.ptr.add(source_start as usize),
            target.ptr.add(target_start as usize),
            count as usize,
        );
        count
    })
}

/// Write a string to a buffer
/// Returns the number of bytes written
#[no_mangle]
pub extern "C" fn js_buffer_write(
    buf_ptr: *mut BufferHeader,
    str_ptr: *const StringHeader,
    offset: i32,
    encoding: i32,
) -> i32 {
    if buf_ptr.is_null() || str_ptr.is_null() {
        return 0;
    }

    unsafe {
        let buf_len = (super::store::length(buf_ptr as usize) as u32) as i32;
        let offset = offset.max(0).min(buf_len);

        let str_len = (*str_ptr).byte_len as usize;
        let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(str_data, str_len);

        let bytes_to_write = super::from::buffer_string_bytes_for_encoding(str_bytes, encoding);

        let available = (buf_len - offset) as usize;
        let write_len = bytes_to_write.len().min(available);

        super::bytes::no_gc(|scope| {
            if let Ok(dst) =
                super::bytes::bytes_mut(crate::value::js_nanbox_pointer(buf_ptr as i64), scope)
            {
                dst[offset as usize..offset as usize + write_len]
                    .copy_from_slice(&bytes_to_write[..write_len]);
            }
        });

        write_len as i32
    }
}

/// Write a string to a buffer, honoring Node's optional `length` argument.
#[no_mangle]
pub extern "C" fn js_buffer_write_len(
    buf_ptr: *mut BufferHeader,
    str_ptr: *const StringHeader,
    offset: i32,
    max_len: i32,
    encoding: i32,
) -> i32 {
    if buf_ptr.is_null() || str_ptr.is_null() {
        return 0;
    }

    unsafe {
        let buf_len = (super::store::length(buf_ptr as usize) as u32) as i32;
        let offset = offset.max(0).min(buf_len);

        let str_len = (*str_ptr).byte_len as usize;
        let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(str_data, str_len);

        // Share the encoding table with the no-length entry point, including
        // utf16le/ucs2, base64url, and latin1/ascii.
        let bytes_to_write = super::from::buffer_string_bytes_for_encoding(str_bytes, encoding);

        let available = (buf_len - offset) as usize;
        let cap = max_len.max(0) as usize;
        let write_len = bytes_to_write.len().min(available).min(cap);

        super::bytes::no_gc(|scope| {
            if let Ok(dst) =
                super::bytes::bytes_mut(crate::value::js_nanbox_pointer(buf_ptr as i64), scope)
            {
                dst[offset as usize..offset as usize + write_len]
                    .copy_from_slice(&bytes_to_write[..write_len]);
            }
        });

        write_len as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_write_entry_points_decode_every_encoding_tag() {
        let cases: &[(&str, &str, i32, &[u8])] = &[
            ("utf8", "Aé", 0, b"A\xc3\xa9"),
            ("hex", "41e9", 1, &[0x41, 0xe9]),
            ("base64", "SGk=", 2, b"Hi"),
            ("base64url", "_w==", 3, &[0xff]),
            ("latin1", "Aé", 4, &[0x41, 0xe9]),
            ("ascii", "Aé", 5, &[0x41, 0xe9]),
            ("utf16le/ucs2", "Aé", 6, &[0x41, 0x00, 0xe9, 0x00]),
        ];

        for &(name, input, encoding, expected) in cases {
            for with_length in [false, true] {
                let buffer = js_buffer_alloc(16, 0x7f);
                let string =
                    crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
                let written = if with_length {
                    js_buffer_write_len(buffer, string, 1, 15, encoding)
                } else {
                    js_buffer_write(buffer, string, 1, encoding)
                };
                assert_eq!(
                    written as usize,
                    expected.len(),
                    "{name}, with_length={with_length}"
                );
                let bytes = super::bytes::no_gc(|scope| {
                    super::bytes::bytes(crate::value::js_nanbox_pointer(buffer as i64), scope)
                        .unwrap()
                        .to_vec()
                });
                assert_eq!(bytes[0], 0x7f, "{name}, with_length={with_length}");
                assert_eq!(
                    &bytes[1..1 + expected.len()],
                    expected,
                    "{name}, with_length={with_length}"
                );
                assert_eq!(
                    bytes[1 + expected.len()],
                    0x7f,
                    "{name}, with_length={with_length}"
                );
            }
        }
    }
}
