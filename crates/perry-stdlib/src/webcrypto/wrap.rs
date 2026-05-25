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
    if lookup_crypto_key(wrapping_key_addr).is_none() {
        return reject_with_dom_exception(
            "InvalidAccessError",
            "Wrapping key is not a valid CryptoKey",
        );
    }
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
        match aes_kw_wrap(&wrapping_key_bytes, &key_bytes) {
            Some(w) => w,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-GCM" {
        let (iv, aad) = match resolve_aes_gcm_iv_aad(wrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        match aes_gcm_encrypt(&wrapping_key_bytes, &iv, &aad, &key_bytes) {
            Some(c) => c,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-CBC" {
        let wrapping_key_addr = strip_ptr(wrapping_key_bits.to_bits());
        if !matches!(lookup_crypto_key(wrapping_key_addr), Some(m) if m.algo == KeyAlgo::AesCbc) {
            return resolve_undefined();
        }
        let iv = match object_field_bytes(wrap_algo_bits.to_bits(), b"iv") {
            Some(v) => v,
            None => return resolve_undefined(),
        };
        match aes_cbc_encrypt(&wrapping_key_bytes, &iv, &key_bytes) {
            Some(c) => c,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-CTR" {
        let wrapping_key_addr = strip_ptr(wrapping_key_bits.to_bits());
        if !matches!(lookup_crypto_key(wrapping_key_addr), Some(m) if m.algo == KeyAlgo::AesCtr) {
            return resolve_undefined();
        }
        let counter = match object_field_bytes(wrap_algo_bits.to_bits(), b"counter") {
            Some(v) => v,
            None => return resolve_undefined(),
        };
        let length = match object_field_number(wrap_algo_bits.to_bits(), b"length") {
            Some(v) => v,
            None => return resolve_undefined(),
        };
        match aes_ctr_apply(&wrapping_key_bytes, &counter, length, &key_bytes) {
            Some(c) => c,
            None => return resolve_undefined(),
        }
    } else if upper == "RSA-OAEP" {
        let wrapping_key_addr = strip_ptr(wrapping_key_bits.to_bits());
        let mat = match lookup_crypto_key(wrapping_key_addr) {
            Some(m) => m,
            None => return resolve_undefined(),
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Public {
            return resolve_undefined();
        }
        let public_key = match RsaPublicKey::from_public_key_der(&wrapping_key_bytes) {
            Ok(k) => k,
            Err(_) => return resolve_undefined(),
        };
        match rsa_oaep_encrypt(mat.hash, &public_key, &key_bytes) {
            Some(c) => c,
            None => return resolve_undefined(),
        }
    } else {
        return resolve_undefined();
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
    if lookup_crypto_key(unwrapping_key_addr).is_none() {
        return reject_with_dom_exception(
            "InvalidAccessError",
            "Unwrapping key is not a valid CryptoKey",
        );
    }
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
        match aes_kw_unwrap(&unwrapping_key_bytes, &wrapped_bytes) {
            Some(r) => r,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-GCM" {
        let (iv, aad) = match resolve_aes_gcm_iv_aad(unwrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        match aes_gcm_decrypt(&unwrapping_key_bytes, &iv, &aad, &wrapped_bytes) {
            Some(p) => p,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-CBC" {
        let unwrapping_key_addr = strip_ptr(unwrapping_key_bits.to_bits());
        if !matches!(lookup_crypto_key(unwrapping_key_addr), Some(m) if m.algo == KeyAlgo::AesCbc) {
            return resolve_undefined();
        }
        let iv = match object_field_bytes(unwrap_algo_bits.to_bits(), b"iv") {
            Some(v) => v,
            None => return resolve_undefined(),
        };
        match aes_cbc_decrypt(&unwrapping_key_bytes, &iv, &wrapped_bytes) {
            Some(p) => p,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-CTR" {
        let unwrapping_key_addr = strip_ptr(unwrapping_key_bits.to_bits());
        if !matches!(lookup_crypto_key(unwrapping_key_addr), Some(m) if m.algo == KeyAlgo::AesCtr) {
            return resolve_undefined();
        }
        let counter = match object_field_bytes(unwrap_algo_bits.to_bits(), b"counter") {
            Some(v) => v,
            None => return resolve_undefined(),
        };
        let length = match object_field_number(unwrap_algo_bits.to_bits(), b"length") {
            Some(v) => v,
            None => return resolve_undefined(),
        };
        match aes_ctr_apply(&unwrapping_key_bytes, &counter, length, &wrapped_bytes) {
            Some(p) => p,
            None => return resolve_undefined(),
        }
    } else if upper == "RSA-OAEP" {
        let unwrapping_key_addr = strip_ptr(unwrapping_key_bits.to_bits());
        let mat = match lookup_crypto_key(unwrapping_key_addr) {
            Some(m) => m,
            None => return resolve_undefined(),
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Private {
            return resolve_undefined();
        }
        let private_key = match RsaPrivateKey::from_pkcs8_der(&unwrapping_key_bytes) {
            Ok(k) => k,
            Err(_) => return resolve_undefined(),
        };
        match rsa_oaep_decrypt(mat.hash, &private_key, &wrapped_bytes) {
            Some(p) => p,
            None => return resolve_undefined(),
        }
    } else {
        return resolve_undefined();
    };

    // Register the recovered bytes as a CryptoKey under the requested
    // unwrappedKeyAlgorithm.
    let unwrapped_name = match wrap_algo_name(unwrapped_algo_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    let key_algo = match unwrapped_name.as_str() {
        "AES-GCM" => KeyAlgo::AesGcm,
        "AES-KW" => KeyAlgo::AesKw,
        "AES-CBC" => KeyAlgo::AesCbc,
        "AES-CTR" => KeyAlgo::AesCtr,
        _ => return resolve_undefined(),
    };
    let buf = alloc_uint8array_from_slice(&recovered);
    if buf.is_null() {
        return resolve_undefined();
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
