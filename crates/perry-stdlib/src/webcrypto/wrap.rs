use super::*;

// =====================================================================
// subtle.wrapKey / subtle.unwrapKey
//
// jose reaches for these to ship key material between A256GCMKW
// (AES-GCM wrap) and the symmetric encrypted-payload flow. We
// support two wrap algorithms:
//
//   - `{ name: "AES-KW" }`  (RFC 3394) — the wrappingKey is an
//     AES key (128/192/256-bit); wrapped output is `keyBytes` + 8.
//   - `{ name: "AES-GCM", iv, additionalData? }` — same shape the
//     existing encrypt/decrypt path takes; wrapped output is
//     `ciphertext || tag`.
//
// `format` is currently restricted to `"raw"` — the only format
// jose uses for symmetric keys. JWK / spki / pkcs8 are TODO follow-
// ups (they require an asymmetric algorithm we haven't wired yet).
// =====================================================================

/// AES-KW wrap — RFC 3394. Returns the wrapped key (8 bytes longer
/// than `plaintext_key`). `aes-kw` 0.3 ships
/// `KwAes128/192/256`; we support all three lengths the WebCrypto
/// spec allows for AES-KW.
pub(super) fn aes_kw_wrap(wrapping_key: &[u8], plaintext_key: &[u8]) -> Option<Vec<u8>> {
    use aes_kw::{KeyInit, KwAes128, KwAes192, KwAes256};
    let mut buf = vec![0u8; plaintext_key.len() + 8];
    match wrapping_key.len() {
        16 => {
            let key_arr: [u8; 16] = wrapping_key.try_into().ok()?;
            let kek = KwAes128::new(&key_arr.into());
            kek.wrap_key(plaintext_key, &mut buf).ok()?;
        }
        24 => {
            let key_arr: [u8; 24] = wrapping_key.try_into().ok()?;
            let kek = KwAes192::new(&key_arr.into());
            kek.wrap_key(plaintext_key, &mut buf).ok()?;
        }
        32 => {
            let key_arr: [u8; 32] = wrapping_key.try_into().ok()?;
            let kek = KwAes256::new(&key_arr.into());
            kek.wrap_key(plaintext_key, &mut buf).ok()?;
        }
        _ => return None,
    }
    Some(buf)
}

/// AES-KW unwrap — RFC 3394.
pub(super) fn aes_kw_unwrap(wrapping_key: &[u8], wrapped_key: &[u8]) -> Option<Vec<u8>> {
    use aes_kw::{KeyInit, KwAes128, KwAes192, KwAes256};
    if wrapped_key.len() < 8 {
        return None;
    }
    let mut buf = vec![0u8; wrapped_key.len() - 8];
    match wrapping_key.len() {
        16 => {
            let key_arr: [u8; 16] = wrapping_key.try_into().ok()?;
            let kek = KwAes128::new(&key_arr.into());
            kek.unwrap_key(wrapped_key, &mut buf).ok()?;
        }
        24 => {
            let key_arr: [u8; 24] = wrapping_key.try_into().ok()?;
            let kek = KwAes192::new(&key_arr.into());
            kek.unwrap_key(wrapped_key, &mut buf).ok()?;
        }
        32 => {
            let key_arr: [u8; 32] = wrapping_key.try_into().ok()?;
            let kek = KwAes256::new(&key_arr.into());
            kek.unwrap_key(wrapped_key, &mut buf).ok()?;
        }
        _ => return None,
    }
    Some(buf)
}

/// Resolve the AES-GCM IV / AAD pair from a wrap-algorithm object.
/// Returns `None` if the IV is missing (the only mandatory field).
pub(super) unsafe fn resolve_aes_gcm_iv_aad(algo_bits: u64) -> Option<(Vec<u8>, Vec<u8>)> {
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let aad = object_field_bytes(algo_bits, b"additionalData").unwrap_or_default();
    Some((iv, aad))
}

/// Read the canonical algorithm-name from an algorithm arg (string or
/// `{ name }` object), upper-cased for matching.
pub(super) unsafe fn wrap_algo_name(algo_bits: u64) -> Option<String> {
    extract_algo_name(algo_bits).map(|s| s.to_ascii_uppercase())
}

