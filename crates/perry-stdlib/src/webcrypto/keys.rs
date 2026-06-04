use super::*;

/// `crypto.subtle.generateKey(algorithm, extractable, keyUsages)` →
/// Promise<CryptoKey>
///
/// Supported `algorithm` shapes:
/// - `{ name: "AES-GCM", length: 128 | 192 | 256 }` — generates a random
///   AES key.
/// - String shorthand `"AES-GCM"` defaults to 256-bit per the WebCrypto
///   convention (jose's `generateSecret('A256GCM')` reaches this).
/// - `{ name: "ECDSA", namedCurve: "P-256" | "P-384" | "P-521" }` — returns a
///   `{ publicKey, privateKey }` CryptoKeyPair that can be used by
///   `subtle.sign` / `subtle.verify`.
///
/// `extractable` and `keyUsages` are preserved on the returned
/// CryptoKey metadata and enforced by later WebCrypto operations.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_generate_key(
    algo_bits: f64,
    extractable_bits: f64,
    usages_bits: f64,
) -> *mut Promise {
    let extractable = bool_from_jsvalue(extractable_bits.to_bits());
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    let algo_is_string = string_from_jsvalue(algo_bits.to_bits()).is_some();
    if algo_upper == "RSA-OAEP" || algo_upper == "RSASSA-PKCS1-V1_5" || algo_upper == "RSA-PSS" {
        let hash = extract_algorithm_hash(algo_bits.to_bits(), HashAlgo::Sha1);
        let key_algo = match algo_upper.as_str() {
            "RSA-OAEP" => KeyAlgo::RsaOaep,
            "RSASSA-PKCS1-V1_5" => KeyAlgo::RsassaPkcs1,
            "RSA-PSS" => KeyAlgo::RsaPss,
            _ => unreachable!(),
        };
        let (private_usages, public_usages) = match validate_key_pair_usages(
            key_algo,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for the requested algorithm",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
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
            CryptoKeyMaterial::new(
                key_algo,
                hash,
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(key_algo, hash, KeyKind::Public, true, public_usages),
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
        let (private_usages, public_usages) = match validate_key_pair_usages(
            KeyAlgo::Ed25519,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for the requested algorithm",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
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
            CryptoKeyMaterial::new(
                KeyAlgo::Ed25519,
                HashAlgo::Sha256,
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::Ed25519,
                HashAlgo::Sha256,
                KeyKind::Public,
                true,
                public_usages,
            ),
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
    if algo_upper == "ED448" {
        let (private_usages, public_usages) = match validate_key_pair_usages(
            KeyAlgo::Ed448,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for an Ed448 key",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let mut seed = [0u8; 57];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let signing_key = match ed448_goldilocks::SigningKey::try_from(seed.as_slice()) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let public_bytes = signing_key.verifying_key().to_bytes().to_vec();

        let private_buf = alloc_uint8array_from_slice(&seed);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::Ed448,
                HashAlgo::Sha256,
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::Ed448,
                HashAlgo::Sha256,
                KeyKind::Public,
                true,
                public_usages,
            ),
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
        let (private_usages, public_usages) = match validate_key_pair_usages(
            KeyAlgo::X25519,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for the requested algorithm",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
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
            CryptoKeyMaterial::new(
                KeyAlgo::X25519,
                HashAlgo::Sha256,
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::X25519,
                HashAlgo::Sha256,
                KeyKind::Public,
                true,
                public_usages,
            ),
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
    if algo_upper == "X448" {
        let (private_usages, public_usages) = match validate_key_pair_usages(
            KeyAlgo::X448,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for the requested algorithm",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let mut seed = [0u8; 56];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let private_key = x448::StaticSecret::from(seed);
        let public_key = x448::PublicKey::from(&private_key);
        let private_bytes = private_key.as_bytes().to_vec();
        let public_bytes = public_key.as_bytes().to_vec();

        let private_buf = alloc_uint8array_from_slice(&private_bytes);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::X448,
                HashAlgo::Sha256,
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::X448,
                HashAlgo::Sha256,
                KeyKind::Public,
                true,
                public_usages,
            ),
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
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve")
            .and_then(|c| parse_ec_named_curve(&c))
        {
            Some(curve) => curve,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let key_algo = ecdh_key_algo_for_curve(curve);
        let (private_usages, public_usages) = match validate_key_pair_usages(
            key_algo,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for the requested algorithm",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let (private_bytes, public_bytes) = match generate_ecdh_key_pair_bytes(curve) {
            Some(pair) => pair,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };

        let private_buf = alloc_uint8array_from_slice(&private_bytes);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial::new(
                key_algo,
                HashAlgo::Sha256,
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(
                key_algo,
                HashAlgo::Sha256,
                KeyKind::Public,
                true,
                public_usages,
            ),
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
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve")
            .and_then(|c| parse_ec_named_curve(&c))
        {
            Some(curve) => curve,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let key_algo = ecdsa_key_algo_for_curve(curve);
        let (private_usages, public_usages) = match validate_key_pair_usages(
            key_algo,
            usages_bits.to_bits(),
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for the requested algorithm",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let (private_bytes, public_bytes) = match generate_ecdsa_key_pair_bytes(curve) {
            Some(pair) => pair,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };

        let private_buf = alloc_uint8array_from_slice(&private_bytes);
        let public_buf = alloc_uint8array_from_slice(&public_bytes);
        if private_buf.is_null() || public_buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            private_buf as usize,
            CryptoKeyMaterial::new(
                key_algo,
                ec_curve_hash(curve),
                KeyKind::Private,
                extractable,
                private_usages,
            ),
        );
        register_crypto_key(
            public_buf as usize,
            CryptoKeyMaterial::new(
                key_algo,
                ec_curve_hash(curve),
                KeyKind::Public,
                true,
                public_usages,
            ),
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

    if algo_upper == "HMAC" {
        let hash = match extract_hmac_hash(algo_bits.to_bits()) {
            Some(h) => h,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let usages = match validate_key_usages(
            KeyAlgo::Hmac,
            KeyKind::Secret,
            usages_bits.to_bits(),
            false,
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for an HMAC key",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let bit_len = object_field_number(algo_bits.to_bits(), b"length").unwrap_or(match hash {
            HashAlgo::Sha1 | HashAlgo::Sha256 => 512,
            HashAlgo::Sha384 | HashAlgo::Sha512 => 1024,
        });
        if bit_len == 0 {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let byte_len = ((bit_len + 7) / 8) as usize;
        let mut key_bytes = vec![0u8; byte_len];
        use rand::RngCore;
        rand::rngs::OsRng.fill_bytes(&mut key_bytes);
        let buf = alloc_uint8array_from_slice(&key_bytes);
        if buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            buf as usize,
            CryptoKeyMaterial::new(KeyAlgo::Hmac, hash, KeyKind::Secret, extractable, usages),
        );
        return resolve_with_bits(JSValue::pointer(buf as *const u8).bits());
    }

    if algo_upper == "CHACHA20-POLY1305" {
        let usages = match validate_key_usages(
            KeyAlgo::ChaCha20Poly1305,
            KeyKind::Secret,
            usages_bits.to_bits(),
            false,
            "Usages cannot be empty when creating a key.",
            "Unsupported key usage for a ChaCha20-Poly1305 key",
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let mut key_bytes = vec![0u8; 32];
        use rand::RngCore;
        rand::rngs::OsRng.fill_bytes(&mut key_bytes);
        let buf = alloc_uint8array_from_slice(&key_bytes);
        if buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            buf as usize,
            CryptoKeyMaterial::new(
                KeyAlgo::ChaCha20Poly1305,
                HashAlgo::Sha256,
                KeyKind::Secret,
                extractable,
                usages,
            ),
        );
        return resolve_with_bits(JSValue::pointer(buf as *const u8).bits());
    }

    if algo_upper == "KMAC128" || algo_upper == "KMAC256" {
        let key_algo = if algo_upper == "KMAC128" {
            KeyAlgo::Kmac128
        } else {
            KeyAlgo::Kmac256
        };
        let bad_message = if key_algo == KeyAlgo::Kmac128 {
            "Unsupported key usage for KMAC128 key"
        } else {
            "Unsupported key usage for KMAC256 key"
        };
        let usages = match validate_key_usages(
            key_algo,
            KeyKind::Secret,
            usages_bits.to_bits(),
            false,
            "Usages cannot be empty when creating a key.",
            bad_message,
        ) {
            Ok(u) => u,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let default_length = if key_algo == KeyAlgo::Kmac128 {
            128
        } else {
            256
        };
        let bit_len = if string_from_jsvalue(algo_bits.to_bits()).is_some() {
            default_length
        } else {
            object_field_number(algo_bits.to_bits(), b"length").unwrap_or(default_length)
        };
        if bit_len == 0 {
            return reject_with_dom_exception(
                "OperationError",
                "KmacKeyGenParams.length cannot be 0",
            );
        }
        if bit_len % 8 != 0 {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unsupported KmacKeyGenParams.length",
            );
        }
        let mut key_bytes = vec![0u8; (bit_len / 8) as usize];
        use rand::RngCore;
        rand::rngs::OsRng.fill_bytes(&mut key_bytes);
        let buf = alloc_uint8array_from_slice(&key_bytes);
        if buf.is_null() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        register_crypto_key(
            buf as usize,
            CryptoKeyMaterial::new(
                key_algo,
                HashAlgo::Sha256,
                KeyKind::Secret,
                extractable,
                usages,
            ),
        );
        return resolve_with_bits(JSValue::pointer(buf as *const u8).bits());
    }

    if algo_upper != "AES-GCM"
        && algo_upper != "AES-KW"
        && algo_upper != "AES-CBC"
        && algo_upper != "AES-CTR"
        && algo_upper != "AES-OCB"
    {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    // Read `length` from the algorithm object; default to 256 for the
    // string-shorthand form.
    let key_algo = if algo_upper == "AES-CBC" {
        KeyAlgo::AesCbc
    } else if algo_upper == "AES-CTR" {
        KeyAlgo::AesCtr
    } else if algo_upper == "AES-KW" {
        KeyAlgo::AesKw
    } else if algo_upper == "AES-OCB" {
        KeyAlgo::AesOcb
    } else {
        KeyAlgo::AesGcm
    };
    if algo_upper == "AES-OCB" && algo_is_string {
        return reject_with_dom_exception(
            "TypeError",
            "Failed to normalize algorithm: length is required",
        );
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
    let length = match object_field_number(algo_bits.to_bits(), b"length") {
        Some(length) => length,
        None if algo_upper == "AES-OCB" => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to normalize algorithm: length is required",
            )
        }
        None => 256,
    };
    let byte_len = match (algo_upper.as_str(), length) {
        (_, 128) => 16,
        ("AES-GCM" | "AES-KW" | "AES-CBC" | "AES-CTR" | "AES-OCB", 192) => 24,
        (_, 256) => 32,
        _ => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    // Pull cryptographically strong random bytes for the key.
    let mut key_bytes = vec![0u8; byte_len];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);

    // Allocate the CryptoKey-shaped buffer and register the requested
    // WebCrypto algorithm so later operations can validate it.
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial::new(
            key_algo,
            HashAlgo::Sha256,
            KeyKind::Secret,
            extractable,
            usages,
        ),
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}
