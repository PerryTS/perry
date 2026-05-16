//! ethers.js utilities
//!
//! Provides formatUnits, parseUnits, parseEther, formatEther, getAddress, and other ethers utilities.

use perry_runtime::{
    js_bigint_from_string, js_object_alloc, js_object_set_field_by_name, js_string_from_bytes,
    BigIntHeader, ObjectHeader, StringHeader,
};

/// getAddress(address: string) -> string
/// Returns the checksummed address (EIP-55 format).
/// Example: getAddress("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48") -> "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
#[no_mangle]
pub extern "C" fn js_ethers_get_address(str_ptr: *const StringHeader) -> *mut StringHeader {
    if str_ptr.is_null() {
        let s = "0x0000000000000000000000000000000000000000";
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        if let Ok(s) = std::str::from_utf8(bytes) {
            let checksummed = to_checksum_address(s.trim());
            js_string_from_bytes(checksummed.as_ptr(), checksummed.len() as u32)
        } else {
            let s = "0x0000000000000000000000000000000000000000";
            js_string_from_bytes(s.as_ptr(), s.len() as u32)
        }
    }
}

/// Wallet.createRandom() -> { address: string, privateKey: string }
/// Generates a random Ethereum wallet with a cryptographically random private key.
/// The address is derived as the last 20 bytes of keccak256(private_key_bytes),
/// formatted with EIP-55 checksum encoding.
#[no_mangle]
pub extern "C" fn js_ethers_wallet_create_random() -> *mut ObjectHeader {
    use rand::Rng;
    let pk_bytes: [u8; 32] = rand::thread_rng().gen();

    // Derive address: keccak256(private_key_bytes)[12..32] = 20-byte address
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

    // Build lowercase address hex then apply EIP-55 checksum
    let mut addr_lower = String::with_capacity(40);
    for &b in addr_bytes {
        addr_lower.push(hex_chars[(b >> 4) as usize] as char);
        addr_lower.push(hex_chars[(b & 0x0f) as usize] as char);
    }
    let addr_checksummed = to_checksum_address(&addr_lower);

    // NaN-box a StringHeader pointer with STRING_TAG
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    let nanbox_str = |ptr: *mut StringHeader| -> f64 {
        f64::from_bits(STRING_TAG | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
    };

    let obj = js_object_alloc(0, 2);

    let key_addr_str = js_string_from_bytes(b"address".as_ptr(), 7);
    let val_addr_str =
        js_string_from_bytes(addr_checksummed.as_ptr(), addr_checksummed.len() as u32);
    js_object_set_field_by_name(obj, key_addr_str, nanbox_str(val_addr_str));

    let key_pk_str = js_string_from_bytes(b"privateKey".as_ptr(), 10);
    let val_pk_str = js_string_from_bytes(pk_hex.as_ptr(), pk_hex.len() as u32);
    js_object_set_field_by_name(obj, key_pk_str, nanbox_str(val_pk_str));

    obj
}

/// parseEther(value: string) -> bigint
/// Parses a string representing ether to a BigInt in wei (18 decimals).
/// Example: parseEther("1.5") -> 1500000000000000000n
#[no_mangle]
pub extern "C" fn js_ethers_parse_ether(str_ptr: *const StringHeader) -> *mut BigIntHeader {
    // parseEther is just parseUnits with 18 decimals
    js_ethers_parse_units(str_ptr, 18.0)
}

/// formatEther(value: bigint) -> string
/// Formats a BigInt in wei to a string in ether (18 decimals).
/// Example: formatEther(1500000000000000000n) -> "1.5"
#[no_mangle]
pub extern "C" fn js_ethers_format_ether(bigint_ptr: *const BigIntHeader) -> *mut StringHeader {
    // formatEther is just formatUnits with 18 decimals
    js_ethers_format_units(bigint_ptr, 18.0)
}

/// Convert an Ethereum address to EIP-55 checksum format
fn to_checksum_address(address: &str) -> String {
    // Remove 0x prefix if present
    let addr = address.strip_prefix("0x").unwrap_or(address);
    let addr = addr.strip_prefix("0X").unwrap_or(addr);

    // Validate length
    if addr.len() != 40 {
        return format!("0x{}", addr.to_lowercase());
    }

    // Validate hex characters
    if !addr.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("0x{}", addr.to_lowercase());
    }

    // Lowercase for hashing
    let addr_lower = addr.to_lowercase();

    // Calculate keccak256 hash of lowercase address
    let hash = keccak256(addr_lower.as_bytes());

    // Apply checksum
    let mut result = String::with_capacity(42);
    result.push_str("0x");

    for (i, c) in addr_lower.chars().enumerate() {
        if c.is_ascii_digit() {
            result.push(c);
        } else {
            // Get the nibble from the hash at position i
            let hash_byte = hash[i / 2];
            let hash_nibble = if i % 2 == 0 {
                (hash_byte >> 4) & 0x0F
            } else {
                hash_byte & 0x0F
            };

            // If hash nibble >= 8, uppercase the character
            if hash_nibble >= 8 {
                result.push(c.to_ascii_uppercase());
            } else {
                result.push(c);
            }
        }
    }

    result
}

