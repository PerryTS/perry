use super::*;

/// `crypto.subtle.deriveBits({ name: "ECDH", public }, privateKey, length)`
/// → Promise<Uint8Array>. Asymmetric derive coverage implements the
/// NIST ECDH curves supported by Node WebCrypto.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_derive_bits(
    algo_bits: f64,
    base_key_bits: f64,
    length_bits: f64,
) -> *mut Promise {
    let base_addr = strip_ptr(base_key_bits.to_bits());
    let base_mat = match lookup_crypto_key(base_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    let algo_name = extract_algo_name(algo_bits.to_bits()).unwrap_or_default();
    let is_argon2 = argon2_key_algo(&algo_name).is_some();
    let usage_message = if is_argon2 {
        "baseKey does not have deriveBits usage"
    } else {
        "The requested operation is not valid for the provided key"
    };
    if let Err((name, message)) = require_usage(base_mat, USAGE_DERIVE_BITS, usage_message) {
        return reject_with_dom_exception(name, message);
    }
    let bit_len = match number_from_bits(length_bits.to_bits()) {
        Some(n) => n,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    if bit_len % 8 != 0 {
        let message = if is_argon2 {
            "length must be a multiple of 8"
        } else {
            "The operation failed"
        };
        return reject_with_dom_exception("OperationError", message);
    }
    if is_argon2 && bit_len < 32 {
        return reject_with_dom_exception("OperationError", "length must be >= 32");
    }
    let byte_len = (bit_len / 8) as usize;
    match kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len) {
        Ok(Some(bytes)) => return resolve_with_bytes(&bytes),
        Ok(None) => {}
        Err((name, message)) => return reject_with_dom_exception(name, message),
    }
    let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
        Some(s) => s,
        None => {
            if ecdh_public_private_curve_mismatch(algo_bits.to_bits(), base_key_bits.to_bits()) {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "The requested operation is not valid for the provided key",
                );
            }
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
    };
    if byte_len > shared.len() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    resolve_with_bytes(&shared[..byte_len])
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_derive_key(
    algo_bits: f64,
    base_key_bits: f64,
    derived_algo_bits: f64,
    extractable_bits: f64,
    usages_bits: f64,
) -> *mut Promise {
    let base_addr = strip_ptr(base_key_bits.to_bits());
    let base_mat = match lookup_crypto_key(base_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    let algo_name = extract_algo_name(algo_bits.to_bits()).unwrap_or_default();
    let is_argon2 = argon2_key_algo(&algo_name).is_some();
    let usage_message = if is_argon2 {
        "baseKey does not have deriveKey usage"
    } else {
        "The requested operation is not valid for the provided key"
    };
    if let Err((name, message)) = require_usage(base_mat, USAGE_DERIVE_KEY, usage_message) {
        return reject_with_dom_exception(name, message);
    }
    let extractable = bool_from_jsvalue(extractable_bits.to_bits());
    let derived_name = match extract_algo_name(derived_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized derived-key algorithm name",
            )
        }
    };
    let derived_upper = derived_name.to_ascii_uppercase();
    let (key_algo, hash, bit_len) = if derived_upper == "HMAC" {
        let hash = match extract_hmac_hash(derived_algo_bits.to_bits()) {
            Some(h) => h,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::Hmac, hash, length)
    } else if derived_upper == "AES-GCM" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesGcm, HashAlgo::Sha256, length)
    } else if derived_upper == "AES-KW" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesKw, HashAlgo::Sha256, length)
    } else if derived_upper == "AES-CBC" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesCbc, HashAlgo::Sha256, length)
    } else if derived_upper == "AES-CTR" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesCtr, HashAlgo::Sha256, length)
    } else {
        return reject_with_dom_exception("OperationError", "The operation failed");
    };
    if bit_len % 8 != 0 || bit_len == 0 || bit_len > 256 {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let usages = match validate_key_usages(
        key_algo,
        KeyKind::Secret,
        usages_bits.to_bits(),
        false,
        "Usages cannot be empty when creating a key.",
        "Unsupported key usage for the requested algorithm",
    ) {
        Ok(u) => u,
        Err((name, message)) => return reject_with_dom_exception(name, message),
    };
    let byte_len = (bit_len / 8) as usize;
    let key_bytes = match kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            let shared =
                match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
                    Some(s) => s,
                    None => {
                        if ecdh_public_private_curve_mismatch(
                            algo_bits.to_bits(),
                            base_key_bits.to_bits(),
                        ) {
                            return reject_with_dom_exception(
                                "InvalidAccessError",
                                "The requested operation is not valid for the provided key",
                            );
                        }
                        return reject_with_dom_exception("OperationError", "The operation failed");
                    }
                };
            if byte_len > shared.len() {
                return reject_with_dom_exception("OperationError", "The operation failed");
            }
            shared[..byte_len].to_vec()
        }
        Err((name, message)) => return reject_with_dom_exception(name, message),
    };
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial::new(key_algo, hash, KeyKind::Secret, extractable, usages),
    );
    resolve_with_bits(JSValue::pointer(buf as *const u8).bits())
}
