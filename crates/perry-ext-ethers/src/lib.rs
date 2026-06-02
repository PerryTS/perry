//! Native bindings for the npm `ethers` utility surface — sync,
//! string/BigInt/Buffer-based. Uses only perry-ffi v0.5: strings,
//! object alloc-with-shape, bigint allocator, buffer allocator.
//!
//! Functional parity with perry-stdlib/src/ethers.rs: getAddress
//! (EIP-55 checksum), Wallet.createRandom, parseEther/formatEther,
//! parseUnits/formatUnits, plus the keccak256 native helpers
//! (`js_keccak256_native` returns a hex string, `js_keccak256_native_bytes`
//! returns a 32-byte Buffer).

use perry_ffi::{
    alloc_bigint_from_str, alloc_buffer, alloc_string, build_object_shape,
    js_object_alloc_with_shape, js_object_set_field, BigIntHeader, BufferHeader, JsValue,
    StringHeader, BIGINT_LIMBS,
};

/// `getAddress(address: string) -> string` — EIP-55 checksummed.
/// Falls back to lowercase-only output for malformed input
/// (matches perry-stdlib's existing tolerance).
#[no_mangle]
pub extern "C" fn js_ethers_get_address(str_ptr: *const StringHeader) -> *mut StringHeader {
    if str_ptr.is_null() {
        return alloc_string("0x0000000000000000000000000000000000000000").as_raw();
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        if let Ok(s) = std::str::from_utf8(bytes) {
            let checksummed = to_checksum_address(s.trim());
            alloc_string(&checksummed).as_raw()
        } else {
            alloc_string("0x0000000000000000000000000000000000000000").as_raw()
        }
    }
}

/// `Wallet.createRandom() -> { address: string, privateKey: string }`.
#[no_mangle]
pub extern "C" fn js_ethers_wallet_create_random() -> JsValue {
    use rand::Rng;
    let pk_bytes: [u8; 32] = rand::thread_rng().gen();

    let hash = keccak256(&pk_bytes);
    let addr_bytes = &hash[12..32];

    let hex_chars = b"0123456789abcdef";

    // Format private key as "0x" + 64 hex chars
    let mut pk_hex = Vec::<u8>::with_capacity(66);
    pk_hex.extend_from_slice(b"0x");
    for &b in &pk_bytes {
        pk_hex.push(hex_chars[(b >> 4) as usize]);
        pk_hex.push(hex_chars[(b & 0x0f) as usize]);
    }
    let pk_str = unsafe { std::str::from_utf8_unchecked(&pk_hex) };

    let mut addr_lower = String::with_capacity(40);
    for &b in addr_bytes {
        addr_lower.push(hex_chars[(b >> 4) as usize] as char);
        addr_lower.push(hex_chars[(b & 0x0f) as usize] as char);
    }
    let addr_checksummed = to_checksum_address(&addr_lower);

    let (packed, shape_id) = build_object_shape(&["address", "privateKey"]);
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 2, packed.as_ptr(), packed.len() as u32) };
    let addr_str = alloc_string(&addr_checksummed);
    unsafe { js_object_set_field(obj, 0, JsValue::from_string_ptr(addr_str.as_raw())) };
    let pk_jsstr = alloc_string(pk_str);
    unsafe { js_object_set_field(obj, 1, JsValue::from_string_ptr(pk_jsstr.as_raw())) };
    JsValue::from_object_ptr(obj)
}

/// `parseEther(value: string) -> bigint` — alias of
/// `parseUnits(value, 18)`.
#[no_mangle]
pub extern "C" fn js_ethers_parse_ether(str_ptr: *const StringHeader) -> *mut BigIntHeader {
    js_ethers_parse_units(str_ptr, 18.0)
}

/// `formatEther(value: bigint) -> string` — alias of
/// `formatUnits(value, 18)`.
#[no_mangle]
pub extern "C" fn js_ethers_format_ether(bigint_ptr: *const BigIntHeader) -> *mut StringHeader {
    js_ethers_format_units(bigint_ptr, 18.0)
}

/// `formatUnits(value: bigint, decimals: number) -> string`.
#[no_mangle]
pub extern "C" fn js_ethers_format_units(
    bigint_ptr: *const BigIntHeader,
    decimals: f64,
) -> *mut StringHeader {
    if bigint_ptr.is_null() {
        return alloc_string("0").as_raw();
    }
    let decimals = decimals as i32;
    if !(0..=77).contains(&decimals) {
        return alloc_string("0").as_raw();
    }
    unsafe {
        let limbs = &(*bigint_ptr).limbs;
        let value_str = limbs_to_string(limbs);
        let formatted = format_with_decimals(&value_str, decimals as usize);
        alloc_string(&formatted).as_raw()
    }
}

/// `parseUnits(value: string, decimals: number) -> bigint`.
#[no_mangle]
pub extern "C" fn js_ethers_parse_units(
    str_ptr: *const StringHeader,
    decimals: f64,
) -> *mut BigIntHeader {
    if str_ptr.is_null() {
        return alloc_bigint_from_str("0");
    }
    let decimals = decimals as i32;
    if !(0..=77).contains(&decimals) {
        return alloc_bigint_from_str("0");
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        if let Ok(s) = std::str::from_utf8(bytes) {
            let parsed = parse_units_to_string(s.trim(), decimals as usize);
            alloc_bigint_from_str(&parsed)
        } else {
            alloc_bigint_from_str("0")
        }
    }
}