/// Simple keccak256 implementation for address checksums
/// This is a minimal implementation specifically for 40-byte hex addresses
fn keccak256(data: &[u8]) -> [u8; 32] {
    use core::convert::TryInto;

    // Keccak-256 constants
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
            // Theta
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

            // Rho and Pi
            let mut t = state[1];
            for i in 0..24 {
                let j = PILN[i];
                let temp = state[j];
                state[j] = t.rotate_left(ROTC[i]);
                t = temp;
            }

            // Chi
            for j in (0..25).step_by(5) {
                let mut bc = [0u64; 5];
                for i in 0..5 {
                    bc[i] = state[j + i];
                }
                for i in 0..5 {
                    state[j + i] ^= (!bc[(i + 1) % 5]) & bc[(i + 2) % 5];
                }
            }

            // Iota
            state[0] ^= RC[round];
        }
    }

    // Initialize state
    let mut state = [0u64; 25];

    // Keccak-256 rate: 1088 bits = 136 bytes
    let rate = 136;

    // Pad the message (Keccak padding: 0x01 ... 0x80)
    let mut padded = data.to_vec();
    padded.push(0x01);
    while padded.len() % rate != rate - 1 {
        padded.push(0x00);
    }
    padded.push(0x80);

    // Absorb
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

    // Squeeze (only need 256 bits = 32 bytes)
    let mut output = [0u8; 32];
    for (i, chunk) in output.chunks_mut(8).enumerate() {
        chunk.copy_from_slice(&state[i].to_le_bytes());
    }

    output
}

