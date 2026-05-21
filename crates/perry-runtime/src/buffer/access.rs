use super::*;

/// Get a byte at the specified index
#[no_mangle]
pub extern "C" fn js_buffer_get(buf_ptr: *const BufferHeader, index: i32) -> i32 {
    if buf_ptr.is_null() || index < 0 {
        return 0;
    }
    unsafe {
        if index as u32 >= (*buf_ptr).length {
            return 0;
        }
        // Issue #1205: if the receiver is a registered view, read from
        // the ultimate backing buffer — otherwise the view's local
        // snapshot can lag any direct-fast-path write made to the
        // backing through codegen.
        let buf_addr = buf_ptr as usize;
        if let Some(info) = super::view::lookup(buf_addr) {
            let back_off = info.offset + index as u32;
            let backing_ptr = info.backing as *const BufferHeader;
            if !backing_ptr.is_null() && back_off < (*backing_ptr).length {
                let back_data = buffer_data(backing_ptr);
                return *back_data.add(back_off as usize) as i32;
            }
        }
        let data = buffer_data(buf_ptr);
        *data.add(index as usize) as i32
    }
}

/// Set a byte at the specified index
#[no_mangle]
pub extern "C" fn js_buffer_set(buf_ptr: *mut BufferHeader, index: i32, value: i32) {
    if buf_ptr.is_null() || index < 0 {
        return;
    }
    unsafe {
        if index as u32 >= (*buf_ptr).length {
            return;
        }
        let byte = (value & 0xFF) as u8;
        // Write the byte to the receiver's own data area first so a
        // direct codegen fast-path read of this buffer still sees the
        // update.
        let data = buffer_data_mut(buf_ptr);
        *data.add(index as usize) = byte;
        // Issue #1205: propagate through the view registry.  If the
        // receiver is itself a slice, mirror the write into the
        // ultimate backing buffer; in either direction, sister views
        // covering the same backing byte must observe the new value.
        let buf_addr = buf_ptr as usize;
        if let Some(info) = super::view::lookup(buf_addr) {
            let back_off = info.offset + index as u32;
            let backing_ptr = info.backing as *mut BufferHeader;
            if !backing_ptr.is_null() && back_off < (*backing_ptr).length {
                let back_data = buffer_data_mut(backing_ptr);
                *back_data.add(back_off as usize) = byte;
                super::view::propagate_byte_to_views(info.backing, back_off, byte, buf_addr);
            }
        } else {
            super::view::propagate_byte_to_views(buf_addr, index as u32, byte, buf_addr);
        }
    }
}

/// Copy bytes from source buffer into target buffer at given offset.
/// Implements Uint8Array.prototype.set(source, offset)
#[no_mangle]
pub extern "C" fn js_buffer_set_from(
    target: *mut BufferHeader,
    source: *const BufferHeader,
    offset: i32,
) {
    if target.is_null() || source.is_null() || offset < 0 {
        return;
    }
    // Strip NaN-boxing tags
    let target = {
        let bits = target as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *mut BufferHeader
        } else {
            target
        }
    };
    let source = {
        let bits = source as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader
        } else {
            source
        }
    };
    if target.is_null() || source.is_null() {
        return;
    }
    unsafe {
        let target_len = (*target).length as usize;
        let source_len = (*source).length as usize;
        let off = offset as usize;
        if off + source_len > target_len {
            return;
        } // Would overflow
        let target_data = buffer_data_mut(target);
        let source_data = buffer_data(source);
        ptr::copy_nonoverlapping(source_data, target_data.add(off), source_len);
    }
}

/// Create a slice of a buffer.  Issue #1205: the returned buffer is
/// a *view* over the source — registered in the view registry so that
/// subsequent reads/writes via the runtime helpers propagate between
/// the slice and the original.
#[no_mangle]
pub extern "C" fn js_buffer_slice(
    buf_ptr: *const BufferHeader,
    start: i32,
    end: i32,
) -> *mut BufferHeader {
    if buf_ptr.is_null() {
        return buffer_alloc(0);
    }

    unsafe {
        let len = (*buf_ptr).length as i32;

        // Handle negative indices
        let start = if start < 0 {
            (len + start).max(0)
        } else {
            start.min(len)
        };
        let end = if end < 0 {
            (len + end).max(0)
        } else {
            end.min(len)
        };

        if start >= end {
            return buffer_alloc(0);
        }

        let slice_len = (end - start) as u32;
        let result = buffer_alloc(slice_len);
        (*result).length = slice_len;

        let src_data = buffer_data(buf_ptr).add(start as usize);
        let dst_data = buffer_data_mut(result);
        ptr::copy_nonoverlapping(src_data, dst_data, slice_len as usize);

        // Register the alias.  `register` flattens slices-of-slices
        // so the recorded backing is always the original allocation.
        super::view::register(result as usize, buf_ptr as usize, start as u32, slice_len);

        result
    }
}