/// `keccak256(data: Uint8Array) -> hex string` (`0x` + 64 hex chars).
///
/// # Safety
///
/// `buf_ptr` is a NaN-stripped pointer to a `BufferHeader` (the
/// runtime hands these in as `i64` after the POINTER_TAG strip).
/// Null is treated as the empty input.
#[no_mangle]
pub unsafe extern "C" fn js_keccak256_native(buf_ptr: i64) -> *mut StringHeader {
    let buf = (buf_ptr as u64 & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader;
    if buf.is_null() {
        let s = "0x0000000000000000000000000000000000000000000000000000000000000000";
        return alloc_string(s).as_raw();
    }
    let len = (*buf).length as usize;
    let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    let hash = keccak256(bytes);

    let hex_chars = b"0123456789abcdef";
    let mut out = Vec::with_capacity(66);
    out.push(b'0');
    out.push(b'x');
    for &b in &hash {
        out.push(hex_chars[(b >> 4) as usize]);
        out.push(hex_chars[(b & 0x0f) as usize]);
    }
    let s = std::str::from_utf8_unchecked(&out);
    alloc_string(s).as_raw()
}

/// `keccak256_native_bytes(data) -> Buffer` — same as
/// `js_keccak256_native` but returns the raw 32-byte digest as a
/// runtime `Buffer` instead of a hex string. Used by ethkit's
/// `computeAddress` and friends that need the bytes for further
/// math.
///
/// # Safety
///
/// `buf_ptr` is a NaN-stripped pointer to a `BufferHeader`. Null
/// is treated as empty input (still produces a digest).
#[no_mangle]
pub unsafe extern "C" fn js_keccak256_native_bytes(buf_ptr: i64) -> *mut BufferHeader {
    let buf = (buf_ptr as u64 & 0x0000_FFFF_FFFF_FFFF) as *const BufferHeader;
    let bytes: &[u8] = if buf.is_null() {
        &[]
    } else {
        let len = (*buf).length as usize;
        let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
        std::slice::from_raw_parts(data, len)
    };
    let hash = keccak256(bytes);
    alloc_buffer(&hash)
}

// ── Helpers ────────────────────────────────────────────────────────

fn to_checksum_address(address: &str) -> String {
    let addr = address.strip_prefix("0x").unwrap_or(address);
    let addr = addr.strip_prefix("0X").unwrap_or(addr);

    if addr.len() != 40 {
        return format!("0x{}", addr.to_lowercase());
    }
    if !addr.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("0x{}", addr.to_lowercase());
    }

    let addr_lower = addr.to_lowercase();
    let hash = keccak256(addr_lower.as_bytes());

    let mut result = String::with_capacity(42);
    result.push_str("0x");
    for (i, c) in addr_lower.chars().enumerate() {
        if c.is_ascii_digit() {
            result.push(c);
        } else {
            let hash_byte = hash[i / 2];
            let hash_nibble = if i % 2 == 0 {
                (hash_byte >> 4) & 0x0F
            } else {
                hash_byte & 0x0F
            };
            if hash_nibble >= 8 {
                result.push(c.to_ascii_uppercase());
            } else {
                result.push(c);
            }
        }
    }
    result
}

