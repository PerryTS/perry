use super::*;

/// `crypto.subtle.generateKey(algorithm, extractable, keyUsages)` →
/// Promise<CryptoKey>
///
/// Supported `algorithm` shapes:
/// - `{ name: "AES-GCM", length: 128 | 192 | 256 }` — generates a random
///   AES key.
/// - String shorthand `"AES-GCM"` defaults to 256-bit per the WebCrypto
///   convention (jose's `generateSecret('A256GCM')` reaches this).
/// - `{ name: "ECDSA", namedCurve: "P-256" }` — returns a
///   `{ publicKey, privateKey }` CryptoKeyPair that can be used by
///   `subtle.sign` / `subtle.verify`.
///
/// Other asymmetric algorithms (RSA-OAEP, RSA-PSS, ECDH) and HMAC
/// keygen are TODO follow-ups — `extractable` and `keyUsages` are
/// accepted but not enforced (perry's threat model treats them as
/// documentation, matching `importKey`).
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_generate_key(
    algo_bits: f64,
    _extractable_bits: f64,
    _usages_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    if algo_upper == "RSA-OAEP" || algo_upper == "RSASSA-PKCS1-V1_5" || algo_upper == "RSA-PSS" {
        let hash = extract_algorithm_hash(algo_bits.to_bits(), HashAlgo::Sha1);
        let key_algo = match algo_upper.as_str() {
            "RSA-OAEP" => KeyAlgo::RsaOaep,
            "RSASSA-PKCS1-V1_5" => KeyAlgo::RsassaPkcs1,
            "RSA-PSS" => KeyAlgo::RsaPss,
            _ => unreachable!(),
        };
        let mut rng = rand::rngs::OsRng;
        let private_key = match RsaPrivateKey::new(&mut rng, 2048) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let public_key = RsaPublicKey::from(&private_key);
        let private_der = match private_key.to_pkcs8_der() {
            Ok(der) => der.as_bytes().to_vec(),
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let public_der = match public_key.to_public_key_der() {
            Ok(der) => der.as_bytes().to_vec(),
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };

        let private_buf = alloc_uint8array_from_slice(&private_der);
        let public_buf = alloc_uint8array_from_slice(&public_der);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial {
                algo: key_algo,
                hash,
                kind: KeyKind::Private,
            },
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial {
                algo: key_algo,
                hash,
                kind: KeyKind::Public,
            },
        );

        let obj = js_object_alloc(0, 2);
        if obj.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let public_key_name = perry_runtime::js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let private_key_name = perry_runtime::js_string_from_bytes(b"privateKey".as_ptr(), 10);
        js_object_set_field_by_name(
            obj,
            public_key_name,
            f64::from_bits(JSValue::pointer(public_buf as *const u8).bits()),
        );
        js_object_set_field_by_name(
            obj,
            private_key_name,
            f64::from_bits(JSValue::pointer(private_buf as *const u8).bits()),
        );
        return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if algo_upper == "ED25519" {
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let public_bytes = signing_key.verifying_key().to_bytes();

        let private_buf = alloc_uint8array_from_slice(&seed);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::Ed25519,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Private,
            },
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::Ed25519,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Public,
            },
        );

        let obj = js_object_alloc(0, 2);
        if obj.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let public_key_name = perry_runtime::js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let private_key_name = perry_runtime::js_string_from_bytes(b"privateKey".as_ptr(), 10);
        js_object_set_field_by_name(
            obj,
            public_key_name,
            f64::from_bits(JSValue::pointer(public_buf as *const u8).bits()),
        );
        js_object_set_field_by_name(
            obj,
            private_key_name,
            f64::from_bits(JSValue::pointer(private_buf as *const u8).bits()),
        );
        return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if algo_upper == "X25519" {
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let private_key = x25519_dalek::StaticSecret::from(seed);
        let public_key = x25519_dalek::PublicKey::from(&private_key);
        let private_bytes = private_key.to_bytes();
        let public_bytes = public_key.to_bytes();

        let private_buf = alloc_uint8array_from_slice(&private_bytes);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::X25519,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Private,
            },
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::X25519,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Public,
            },
        );

        let obj = js_object_alloc(0, 2);
        if obj.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let public_key_name = perry_runtime::js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let private_key_name = perry_runtime::js_string_from_bytes(b"privateKey".as_ptr(), 10);
        js_object_set_field_by_name(
            obj,
            public_key_name,
            f64::from_bits(JSValue::pointer(public_buf as *const u8).bits()),
        );
        js_object_set_field_by_name(
            obj,
            private_key_name,
            f64::from_bits(JSValue::pointer(private_buf as *const u8).bits()),
        );
        return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if algo_upper == "ECDH" {
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve") {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let curve_upper = curve.to_ascii_uppercase();
        if curve_upper != "P-256" && curve_upper != "PRIME256V1" && curve_upper != "SECP256R1" {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let private_key = match generate_p256_secret_key() {
            Some(k) => k,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let private_bytes = private_key.to_bytes().as_slice().to_vec();
        let public_bytes = private_key
            .public_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();

        let private_buf = alloc_uint8array_from_slice(&private_bytes);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::EcdhP256,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Private,
            },
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::EcdhP256,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Public,
            },
        );

        let obj = js_object_alloc(0, 2);
        if obj.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let public_key_name = perry_runtime::js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let private_key_name = perry_runtime::js_string_from_bytes(b"privateKey".as_ptr(), 10);
        js_object_set_field_by_name(
            obj,
            public_key_name,
            f64::from_bits(JSValue::pointer(public_buf as *const u8).bits()),
        );
        js_object_set_field_by_name(
            obj,
            private_key_name,
            f64::from_bits(JSValue::pointer(private_buf as *const u8).bits()),
        );
        return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
    }
    if algo_upper == "ECDSA" {
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve") {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let curve_upper = curve.to_ascii_uppercase();
        if curve_upper != "P-256" && curve_upper != "PRIME256V1" && curve_upper != "SECP256R1" {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let signing_key = match generate_p256_signing_key() {
            Some(k) => k,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let private_bytes = signing_key.to_bytes().as_slice().to_vec();
        let public_bytes = signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();

        let private_buf = alloc_uint8array_from_slice(&private_bytes);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::EcdsaP256,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Private,
            },
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial {
                algo: KeyAlgo::EcdsaP256,
                hash: HashAlgo::Sha256,
                kind: KeyKind::Public,
            },
        );

        let obj = js_object_alloc(0, 2);
        if obj.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let public_key_name = perry_runtime::js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let private_key_name = perry_runtime::js_string_from_bytes(b"privateKey".as_ptr(), 10);
        js_object_set_field_by_name(
            obj,
            public_key_name,
            f64::from_bits(JSValue::pointer(public_buf as *const u8).bits()),
        );
        js_object_set_field_by_name(
            obj,
            private_key_name,
            f64::from_bits(JSValue::pointer(private_buf as *const u8).bits()),
        );
        return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
    }

    if algo_upper != "AES-GCM"
        && algo_upper != "AES-KW"
        && algo_upper != "AES-CBC"
        && algo_upper != "AES-CTR"
    {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    // Read `length` from the algorithm object; default to 256 for the
    // string-shorthand form.
    let length = object_field_number(algo_bits.to_bits(), b"length").unwrap_or(256);
    let byte_len = match (algo_upper.as_str(), length) {
        (_, 128) => 16,
        ("AES-GCM" | "AES-KW" | "AES-CBC" | "AES-CTR", 192) => 24,
        (_, 256) => 32,
        _ => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    // Pull cryptographically strong random bytes for the key.
    let mut key_bytes = vec![0u8; byte_len];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);

    // Allocate the CryptoKey-shaped buffer + register it as AES-GCM so
    // the importKey/encrypt/decrypt path works on the result.
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: if algo_upper == "AES-CBC" {
                KeyAlgo::AesCbc
            } else if algo_upper == "AES-CTR" {
                KeyAlgo::AesCtr
            } else if algo_upper == "AES-KW" {
                KeyAlgo::AesKw
            } else {
                KeyAlgo::AesGcm
            },
            hash: HashAlgo::Sha256,
            kind: KeyKind::Secret,
        },
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}
