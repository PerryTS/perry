use super::*;

/// Resolve `(start, end)` byte indices from Node-style `offset` / `size`
/// arguments against a buffer of `total` bytes. Out-of-range values are
/// clamped to `[0, total]`.
pub(super) fn resolve_range(
    total: usize,
    offset: Option<usize>,
    size: Option<usize>,
) -> (usize, usize) {
    let start = offset.unwrap_or(0).min(total);
    let end = match size {
        Some(s) => start.saturating_add(s).min(total),
        None => total,
    };
    (start, end)
}

/// Create HMAC-SHA256
/// crypto.createHmac('sha256', key).update(data).digest('hex') -> string
#[no_mangle]
pub unsafe extern "C" fn js_crypto_hmac_sha256(
    key_ptr: *const StringHeader,
    data_ptr: *const StringHeader,
) -> *mut StringHeader {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };

    let data = match string_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut mac = match HmacSha256::new_from_slice(&key) {
        Ok(m) => m,
        Err(_) => return std::ptr::null_mut(),
    };

    mac.update(&data);
    let result = mac.finalize();
    let hex_str = hex::encode(result.into_bytes());

    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// HMAC-SHA-256 over arbitrary bytes, returning a Buffer. Used by
/// `.digest()` (no arg) for SCRAM-SHA-256 key derivation.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_hmac_sha256_bytes(
    key_ptr: i64,
    data_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    use hmac::{Hmac, KeyInit, Mac};
    type HmacSha256 = Hmac<Sha256>;

    let key = bytes_from_ptr(key_ptr);
    let data = bytes_from_ptr(data_ptr);
    let mut mac = match HmacSha256::new_from_slice(&key) {
        Ok(m) => m,
        Err(_) => return perry_runtime::buffer::buffer_alloc(0),
    };
    mac.update(&data);
    let digest = mac.finalize().into_bytes();
    alloc_buffer_from_slice(&digest)
}

/// crypto.sign("RSA-SHA256", data, privateKeyPem) -> Buffer.
///
/// Covers Node's one-shot RSASSA-PKCS1-v1_5 SHA-256 signing path for PEM RSA
/// private keys, a large asymmetric-crypto area exercised by Node/Bun parity
/// suites and many real packages.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_sign_rsa_sha256(
    alg_ptr: i64,
    data_ptr: i64,
    key_val: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let data = bytes_from_ptr(data_ptr);
    let key_bits = key_val.to_bits();
    let pem = match crypto_key_input_to_private_pem(key_bits) {
        Some(pem) => pem,
        None => return alloc_buffer_from_slice(&[]),
    };
    if let Some(signing_key) = parse_ed25519_private_surrogate(&pem) {
        use ed25519_dalek::Signer as _;
        let signature = signing_key.sign(&data);
        return alloc_buffer_from_slice(&signature.to_bytes());
    }
    let alg = bytes_from_ptr(alg_ptr);
    let alg = match normalize_sign_algorithm(&alg) {
        Some(alg) => alg,
        None => return alloc_buffer_from_slice(&[]),
    };
    if let Some(signing_key) = parse_p256_signing_key_pem(&pem) {
        let signature: P256EcdsaSignature = signing_key.sign(&data);
        if key_input_uses_ieee_p1363(key_bits) {
            let raw = signature.to_bytes();
            return alloc_buffer_from_slice(raw.as_slice());
        }
        let der = signature.to_der();
        return alloc_buffer_from_slice(der.as_bytes());
    }
    let private_key = match parse_rsa_private_key_pem(&pem) {
        Some(key) => key,
        None => return alloc_buffer_from_slice(&[]),
    };

    let signature = if key_input_uses_rsa_pss(key_bits) {
        let salt_len = key_input_pss_salt_len(key_bits, alg);
        sign_rsa_pss_data(alg, private_key, &data, salt_len)
    } else {
        sign_rsa_data(alg, private_key, &data)
    };
    alloc_buffer_from_slice(&signature)
}

