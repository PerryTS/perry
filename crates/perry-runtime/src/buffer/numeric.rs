use super::*;

// ---------------------------------------------------------------------
// Numeric read/write helpers
// ---------------------------------------------------------------------

#[inline]
fn buffer_slice_at<'a>(buf: *const BufferHeader, offset: i32, n: usize) -> Option<&'a [u8]> {
    if buf.is_null() || offset < 0 {
        return None;
    }
    unsafe {
        let len = (*buf).length as usize;
        let off = offset as usize;
        if off.checked_add(n)? > len {
            return None;
        }
        Some(std::slice::from_raw_parts(buffer_data(buf).add(off), n))
    }
}

#[inline]
fn buffer_slice_at_mut<'a>(buf: *mut BufferHeader, offset: i32, n: usize) -> Option<&'a mut [u8]> {
    if buf.is_null() || offset < 0 {
        return None;
    }
    unsafe {
        let len = (*buf).length as usize;
        let off = offset as usize;
        if off.checked_add(n)? > len {
            return None;
        }
        Some(std::slice::from_raw_parts_mut(
            buffer_data_mut(buf).add(off),
            n,
        ))
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint8(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 1) {
        Some(s) => s[0] as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int8(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 1) {
        Some(s) => (s[0] as i8) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint16_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) {
        Some(s) => u16::from_be_bytes([s[0], s[1]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint16_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) {
        Some(s) => u16::from_le_bytes([s[0], s[1]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int16_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) {
        Some(s) => i16::from_be_bytes([s[0], s[1]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int16_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 2) {
        Some(s) => i16::from_le_bytes([s[0], s[1]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint32_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) {
        Some(s) => u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint32_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) {
        Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int32_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) {
        Some(s) => i32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int32_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) {
        Some(s) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_float_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) {
        Some(s) => f32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_float_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 4) {
        Some(s) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_double_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 8) {
        Some(s) => f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_double_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    match buffer_slice_at(buf, offset, 8) {
        Some(s) => f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0.0,
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint8(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 1) {
        s[0] = (value as i64 & 0xFF) as u8;
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int8(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint8(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint16_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 2) {
        let bytes = (value as i64 as u16).to_be_bytes();
        s[0] = bytes[0];
        s[1] = bytes[1];
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint16_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 2) {
        let bytes = (value as i64 as u16).to_le_bytes();
        s[0] = bytes[0];
        s[1] = bytes[1];
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int16_be(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint16_be(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int16_le(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint16_le(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint32_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as i64 as u32).to_be_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint32_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as i64 as u32).to_le_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int32_be(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint32_be(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int32_le(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_uint32_le(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_float_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as f32).to_be_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_float_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 4) {
        let bytes = (value as f32).to_le_bytes();
        s[..4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_double_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&value.to_be_bytes());
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_double_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&value.to_le_bytes());
    }
}

// ---- Variable byteLength read/write (1..=6) ----
// Node-spec `buf.{read,write}{U,}Int{BE,LE}(offset, byteLength)` — accept
// any `byteLength` in 1..=6 and decode/encode that many bytes in the
// requested endianness. Used by BSON `ObjectId` (3-byte counter) and any
// other code that wants a width unknown at compile time. Out-of-range
// `byteLength` falls back to `undefined` for reads / no-op for writes,
// matching Perry's existing tolerant-on-bad-args buffer convention.

#[no_mangle]
pub extern "C" fn js_buffer_read_uint_be(buf_ptr: f64, offset: i32, byte_length: i32) -> f64 {
    if !(1..=6).contains(&byte_length) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let n = byte_length as usize;
    match buffer_slice_at(buf, offset, n) {
        Some(s) => {
            let mut v: u64 = 0;
            for &b in s.iter() {
                v = (v << 8) | (b as u64);
            }
            v as f64
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_uint_le(buf_ptr: f64, offset: i32, byte_length: i32) -> f64 {
    if !(1..=6).contains(&byte_length) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let n = byte_length as usize;
    match buffer_slice_at(buf, offset, n) {
        Some(s) => {
            let mut v: u64 = 0;
            for (i, &b) in s.iter().enumerate() {
                v |= (b as u64) << (i * 8);
            }
            v as f64
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

#[inline]
fn sign_extend(v: u64, bits: u32) -> i64 {
    let sign_bit = 1u64 << (bits - 1);
    let mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let v = v & mask;
    if v & sign_bit != 0 {
        (v | !mask) as i64
    } else {
        v as i64
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int_be(buf_ptr: f64, offset: i32, byte_length: i32) -> f64 {
    if !(1..=6).contains(&byte_length) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let n = byte_length as usize;
    match buffer_slice_at(buf, offset, n) {
        Some(s) => {
            let mut v: u64 = 0;
            for &b in s.iter() {
                v = (v << 8) | (b as u64);
            }
            sign_extend(v, (n * 8) as u32) as f64
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_read_int_le(buf_ptr: f64, offset: i32, byte_length: i32) -> f64 {
    if !(1..=6).contains(&byte_length) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let n = byte_length as usize;
    match buffer_slice_at(buf, offset, n) {
        Some(s) => {
            let mut v: u64 = 0;
            for (i, &b) in s.iter().enumerate() {
                v |= (b as u64) << (i * 8);
            }
            sign_extend(v, (n * 8) as u32) as f64
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint_be(buf_ptr: f64, value: f64, offset: i32, byte_length: i32) {
    if !(1..=6).contains(&byte_length) {
        return;
    }
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    let n = byte_length as usize;
    if let Some(s) = buffer_slice_at_mut(buf, offset, n) {
        let v = value as i64 as u64;
        for i in 0..n {
            s[n - 1 - i] = ((v >> (i * 8)) & 0xFF) as u8;
        }
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_uint_le(buf_ptr: f64, value: f64, offset: i32, byte_length: i32) {
    if !(1..=6).contains(&byte_length) {
        return;
    }
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    let n = byte_length as usize;
    if let Some(s) = buffer_slice_at_mut(buf, offset, n) {
        let v = value as i64 as u64;
        for i in 0..n {
            s[i] = ((v >> (i * 8)) & 0xFF) as u8;
        }
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int_be(buf_ptr: f64, value: f64, offset: i32, byte_length: i32) {
    js_buffer_write_uint_be(buf_ptr, value, offset, byte_length);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_int_le(buf_ptr: f64, value: f64, offset: i32, byte_length: i32) {
    js_buffer_write_uint_le(buf_ptr, value, offset, byte_length);
}

// ---- BigInt 64-bit read/write ----

#[no_mangle]
pub extern "C" fn js_buffer_read_bigint64_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => i64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_read_bigint64_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => i64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]),
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_read_biguint64_be(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as i64,
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_read_biguint64_le(buf_ptr: f64, offset: i32) -> f64 {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *const BufferHeader;
    let val = match buffer_slice_at(buf, offset, 8) {
        Some(s) => u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as i64,
        None => 0,
    };
    let bi = crate::bigint::js_bigint_from_i64(val);
    f64::from_bits(crate::JSValue::bigint_ptr(bi).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_write_bigint64_be(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    let val = bigint_value_to_i64(value);
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&val.to_be_bytes());
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_bigint64_le(buf_ptr: f64, value: f64, offset: i32) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    let val = bigint_value_to_i64(value);
    if let Some(s) = buffer_slice_at_mut(buf, offset, 8) {
        s[..8].copy_from_slice(&val.to_le_bytes());
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_write_biguint64_be(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_bigint64_be(buf_ptr, value, offset);
}

#[no_mangle]
pub extern "C" fn js_buffer_write_biguint64_le(buf_ptr: f64, value: f64, offset: i32) {
    js_buffer_write_bigint64_le(buf_ptr, value, offset);
}

fn bigint_value_to_i64(value: f64) -> i64 {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    // BigInt pointers can carry either BIGINT_TAG (0x7FFA) or — when the
    // codegen folds them through the generic `nanbox_pointer_inline` path
    // (Expr::BigInt) — POINTER_TAG (0x7FFD). Both encode the lower 48 bits
    // as the heap address. Detect either and use `clean_bigint_ptr` to
    // strip and validate the address before reading the limb.
    if top16 >= 0x7FF8 {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader;
        let cleaned = crate::bigint::clean_bigint_ptr(ptr);
        if cleaned.is_null() {
            return 0;
        }
        unsafe { (*cleaned).limbs[0] as i64 }
    } else if value.is_finite() {
        value as i64
    } else {
        0
    }
}
