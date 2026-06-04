use super::*;

/// `crypto.subtle.sign(algorithm, key, data)` → Promise<Uint8Array>
///
/// Supports HMAC and ECDSA NIST curves. HMAC reads the hash from the
/// CryptoKey's stored material; ECDSA expects a private key produced by
/// `generateKey` or `importKey`.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_sign(
    algo_bits: f64,
    key_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_hmac_or_hash(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let data_bytes = bytes_from_jsvalue(data_bits.to_bits());
    let sig = if algo_upper == "HMAC" {
        if mat.algo != KeyAlgo::Hmac || mat.kind != KeyKind::Secret {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_SIGN, "Unable to use this key to sign")
        {
            return reject_with_dom_exception(name, message);
        }
        match compute_hmac(mat.hash, &key_bytes, &data_bytes) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if algo_upper == "KMAC128" || algo_upper == "KMAC256" {
        let key_algo = if algo_upper == "KMAC128" {
            KeyAlgo::Kmac128
        } else {
            KeyAlgo::Kmac256
        };
        if mat.algo != key_algo || mat.kind != KeyKind::Secret {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_SIGN, "Unable to use this key to sign")
        {
            return reject_with_dom_exception(name, message);
        }
        let output_length = match kmac_output_length(algo_bits.to_bits()) {
            Ok(length) => length,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        let customization =
            object_field_bytes(algo_bits.to_bits(), b"customization").unwrap_or_else(Vec::new);
        match compute_kmac(
            key_algo,
            &key_bytes,
            &customization,
            &data_bytes,
            output_length,
        ) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if algo_upper == "ECDSA" {
        if !is_ecdsa_key_algo(mat.algo) || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_SIGN, "Unable to use this key to sign")
        {
            return reject_with_dom_exception(name, message);
        }
        match mat.algo {
            KeyAlgo::EcdsaP256 => {
                let signing_key = match P256EcdsaSigningKey::from_slice(&key_bytes) {
                    Ok(k) => k,
                    Err(_) => {
                        return reject_with_dom_exception("OperationError", "The operation failed")
                    }
                };
                let sig: P256EcdsaSignature = signing_key.sign(&data_bytes);
                sig.to_bytes().as_slice().to_vec()
            }
            KeyAlgo::EcdsaP384 => {
                let signing_key = match P384EcdsaSigningKey::from_slice(&key_bytes) {
                    Ok(k) => k,
                    Err(_) => {
                        return reject_with_dom_exception("OperationError", "The operation failed")
                    }
                };
                let sig: P384EcdsaSignature = signing_key.sign(&data_bytes);
                sig.to_bytes().as_slice().to_vec()
            }
            KeyAlgo::EcdsaP521 => {
                let signing_key = match P521EcdsaSigningKey::from_slice(&key_bytes) {
                    Ok(k) => k,
                    Err(_) => {
                        return reject_with_dom_exception("OperationError", "The operation failed")
                    }
                };
                let mut rng = rand::rngs::OsRng;
                let sig: P521EcdsaSignature = signing_key.sign_with_rng(&mut rng, &data_bytes);
                sig.to_bytes().as_slice().to_vec()
            }
            _ => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if algo_upper == "ED25519" {
        if mat.algo != KeyAlgo::Ed25519 || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_SIGN, "Unable to use this key to sign")
        {
            return reject_with_dom_exception(name, message);
        }
        let secret: [u8; 32] = match key_bytes.as_slice().try_into() {
            Ok(s) => s,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret);
        use ed25519_dalek::Signer as _;
        signing_key.sign(&data_bytes).to_bytes().to_vec()
    } else if algo_upper == "RSASSA-PKCS1-V1_5" {
        if mat.algo != KeyAlgo::RsassaPkcs1 || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_SIGN, "Unable to use this key to sign")
        {
            return reject_with_dom_exception(name, message);
        }
        let private_key = match RsaPrivateKey::from_pkcs8_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        match rsa_pkcs1_sign(mat.hash, private_key, &data_bytes) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if algo_upper == "RSA-PSS" {
        if mat.algo != KeyAlgo::RsaPss || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_SIGN, "Unable to use this key to sign")
        {
            return reject_with_dom_exception(name, message);
        }
        let salt_len = object_field_bits(algo_bits.to_bits(), b"saltLength")
            .and_then(number_from_bits)
            .unwrap_or(32) as usize;
        let private_key = match RsaPrivateKey::from_pkcs8_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        match rsa_pss_sign(mat.hash, private_key, &data_bytes, salt_len) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    };
    resolve_with_bytes(&sig)
}

/// `crypto.subtle.verify(algorithm, key, signature, data)` → Promise<boolean>
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_verify(
    algo_bits: f64,
    key_bits: f64,
    sig_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_hmac_or_hash(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let data_bytes = bytes_from_jsvalue(data_bits.to_bits());
    let provided_sig = bytes_from_jsvalue(sig_bits.to_bits());
    let ok = if algo_upper == "HMAC" {
        if mat.algo != KeyAlgo::Hmac || mat.kind != KeyKind::Secret {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_VERIFY, "Unable to use this key to verify")
        {
            return reject_with_dom_exception(name, message);
        }
        let expected_sig = match compute_hmac(mat.hash, &key_bytes, &data_bytes) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        constant_time_eq(&expected_sig, &provided_sig)
    } else if algo_upper == "KMAC128" || algo_upper == "KMAC256" {
        let key_algo = if algo_upper == "KMAC128" {
            KeyAlgo::Kmac128
        } else {
            KeyAlgo::Kmac256
        };
        if mat.algo != key_algo || mat.kind != KeyKind::Secret {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_VERIFY, "Unable to use this key to verify")
        {
            return reject_with_dom_exception(name, message);
        }
        let output_length = match kmac_output_length(algo_bits.to_bits()) {
            Ok(length) => length,
            Err((name, message)) => return reject_with_dom_exception(name, message),
        };
        if output_length == 0 {
            return resolve_with_bool(false);
        }
        let customization =
            object_field_bytes(algo_bits.to_bits(), b"customization").unwrap_or_else(Vec::new);
        let expected_sig = match compute_kmac(
            key_algo,
            &key_bytes,
            &customization,
            &data_bytes,
            output_length,
        ) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        constant_time_eq(&expected_sig, &provided_sig)
    } else if algo_upper == "ECDSA" {
        if !is_ecdsa_key_algo(mat.algo) || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_VERIFY, "Unable to use this key to verify")
        {
            return reject_with_dom_exception(name, message);
        }
        match mat.algo {
            KeyAlgo::EcdsaP256 => {
                let verifying_key = match P256EcdsaVerifyingKey::from_sec1_bytes(&key_bytes) {
                    Ok(k) => k,
                    Err(_) => {
                        return reject_with_dom_exception("OperationError", "The operation failed")
                    }
                };
                let sig = match P256EcdsaSignature::from_slice(&provided_sig) {
                    Ok(s) => s,
                    Err(_) => return resolve_with_bool(false),
                };
                verifying_key.verify(&data_bytes, &sig).is_ok()
            }
            KeyAlgo::EcdsaP384 => {
                let verifying_key = match P384EcdsaVerifyingKey::from_sec1_bytes(&key_bytes) {
                    Ok(k) => k,
                    Err(_) => {
                        return reject_with_dom_exception("OperationError", "The operation failed")
                    }
                };
                let sig = match P384EcdsaSignature::from_slice(&provided_sig) {
                    Ok(s) => s,
                    Err(_) => return resolve_with_bool(false),
                };
                verifying_key.verify(&data_bytes, &sig).is_ok()
            }
            KeyAlgo::EcdsaP521 => {
                let verifying_key = match P521EcdsaVerifyingKey::from_sec1_bytes(&key_bytes) {
                    Ok(k) => k,
                    Err(_) => {
                        return reject_with_dom_exception("OperationError", "The operation failed")
                    }
                };
                let sig = match P521EcdsaSignature::from_slice(&provided_sig) {
                    Ok(s) => s,
                    Err(_) => return resolve_with_bool(false),
                };
                verifying_key.verify(&data_bytes, &sig).is_ok()
            }
            _ => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if algo_upper == "ED25519" {
        if mat.algo != KeyAlgo::Ed25519 || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_VERIFY, "Unable to use this key to verify")
        {
            return reject_with_dom_exception(name, message);
        }
        let public: [u8; 32] = match key_bytes.as_slice().try_into() {
            Ok(p) => p,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&public) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let signature = match ed25519_dalek::Signature::try_from(provided_sig.as_slice()) {
            Ok(sig) => sig,
            Err(_) => return resolve_with_bool(false),
        };
        use ed25519_dalek::Verifier as _;
        verifying_key.verify(&data_bytes, &signature).is_ok()
    } else if algo_upper == "RSASSA-PKCS1-V1_5" {
        if mat.algo != KeyAlgo::RsassaPkcs1 || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_VERIFY, "Unable to use this key to verify")
        {
            return reject_with_dom_exception(name, message);
        }
        let public_key = match RsaPublicKey::from_public_key_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        rsa_pkcs1_verify(mat.hash, public_key, &data_bytes, &provided_sig).unwrap_or(false)
    } else if algo_upper == "RSA-PSS" {
        if mat.algo != KeyAlgo::RsaPss || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        if let Err((name, message)) =
            require_usage(mat, USAGE_VERIFY, "Unable to use this key to verify")
        {
            return reject_with_dom_exception(name, message);
        }
        let salt_len = object_field_bits(algo_bits.to_bits(), b"saltLength")
            .and_then(number_from_bits)
            .unwrap_or(32) as usize;
        let public_key = match RsaPublicKey::from_public_key_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        rsa_pss_verify(mat.hash, public_key, &data_bytes, &provided_sig, salt_len).unwrap_or(false)
    } else {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    };
    resolve_with_bool(ok)
}

/// Algorithm-arg coercion shared by sign / verify: accepts a string
/// ("HMAC") or an object with a `.name` field ({ name: "HMAC" }).
pub(super) unsafe fn extract_hmac_or_hash(bits: u64) -> Option<String> {
    if let Some(s) = string_from_jsvalue(bits) {
        return Some(s);
    }
    let obj_ptr = strip_ptr(bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(b"name".as_ptr(), 4);
    let name_val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    string_from_jsvalue(name_val.bits())
}

/// Constant-time byte slice equality, to keep `verify` from leaking the
/// position of the first mismatching byte through timing.
pub(super) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

pub(super) fn number_from_bits(bits: u64) -> Option<u32> {
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFE {
        let raw = (bits & 0xFFFF_FFFF) as i32;
        return (raw >= 0).then_some(raw as u32);
    }
    let f = f64::from_bits(bits);
    if f.is_finite() && f >= 0.0 && f <= u32::MAX as f64 {
        Some(f as u32)
    } else {
        None
    }
}

unsafe fn kmac_output_length(algo_bits: u64) -> Result<u32, (&'static str, &'static str)> {
    let Some(length) = object_field_number(algo_bits, b"outputLength") else {
        return Err(("TypeError", "KmacParams.outputLength is required"));
    };
    if length % 8 != 0 {
        return Err(("NotSupportedError", "Unsupported KmacParams outputLength"));
    }
    Ok(length)
}

pub(super) unsafe fn ecdh_shared_secret_bytes(
    algo_bits: u64,
    base_key_bits: u64,
) -> Option<Vec<u8>> {
    let algo_name = match extract_algo_name(algo_bits) {
        Some(name) => name,
        None => return None,
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    if algo_upper != "ECDH" && algo_upper != "X25519" && algo_upper != "X448" {
        return None;
    }
    let public_bits = object_field_bits(algo_bits, b"public")?;
    let public_addr = strip_ptr(public_bits);
    let public_mat = lookup_crypto_key(public_addr)?;
    let base_key_addr = strip_ptr(base_key_bits);
    let base_mat = lookup_crypto_key(base_key_addr)?;
    if public_mat.kind != KeyKind::Public || base_mat.kind != KeyKind::Private {
        return None;
    }
    let private_bytes = bytes_from_jsvalue(base_key_bits);
    let public_bytes = bytes_from_jsvalue(public_bits);
    if algo_upper == "X25519" {
        if public_mat.algo != KeyAlgo::X25519 || base_mat.algo != KeyAlgo::X25519 {
            return None;
        }
        let private: [u8; 32] = private_bytes.as_slice().try_into().ok()?;
        let public: [u8; 32] = public_bytes.as_slice().try_into().ok()?;
        let private = x25519_dalek::StaticSecret::from(private);
        let public = x25519_dalek::PublicKey::from(public);
        return Some(private.diffie_hellman(&public).as_bytes().to_vec());
    }
    if algo_upper == "X448" {
        if public_mat.algo != KeyAlgo::X448 || base_mat.algo != KeyAlgo::X448 {
            return None;
        }
        let private: [u8; 56] = private_bytes.as_slice().try_into().ok()?;
        let public = x448::PublicKey::from_bytes_unchecked(&public_bytes)?;
        let private = x448::StaticSecret::from(private);
        return Some(private.diffie_hellman(&public).as_bytes().to_vec());
    }
    match (public_mat.algo, base_mat.algo) {
        (KeyAlgo::EcdhP256, KeyAlgo::EcdhP256) => {
            let private_key = P256SecretKey::from_slice(&private_bytes).ok()?;
            let public_key = P256PublicKey::from_sec1_bytes(&public_bytes).ok()?;
            let secret =
                p256_diffie_hellman(private_key.to_nonzero_scalar(), public_key.as_affine());
            Some(secret.raw_secret_bytes().to_vec())
        }
        (KeyAlgo::EcdhP384, KeyAlgo::EcdhP384) => {
            let private_key = P384SecretKey::from_slice(&private_bytes).ok()?;
            let public_key = P384PublicKey::from_sec1_bytes(&public_bytes).ok()?;
            let secret =
                p384_diffie_hellman(private_key.to_nonzero_scalar(), public_key.as_affine());
            Some(secret.raw_secret_bytes().to_vec())
        }
        (KeyAlgo::EcdhP521, KeyAlgo::EcdhP521) => {
            let private_key = P521SecretKey::from_slice(&private_bytes).ok()?;
            let public_key = P521PublicKey::from_sec1_bytes(&public_bytes).ok()?;
            let secret =
                p521_diffie_hellman(private_key.to_nonzero_scalar(), public_key.as_affine());
            Some(secret.raw_secret_bytes().to_vec())
        }
        _ => None,
    }
}

pub(super) unsafe fn ecdh_public_private_curve_mismatch(
    algo_bits: u64,
    base_key_bits: u64,
) -> bool {
    let algo_name = match extract_algo_name(algo_bits) {
        Some(name) => name,
        None => return false,
    };
    if algo_name.to_ascii_uppercase() != "ECDH" {
        return false;
    }
    let public_bits = match object_field_bits(algo_bits, b"public") {
        Some(bits) => bits,
        None => return false,
    };
    let public_mat = match lookup_crypto_key(strip_ptr(public_bits)) {
        Some(mat) => mat,
        None => return false,
    };
    let base_mat = match lookup_crypto_key(strip_ptr(base_key_bits)) {
        Some(mat) => mat,
        None => return false,
    };
    public_mat.kind == KeyKind::Public
        && base_mat.kind == KeyKind::Private
        && is_ecdh_key_algo(public_mat.algo)
        && is_ecdh_key_algo(base_mat.algo)
        && public_mat.algo != base_mat.algo
}

pub(super) fn hkdf_expand(
    hash: HashAlgo,
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    out: &mut [u8],
) -> bool {
    match hash {
        HashAlgo::Sha1 => hkdf::Hkdf::<Sha1>::new(Some(salt), ikm)
            .expand(info, out)
            .is_ok(),
        HashAlgo::Sha256 => hkdf::Hkdf::<Sha256>::new(Some(salt), ikm)
            .expand(info, out)
            .is_ok(),
        HashAlgo::Sha384 => hkdf::Hkdf::<Sha384>::new(Some(salt), ikm)
            .expand(info, out)
            .is_ok(),
        HashAlgo::Sha512 => hkdf::Hkdf::<Sha512>::new(Some(salt), ikm)
            .expand(info, out)
            .is_ok(),
    }
}

pub(super) fn pbkdf2_derive(
    hash: HashAlgo,
    pass: &[u8],
    salt: &[u8],
    iterations: u32,
    out: &mut [u8],
) {
    match hash {
        HashAlgo::Sha1 => pbkdf2::pbkdf2_hmac::<Sha1>(pass, salt, iterations, out),
        HashAlgo::Sha256 => pbkdf2::pbkdf2_hmac::<Sha256>(pass, salt, iterations, out),
        HashAlgo::Sha384 => pbkdf2::pbkdf2_hmac::<Sha384>(pass, salt, iterations, out),
        HashAlgo::Sha512 => pbkdf2::pbkdf2_hmac::<Sha512>(pass, salt, iterations, out),
    }
}

fn argon2_algorithm_kind(algo: KeyAlgo) -> Option<argon2::Algorithm> {
    match algo {
        KeyAlgo::Argon2d => Some(argon2::Algorithm::Argon2d),
        KeyAlgo::Argon2i => Some(argon2::Algorithm::Argon2i),
        KeyAlgo::Argon2id => Some(argon2::Algorithm::Argon2id),
        _ => None,
    }
}

unsafe fn argon2_derive(
    algo_bits: u64,
    base_mat: CryptoKeyMaterial,
    base_key: &[u8],
    byte_len: usize,
) -> Result<Option<Vec<u8>>, (&'static str, &'static str)> {
    let algo_name = match extract_algo_name(algo_bits) {
        Some(name) => name,
        None => return Ok(None),
    };
    let requested = match argon2_key_algo(&algo_name) {
        Some(algo) => algo,
        None => return Ok(None),
    };
    if base_mat.algo != requested {
        return Err(("InvalidAccessError", "Key algorithm mismatch"));
    }
    if byte_len < 4 {
        return Err(("OperationError", "length must be >= 32"));
    }

    let nonce = match object_field_bytes(algo_bits, b"nonce") {
        Some(bytes) => bytes,
        None => {
            return Err((
                "TypeError",
                "Failed to normalize algorithm: passed algorithm cannot be converted to 'Argon2Params' because 'nonce' is required in 'Argon2Params'.",
            ))
        }
    };
    if nonce.len() < 8 {
        return Err(("OperationError", "nonce must be at least 8 bytes"));
    }
    let parallelism = match object_field_number(algo_bits, b"parallelism") {
        Some(value) => value,
        None => {
            return Err((
                "TypeError",
                "Failed to normalize algorithm: passed algorithm cannot be converted to 'Argon2Params' because 'parallelism' is required in 'Argon2Params'.",
            ))
        }
    };
    if parallelism == 0 || parallelism > 0xFF_FFFF {
        return Err(("OperationError", "parallelism must be > 0 and <= 16777215"));
    }
    let memory = match object_field_number(algo_bits, b"memory") {
        Some(value) => value,
        None => {
            return Err((
                "TypeError",
                "Failed to normalize algorithm: passed algorithm cannot be converted to 'Argon2Params' because 'memory' is required in 'Argon2Params'.",
            ))
        }
    };
    if memory < parallelism.saturating_mul(8).max(8) {
        return Err((
            "OperationError",
            "memory must be at least 8 times the degree of parallelism",
        ));
    }
    let passes = match object_field_number(algo_bits, b"passes") {
        Some(value) => value,
        None => {
            return Err((
                "TypeError",
                "Failed to normalize algorithm: passed algorithm cannot be converted to 'Argon2Params' because 'passes' is required in 'Argon2Params'.",
            ))
        }
    };
    if passes == 0 {
        return Err(("OperationError", "passes must be > 0"));
    }
    let version = object_field_number(algo_bits, b"version").unwrap_or(0x13);
    if version != 0x13 {
        return Err(("OperationError", "version must be 0x13"));
    }

    let associated_data = object_field_bytes(algo_bits, b"associatedData").unwrap_or_default();
    let secret = object_field_bytes(algo_bits, b"secretValue").unwrap_or_default();

    let mut builder = argon2::ParamsBuilder::new();
    builder
        .m_cost(memory)
        .t_cost(passes)
        .p_cost(parallelism)
        .output_len(byte_len);
    if !associated_data.is_empty() {
        let data = argon2::AssociatedData::new(&associated_data)
            .map_err(|_| ("OperationError", "The operation failed"))?;
        builder.data(data);
    }
    let params = builder
        .build()
        .map_err(|_| ("OperationError", "The operation failed"))?;
    let algorithm = argon2_algorithm_kind(requested)
        .ok_or(("NotSupportedError", "Unrecognized algorithm name"))?;
    let argon = if secret.is_empty() {
        argon2::Argon2::new(algorithm, argon2::Version::V0x13, params)
    } else {
        argon2::Argon2::new_with_secret(&secret, algorithm, argon2::Version::V0x13, params)
            .map_err(|_| ("OperationError", "The operation failed"))?
    };
    let mut out = vec![0u8; byte_len];
    argon
        .hash_password_into(base_key, &nonce, &mut out)
        .map_err(|_| ("OperationError", "The operation failed"))?;
    Ok(Some(out))
}

pub(super) unsafe fn kdf_derive_bytes(
    algo_bits: u64,
    base_key_bits: u64,
    byte_len: usize,
) -> Result<Option<Vec<u8>>, (&'static str, &'static str)> {
    let algo_name = match extract_algo_name(algo_bits) {
        Some(name) => name,
        None => return Ok(None),
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    let base_key_addr = strip_ptr(base_key_bits);
    let base_mat = match lookup_crypto_key(base_key_addr) {
        Some(mat) => mat,
        None => return Ok(None),
    };
    if base_mat.kind != KeyKind::Secret {
        return Ok(None);
    }
    let base_key = bytes_from_jsvalue(base_key_bits);
    if let Some(bytes) = argon2_derive(algo_bits, base_mat, &base_key, byte_len)? {
        return Ok(Some(bytes));
    }
    let mut out = vec![0u8; byte_len];
    if algo_upper == "HKDF" {
        if base_mat.algo != KeyAlgo::Hkdf {
            return Ok(None);
        }
        let hash = extract_algorithm_hash(algo_bits, base_mat.hash);
        let salt = object_field_bytes(algo_bits, b"salt").unwrap_or_default();
        let info = object_field_bytes(algo_bits, b"info").unwrap_or_default();
        if hkdf_expand(hash, &base_key, &salt, &info, &mut out) {
            return Ok(Some(out));
        }
        return Ok(None);
    }
    if algo_upper == "PBKDF2" {
        if base_mat.algo != KeyAlgo::Pbkdf2 {
            return Ok(None);
        }
        let hash = extract_algorithm_hash(algo_bits, base_mat.hash);
        let salt = object_field_bytes(algo_bits, b"salt").unwrap_or_default();
        let iterations = match object_field_number(algo_bits, b"iterations") {
            Some(value) => value,
            None => return Ok(None),
        };
        if iterations == 0 {
            return Ok(None);
        }
        pbkdf2_derive(hash, &base_key, &salt, iterations, &mut out);
        return Ok(Some(out));
    }
    Ok(None)
}
