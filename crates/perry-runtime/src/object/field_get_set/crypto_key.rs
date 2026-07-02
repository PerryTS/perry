//! CryptoKey property reads + small field-setter helpers.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

pub(crate) const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
pub(crate) const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
pub(crate) const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_00D2;
pub(crate) const CLASS_ID_BOXED_BIGINT: u32 = 0xFFFF_00D3;
pub(crate) const CLASS_ID_BOXED_SYMBOL: u32 = 0xFFFF_00D4;

const CRYPTO_USAGE_ENCRYPT: u32 = 1 << 0;
const CRYPTO_USAGE_DECRYPT: u32 = 1 << 1;
const CRYPTO_USAGE_SIGN: u32 = 1 << 2;
const CRYPTO_USAGE_VERIFY: u32 = 1 << 3;
const CRYPTO_USAGE_DERIVE_KEY: u32 = 1 << 4;
const CRYPTO_USAGE_DERIVE_BITS: u32 = 1 << 5;
const CRYPTO_USAGE_WRAP_KEY: u32 = 1 << 6;
const CRYPTO_USAGE_UNWRAP_KEY: u32 = 1 << 7;
const CRYPTO_USAGE_ENCAPSULATE_BITS: u32 = 1 << 8;
const CRYPTO_USAGE_DECAPSULATE_BITS: u32 = 1 << 9;
const CRYPTO_USAGE_ENCAPSULATE_KEY: u32 = 1 << 10;
const CRYPTO_USAGE_DECAPSULATE_KEY: u32 = 1 << 11;

pub(crate) unsafe fn crypto_key_property_value(addr: usize, key_bytes: &[u8]) -> Option<JSValue> {
    let (algo, hash, kind, extractable, usages) = crate::buffer::crypto_key_meta(addr)?;
    match key_bytes {
        b"algorithm" => Some(crypto_key_algorithm_value(addr, algo, hash)),
        b"extractable" => Some(JSValue::bool(extractable)),
        b"type" => Some(string_value(match kind {
            2 => "private",
            3 => "public",
            _ => "secret",
        })),
        b"usages" => Some(crypto_key_usages_value(usages)),
        b"constructor" => {
            let ctor = super::super::js_get_global_this_builtin_value(b"CryptoKey".as_ptr(), 9);
            Some(JSValue::from_bits(ctor.to_bits()))
        }
        _ => None,
    }
}

unsafe fn crypto_key_algorithm_value(addr: usize, algo: u8, hash: u8) -> JSValue {
    let obj = js_object_alloc(0, 3);
    if obj.is_null() {
        return JSValue::undefined();
    }
    set_string_field(obj, b"name", crypto_key_algorithm_name(algo));
    if crypto_key_algorithm_has_hash(algo) {
        let hash_obj = js_object_alloc(0, 1);
        if !hash_obj.is_null() {
            set_string_field(hash_obj, b"name", crypto_key_hash_name(hash));
            set_value_field(obj, b"hash", JSValue::pointer(hash_obj as *const u8));
        }
    }
    if crypto_key_algorithm_has_length(algo) {
        let key = addr as *const crate::buffer::BufferHeader;
        let bits = if key.is_null() {
            0.0
        } else {
            crate::buffer::js_buffer_length(key) as f64 * 8.0
        };
        set_value_field(obj, b"length", JSValue::number(bits));
    }
    if let Some(curve) = crypto_key_named_curve(algo) {
        set_string_field(obj, b"namedCurve", curve);
    }
    JSValue::pointer(obj as *const u8)
}

fn crypto_key_algorithm_name(algo: u8) -> &'static str {
    match algo {
        1 => "HMAC",
        2 => "AES-GCM",
        3 => "AES-KW",
        4 => "AES-CBC",
        5 => "AES-CTR",
        6 => "HKDF",
        7 => "PBKDF2",
        8 => "ECDSA",
        9 => "ECDH",
        10 => "Ed25519",
        11 => "X25519",
        12 => "RSASSA-PKCS1-v1_5",
        13 => "RSA-OAEP",
        14 => "RSA-PSS",
        15 | 17 => "ECDSA",
        16 | 18 => "ECDH",
        19 => "Argon2d",
        20 => "Argon2i",
        21 => "Argon2id",
        22 => "ChaCha20-Poly1305",
        23 => "KMAC128",
        24 => "KMAC256",
        25 => "AES-OCB",
        26 => "X448",
        27 => "Ed448",
        30 => "ML-KEM-512",
        31 => "ML-KEM-768",
        32 => "ML-KEM-1024",
        _ => "",
    }
}

fn crypto_key_hash_name(hash: u8) -> &'static str {
    match hash {
        1 => "SHA-1",
        3 => "SHA-384",
        4 => "SHA-512",
        _ => "SHA-256",
    }
}

fn crypto_key_algorithm_has_hash(algo: u8) -> bool {
    matches!(algo, 1 | 12 | 13 | 14)
}

fn crypto_key_algorithm_has_length(algo: u8) -> bool {
    matches!(algo, 1 | 2 | 3 | 4 | 5 | 21 | 23 | 24 | 25)
}

fn crypto_key_named_curve(algo: u8) -> Option<&'static str> {
    match algo {
        8 | 9 => Some("P-256"),
        15 | 16 => Some("P-384"),
        17 | 18 => Some("P-521"),
        _ => None,
    }
}

unsafe fn crypto_key_usages_value(usages: u32) -> JSValue {
    let entries = [
        (CRYPTO_USAGE_ENCRYPT, "encrypt"),
        (CRYPTO_USAGE_DECRYPT, "decrypt"),
        (CRYPTO_USAGE_SIGN, "sign"),
        (CRYPTO_USAGE_VERIFY, "verify"),
        (CRYPTO_USAGE_DERIVE_KEY, "deriveKey"),
        (CRYPTO_USAGE_DERIVE_BITS, "deriveBits"),
        (CRYPTO_USAGE_WRAP_KEY, "wrapKey"),
        (CRYPTO_USAGE_UNWRAP_KEY, "unwrapKey"),
        (CRYPTO_USAGE_ENCAPSULATE_BITS, "encapsulateBits"),
        (CRYPTO_USAGE_DECAPSULATE_BITS, "decapsulateBits"),
        (CRYPTO_USAGE_ENCAPSULATE_KEY, "encapsulateKey"),
        (CRYPTO_USAGE_DECAPSULATE_KEY, "decapsulateKey"),
    ];
    let count = entries.iter().filter(|(bit, _)| usages & *bit != 0).count();
    let mut arr = crate::array::js_array_alloc(count as u32);
    for (bit, name) in entries {
        if usages & bit == 0 {
            continue;
        }
        let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        arr = crate::array::js_array_push(arr, JSValue::string_ptr(s));
    }
    JSValue::array_ptr(arr)
}

unsafe fn set_string_field(obj: *mut ObjectHeader, key: &[u8], value: &str) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let value = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_object_set_field_by_name(obj, key, f64::from_bits(JSValue::string_ptr(value).bits()));
}

unsafe fn set_value_field(obj: *mut ObjectHeader, key: &[u8], value: JSValue) {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key, f64::from_bits(value.bits()));
}

unsafe fn string_value(value: &str) -> JSValue {
    let s = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    JSValue::string_ptr(s)
}
