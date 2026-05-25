use super::*;

/// Generate random bytes and return as a Buffer
/// crypto.randomBytes(size) -> Buffer
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_buffer(
    size: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let size = size as usize;
    if size == 0 || size > 1024 * 1024 {
        return perry_runtime::buffer::buffer_alloc(0);
    }

    let buf = perry_runtime::buffer::buffer_alloc(size as u32);
    unsafe {
        (*buf).length = size as u32;
        let data = perry_runtime::buffer::buffer_data_mut(buf);
        let bytes = std::slice::from_raw_parts_mut(data, size);
        rand::thread_rng().fill_bytes(bytes);
    }
    buf
}

/// `crypto.randomBytes(size, callback)` — callback form.
///
/// Perry executes the callback synchronously, but preserves Node's
/// observable callback shape `(err, buffer)` for parity tests and common
/// compatibility paths.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_bytes_async(size: f64, callback_bits: f64) -> f64 {
    let buf = js_crypto_random_bytes_buffer(size);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// Generate random bytes and return as hex string
/// crypto.randomBytes(size).toString('hex') -> string
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_hex(size: f64) -> *mut StringHeader {
    let size = size as usize;
    if size == 0 || size > 1024 * 1024 {
        // Limit to 1MB
        return std::ptr::null_mut();
    }

    let mut bytes = vec![0u8; size];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex_str = hex::encode(&bytes);

    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// Generate a random UUID v4 using crypto-secure random
/// crypto.randomUUID() -> string
#[no_mangle]
pub extern "C" fn js_crypto_random_uuid() -> *mut StringHeader {
    let uuid = uuid::Uuid::new_v4();
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// `crypto.randomInt(max)` / `crypto.randomInt(min, max)` synchronous form.
/// Returns a uniformly distributed integer in `[min, max)`.
#[no_mangle]
pub extern "C" fn js_crypto_random_int(min_bits: f64, max_bits: f64) -> f64 {
    let Some(min) = nanboxed_to_i64(min_bits) else {
        return f64::NAN;
    };
    let Some(max) = nanboxed_to_i64(max_bits) else {
        return f64::NAN;
    };
    if max <= min {
        return f64::NAN;
    }
    rand::thread_rng().gen_range(min..max) as f64
}

/// #1577: dispatcher for captured-then-called `crypto.*` methods
/// (`const f = crypto.createHash; f("sha256")`). The runtime's native-module
/// dispatch (`dispatch_native_module_method`) routes `("crypto", method)`
/// here once it's registered in `js_stdlib_init_dispatch` — the runtime can't
/// call these stdlib impls directly (perry-stdlib depends on perry-runtime).
/// Args arrive as NaN-boxed f64s; string args are unboxed SSO-safely the same
/// way the direct-call lowering does. Unhandled methods return undefined.
///
/// # Safety
/// `method_ptr`/`args_ptr` must be valid for their stated lengths (the runtime
/// passes the live method name and call-arg buffer).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(JSValue::undefined().bits());
    let method = if method_ptr.is_null() || method_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("")
    };
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            undefined
        }
    };
    // SSO-safe StringHeader pointer (matches `unbox_to_i64` on the direct path).
    let str_ptr = |n: usize| -> i64 { perry_runtime::js_get_string_pointer_unified(arg(n)) as i64 };
    // A buffer-or-string arg's raw pointer (bytes_from_ptr handles both).
    let bytes_ptr = |n: usize| -> i64 {
        let v = arg(n);
        if JSValue::from_bits(v.to_bits()).is_any_string() {
            perry_runtime::js_get_string_pointer_unified(v) as i64
        } else {
            perry_runtime::js_nanbox_get_pointer(v)
        }
    };
    match method {
        "createHash" => js_crypto_create_hash(str_ptr(0)),
        "createHmac" => js_crypto_create_hmac(str_ptr(0), bytes_ptr(1)),
        "randomUUID" => f64::from_bits(JSValue::string_ptr(js_crypto_random_uuid()).bits()),
        "randomBytes" => {
            let buf = js_crypto_random_bytes_buffer(arg(0));
            f64::from_bits(JSValue::pointer(buf as *const u8).bits())
        }
        // Node: randomInt(max) → [0,max); randomInt(min,max) → [min,max).
        "randomInt" if args_len >= 2 => js_crypto_random_int(arg(0), arg(1)),
        "randomInt" => js_crypto_random_int(0.0, arg(0)),
        _ => undefined,
    }
}

/// `crypto.randomInt(min, max, callback)` callback form. The random value is
/// generated through the synchronous helper and delivered as `(err, n)`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_int_async(
    min_bits: f64,
    max_bits: f64,
    callback_bits: f64,
) -> f64 {
    let n = js_crypto_random_int(min_bits, max_bits);
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), n);
    f64::from_bits(JSValue::undefined().bits())
}

