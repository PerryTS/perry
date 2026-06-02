struct TypedArrayView {
    kind: u8,
    data: *const u8,
    byte_len: usize,
}

fn value_pointer_addr(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let jv = crate::value::JSValue::from_bits(bits);
    if jv.is_pointer() {
        return Some(jv.as_pointer::<u8>() as usize);
    }
    if bits > 0x1000 && (bits >> 48) == 0 {
        return Some(bits as usize);
    }
    None
}

fn typed_array_view(value: f64) -> Option<TypedArrayView> {
    let addr = value_pointer_addr(value)?;
    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        let bytes = unsafe { crate::typedarray::typed_array_bytes(ta)? };
        return Some(TypedArrayView {
            kind,
            data: bytes.as_ptr(),
            byte_len: bytes.len(),
        });
    }
    if crate::buffer::is_registered_buffer(addr) && crate::buffer::is_uint8array_buffer(addr) {
        let buf = addr as *const crate::buffer::BufferHeader;
        let byte_len = unsafe { (*buf).length as usize };
        return Some(TypedArrayView {
            kind: crate::typedarray::KIND_UINT8,
            data: crate::buffer::buffer_data(buf),
            byte_len,
        });
    }
    None
}

pub(super) fn deep_strict_typed_array_equal(left: f64, right: f64) -> Option<bool> {
    let left_view = typed_array_view(left);
    let right_view = typed_array_view(right);
    match (left_view, right_view) {
        (Some(left_view), Some(right_view)) => {
            if left_view.kind != right_view.kind || left_view.byte_len != right_view.byte_len {
                return Some(false);
            }
            if left_view.byte_len == 0 {
                return Some(true);
            }
            unsafe {
                let left_bytes = std::slice::from_raw_parts(left_view.data, left_view.byte_len);
                let right_bytes = std::slice::from_raw_parts(right_view.data, right_view.byte_len);
                Some(left_bytes == right_bytes)
            }
        }
        (Some(_), None) | (None, Some(_)) => Some(false),
        (None, None) => None,
    }
}