/// `crypto.subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm)` →
/// Promise<Uint8Array>
///
/// Supported `format`: `"raw"`. Supported `wrapAlgorithm`:
/// - `{ name: "AES-KW" }` — RFC 3394 (wrappingKey is AES-128/256).
/// - `{ name: "AES-GCM", iv, additionalData? }` — same shape as
///   the existing encrypt path; wrapped output is `ciphertext || tag`.
///
/// Returns a Uint8Array of the wrapped key bytes. Errors (unsupported
/// format, missing IV for AES-GCM, key-length mismatch) resolve to
/// undefined so the caller's `await` rejects with a TypeError.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_wrap_key(
    format_bits: f64,
    key_bits: f64,
    wrapping_key_bits: f64,
    wrap_algo_bits: f64,
) -> *mut Promise {
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to execute 'wrapKey' on 'SubtleCrypto': format must be a string",
            )
        }
    };
    if format != "raw" {
        return reject_with_dom_exception("NotSupportedError", "Unsupported key format");
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    if lookup_crypto_key(key_addr).is_none() {
        return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey");
    }
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let wrapping_key_addr = strip_ptr(wrapping_key_bits.to_bits());
    let wrapping_mat = match lookup_crypto_key(wrapping_key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Wrapping key is not a valid CryptoKey",
            )
        }
    };
    let wrapping_key_bytes = bytes_from_jsvalue(wrapping_key_bits.to_bits());

    let upper = match wrap_algo_name(wrap_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized wrap-algorithm name",
            )
        }
    };
    let wrapped = if upper == "AES-KW" {
        if wrapping_mat.algo != KeyAlgo::AesKw {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        match aes_kw_wrap(&wrapping_key_bytes, &key_bytes) {
            Some(w) => w,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-GCM" {
        if wrapping_mat.algo != KeyAlgo::AesGcm {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (iv, aad) = match resolve_aes_gcm_iv_aad(wrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        match aes_gcm_encrypt(&wrapping_key_bytes, &iv, &aad, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CBC" {
        if wrapping_mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let iv = match object_field_bytes(wrap_algo_bits.to_bits(), b"iv") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        match aes_cbc_encrypt(&wrapping_key_bytes, &iv, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CTR" {
        if wrapping_mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let counter = match object_field_bytes(wrap_algo_bits.to_bits(), b"counter") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        let length = match object_field_number(wrap_algo_bits.to_bits(), b"length") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        match aes_ctr_apply(&wrapping_key_bytes, &counter, length, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "RSA-OAEP" {
        if wrapping_mat.algo != KeyAlgo::RsaOaep || wrapping_mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let public_key = match RsaPublicKey::from_public_key_der(&wrapping_key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        match rsa_oaep_encrypt(wrapping_mat.hash, &public_key, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized wrap-algorithm name");
    };
    resolve_with_bytes(&wrapped)
}

/// `crypto.subtle.unwrapKey(format, wrappedKey, unwrappingKey,
///   unwrapAlgorithm, unwrappedKeyAlgorithm, extractable, usages)` →
/// Promise<CryptoKey>
///
/// Inverts `wrapKey`. The recovered raw bytes are wrapped in a fresh
/// Buffer + registered with `unwrappedKeyAlgorithm` (currently
/// AES-GCM) so subsequent `encrypt`/`decrypt` calls find the right
/// `CryptoKeyMaterial`.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_unwrap_key(
    format_bits: f64,
    wrapped_key_bits: f64,
    unwrapping_key_bits: f64,
    unwrap_algo_bits: f64,
    unwrapped_algo_bits: f64,
    _extractable_bits: f64,
    _usages_bits: f64,
) -> *mut Promise {
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to execute 'unwrapKey' on 'SubtleCrypto': format must be a string",
            )
        }
    };
    if format != "raw" {
        return reject_with_dom_exception("NotSupportedError", "Unsupported key format");
    }
    let wrapped_bytes = bytes_from_jsvalue(wrapped_key_bits.to_bits());
    let unwrapping_key_addr = strip_ptr(unwrapping_key_bits.to_bits());
    let unwrapping_mat = match lookup_crypto_key(unwrapping_key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unwrapping key is not a valid CryptoKey",
            )
        }
    };
    let unwrapping_key_bytes = bytes_from_jsvalue(unwrapping_key_bits.to_bits());

    let upper = match wrap_algo_name(unwrap_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized unwrap-algorithm name",
            )
        }
    };
    let recovered = if upper == "AES-KW" {
        if unwrapping_mat.algo != KeyAlgo::AesKw {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        match aes_kw_unwrap(&unwrapping_key_bytes, &wrapped_bytes) {
            Some(r) => r,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-GCM" {
        if unwrapping_mat.algo != KeyAlgo::AesGcm {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (iv, aad) = match resolve_aes_gcm_iv_aad(unwrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        match aes_gcm_decrypt(&unwrapping_key_bytes, &iv, &aad, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CBC" {
        if unwrapping_mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let iv = match object_field_bytes(unwrap_algo_bits.to_bits(), b"iv") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        match aes_cbc_decrypt(&unwrapping_key_bytes, &iv, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CTR" {
        if unwrapping_mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let counter = match object_field_bytes(unwrap_algo_bits.to_bits(), b"counter") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        let length = match object_field_number(unwrap_algo_bits.to_bits(), b"length") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        match aes_ctr_apply(&unwrapping_key_bytes, &counter, length, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "RSA-OAEP" {
        if unwrapping_mat.algo != KeyAlgo::RsaOaep || unwrapping_mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let private_key = match RsaPrivateKey::from_pkcs8_der(&unwrapping_key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        match rsa_oaep_decrypt(unwrapping_mat.hash, &private_key, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else {
        return reject_with_dom_exception(
            "NotSupportedError",
            "Unrecognized unwrap-algorithm name",
        );
    };

    // Register the recovered bytes as a CryptoKey under the requested
    // unwrappedKeyAlgorithm.
    let unwrapped_name = match wrap_algo_name(unwrapped_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized unwrapped-key algorithm name",
            )
        }
    };
    let key_algo = match unwrapped_name.as_str() {
        "AES-GCM" => KeyAlgo::AesGcm,
        "AES-KW" => KeyAlgo::AesKw,
        "AES-CBC" => KeyAlgo::AesCbc,
        "AES-CTR" => KeyAlgo::AesCtr,
        _ => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    let buf = alloc_uint8array_from_slice(&recovered);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: key_algo,
            hash: HashAlgo::Sha256,
            kind: KeyKind::Secret,
        },
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hash_alg_accepts_canonical_and_aliased_forms() {
        assert_eq!(parse_hash_alg("SHA-256"), Some(HashAlgo::Sha256));
        assert_eq!(parse_hash_alg("sha-256"), Some(HashAlgo::Sha256));
        assert_eq!(parse_hash_alg("SHA256"), Some(HashAlgo::Sha256));
        assert_eq!(parse_hash_alg("SHA-1"), Some(HashAlgo::Sha1));
        assert_eq!(parse_hash_alg("SHA-384"), Some(HashAlgo::Sha384));
        assert_eq!(parse_hash_alg("SHA-512"), Some(HashAlgo::Sha512));
        assert_eq!(parse_hash_alg("MD5"), None);
        assert_eq!(parse_hash_alg(""), None);
    }

    #[test]
    fn aws_sigv4_test_vector() {
        // From the AWS SigV4 documentation:
        //   key = "AWS4" + secret_access_key
        //   k_date = HMAC-SHA-256(key, "20150830")
        // Vector at https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html
        let key = b"AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let date = b"20150830";
        let mac = compute_hmac(HashAlgo::Sha256, key, date).unwrap();
        // Expected k_date from the docs example:
        let expected =
            hex::decode("0138c7a6cbd60aa727b2f653a522567439dfb9f3e72b21f9b25941a42f04a7cd")
                .unwrap();
        assert_eq!(mac, expected);
    }

    #[test]
    fn sha256_test_vector_empty() {
        let digest = compute_digest(HashAlgo::Sha256, b"");
        assert_eq!(
            hex::encode(&digest),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_test_vector_abc() {
        let digest = compute_digest(HashAlgo::Sha256, b"abc");
        assert_eq!(
            hex::encode(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn constant_time_eq_matches() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn aes_gcm_round_trip_128() {
        let key = [0x42u8; 16];
        let iv = [0x11u8; 12];
        let aad = b"context";
        let plaintext = b"hello aes-gcm";
        let ct = aes_gcm_encrypt(&key, &iv, aad, plaintext).expect("encrypt");
        assert_eq!(ct.len(), plaintext.len() + 16); // ciphertext || tag
        let pt = aes_gcm_decrypt(&key, &iv, aad, &ct).expect("decrypt");
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_gcm_round_trip_256_no_aad() {
        let key = [0x37u8; 32];
        let iv = [0x22u8; 12];
        let plaintext = b"";
        let ct = aes_gcm_encrypt(&key, &iv, b"", plaintext).expect("encrypt");
        assert_eq!(ct.len(), 16); // empty payload + tag
        let pt = aes_gcm_decrypt(&key, &iv, b"", &ct).expect("decrypt");
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_gcm_rejects_short_iv() {
        let key = [0u8; 16];
        let iv = [0u8; 8]; // wrong length
        assert!(aes_gcm_encrypt(&key, &iv, b"", b"x").is_none());
        assert!(aes_gcm_decrypt(&key, &iv, b"", b"xxxxxxxxxxxxxxxx").is_none());
    }

    #[test]
    fn aes_gcm_round_trip_192() {
        // Node accepts AES-192-GCM in `crypto.subtle.*` even though the
        // browser WebCrypto spec only lists 128 / 256. We support it via
        // the typed `AesGcm<Aes192, U12>` alias to match Node parity
        // (see `test-parity/node-suite/crypto/webcrypto/aes-gcm-192.ts`).
        // This test was previously asserting rejection of 24-byte keys —
        // now it asserts a clean encrypt + decrypt round-trip instead.
        let key = [0u8; 24];
        let iv = [0u8; 12];
        let aad = b"aad";
        let plaintext = b"the quick brown fox";
        let ciphertext =
            aes_gcm_encrypt(&key, &iv, aad, plaintext).expect("192-bit GCM should encrypt");
        let recovered =
            aes_gcm_decrypt(&key, &iv, aad, &ciphertext).expect("192-bit GCM should decrypt");
        assert_eq!(recovered, plaintext);
    }
}