/// `crypto.sign(algorithm, data, key, callback)` callback form.
///
/// Perry executes the work synchronously but preserves Node's observable
/// callback shape `(err, signature)` and returns `undefined`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_sign_async(
    alg_ptr: i64,
    data_ptr: i64,
    key_val: f64,
    callback_bits: f64,
) -> f64 {
    let buf = js_crypto_sign_rsa_sha256(alg_ptr, data_ptr, key_val);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// crypto.verify("RSA-SHA256", data, publicOrPrivateKeyPem, signature) -> boolean.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_verify_rsa_sha256(
    alg_ptr: i64,
    data_ptr: i64,
    key_val: f64,
    sig_ptr: i64,
) -> f64 {
    let data = bytes_from_ptr(data_ptr);
    let key_bits = key_val.to_bits();
    let sig_bytes = bytes_from_ptr(sig_ptr);
    let pem = match crypto_key_input_to_public_pem(key_bits) {
        Some(pem) => pem,
        None => return js_bool(false),
    };
    if let Some(verifying_key) = parse_ed25519_public_surrogate(&pem) {
        use ed25519_dalek::Verifier as _;
        let signature = match ed25519_dalek::Signature::try_from(sig_bytes.as_slice()) {
            Ok(sig) => sig,
            Err(_) => return js_bool(false),
        };
        return js_bool(verifying_key.verify(&data, &signature).is_ok());
    }
    let alg = bytes_from_ptr(alg_ptr);
    let alg = match normalize_sign_algorithm(&alg) {
        Some(alg) => alg,
        None => return js_bool(false),
    };
    if let Some(verifying_key) = parse_p256_verifying_key_pem(&pem) {
        let signature = if key_input_uses_ieee_p1363(key_bits) {
            P256EcdsaSignature::from_slice(&sig_bytes)
        } else {
            P256EcdsaSignature::from_der(&sig_bytes)
        };
        let signature = match signature {
            Ok(sig) => sig,
            Err(_) => return js_bool(false),
        };
        return js_bool(verifying_key.verify(&data, &signature).is_ok());
    }
    let public_key = match parse_rsa_public_key_pem(&pem) {
        Some(key) => key,
        None => return js_bool(false),
    };
    if key_input_uses_rsa_pss(key_bits) {
        let signature = match RsaPssSignature::try_from(sig_bytes.as_slice()) {
            Ok(sig) => sig,
            Err(_) => return js_bool(false),
        };
        let salt_len = key_input_pss_salt_len(key_bits, alg);
        return js_bool(verify_rsa_pss_data(
            alg, public_key, &data, &signature, salt_len,
        ));
    }
    let signature = match RsaPkcs1v15Signature::try_from(sig_bytes.as_slice()) {
        Ok(sig) => sig,
        Err(_) => return js_bool(false),
    };

    let ok = verify_rsa_data(alg, public_key, &data, &signature);
    js_bool(ok)
}

/// `crypto.verify(algorithm, data, key, signature, callback)` callback form.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_verify_async(
    alg_ptr: i64,
    data_ptr: i64,
    key_val: f64,
    sig_ptr: i64,
    callback_bits: f64,
) -> f64 {
    let ok = js_crypto_verify_rsa_sha256(alg_ptr, data_ptr, key_val, sig_ptr);
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), ok);
    f64::from_bits(JSValue::undefined().bits())
}

/// crypto.publicEncrypt(publicOrPrivateKeyPem, data) -> Buffer.
///
/// Matches Node's default RSA_PKCS1_OAEP_PADDING with SHA-1 for PEM keys.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_public_encrypt(
    key_ptr: i64,
    data_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let key_bytes = bytes_from_ptr(key_ptr);
    let pem = match String::from_utf8(key_bytes) {
        Ok(pem) => pem,
        Err(_) => return alloc_buffer_from_slice(&[]),
    };
    let public_key = match parse_rsa_public_key_pem(&pem) {
        Some(key) => key,
        None => return alloc_buffer_from_slice(&[]),
    };
    let data = bytes_from_ptr(data_ptr);
    let mut rng = rand::thread_rng();
    match public_key.encrypt(&mut rng, Oaep::new::<rsa_sha1::Sha1>(), &data) {
        Ok(ciphertext) => alloc_buffer_from_slice(&ciphertext),
        Err(_) => alloc_buffer_from_slice(&[]),
    }
}

/// crypto.privateDecrypt(privateKeyPem, ciphertext) -> Buffer.
///
/// Matches Node's default RSA_PKCS1_OAEP_PADDING with SHA-1 for PEM keys.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_private_decrypt(
    key_ptr: i64,
    data_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let key_bytes = bytes_from_ptr(key_ptr);
    let pem = match String::from_utf8(key_bytes) {
        Ok(pem) => pem,
        Err(_) => return alloc_buffer_from_slice(&[]),
    };
    let private_key = match parse_rsa_private_key_pem(&pem) {
        Some(key) => key,
        None => return alloc_buffer_from_slice(&[]),
    };
    let ciphertext = bytes_from_ptr(data_ptr);
    let mut rng = rand::thread_rng();
    match private_key.decrypt_blinded(&mut rng, Oaep::new::<rsa_sha1::Sha1>(), &ciphertext) {
        Ok(plaintext) => alloc_buffer_from_slice(&plaintext),
        Err(_) => alloc_buffer_from_slice(&[]),
    }
}