/// Native keccak256(data: Uint8Array) -> hex string
/// Provides a correct native implementation bypassing the compiled TypeScript version.
/// Takes a buffer pointer (I64) and returns a "0x"-prefixed hex string pointer.
#[no_mangle]
pub unsafe extern "C" fn js_keccak256_native(buf_ptr: i64) -> *mut StringHeader {
    let buf_ptr =
        (buf_ptr as u64 & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::buffer::BufferHeader;
    if buf_ptr.is_null() {
        let s = "0x0000000000000000000000000000000000000000000000000000000000000000";
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    let len = (*buf_ptr).length as usize;
    let data =
        (buf_ptr as *const u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);

    let hash = keccak256(bytes);

    // Format as "0x" + 64 hex chars
    let hex_chars = b"0123456789abcdef";
    let mut out = Vec::with_capacity(66);
    out.push(b'0');
    out.push(b'x');
    for &byte in &hash {
        out.push(hex_chars[(byte >> 4) as usize]);
        out.push(hex_chars[(byte & 0x0f) as usize]);
    }

    js_string_from_bytes(out.as_ptr(), out.len() as u32)
}

/// Native keccak256 returning raw bytes (Uint8Array/buffer).
/// Used for internal ethkit calls (computeAddress, etc.) that need raw hash bytes.
#[no_mangle]
pub unsafe extern "C" fn js_keccak256_native_bytes(
    buf_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let buf_ptr =
        (buf_ptr as u64 & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::buffer::BufferHeader;
    let (data, _len) = if buf_ptr.is_null() {
        (&[] as &[u8], 0)
    } else {
        let len = (*buf_ptr).length as usize;
        let data =
            (buf_ptr as *const u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>());
        (std::slice::from_raw_parts(data, len), len)
    };

    let hash = keccak256(data);

    // Allocate a buffer and copy hash bytes
    let result = perry_runtime::buffer::buffer_alloc(32);
    (*result).length = 32;
    let dst = (result as *mut u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>());
    std::ptr::copy_nonoverlapping(hash.as_ptr(), dst, 32);
    result
}

/// formatUnits(value: bigint, decimals: number) -> string
/// Converts a BigInt to a human-readable string with the given number of decimals.
/// Example: formatUnits(1000000n, 6) -> "1.0"
#[no_mangle]
pub extern "C" fn js_ethers_format_units(
    bigint_ptr: *const BigIntHeader,
    decimals: f64,
) -> *mut StringHeader {
    if bigint_ptr.is_null() {
        let s = "0";
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    let decimals = decimals as i32;
    if !(0..=77).contains(&decimals) {
        let s = "0";
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }

    unsafe {
        // Read the BigInt value - fixed 256-bit (4 limbs)
        let bigint = &*bigint_ptr;
        let limbs = &bigint.limbs;

        // Convert to big integer string (always positive in current impl)
        let value_str = limbs_to_string(limbs);

        // Format with decimals
        let formatted = format_with_decimals(&value_str, decimals as usize);

        js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32)
    }
}

/// parseUnits(value: string, decimals: number) -> bigint
/// Parses a human-readable string to a BigInt with the given number of decimals.
/// Example: parseUnits("1.0", 6) -> 1000000n
#[no_mangle]
pub extern "C" fn js_ethers_parse_units(
    str_ptr: *const StringHeader,
    decimals: f64,
) -> *mut BigIntHeader {
    if str_ptr.is_null() {
        let s = "0";
        return js_bigint_from_string(s.as_ptr(), s.len() as u32);
    }

    let decimals = decimals as i32;
    if !(0..=77).contains(&decimals) {
        let s = "0";
        return js_bigint_from_string(s.as_ptr(), s.len() as u32);
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        if let Ok(s) = std::str::from_utf8(bytes) {
            let parsed = parse_units_to_string(s.trim(), decimals as usize);
            js_bigint_from_string(parsed.as_ptr(), parsed.len() as u32)
        } else {
            let s = "0";
            js_bigint_from_string(s.as_ptr(), s.len() as u32)
        }
    }
}

/// Number of limbs in BigIntHeader (must match perry-runtime)
const BIGINT_LIMBS: usize = perry_runtime::bigint::BIGINT_LIMBS;

/// Convert limbs (little-endian u64 array) to decimal string
fn limbs_to_string(limbs: &[u64; BIGINT_LIMBS]) -> String {
    if limbs.iter().all(|&x| x == 0) {
        return "0".to_string();
    }

    let mut work = *limbs;
    let mut digits = Vec::with_capacity(155); // max digits for 512-bit

    while !is_zero(&work) {
        let remainder = div_by_10(&mut work);
        digits.push((b'0' + remainder) as char);
    }

    // Reverse to get correct order
    digits.reverse();
    digits.into_iter().collect()
}

/// Check if limbs are zero
fn is_zero(limbs: &[u64; BIGINT_LIMBS]) -> bool {
    limbs.iter().all(|&x| x == 0)
}

/// Divide limbs (little-endian) by 10, return remainder
fn div_by_10(limbs: &mut [u64; BIGINT_LIMBS]) -> u8 {
    let mut remainder: u128 = 0;

    // Process from most significant to least significant limb
    for i in (0..BIGINT_LIMBS).rev() {
        let current = (remainder << 64) | (limbs[i] as u128);
        limbs[i] = (current / 10) as u64;
        remainder = current % 10;
    }

    remainder as u8
}

/// Format a number string with decimal places
fn format_with_decimals(value: &str, decimals: usize) -> String {
    if decimals == 0 {
        return value.to_string();
    }

    let is_negative = value.starts_with('-');
    let value = if is_negative { &value[1..] } else { value };

    let len = value.len();

    if len <= decimals {
        // Value is less than 1
        let zeros = decimals - len;
        let result = format!("0.{}{}", "0".repeat(zeros), value);
        if is_negative {
            format!("-{}", result)
        } else {
            result
        }
    } else {
        // Insert decimal point
        let split_pos = len - decimals;
        let result = format!("{}.{}", &value[..split_pos], &value[split_pos..]);
        if is_negative {
            format!("-{}", result)
        } else {
            result
        }
    }
}

/// Parse a decimal string to an integer string with the given decimal places
fn parse_units_to_string(value: &str, decimals: usize) -> String {
    let is_negative = value.starts_with('-');
    let value = if is_negative { &value[1..] } else { value };

    // Split on decimal point
    let parts: Vec<&str> = value.split('.').collect();
    let integer_part = parts[0];
    let decimal_part = if parts.len() > 1 { parts[1] } else { "" };

    // Build the result by padding or truncating decimal part
    let decimal_len = decimal_part.len();
    let result = if decimal_len == decimals {
        format!("{}{}", integer_part, decimal_part)
    } else if decimal_len < decimals {
        // Pad with zeros
        format!(
            "{}{}{}",
            integer_part,
            decimal_part,
            "0".repeat(decimals - decimal_len)
        )
    } else {
        // Truncate (round down)
        format!("{}{}", integer_part, &decimal_part[..decimals])
    };

    // Remove leading zeros (but keep at least one digit)
    let result = result.trim_start_matches('0');
    let result = if result.is_empty() { "0" } else { result };

    if is_negative && result != "0" {
        format!("-{}", result)
    } else {
        result.to_string()
    }
}
