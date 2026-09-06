//! Serialize dense primitive arrays without recursive traversal or GC roots.
//! All scratch stays in the output buffer or on the stack. The general array
//! entry has already resolved forwarding and applied the array's `toJSON`.

use super::*;

/// Try a callback-free array walk. A failed attempt leaves the output and all
/// serializer state alone. Rust output-buffer growth does not
/// allocate in Perry's managed heap or reach a GC safepoint.
///
/// `arr` must be a resolved receiver, with its `toJSON` already handled.
/// `clean_arr_ptr` also accepts registered buffers/typed arrays for legacy
/// callers; require a tracked ARRAY header before reading the layout flags.
pub(super) unsafe fn try_emit(arr: *const crate::ArrayHeader, buf: &mut String) -> bool {
    let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(arr as usize) else {
        return false;
    };
    let header = header.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    let flags = header._reserved;
    if (*arr).length > (*arr).capacity
        || flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        || crate::array::array_has_named_properties_resolved(arr)
    {
        return false;
    }
    let data = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let len = (*arr).length as usize;
    let elements = std::slice::from_raw_parts(data, len);
    // The existing layout flag proves every live slot is an unboxed number,
    // with neither holes nor callbacks. No validation pass is needed.
    if flags & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0 {
        buf.push('[');
        for (i, &number) in elements.iter().enumerate() {
            if i != 0 {
                buf.push(',');
            }
            write_number(buf, number);
        }
        buf.push(']');
        return true;
    }
    // Validate before emitting, so an array ending in a complex value does
    // not speculatively serialize its entire prefix twice. Arrays of records
    // fail on their first element without growing the output buffer.
    if !elements.iter().all(|value| is_primitive(value.to_bits())) {
        return false;
    }
    buf.push('[');
    for (i, &value) in elements.iter().enumerate() {
        let bits = value.to_bits();
        if i != 0 {
            buf.push(',');
        }
        match bits {
            TAG_NULL | TAG_UNDEFINED => buf.push_str("null"),
            TAG_TRUE => buf.push_str("true"),
            TAG_FALSE => buf.push_str("false"),
            _ => match bits & crate::value::TAG_MASK {
                STRING_TAG => {
                    let ptr = (bits & POINTER_MASK) as *const StringHeader;
                    if let Some(s) = str_from_header(ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
                crate::value::SHORT_STRING_TAG => {
                    let value = JSValue::from_bits(bits);
                    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
                    let n = value.short_string_to_buf(&mut scratch);
                    if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
                _ => write_number(buf, f64::from_bits(bits)),
            },
        }
    }
    buf.push(']');
    true
}

#[inline]
unsafe fn is_primitive(bits: u64) -> bool {
    if bits == crate::value::TAG_HOLE {
        return false;
    }
    let tag = bits & crate::value::TAG_MASK;
    // BigInt can run a user-installed toJSON. Untagged pointers need the
    // same tracked-allocation disambiguation as the existing numeric path:
    // positive subnormal doubles must not be mistaken for addresses.
    tag != POINTER_TAG && tag != BIGINT_TAG && !is_raw_pointer(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_array_rejects_non_array_receivers_without_reading_their_layout() {
        unsafe {
            let mut output = String::from("prefix");
            for size in [4, 65536] {
                let buffer = crate::buffer::js_buffer_alloc(size, 0);
                assert!(!try_emit(buffer as *const crate::ArrayHeader, &mut output));
            }
            assert!(!try_emit(std::ptr::null(), &mut output));
            assert_eq!(output, "prefix");
        }
    }

    #[test]
    fn primitive_array_rejects_complex_values_before_emission() {
        unsafe {
            let arr = crate::array::js_array_alloc(4);
            crate::array::js_array_push(arr, JSValue::number(1.0));
            crate::array::js_array_push(arr, JSValue::number(2.5));
            let object = crate::object::js_object_alloc(0, 0);
            crate::array::js_array_push(arr, JSValue::pointer(object as *const u8));
            let mut output = String::from("prefix");
            let capacity = output.capacity();
            assert!(!try_emit(arr, &mut output));
            assert_eq!(output, "prefix");
            assert_eq!(output.capacity(), capacity);
        }
    }

    #[test]
    fn primitive_array_emits_without_changing_serializer_state() {
        unsafe {
            let text = b"[null,true,false,1,2.5,\"a\",\"longer string\",\"\\ud800\"]";
            let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let value = super::super::test_json_parse_direct(source);
            let arr = value.as_pointer() as *const crate::ArrayHeader;
            let mut output = String::from("prefix");
            let stack_len = STRINGIFY_STACK.with(|s| s.borrow().len());
            assert!(try_emit(arr, &mut output));
            assert_eq!(output.as_bytes(), [b"prefix".as_slice(), text].concat());
            assert_eq!(STRINGIFY_STACK.with(|s| s.borrow().len()), stack_len);
        }
    }
}
