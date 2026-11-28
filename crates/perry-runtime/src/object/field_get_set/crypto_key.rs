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
    let (algo, hash, kind, extractable, usages, bit_length) = crate::buffer::crypto_key_meta(addr)?;
    match key_bytes {
        b"algorithm" => Some(crypto_key_algorithm_value(addr, algo, hash, bit_length)),
        b"extractable" => Some(JSValue::bool(extractable)),
        b"type" => Some(string_value(match kind {
            2 => "private",
            3 => "public",
            _ => "secret",
        })),
        b"usages" => Some(crypto_key_usages_value(usages)),
        b"constructor" => {
            let ctor = crate::object::builtin_constructor_value("CryptoKey");
            Some(JSValue::from_bits(ctor.to_bits()))
        }
        _ => None,
    }
}

unsafe fn crypto_key_algorithm_value(addr: usize, algo: u8, hash: u8, bit_length: u32) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_nanbox_u64(JSValue::pointer(addr as *const u8).bits());
    let obj = js_object_alloc(0, 4);
    if obj.is_null() {
        return JSValue::undefined();
    }
    let obj = scope.root_nanbox_u64(JSValue::pointer(obj as *const u8).bits());
    set_string_field(&scope, &obj, b"name", crypto_key_algorithm_name(algo));
    if crypto_key_algorithm_is_rsa(algo) {
        set_rsa_key_fields(&key, &obj);
    }
    if crypto_key_algorithm_has_hash(algo) {
        let hash_obj = js_object_alloc(0, 1);
        if !hash_obj.is_null() {
            let hash_obj = scope.root_nanbox_u64(JSValue::pointer(hash_obj as *const u8).bits());
            set_string_field(&scope, &hash_obj, b"name", crypto_key_hash_name(hash));
            let hash_value = JSValue::from_bits(hash_obj.get_nanbox_u64());
            set_value_field(&scope, &obj, b"hash", hash_value);
        }
    }
    if crypto_key_algorithm_has_length(algo) {
        let key =
            JSValue::from_bits(key.get_nanbox_u64()).as_pointer::<crate::buffer::BufferHeader>();
        let bits = if bit_length != 0 {
            bit_length as f64
        } else if key.is_null() {
            0.0
        } else {
            crate::buffer::js_buffer_length(key) as f64 * 8.0
        };
        set_value_field(&scope, &obj, b"length", JSValue::number(bits));
    }
    if let Some(curve) = crypto_key_named_curve(algo) {
        set_string_field(&scope, &obj, b"namedCurve", curve);
    }
    JSValue::from_bits(obj.get_nanbox_u64())
}

/// Adds `modulusLength` and `publicExponent`. They are encoded in the key's DER,
/// and the decoder lives in perry-stdlib.
unsafe fn set_rsa_key_fields(
    key: &crate::gc::RuntimeHandle<'_>,
    algorithm: &crate::gc::RuntimeHandle<'_>,
) {
    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        std::mem::transmute(ptr);
    let args = [key.get_nanbox_f64(), algorithm.get_nanbox_f64()];
    let method = "setRsaKeyAlgorithmFields";
    dispatch(method.as_ptr(), method.len(), args.as_ptr(), args.len());
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
        33 => "ML-DSA-44",
        34 => "ML-DSA-65",
        35 => "ML-DSA-87",
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

fn crypto_key_algorithm_is_rsa(algo: u8) -> bool {
    matches!(algo, 12 | 13 | 14)
}

fn crypto_key_algorithm_has_hash(algo: u8) -> bool {
    algo == 1 || crypto_key_algorithm_is_rsa(algo)
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

unsafe fn set_string_field(
    scope: &crate::gc::RuntimeHandleScope,
    obj: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    value: &str,
) {
    let value = string_value(value);
    set_value_field(scope, obj, key, value);
}

unsafe fn set_value_field(
    scope: &crate::gc::RuntimeHandleScope,
    obj: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    value: JSValue,
) {
    let value = scope.root_nanbox_f64(f64::from_bits(value.bits()));
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let obj = JSValue::from_bits(obj.get_nanbox_u64()).as_pointer::<ObjectHeader>();
    js_object_set_field_by_name(obj.cast_mut(), key, value.get_nanbox_f64());
}

unsafe fn string_value(value: &str) -> JSValue {
    let s = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    JSValue::string_ptr(s)
}

/// The KeyObject surface of an asymmetric key, whose Perry form is a heap
/// string flagged by `mark_as_asymmetric_key`. `None` leaves the read to the
/// string's own properties and `String.prototype`.
pub(crate) unsafe fn asymmetric_key_string_property(
    obj: *const ObjectHeader,
    key_bytes: &[u8],
) -> Option<JSValue> {
    let (kind, asym_type) = crate::buffer::asymmetric_key_meta(obj as usize)?;
    if key_bytes == b"type" {
        let label = if kind == 1 {
            b"public".as_slice()
        } else {
            b"private".as_slice()
        };
        let s = crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
        return Some(JSValue::from_bits(JSValue::string_ptr(s).bits()));
    }
    if key_bytes == b"asymmetricKeyType" {
        let label = match asym_type {
            1 => b"rsa".as_slice(),
            2 | 5 | 6 => b"ec".as_slice(),
            3 => b"ed25519".as_slice(),
            4 => b"x25519".as_slice(),
            7 => b"ml-kem-512".as_slice(),
            8 => b"ml-kem-768".as_slice(),
            9 => b"ml-kem-1024".as_slice(),
            10 => b"ml-dsa-44".as_slice(),
            11 => b"ml-dsa-65".as_slice(),
            12 => b"ml-dsa-87".as_slice(),
            _ => b"".as_slice(),
        };
        if !label.is_empty() {
            let s = crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32);
            return Some(JSValue::from_bits(JSValue::string_ptr(s).bits()));
        }
    }
    if key_bytes == b"asymmetricKeyDetails" {
        // 2/5/6: EC over P-256/P-384/P-521 (perry-stdlib's ec_sign).
        let curve: &[u8] = match asym_type {
            2 => b"prime256v1",
            5 => b"secp384r1",
            6 => b"secp521r1",
            _ => b"",
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let details = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_object_alloc(
            0,
            if curve.is_empty() { 0 } else { 1 },
        ) as i64));
        if !curve.is_empty() {
            let name = scope.root_nanbox_f64(crate::value::js_nanbox_string(
                crate::string::js_string_from_bytes(b"namedCurve".as_ptr(), 10) as i64,
            ));
            let val = crate::string::js_string_from_bytes(curve.as_ptr(), curve.len() as u32);
            js_object_set_field_by_name(
                crate::value::js_nanbox_get_pointer(details.get_nanbox_f64()) as *mut ObjectHeader,
                crate::value::js_nanbox_get_pointer(name.get_nanbox_f64())
                    as *const crate::StringHeader,
                f64::from_bits(JSValue::string_ptr(val).bits()),
            );
        }
        return Some(JSValue::from_bits(details.get_nanbox_u64()));
    }
    // `js_class_method_bind` only needs a pointer that stays
    // valid for the closure's lifetime — the static byte
    // literals satisfy that without per-read allocation.
    let static_name: Option<&'static [u8]> = match key_bytes {
        b"export" => Some(b"export"),
        b"equals" => Some(b"equals"),
        b"toCryptoKey" => Some(b"toCryptoKey"),
        _ => None,
    };
    if let Some(name) = static_name {
        let this_f64 = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
        let result = js_class_method_bind(this_f64, name.as_ptr(), name.len());
        return Some(JSValue::from_bits(result.to_bits()));
    }
    None
}