/// crypto.privateEncrypt(privateKeyPem, data) -> Buffer.
///
/// Implements Node's default RSA_PKCS1_PADDING using the same PKCS#1 v1.5
/// type-1 block shape as unprefixed RSA signatures. Paired with
/// `publicDecrypt` below for the RSA public/private transform tests present
/// in Node and Bun.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_private_encrypt(
    key_ptr: i64,
    data_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let key_bytes = bytes_from_ptr(key_ptr);
    let pem = match String::from_utf8(key_bytes) {
        Ok(pem) => pem,
        Err(_) => return alloc_buffer_from_slice(&[]),
    };
    let private_key = match parse_rsa_private_key_pem(&pem) {
        Some(key) => key,
        None => return alloc_buffer_from_slice(&[]),
    };
    let data = bytes_from_ptr(data_ptr);
    match private_key.sign(Pkcs1v15Sign::new_unprefixed(), &data) {
        Ok(ciphertext) => alloc_buffer_from_slice(&ciphertext),
        Err(_) => alloc_buffer_from_slice(&[]),
    }
}

/// crypto.publicDecrypt(publicOrPrivateKeyPem, ciphertext) -> Buffer.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_public_decrypt(
    key_ptr: i64,
    data_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let key_bytes = bytes_from_ptr(key_ptr);
    let pem = match String::from_utf8(key_bytes) {
        Ok(pem) => pem,
        Err(_) => return alloc_buffer_from_slice(&[]),
    };
    let public_key = match parse_rsa_public_key_pem(&pem) {
        Some(key) => key,
        None => return alloc_buffer_from_slice(&[]),
    };
    let ciphertext = bytes_from_ptr(data_ptr);
    match rsa_public_unpad_pkcs1_type1(&public_key, &ciphertext) {
        Some(plaintext) => alloc_buffer_from_slice(&plaintext),
        None => alloc_buffer_from_slice(&[]),
    }
}

