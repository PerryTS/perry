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

    unsafe {
        let src_len = (*src_ptr).length as i32;
        let dst_len = (*dst_ptr).length as i32;

        let target_start = target_start.max(0).min(dst_len);
        let source_start = source_start.max(0).min(src_len);
        let source_end = if source_end < 0 {
            src_len
        } else {
            source_end.min(src_len)
        };

        if source_start >= source_end {
            return 0;
        }

        let copy_len = (source_end - source_start).min(dst_len - target_start);
        if copy_len <= 0 {
            return 0;
        }

        let src_data = buffer_data(src_ptr).add(source_start as usize);
        let dst_data = buffer_data_mut(dst_ptr).add(target_start as usize);
        ptr::copy_nonoverlapping(src_data, dst_data, copy_len as usize);

        copy_len
    }
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
        let buf_len = (*buf_ptr).length as i32;
        let offset = offset.max(0).min(buf_len);

        let str_len = (*str_ptr).byte_len as usize;
        let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(str_data, str_len);

        let bytes_to_write = match encoding {
            1 => decode_hex(str_bytes),
            2 => decode_base64(str_bytes),
            _ => str_bytes.to_vec(),
        };

        let available = (buf_len - offset) as usize;
        let write_len = bytes_to_write.len().min(available);

        let dst_data = buffer_data_mut(buf_ptr).add(offset as usize);
        ptr::copy_nonoverlapping(bytes_to_write.as_ptr(), dst_data, write_len);

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
        let buf_len = (*buf_ptr).length as i32;
        let offset = offset.max(0).min(buf_len);

        let str_len = (*str_ptr).byte_len as usize;
        let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let str_bytes = std::slice::from_raw_parts(str_data, str_len);

        let bytes_to_write = match encoding {
            1 => decode_hex(str_bytes),
            2 | 3 => decode_base64(str_bytes),
            _ => str_bytes.to_vec(),
        };

        let available = (buf_len - offset) as usize;
        let cap = max_len.max(0) as usize;
        let write_len = bytes_to_write.len().min(available).min(cap);

        let dst_data = buffer_data_mut(buf_ptr).add(offset as usize);
        ptr::copy_nonoverlapping(bytes_to_write.as_ptr(), dst_data, write_len);

        write_len as i32
    }
}