/// Minimal Keccak-256 — same hand-rolled implementation
/// perry-stdlib's ethers ships, kept here so perry-ext-ethers has
/// no extra hashing dep beyond `rand`.
fn keccak256(data: &[u8]) -> [u8; 32] {
    use core::convert::TryInto;

    const ROUNDS: usize = 24;
    const RC: [u64; 24] = [
        0x0000000000000001,
        0x0000000000008082,
        0x800000000000808a,
        0x8000000080008000,
        0x000000000000808b,
        0x0000000080000001,
        0x8000000080008081,
        0x8000000000008009,
        0x000000000000008a,
        0x0000000000000088,
        0x0000000080008009,
        0x000000008000000a,
        0x000000008000808b,
        0x800000000000008b,
        0x8000000000008089,
        0x8000000000008003,
        0x8000000000008002,
        0x8000000000000080,
        0x000000000000800a,
        0x800000008000000a,
        0x8000000080008081,
        0x8000000000008080,
        0x0000000080000001,
        0x8000000080008008,
    ];
    const ROTC: [u32; 24] = [
        1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
    ];
    const PILN: [usize; 24] = [
        10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
    ];

    fn keccak_f(state: &mut [u64; 25]) {
        for round in 0..ROUNDS {
            let mut bc = [0u64; 5];
            for i in 0..5 {
                bc[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
            }
            for i in 0..5 {
                let t = bc[(i + 4) % 5] ^ bc[(i + 1) % 5].rotate_left(1);
                for j in (0..25).step_by(5) {
                    state[j + i] ^= t;
                }
            }
            let mut t = state[1];
            for i in 0..24 {
                let j = PILN[i];
                let temp = state[j];
                state[j] = t.rotate_left(ROTC[i]);
                t = temp;
            }
            for j in (0..25).step_by(5) {
                let mut bc = [0u64; 5];
                for i in 0..5 {
                    bc[i] = state[j + i];
                }
                for i in 0..5 {
                    state[j + i] ^= (!bc[(i + 1) % 5]) & bc[(i + 2) % 5];
                }
            }
            state[0] ^= RC[round];
        }
    }

    let mut state = [0u64; 25];
    let rate = 136;
    let mut padded = data.to_vec();
    padded.push(0x01);
    while padded.len() % rate != rate - 1 {
        padded.push(0x00);
    }
    padded.push(0x80);

    for chunk in padded.chunks(rate) {
        for (i, block) in chunk.chunks(8).enumerate() {
            if block.len() == 8 {
                state[i] ^= u64::from_le_bytes(block.try_into().unwrap());
            } else {
                let mut bytes = [0u8; 8];
                bytes[..block.len()].copy_from_slice(block);
                state[i] ^= u64::from_le_bytes(bytes);
            }
        }
        keccak_f(&mut state);
    }

    let mut output = [0u8; 32];
    for (i, chunk) in output.chunks_mut(8).enumerate() {
        chunk.copy_from_slice(&state[i].to_le_bytes());
    }
    output
}

fn limbs_to_string(limbs: &[u64; BIGINT_LIMBS]) -> String {
    if limbs.iter().all(|&x| x == 0) {
        return "0".to_string();
    }
    let mut work = *limbs;
    let mut digits = Vec::with_capacity(155);
    while !is_zero(&work) {
        let remainder = div_by_10(&mut work);
        digits.push((b'0' + remainder) as char);
    }
    digits.reverse();
    digits.into_iter().collect()
}

fn is_zero(limbs: &[u64; BIGINT_LIMBS]) -> bool {
    limbs.iter().all(|&x| x == 0)
}

fn div_by_10(limbs: &mut [u64; BIGINT_LIMBS]) -> u8 {
    let mut remainder: u128 = 0;
    for i in (0..BIGINT_LIMBS).rev() {
        let current = (remainder << 64) | (limbs[i] as u128);
        limbs[i] = (current / 10) as u64;
        remainder = current % 10;
    }
    remainder as u8
}

fn format_with_decimals(value: &str, decimals: usize) -> String {
    if decimals == 0 {
        return value.to_string();
    }
    let is_negative = value.starts_with('-');
    let value = if is_negative { &value[1..] } else { value };
    let len = value.len();
    if len <= decimals {
        let zeros = decimals - len;
        let result = format!("0.{}{}", "0".repeat(zeros), value);
        if is_negative {
            format!("-{}", result)
        } else {
            result
        }
    } else {
        let split_pos = len - decimals;
        let result = format!("{}.{}", &value[..split_pos], &value[split_pos..]);
        if is_negative {
            format!("-{}", result)
        } else {
            result
        }
    }
}

fn parse_units_to_string(value: &str, decimals: usize) -> String {
    let is_negative = value.starts_with('-');
    let value = if is_negative { &value[1..] } else { value };
    let parts: Vec<&str> = value.split('.').collect();
    let integer_part = parts[0];
    let decimal_part = if parts.len() > 1 { parts[1] } else { "" };
    let decimal_len = decimal_part.len();
    let result = if decimal_len == decimals {
        format!("{}{}", integer_part, decimal_part)
    } else if decimal_len < decimals {
        format!(
            "{}{}{}",
            integer_part,
            decimal_part,
            "0".repeat(decimals - decimal_len)
        )
    } else {
        format!("{}{}", integer_part, &decimal_part[..decimals])
    };
    let result = result.trim_start_matches('0');
    let result = if result.is_empty() { "0" } else { result };
    if is_negative && result != "0" {
        format!("-{}", result)
    } else {
        result.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keccak256_known_vector() {
        // keccak256("") = c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470
        let h = keccak256(b"");
        assert_eq!(
            h,
            [
                0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
                0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
                0x5d, 0x85, 0xa4, 0x70,
            ]
        );
    }

    #[test]
    fn checksum_eip55_known_vector() {
        // Vitalik's address — EIP-55 reference vector.
        let cs = to_checksum_address("0x52908400098527886e0f7030069857d2e4169ee7");
        assert_eq!(cs, "0x52908400098527886E0F7030069857D2E4169EE7");
    }

    #[test]
    fn format_with_decimals_basic() {
        assert_eq!(
            format_with_decimals("1500000000000000000", 18),
            "1.500000000000000000"
        );
        assert_eq!(format_with_decimals("1", 18), "0.000000000000000001");
        assert_eq!(format_with_decimals("0", 18), "0.000000000000000000");
    }

    #[test]
    fn parse_units_round_trip() {
        assert_eq!(parse_units_to_string("1.0", 6), "1000000");
        assert_eq!(parse_units_to_string("0.5", 6), "500000");
        assert_eq!(parse_units_to_string("1.000001", 6), "1000001");
    }
}