/// crypto.createPublicKey(key) minimal PEM-KeyObject surrogate.
///
/// Perry's native crypto paths accept PEM strings directly. This helper
/// converts a public or private RSA PEM into the matching public PEM, so
/// `createPublicKey(createPrivateKey(pem))` can be used as input to
/// sign/verify/encrypt parity tests.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_public_key(
    key_ptr: i64,
) -> *mut perry_runtime::StringHeader {
    let key_bytes = bytes_from_ptr(key_ptr);
    let pem = match String::from_utf8(key_bytes) {
        Ok(pem) => pem,
        Err(_) => return std::ptr::null_mut(),
    };
    if let Some(verifying_key) = parse_p256_verifying_key_pem(&pem) {
        use p256::pkcs8::EncodePublicKey;
        if let Ok(public_pem) = verifying_key.to_public_key_pem(Default::default()) {
            return js_string_from_bytes(public_pem.as_ptr(), public_pem.len() as u32);
        }
    }
    let public_key = match parse_rsa_public_key_pem(&pem) {
        Some(key) => key,
        None => return std::ptr::null_mut(),
    };
    let public_pem = match rsa_public_key_to_pem(&public_key) {
        Some(pem) => pem,
        None => return std::ptr::null_mut(),
    };
    js_string_from_bytes(public_pem.as_ptr(), public_pem.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_private_key_value(key_bits: f64) -> *mut StringHeader {
    let pem = match crypto_key_input_to_private_pem(key_bits.to_bits()) {
        Some(pem) => pem,
        None => return std::ptr::null_mut(),
    };
    let ptr = js_string_from_bytes(pem.as_ptr(), pem.len() as u32);
    if let Some(asym_type) = classify_private_key_surrogate(&pem) {
        mark_keyobject_string(ptr, KeyKind::Private, asym_type);
    }
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_public_key_value(key_bits: f64) -> *mut StringHeader {
    let pem = match crypto_key_input_to_public_pem(key_bits.to_bits()) {
        Some(pem) => pem,
        None => return std::ptr::null_mut(),
    };
    let ptr = js_string_from_bytes(pem.as_ptr(), pem.len() as u32);
    if let Some(asym_type) = classify_public_key_surrogate(&pem) {
        mark_keyobject_string(ptr, KeyKind::Public, asym_type);
    }
    ptr
}

/// crypto.generateKeyPairSync("rsa", options) -> { publicKey, privateKey }.
///
/// This covers the high-value Node/Bun shape where `publicKeyEncoding` and
/// `privateKeyEncoding` request PEM output. Perry currently returns PEM
/// strings unconditionally, which are accepted by the rest of the native RSA
/// helpers as KeyObject surrogates.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_sync_rsa(
    options_bits: f64,
) -> *mut ObjectHeader {
    use rsa::pkcs8::EncodePrivateKey;

    let mut rng = rand::thread_rng();
    let private_key = match RsaPrivateKey::new(&mut rng, 2048) {
        Ok(key) => key,
        Err(_) => return js_object_alloc(0, 0),
    };
    let public_key = RsaPublicKey::from(&private_key);
    let public_pem = rsa_public_key_to_pem(&public_key).unwrap_or_default();
    let options = options_bits.to_bits();
    let public_as_jwk = keygen_encoding_wants_jwk(options, b"publicKeyEncoding");
    let private_as_jwk = keygen_encoding_wants_jwk(options, b"privateKeyEncoding");
    let private_passphrase = keygen_rsa_private_encryption_passphrase(options);
    let private_pem = if private_as_jwk {
        String::new()
    } else if let Some(passphrase) = private_passphrase.as_deref() {
        private_key
            .to_pkcs8_encrypted_pem(&mut rng, passphrase, Default::default())
            .map(|pem| pem.to_string())
            .unwrap_or_default()
    } else {
        private_key
            .to_pkcs8_pem(Default::default())
            .map(|pem| pem.to_string())
            .unwrap_or_default()
    };

    let obj = js_object_alloc(0, 2);

    if public_as_jwk {
        if let Some(public_jwk) = rsa_public_jwk_object(&public_key) {
            set_object_value_field(obj, b"publicKey", nanbox_pointer(public_jwk));
        }
    } else {
        let name = js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let val = js_string_from_bytes(public_pem.as_ptr(), public_pem.len() as u32);
        mark_keyobject_string(val, KeyKind::Public, 1);
        js_object_set_field_by_name(obj, name, nanbox_str(val));
    }

    if private_as_jwk {
        if let Some(private_jwk) = rsa_private_jwk_object(&private_key) {
            set_object_value_field(obj, b"privateKey", nanbox_pointer(private_jwk));
        }
    } else {
        let name = js_string_from_bytes(b"privateKey".as_ptr(), 10);
        let val = js_string_from_bytes(private_pem.as_ptr(), private_pem.len() as u32);
        mark_keyobject_string(val, KeyKind::Private, 1);
        js_object_set_field_by_name(obj, name, nanbox_str(val));
    }

    obj
}

/// crypto.generateKeyPairSync("ec", { namedCurve: "prime256v1", ...pem }) ->
/// { publicKey, privateKey }.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_sync_ec_p256(
    options_bits: f64,
) -> *mut ObjectHeader {
    use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};

    let private_key = match generate_p256_secret_key() {
        Some(key) => key,
        None => return js_object_alloc(0, 0),
    };
    let public_key = private_key.public_key();
    let private_pem = private_key
        .to_pkcs8_pem(Default::default())
        .map(|pem| pem.to_string())
        .unwrap_or_default();
    let public_pem = public_key
        .to_public_key_pem(Default::default())
        .unwrap_or_default();
    let options = options_bits.to_bits();
    let public_as_jwk = keygen_encoding_wants_jwk(options, b"publicKeyEncoding");
    let private_as_jwk = keygen_encoding_wants_jwk(options, b"privateKeyEncoding");

    let obj = js_object_alloc(0, 2);

    if public_as_jwk {
        if let Some(public_jwk) = ec_p256_public_jwk_object(&public_key) {
            set_object_value_field(obj, b"publicKey", nanbox_pointer(public_jwk));
        }
    } else {
        let name = js_string_from_bytes(b"publicKey".as_ptr(), 9);
        let val = js_string_from_bytes(public_pem.as_ptr(), public_pem.len() as u32);
        mark_keyobject_string(val, KeyKind::Public, 2);
        js_object_set_field_by_name(obj, name, nanbox_str(val));
    }

    if private_as_jwk {
        if let Some(private_jwk) = ec_p256_private_jwk_object(&private_key) {
            set_object_value_field(obj, b"privateKey", nanbox_pointer(private_jwk));
        }
    } else {
        let name = js_string_from_bytes(b"privateKey".as_ptr(), 10);
        let val = js_string_from_bytes(private_pem.as_ptr(), private_pem.len() as u32);
        mark_keyobject_string(val, KeyKind::Private, 2);
        js_object_set_field_by_name(obj, name, nanbox_str(val));
    }

    obj
}

