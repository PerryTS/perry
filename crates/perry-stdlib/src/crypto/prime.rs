use super::*;

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_prime_sync(size_bits: f64, options_bits: f64) -> f64 {
    let size = match nanboxed_to_usize(size_bits) {
        Some(s) => s,
        None => return buffer_value(&[]),
    };
    let options_raw = options_bits.to_bits();
    let safe = object_field_bool(options_raw, b"safe").unwrap_or(false);
    let bigint = object_field_bool(options_raw, b"bigint").unwrap_or(false);
    let add = object_field_u128(options_raw, b"add");
    let rem = object_field_u128(options_raw, b"rem");
    let Some(prime) = generate_prime_biguint(size, safe, add, rem) else {
        return buffer_value(&[]);
    };
    if bigint {
        biguint_value(&prime)
    } else {
        buffer_value(&biguint_to_be_bytes(&prime, size))
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_check_prime_sync(
    candidate_bits: f64,
    _options_bits: f64,
) -> f64 {
    let bytes = crypto_prime_candidate_bytes(candidate_bits);
    let Some(n) = biguint_from_candidate_bytes(&bytes) else {
        return js_bool(false);
    };
    js_bool(is_prime_biguint(&n))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_prime_async(
    size_bits: f64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let value = js_crypto_generate_prime_sync(size_bits, options_bits);
    call_node_style_callback2(
        callback_bits,
        f64::from_bits(JSValue::undefined().bits()),
        value,
    );
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_check_prime_async(
    candidate_bits: f64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let result = js_crypto_check_prime_sync(candidate_bits, options_bits);
    call_node_style_callback2(
        callback_bits,
        f64::from_bits(JSValue::null().bits()),
        result,
    );
    f64::from_bits(JSValue::undefined().bits())
}

unsafe fn buffer_value(bytes: &[u8]) -> f64 {
    let buf = alloc_buffer_from_slice(bytes);
    if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    }
}

fn biguint_value(value: &RsaBigUint) -> f64 {
    let decimal = value.to_string();
    let ptr = perry_runtime::bigint::js_bigint_from_string(decimal.as_ptr(), decimal.len() as u32);
    perry_runtime::value::js_nanbox_bigint(ptr as i64)
}

fn biguint_to_be_bytes(value: &RsaBigUint, bits: usize) -> Vec<u8> {
    let len = ((bits.max(1) + 7) / 8).max(1);
    let mut bytes = value.to_bytes_be();
    if bytes.len() > len {
        bytes = bytes[bytes.len() - len..].to_vec();
    } else if bytes.len() < len {
        let mut padded = vec![0; len - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    }
    bytes
}

fn runtime_bigint_bytes(value: &JSValue) -> Vec<u8> {
    let ptr = value.as_bigint_ptr();
    if ptr.is_null() {
        return Vec::new();
    }
    unsafe {
        let ptr = perry_runtime::bigint::clean_bigint_ptr(ptr);
        if ptr.is_null() {
            return Vec::new();
        }
        let limbs = (*ptr).limbs;
        if limbs[perry_runtime::bigint::BIGINT_LIMBS - 1] & (1u64 << 63) != 0 {
            return Vec::new();
        }
        let mut bytes = Vec::with_capacity(perry_runtime::bigint::BIGINT_LIMBS * 8);
        for limb in limbs.iter().rev() {
            bytes.extend_from_slice(&limb.to_be_bytes());
        }
        let first_non_zero = bytes
            .iter()
            .position(|&b| b != 0)
            .unwrap_or(bytes.len() - 1);
        bytes[first_non_zero..].to_vec()
    }
}

unsafe fn crypto_prime_candidate_bytes(bits: f64) -> Vec<u8> {
    let value = JSValue::from_bits(bits.to_bits());
    if value.is_bigint() {
        runtime_bigint_bytes(&value)
    } else {
        crypto_value_bytes(bits)
    }
}

fn biguint_from_candidate_bytes(bytes: &[u8]) -> Option<RsaBigUint> {
    if bytes.is_empty() {
        return None;
    }
    Some(RsaBigUint::from_bytes_be(bytes))
}

fn is_even_biguint(value: &RsaBigUint) -> bool {
    value.to_bytes_be().last().copied().unwrap_or(0) & 1 == 0
}

fn is_prime_biguint(n: &RsaBigUint) -> bool {
    let two = RsaBigUint::from(2u32);
    if n < &two {
        return false;
    }
    for p in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let p_big = RsaBigUint::from(p);
        if n == &p_big {
            return true;
        }
        if n % &p_big == RsaBigUint::from(0u32) {
            return false;
        }
    }

    let one = RsaBigUint::from(1u32);
    let n_minus_one = n - &one;
    let mut d = n_minus_one.clone();
    let mut rounds = 0usize;
    while is_even_biguint(&d) {
        d >>= 1;
        rounds += 1;
    }

    for a in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let base = RsaBigUint::from(a);
        if base >= n_minus_one {
            continue;
        }
        let mut x = base.modpow(&d, n);
        if x == one || x == n_minus_one {
            continue;
        }
        let mut composite = true;
        for _ in 1..rounds {
            x = x.modpow(&two, n);
            if x == n_minus_one {
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

fn bit_length(value: &RsaBigUint) -> usize {
    let bytes = value.to_bytes_be();
    let Some(first) = bytes.first() else {
        return 0;
    };
    (bytes.len() - 1) * 8 + (8 - first.leading_zeros() as usize)
}

fn generate_candidate_bytes(bits: usize) -> Vec<u8> {
    let len = ((bits + 7) / 8).max(1);
    let mut bytes = vec![0; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    let excess = len * 8 - bits;
    if excess > 0 {
        bytes[0] &= 0xffu8 >> excess;
    }
    bytes[0] |= 1u8 << (7 - excess);
    bytes[len - 1] |= 1;
    bytes
}

fn adjust_congruence(
    value: RsaBigUint,
    bits: usize,
    safe: bool,
    add: Option<u128>,
    rem: Option<u128>,
) -> Option<RsaBigUint> {
    let Some(add) = add else {
        return Some(value);
    };
    if add == 0 {
        return None;
    }
    let rem = rem.unwrap_or(if safe { 3 } else { 1 });
    let cur = &value % RsaBigUint::from(add);
    let cur = bytes_to_u128(&cur.to_bytes_be()).unwrap_or(0);
    let delta = (rem + add - cur) % add;
    let adjusted = value + RsaBigUint::from(delta);
    (bit_length(&adjusted) <= bits).then_some(adjusted)
}

fn generate_prime_biguint(
    bits: usize,
    safe: bool,
    add: Option<u128>,
    rem: Option<u128>,
) -> Option<RsaBigUint> {
    if bits == 0 || bits > perry_runtime::bigint::BIGINT_LIMBS * 64 {
        return None;
    }
    for _ in 0..1_000_000 {
        let candidate = RsaBigUint::from_bytes_be(&generate_candidate_bytes(bits));
        let Some(candidate) = adjust_congruence(candidate, bits, safe, add, rem) else {
            return None;
        };
        if !is_prime_biguint(&candidate) {
            continue;
        }
        if safe {
            let sophie = (&candidate - RsaBigUint::from(1u32)) >> 1;
            if !is_prime_biguint(&sophie) {
                continue;
            }
        }
        return Some(candidate);
    }
    None
}