/// `crypto.randomFillSync(buffer, offset?, size?)` — fills the given
/// Buffer or TypedArray with cryptographically strong random bytes
/// in-place and returns the **same** NaN-boxed buffer value.
///
/// All three arguments arrive as NaN-boxed `f64`. `offset` and `size`
/// may be `undefined` (Node's defaults are `offset = 0`, `size =
/// buffer.byteLength - offset`). Bounds-clamping mirrors Node: out-of-
/// range offsets/sizes are clamped to the buffer's byte length rather
/// than throwing, so misuse degrades to "fewer bytes filled" instead
/// of an opaque crash.
///
/// Required by axios (#? jose / axios native compile) — axios's
/// `generateString` passes a `Uint32Array`; jose can pass a `Buffer`.
#[no_mangle]
pub extern "C" fn js_crypto_random_fill_sync(
    buf_bits: f64,
    offset_bits: f64,
    size_bits: f64,
) -> f64 {
    let bits = buf_bits.to_bits();
    // Accept either form the codegen can hand us:
    //   - NaN-boxed POINTER_TAG (top16 0x7FFD) → Buffer / Uint8Array
    //   - Raw heap pointer in low 48 bits (top16 == 0) → TypedArray
    //     (Uint32Array etc — see `Expr::TypedArrayNew` codegen, the
    //     pointer is `bitcast_i64_to_double` without a tag).
    let top16 = (bits >> 48) as u16;
    let raw = if top16 >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    if raw < 0x1000 {
        return buf_bits;
    }

    // Read optional numeric args; undefined / NaN → use default.
    let offset_arg = nanboxed_to_usize(offset_bits);
    let size_arg = nanboxed_to_usize(size_bits);

    unsafe {
        // TypedArrayHeader path (Uint8Array, Uint32Array, Float32Array, …).
        if perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some() {
            let ta = raw as *mut perry_runtime::typedarray::TypedArrayHeader;
            let len = (*ta).length as usize;
            let elem_size = (*ta).elem_size as usize;
            // Node interprets offset/size for TypedArray inputs in elements,
            // not bytes. Convert the resolved element range back to byte
            // offsets before filling the underlying storage.
            let (start_elem, end_elem) = resolve_range(len, offset_arg, size_arg);
            let start = start_elem.saturating_mul(elem_size);
            let end = end_elem.saturating_mul(elem_size);
            if end > start {
                let data = (raw as *mut u8).add(std::mem::size_of::<
                    perry_runtime::typedarray::TypedArrayHeader,
                >());
                let slice = std::slice::from_raw_parts_mut(data.add(start), end - start);
                rand::thread_rng().fill_bytes(slice);
            }
            return buf_bits;
        }
        // BufferHeader / Uint8Array path.
        if perry_runtime::buffer::is_registered_buffer(raw) {
            let buf = raw as *mut perry_runtime::buffer::BufferHeader;
            let total = (*buf).length as usize;
            let (start, end) = resolve_range(total, offset_arg, size_arg);
            if end > start {
                let data = perry_runtime::buffer::buffer_data_mut(buf);
                let slice = std::slice::from_raw_parts_mut(data.add(start), end - start);
                rand::thread_rng().fill_bytes(slice);
            }
            // Hand back the same NaN-boxed value the caller passed.
            return buf_bits;
        }
    }

    // Unsupported value shape — return the original (no-op) rather
    // than crashing. The HIR-level type check is "any", so the
    // compiler can't statically rule this out.
    buf_bits
}

/// `crypto.randomFill(buffer[, offset][, size], callback)` — async callback
/// form. Fills in-place using the same implementation as `randomFillSync`,
/// then invokes `(err, buffer)`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_fill_async(
    buf_bits: f64,
    offset_bits: f64,
    size_bits: f64,
    callback_bits: f64,
) -> f64 {
    let value = js_crypto_random_fill_sync(buf_bits, offset_bits, size_bits);
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// Best-effort extract a non-negative integer from a NaN-boxed `f64`.
/// `undefined`, `null`, NaN, negatives → `None` (caller picks default).
pub(super) fn nanboxed_to_usize(bits: f64) -> Option<usize> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    // Undefined / null / false / true sentinels.
    if matches!(raw, 0x7FFC_0000_0000_0001 | 0x7FFC_0000_0000_0002) {
        return None;
    }
    // Int32 tag (0x7FFE).
    if top16 == 0x7FFE {
        let i = (raw & 0xFFFF_FFFF) as u32 as i32;
        if i < 0 {
            return None;
        }
        return Some(i as usize);
    }
    // Otherwise treat as plain f64.
    if bits.is_nan() || bits.is_sign_negative() || bits.is_infinite() {
        return None;
    }
    Some(bits as usize)
}

