use crate::JSValue;

fn value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

fn number_arg(value: f64, name: &str) -> Option<f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return None;
    }
    if !js.is_number() && !js.is_int32() {
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            name,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    Some(if js.is_int32() {
        js.as_int32() as f64
    } else {
        value
    })
}

fn range(total: usize, offset_bits: f64, size_bits: f64) -> (usize, usize) {
    let offset = match number_arg(offset_bits, "offset") {
        Some(n) if n.is_finite() && n >= 0.0 && n <= total as f64 => n as usize,
        Some(n) => {
            let message = format!(
                "The value of \"offset\" is out of range. It must be >= 0 && <= {}. Received {}",
                total, n
            );
            crate::fs::validate::throw_range_error_with_code(&message);
        }
        None => 0,
    };
    let size = match number_arg(size_bits, "size") {
        Some(n) if n.is_finite() && n >= 0.0 && n <= i32::MAX as f64 => n as usize,
        Some(n) => {
            let message = format!(
                "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
                n
            );
            crate::fs::validate::throw_range_error_with_code(&message);
        }
        None => total.saturating_sub(offset),
    };
    let end = offset.saturating_add(size);
    if end > total {
        let message = format!(
            "The value of \"size + offset\" is out of range. It must be <= {}. Received {}",
            total, end
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    (offset, size)
}

fn invalid_buf(value: f64) -> ! {
    let message = format!(
        "The \"buf\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(super) unsafe fn random_fill_sync(target: f64, offset_bits: f64, size_bits: f64) -> f64 {
    use rand::Rng;

    let addr = value_addr(target);
    let elem_size = if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
        crate::typedarray::elem_size_for_kind(kind).max(1)
    } else if crate::buffer::is_registered_buffer(addr) {
        1
    } else {
        invalid_buf(target)
    };
    let value = crate::value::js_nanbox_pointer(addr as i64);
    let byte_len = crate::buffer::bytes::no_gc(|scope| {
        crate::buffer::bytes::bytes(value, scope).map(<[u8]>::len)
    })
    .unwrap_or_else(|_| invalid_buf(target));
    let (start, count) = range(byte_len / elem_size, offset_bits, size_bits);
    let start = start.saturating_mul(elem_size);
    let count = count
        .saturating_mul(elem_size)
        .min(byte_len.saturating_sub(start));
    crate::buffer::bytes::no_gc(|scope| {
        let data = crate::buffer::bytes::bytes_mut(value, scope)?;
        rand::rng().fill_bytes(&mut data[start..start + count]);
        Ok::<_, crate::buffer::bytes::NotBytes>(())
    })
    .unwrap_or_else(|_| invalid_buf(target));
    target
}
