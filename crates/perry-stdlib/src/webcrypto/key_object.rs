use super::*;

use crate::crypto::util::{
    parse_ed25519_private_surrogate, parse_ed25519_public_surrogate, parse_p256_signing_key_pem,
    parse_p256_verifying_key_pem, parse_rsa_private_key_pem, parse_rsa_public_key_pem,
    parse_x25519_private_surrogate, parse_x25519_public_surrogate,
};

unsafe fn throw_type_error(message: &str) -> ! {
    let msg = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

unsafe fn throw_dom_exception(name: &str, message: &str) -> ! {
    let name_str = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let message_str = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let name_val = f64::from_bits(JSValue::string_ptr(name_str).bits());
    let message_val = f64::from_bits(JSValue::string_ptr(message_str).bits());
    let err = perry_runtime::event_target::js_dom_exception_new(message_val, name_val);
    if err.is_null() {
        throw_type_error(message);
    }
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

unsafe fn require_hash(algo_bits: u64) -> HashAlgo {
    let hash_bits = object_field_bits(algo_bits, b"hash").unwrap_or_else(|| {
        throw_type_error("KeyObject.toCryptoKey algorithm.hash is required");
    });
    extract_hash_algo(hash_bits).unwrap_or_else(|| {
        throw_dom_exception(
            "NotSupportedError",
            "Unrecognized hash name for KeyObject.toCryptoKey",
        );
    })
}

unsafe fn require_p256_curve(algo_bits: u64) {
    let curve = object_field_string(algo_bits, b"namedCurve").unwrap_or_else(|| {
        throw_type_error("KeyObject.toCryptoKey algorithm.namedCurve is required");
    });
    match curve.to_ascii_lowercase().as_str() {
        "p-256" | "prime256v1" | "secp256r1" => {}
        _ => throw_dom_exception("DataError", "Named curve does not match the key"),
    }
}

unsafe fn require_extractable(bits: u64) -> bool {
    match bits {
        TAG_TRUE => true,
        TAG_FALSE => false,
        _ => throw_type_error("KeyObject.toCryptoKey extractable must be a boolean"),
    }
}

unsafe fn require_usage_sequence(bits: u64) {
    let is_array =
        JSValue::from_bits(perry_runtime::js_array_is_array(f64::from_bits(bits)).to_bits());
    if !is_array.is_bool() || !is_array.as_bool() {
        throw_type_error("KeyObject.toCryptoKey keyUsages must be an array");
    }
}

unsafe fn key_material(value: &str, kind: KeyKind, asym_type: u8) -> Vec<u8> {
    match (asym_type, kind) {
        (1, KeyKind::Private) => parse_rsa_private_key_pem(value)
            .and_then(|key| key.to_pkcs8_der().ok().map(|der| der.as_bytes().to_vec())),
        (1, KeyKind::Public) => parse_rsa_public_key_pem(value).and_then(|key| {
            key.to_public_key_der()
                .ok()
                .map(|der| der.as_bytes().to_vec())
        }),
        (2, KeyKind::Private) => {
            parse_p256_signing_key_pem(value).map(|key| key.to_bytes().as_slice().to_vec())
        }
        (2, KeyKind::Public) => parse_p256_verifying_key_pem(value)
            .map(|key| key.to_encoded_point(false).as_bytes().to_vec()),
        (3, KeyKind::Private) => {
            parse_ed25519_private_surrogate(value).map(|key| key.to_bytes().to_vec())
        }
        (3, KeyKind::Public) => {
            parse_ed25519_public_surrogate(value).map(|key| key.to_bytes().to_vec())
        }
        (4, KeyKind::Private) => parse_x25519_private_surrogate(value).map(|key| key.to_vec()),
        (4, KeyKind::Public) => parse_x25519_public_surrogate(value).map(|key| key.to_vec()),
        _ => None,
    }
    .unwrap_or_else(|| throw_dom_exception("DataError", "The key data is invalid"))
}

unsafe fn select_key_algorithm(algo_bits: u64, asym_type: u8) -> (KeyAlgo, HashAlgo) {
    let name = extract_algo_name(algo_bits).unwrap_or_else(|| {
        throw_dom_exception(
            "NotSupportedError",
            "Unrecognized algorithm name for KeyObject.toCryptoKey",
        );
    });
    let upper = name.to_ascii_uppercase();
    match (asym_type, upper.as_str()) {
        (1, "RSASSA-PKCS1-V1_5") => (KeyAlgo::RsassaPkcs1, require_hash(algo_bits)),
        (1, "RSA-OAEP") => (KeyAlgo::RsaOaep, require_hash(algo_bits)),
        (1, "RSA-PSS") => (KeyAlgo::RsaPss, require_hash(algo_bits)),
        (2, "ECDSA") => {
            require_p256_curve(algo_bits);
            (KeyAlgo::EcdsaP256, HashAlgo::Sha256)
        }
        (2, "ECDH") => {
            require_p256_curve(algo_bits);
            (KeyAlgo::EcdhP256, HashAlgo::Sha256)
        }
        (3, "ED25519") => (KeyAlgo::Ed25519, HashAlgo::Sha256),
        (4, "X25519") => (KeyAlgo::X25519, HashAlgo::Sha256),
        _ => throw_dom_exception(
            "NotSupportedError",
            "The requested algorithm is not supported for this key",
        ),
    }
}

pub(super) unsafe fn js_webcrypto_key_object_to_crypto_key(
    key_bits: f64,
    algorithm_bits: f64,
    extractable_bits: f64,
    usages_bits: f64,
) -> f64 {
    let key_addr = strip_ptr(key_bits.to_bits());
    let (runtime_kind, asym_type) = perry_runtime::buffer::asymmetric_key_meta(key_addr)
        .unwrap_or_else(|| throw_type_error("KeyObject.toCryptoKey receiver is not a KeyObject"));
    let kind = match runtime_kind {
        1 => KeyKind::Public,
        2 => KeyKind::Private,
        _ => throw_type_error("KeyObject.toCryptoKey receiver is not an asymmetric KeyObject"),
    };
    let key_string = string_from_jsvalue(key_bits.to_bits())
        .unwrap_or_else(|| throw_type_error("KeyObject.toCryptoKey receiver is not a KeyObject"));
    let extractable = require_extractable(extractable_bits.to_bits());
    require_usage_sequence(usages_bits.to_bits());
    let (key_algo, hash) = select_key_algorithm(algorithm_bits.to_bits(), asym_type);
    let usages = match validate_key_usages(
        key_algo,
        kind,
        usages_bits.to_bits(),
        matches!(kind, KeyKind::Public),
        "Usages cannot be empty when creating a key.",
        "Unsupported key usage for the requested key",
    ) {
        Ok(usages) => usages,
        Err((name, message)) => throw_dom_exception(name, message),
    };
    let bytes = key_material(&key_string, kind, asym_type);
    let buf = alloc_uint8array_from_slice(&bytes);
    if buf.is_null() {
        throw_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial::new(key_algo, hash, kind, extractable, usages),
    );
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}
