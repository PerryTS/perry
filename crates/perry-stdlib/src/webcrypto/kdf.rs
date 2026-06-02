use super::*;

/// `crypto.subtle.deriveBits({ name: "ECDH", public }, privateKey, length)`
/// → Promise<Uint8Array>. Initial asymmetric-derive coverage implements
/// P-256 ECDH, matching Node/Bun WebCrypto suites.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_derive_bits(
    algo_bits: f64,
    base_key_bits: f64,
    length_bits: f64,
) -> *mut Promise {
    let bit_len = match number_from_bits(length_bits.to_bits()) {
        Some(n) => n,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    if bit_len % 8 != 0 {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let byte_len = (bit_len / 8) as usize;
    if let Some(bytes) = kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len) {
        return resolve_with_bytes(&bytes);
    }
    if bit_len > 256 {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
        Some(s) => s,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    resolve_with_bytes(&shared[..byte_len])
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_derive_key(
    algo_bits: f64,
    base_key_bits: f64,
    derived_algo_bits: f64,
    _extractable_bits: f64,
    _usages_bits: f64,
) -> *mut Promise {
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
    let byte_len = (bit_len / 8) as usize;
    let key_bytes = if let Some(bytes) =
        kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len)
    {
        bytes
    } else {
        let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        if byte_len > shared.len() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        shared[..byte_len].to_vec()
    };
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: key_algo,
            hash,
            kind: KeyKind::Secret,
        },
    );
    resolve_with_bits(JSValue::pointer(buf as *const u8).bits())
}
