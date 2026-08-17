//! Native-memory views for `bun:ffi` (#6562 stage 2).
//!
//! `toArrayBuffer` and `toBuffer` create GC-managed JS wrappers whose data
//! pointer refers directly to memory owned by the native library. Perry never
//! frees that memory: as in Bun, the caller is responsible for keeping the
//! allocation alive while the returned view is reachable.

use crate::value::JSValue;

fn throw_type(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn throw_range(message: &str) -> ! {
    crate::fs::validate::throw_range_error_with_code(message)
}

unsafe fn pointer_address(value: f64) -> usize {
    let jv = JSValue::from_bits(value.to_bits());
    let address = if jv.is_int32() {
        let n = jv.as_int32();
        if n < 0 {
            throw_type("ptr must be a non-negative number or bigint");
        }
        n as usize
    } else if jv.is_number() {
        let n = jv.as_number();
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > usize::MAX as f64 {
            throw_type("ptr must be a non-negative integer number or bigint");
        }
        n as usize
    } else if jv.is_bigint() {
        let raw = crate::value::js_nanbox_get_bigint(value);
        if raw == 0 {
            0
        } else {
            let bigint = &*(raw as usize as *const crate::bigint::BigIntHeader);
            if bigint.limbs[1..].iter().any(|&limb| limb != 0) {
                throw_type("ptr bigint is outside the native pointer range");
            }
            bigint.limbs[0] as usize
        }
    } else {
        throw_type("ptr must be a number or bigint");
    };
    if address == 0 {
        throw_type("ptr cannot be zero");
    }
    address
}

fn optional_integer(value: f64, name: &str) -> Option<i64> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return None;
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else {
        throw_type(&format!("{name} must be a number"));
    };
    if !n.is_finite() || n < i64::MIN as f64 || n > i64::MAX as f64 {
        throw_range(&format!("{name} is out of range"));
    }
    Some(n.trunc() as i64)
}

fn address_with_offset(address: usize, offset: i64) -> usize {
    let start = address as i128 + offset as i128;
    if start <= 0 || start > usize::MAX as i128 {
        throw_range("ptr + byteOffset is outside the native pointer range");
    }
    start as usize
}

unsafe fn nul_terminated_len(start: usize) -> u32 {
    let ptr = start as *const u8;
    let mut len = 0u32;
    while *ptr.add(len as usize) != 0 {
        if len == u32::MAX {
            throw_range("native memory view exceeds Perry's maximum byteLength");
        }
        len += 1;
    }
    len
}

/// `toArrayBuffer(ptr[, byteOffset[, byteLength]])` and the sibling
/// `toBuffer` operation. When `byteLength` is omitted the span is treated as
/// NUL-terminated, matching Bun's public API.
pub(crate) unsafe fn view_value(
    pointer_arg: f64,
    offset_arg: f64,
    length_arg: f64,
    array_buffer: bool,
) -> f64 {
    let address = pointer_address(pointer_arg);
    let offset = optional_integer(offset_arg, "byteOffset").unwrap_or(0);
    let start = address_with_offset(address, offset);
    let length = match optional_integer(length_arg, "byteLength") {
        Some(n) if n < 0 => throw_range("byteLength is out of range"),
        Some(n) if n > u32::MAX as i64 => {
            throw_range("byteLength exceeds Perry's maximum ArrayBuffer size")
        }
        Some(n) => n as u32,
        None => nul_terminated_len(start),
    };

    let buffer = crate::buffer::buffer_alloc_foreign(start as *mut u8, length);
    if array_buffer {
        crate::buffer::mark_as_array_buffer(buffer as usize);
    }
    f64::from_bits(JSValue::pointer(buffer as *mut u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_pointer_offset_accepts_both_directions() {
        assert_eq!(address_with_offset(0x2000, 17), 0x2011);
        assert_eq!(address_with_offset(0x2000, -17), 0x1fef);
    }

    #[test]
    fn external_array_buffer_aliases_native_memory_without_copying() {
        let mut bytes = [10u8, 20, 30, 0];
        let value = unsafe { view_value(bytes.as_mut_ptr() as usize as f64, 1.0, 2.0, true) };
        let address = crate::value::js_nanbox_get_pointer(value) as usize;
        let buffer = address as *mut crate::buffer::BufferHeader;

        assert!(crate::buffer::is_array_buffer(address));
        assert_eq!(unsafe { (*buffer).length }, 2);
        assert_eq!(crate::buffer::buffer_data(buffer), unsafe {
            bytes.as_ptr().add(1)
        });

        unsafe {
            *crate::buffer::buffer_data_mut(buffer).add(1) = 99;
        }
        assert_eq!(bytes, [10, 20, 99, 0]);

        // The wrapper owns no native bytes; finalization only forgets the
        // mapping so a recycled GC address cannot inherit this stack pointer.
        crate::buffer::finalize_collected_dead_buffer(address);
    }
}