/// crypto.generateKeyPairSync("ed25519") -> { publicKey, privateKey }.
///
/// Perry represents the keys as internal string surrogates that the native
/// one-shot sign/verify path understands. This covers the Node/Bun Ed25519
/// keygen + sign/verify compatibility shape without exposing real KeyObjects.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_sync_ed25519(
    _options_bits: f64,
) -> *mut ObjectHeader {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let private_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public_key = private_key.verifying_key();
    let private_surrogate = ed25519_private_surrogate(&private_key);
    let public_surrogate = ed25519_public_surrogate(&public_key);

    let obj = js_object_alloc(0, 2);
    let pub_name = js_string_from_bytes(b"publicKey".as_ptr(), 9);
    let pub_val = js_string_from_bytes(public_surrogate.as_ptr(), public_surrogate.len() as u32);
    mark_keyobject_string(pub_val, KeyKind::Public, 3);
    js_object_set_field_by_name(obj, pub_name, nanbox_str(pub_val));
    let priv_name = js_string_from_bytes(b"privateKey".as_ptr(), 10);
    let priv_val = js_string_from_bytes(private_surrogate.as_ptr(), private_surrogate.len() as u32);
    mark_keyobject_string(priv_val, KeyKind::Private, 3);
    js_object_set_field_by_name(obj, priv_name, nanbox_str(priv_val));
    obj
}

/// crypto.generateKeyPairSync("x25519") -> { publicKey, privateKey }.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_sync_x25519(
    _options_bits: f64,
) -> *mut ObjectHeader {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let private_key = x25519_dalek::StaticSecret::from(seed);
    let public_key = x25519_dalek::PublicKey::from(&private_key);
    let private_surrogate = x25519_private_surrogate(&private_key.to_bytes());
    let public_surrogate = x25519_public_surrogate(&public_key.to_bytes());

    let obj = js_object_alloc(0, 2);
    let pub_name = js_string_from_bytes(b"publicKey".as_ptr(), 9);
    let pub_val = js_string_from_bytes(public_surrogate.as_ptr(), public_surrogate.len() as u32);
    mark_keyobject_string(pub_val, KeyKind::Public, 4);
    js_object_set_field_by_name(obj, pub_name, nanbox_str(pub_val));
    let priv_name = js_string_from_bytes(b"privateKey".as_ptr(), 10);
    let priv_val = js_string_from_bytes(private_surrogate.as_ptr(), private_surrogate.len() as u32);
    mark_keyobject_string(priv_val, KeyKind::Private, 4);
    js_object_set_field_by_name(obj, priv_name, nanbox_str(priv_val));
    obj
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_diffie_hellman(
    options_val: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let options_bits = options_val.to_bits();
    let private_bits = match object_field_bits(options_bits, b"privateKey") {
        Some(bits) => bits,
        None => return alloc_buffer_from_slice(&[]),
    };
    let public_bits = match object_field_bits(options_bits, b"publicKey") {
        Some(bits) => bits,
        None => return alloc_buffer_from_slice(&[]),
    };
    let private_value = match crypto_key_input_to_private_pem(private_bits) {
        Some(value) => value,
        None => return alloc_buffer_from_slice(&[]),
    };
    let public_value = match crypto_key_input_to_public_pem(public_bits) {
        Some(value) => value,
        None => return alloc_buffer_from_slice(&[]),
    };
    let private = match parse_x25519_private_surrogate(&private_value) {
        Some(private) => private,
        None => return alloc_buffer_from_slice(&[]),
    };
    let public = match parse_x25519_public_surrogate(&public_value) {
        Some(public) => public,
        None => return alloc_buffer_from_slice(&[]),
    };
    let private = x25519_dalek::StaticSecret::from(private);
    let public = x25519_dalek::PublicKey::from(public);
    let secret = private.diffie_hellman(&public);
    alloc_buffer_from_slice(secret.as_bytes())
}