pub(super) fn nanboxed_to_i64(bits: f64) -> Option<i64> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    if matches!(raw, 0x7FFC_0000_0000_0001 | 0x7FFC_0000_0000_0002) {
        return None;
    }
    if top16 == 0x7FFE {
        return Some((raw & 0xFFFF_FFFF) as u32 as i32 as i64);
    }
    if bits.is_nan() || bits.is_infinite() {
        return None;
    }
    Some(bits as i64)
}

pub(super) unsafe fn crypto_value_bytes(bits: f64) -> Vec<u8> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    if top16 == 0x7FFE {
        let i = (raw & 0xFFFF_FFFF) as u32 as i32;
        if i < 0 {
            return Vec::new();
        }
        let n = i as u64;
        return if n <= 0xff {
            vec![n as u8]
        } else if n <= 0xffff {
            (n as u16).to_be_bytes().to_vec()
        } else if n <= 0xffff_ffff {
            (n as u32).to_be_bytes().to_vec()
        } else {
            n.to_be_bytes().to_vec()
        };
    }
    let ptr = (raw & 0x0000_FFFF_FFFF_FFFF) as i64;
    bytes_from_ptr(ptr)
}

pub(super) fn bytes_to_u128(bytes: &[u8]) -> Option<u128> {
    if bytes.len() > 16 {
        return None;
    }
    let mut n = 0u128;
    for &b in bytes {
        n = (n << 8) | b as u128;
    }
    Some(n)
}

pub(super) fn mod_pow_u128(mut base: u128, mut exp: u128, modu: u128) -> u128 {
    if modu == 1 {
        return 0;
    }
    let mut acc = 1u128;
    base %= modu;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = acc.wrapping_mul(base) % modu;
        }
        base = base.wrapping_mul(base) % modu;
        exp >>= 1;
    }
    acc
}

pub(super) fn is_prime_u128(n: u128) -> bool {
    if n < 2 {
        return false;
    }
    for p in [2u128, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n == p {
            return true;
        }
        if n % p == 0 {
            return false;
        }
    }
    let mut d = n - 1;
    let mut s = 0u32;
    while d % 2 == 0 {
        d /= 2;
        s += 1;
    }
    for a in [2u128, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if a >= n - 2 {
            continue;
        }
        let mut x = mod_pow_u128(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        let mut composite = true;
        for _ in 1..s {
            x = x.wrapping_mul(x) % n;
            if x == n - 1 {
                composite = false;
                break;
            }
        }
        if composite {
            return false;
        }
    }
    true
}

pub(super) fn prime_to_be_bytes(n: u128, bits: usize) -> Vec<u8> {
    let len = ((bits.max(1) + 7) / 8).max(1);
    let all = n.to_be_bytes();
    all[all.len() - len..].to_vec()
}

pub(super) unsafe fn object_field_bool(obj_bits: u64, name: &[u8]) -> Option<bool> {
    match object_field_bits(obj_bits, name)? {
        0x7FFC_0000_0000_0004 => Some(true),
        0x7FFC_0000_0000_0003 => Some(false),
        _ => None,
    }
}

pub(super) unsafe fn object_field_u128(obj_bits: u64, name: &[u8]) -> Option<u128> {
    let bits = object_field_bits(obj_bits, name)?;
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFE {
        let i = (bits & 0xFFFF_FFFF) as u32 as i32;
        return (i >= 0).then_some(i as u128);
    }
    bytes_to_u128(&crypto_value_bytes(f64::from_bits(bits)))
}

pub(super) fn generate_prime_u128(
    bits: usize,
    safe: bool,
    add: Option<u128>,
    rem: Option<u128>,
) -> Option<u128> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let high_bit = 1u128 << (bits - 1);
    let mask = (1u128 << bits) - 1;
    let mut rng = rand::thread_rng();
    for _ in 0..1_000_000 {
        let mut n = (rng.next_u64() as u128) & mask;
        n |= high_bit;
        n |= 1;
        if let Some(add) = add {
            if add == 0 {
                return None;
            }
            let rem = rem.unwrap_or(if safe { 3 } else { 1 });
            let cur = n % add;
            n = n.wrapping_add((rem + add - cur) % add);
            if n > mask || n < high_bit {
                continue;
            }
        }
        if !is_prime_u128(n) {
            continue;
        }
        if safe && !is_prime_u128((n - 1) / 2) {
            continue;
        }
        return Some(n);
    }
    None
}
