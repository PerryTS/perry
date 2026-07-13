use super::*;

use crate::crypto::util::{
    ed25519_private_surrogate, ed25519_public_surrogate, parse_ed25519_private_surrogate,
    parse_ed25519_public_surrogate, parse_p256_signing_key_pem, parse_p256_verifying_key_pem,
    parse_rsa_private_key_pem, parse_rsa_public_key_pem, parse_x25519_private_surrogate,
    parse_x25519_public_surrogate, x25519_private_surrogate, x25519_public_surrogate,
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

/// Read a registered CryptoKey's raw material straight out of its BufferHeader.
///
/// SAFETY: callers must have resolved `addr` through `lookup_crypto_key` first,
/// which only succeeds for an address the WebCrypto key factories registered —
/// i.e. a live `BufferHeader` holding the key material.
///
/// `bytes_from_jsvalue` cannot be used here: it gates on `is_registered_buffer`,
/// which is exactly the thread-local check #6302 is about — a CryptoKey whose
/// metadata resolves through the process-global registry still has readable
/// bytes at `addr`.
unsafe fn crypto_key_bytes(addr: usize) -> Vec<u8> {
    let buf = addr as *const BufferHeader;
    let len = (*buf).length as usize;
    if len == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(buffer_payload(buf), len).to_vec()
}

/// Re-encode an asymmetric CryptoKey's WebCrypto key material (SPKI/PKCS#8 DER
/// for RSA, a SEC1 point / raw scalar for EC, raw 32-byte keys for Ed/X25519)
/// into the PEM / internal-surrogate string form Perry's KeyObject surrogates
/// use, plus the `asymmetric_key_meta` type id (1 rsa, 2 ec, 3 ed25519,
/// 4 x25519). Returns `None` for key types Perry has no KeyObject surrogate for
/// (Ed448 / X448 / ML-KEM) — the caller turns that into a throw, never a silent
/// `undefined`.
fn asymmetric_key_surrogate(mat: CryptoKeyMaterial, bytes: &[u8]) -> Option<(String, u8)> {
    let ed_key: Option<[u8; 32]> = bytes.try_into().ok();
    match (mat.algo, mat.kind) {
        (KeyAlgo::RsassaPkcs1 | KeyAlgo::RsaPss | KeyAlgo::RsaOaep, KeyKind::Public) => {
            let key = RsaPublicKey::from_public_key_der(bytes).ok()?;
            Some((key.to_public_key_pem(Default::default()).ok()?, 1))
        }
        (KeyAlgo::RsassaPkcs1 | KeyAlgo::RsaPss | KeyAlgo::RsaOaep, KeyKind::Private) => {
            let key = RsaPrivateKey::from_pkcs8_der(bytes).ok()?;
            Some((key.to_pkcs8_pem(Default::default()).ok()?.to_string(), 1))
        }
        (KeyAlgo::EcdsaP256 | KeyAlgo::EcdhP256, KeyKind::Public) => {
            let key = P256PublicKey::from_sec1_bytes(bytes).ok()?;
            Some((key.to_public_key_pem(Default::default()).ok()?, 2))
        }
        (KeyAlgo::EcdsaP256 | KeyAlgo::EcdhP256, KeyKind::Private) => {
            let key = P256SecretKey::from_slice(bytes).ok()?;
            Some((key.to_pkcs8_pem(Default::default()).ok()?.to_string(), 2))
        }
        (KeyAlgo::EcdsaP384 | KeyAlgo::EcdhP384, KeyKind::Public) => {
            let key = P384PublicKey::from_sec1_bytes(bytes).ok()?;
            Some((key.to_public_key_pem(Default::default()).ok()?, 2))
        }
        (KeyAlgo::EcdsaP384 | KeyAlgo::EcdhP384, KeyKind::Private) => {
            let key = P384SecretKey::from_slice(bytes).ok()?;
            Some((key.to_pkcs8_pem(Default::default()).ok()?.to_string(), 2))
        }
        (KeyAlgo::EcdsaP521 | KeyAlgo::EcdhP521, KeyKind::Public) => {
            let key = P521PublicKey::from_sec1_bytes(bytes).ok()?;
            Some((key.to_public_key_pem(Default::default()).ok()?, 2))
        }
        (KeyAlgo::EcdsaP521 | KeyAlgo::EcdhP521, KeyKind::Private) => {
            let key = P521SecretKey::from_slice(bytes).ok()?;
            Some((key.to_pkcs8_pem(Default::default()).ok()?.to_string(), 2))
        }
        (KeyAlgo::Ed25519, KeyKind::Public) => {
            let key = ed25519_dalek::VerifyingKey::from_bytes(&ed_key?).ok()?;
            Some((ed25519_public_surrogate(&key), 3))
        }
        (KeyAlgo::Ed25519, KeyKind::Private) => {
            let key = ed25519_dalek::SigningKey::from_bytes(&ed_key?);
            Some((ed25519_private_surrogate(&key), 3))
        }
        (KeyAlgo::X25519, KeyKind::Public) => Some((x25519_public_surrogate(&ed_key?), 4)),
        (KeyAlgo::X25519, KeyKind::Private) => Some((x25519_private_surrogate(&ed_key?), 4)),
        _ => None,
    }
}

/// `crypto.KeyObject.from(cryptoKey)` for **asymmetric** CryptoKeys (#6302).
///
/// The runtime handles the secret-key shape itself (a Buffer flagged as a
/// secret key); public/private keys need the encoders above, so
/// `native_module_crypto_key_object::key_object_from` routes them here through
/// the WebCrypto dispatch hook. The result is a PEM/surrogate string flagged
/// with `mark_as_asymmetric_key`, i.e. exactly what `createPublicKey()` /
/// `createPrivateKey()` / `generateKeyPairSync()` hand back — so `type`,
/// `asymmetricKeyType`, `export()`, `equals()`, `toCryptoKey()`, and the
/// sign/verify paths all work on it.
pub(super) unsafe fn js_webcrypto_key_object_from_crypto_key(key_bits: f64) -> f64 {
    let addr = strip_ptr(key_bits.to_bits());
    let mat = lookup_crypto_key(addr)
        .unwrap_or_else(|| throw_type_error("KeyObject.from() argument is not a CryptoKey"));
    let kind_id = match mat.kind {
        KeyKind::Public => 1u8,
        KeyKind::Private => 2u8,
        // Secret keys never reach the bridge — the runtime converts them.
        KeyKind::Secret => {
            throw_type_error("KeyObject.from() received a secret key on the asymmetric path")
        }
    };
    let bytes = crypto_key_bytes(addr);
    let (surrogate, asym_type) = asymmetric_key_surrogate(mat, &bytes).unwrap_or_else(|| {
        let message = format!(
            "KeyObject.from() does not support {:?} {} keys",
            mat.algo,
            if kind_id == 1 { "public" } else { "private" }
        );
        perry_runtime::fs::validate::throw_error_with_code(
            &message,
            "ERR_CRYPTO_UNSUPPORTED_OPERATION",
        )
    });
    let ptr = perry_runtime::js_string_from_bytes(surrogate.as_ptr(), surrogate.len() as u32);
    if ptr.is_null() {
        throw_dom_exception("OperationError", "The operation failed");
    }
    perry_runtime::buffer::mark_as_asymmetric_key(ptr as usize, kind_id, asym_type);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}
