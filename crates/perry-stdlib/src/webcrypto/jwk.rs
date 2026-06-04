use super::*;

/// `crypto.subtle.importKey("raw", keyBytes, algorithm, extractable, keyUsages)`
/// → Promise<CryptoKey>
///
/// `format == "raw"` only. Supported algorithms:
/// - `{ name: "HMAC", hash: "SHA-256" }` (and SHA-1/384/512)
/// - `"AES-GCM"` or `{ name: "AES-GCM" }` — keyed by 128/192/256-bit
///   bytes; the IV / additionalData come in at encrypt/decrypt time.
///
/// `extractable` and `keyUsages` are stored on the returned key and
/// enforced by later WebCrypto operations.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_import_key(
    format_bits: f64,
    key_bits: f64,
    algo_bits: f64,
    extractable_bits: f64,
    usages_bits: f64,
) -> *mut Promise {
    let extractable = bool_from_jsvalue(extractable_bits.to_bits());
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to execute 'importKey' on 'SubtleCrypto': format must be a string",
            )
        }
    };
    let format_lower = format.to_ascii_lowercase();
    if format_lower != "raw"
        && format_lower != "raw-secret"
        && format_lower != "spki"
        && format_lower != "pkcs8"
        && format_lower != "jwk"
    {
        return reject_with_dom_exception("NotSupportedError", "Unsupported key format");
    }
    // Algorithm name — accepts string shorthand ("AES-GCM") or
    // `{ name: "..." }` object form.
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    if (algo_upper == "KMAC128" || algo_upper == "KMAC256") && format_lower == "raw" {
        let message = if algo_upper == "KMAC128" {
            "Unable to import KMAC128 using raw format"
        } else {
            "Unable to import KMAC256 using raw format"
        };
        return reject_with_dom_exception("NotSupportedError", message);
    }
    let (key_algo, hash, kind) = if algo_upper == "HMAC"
        && (format_lower == "raw" || format_lower == "jwk")
    {
        let hash = match extract_hmac_hash(algo_bits.to_bits()) {
            Some(h) => h,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        (KeyAlgo::Hmac, hash, KeyKind::Secret)
    } else if algo_upper == "HKDF" && format_lower == "raw" {
        let hash = extract_algorithm_hash(algo_bits.to_bits(), HashAlgo::Sha256);
        (KeyAlgo::Hkdf, hash, KeyKind::Secret)
    } else if algo_upper == "PBKDF2" && format_lower == "raw" {
        let hash = extract_algorithm_hash(algo_bits.to_bits(), HashAlgo::Sha256);
        (KeyAlgo::Pbkdf2, hash, KeyKind::Secret)
    } else if let Some(argon_algo) = argon2_key_algo(&algo_name) {
        if format_lower != "raw-secret" {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unsupported algorithm for the given key format",
            );
        }
        if extractable {
            let message = match argon_algo {
                KeyAlgo::Argon2d => "Argon2d keys are not extractable",
                KeyAlgo::Argon2i => "Argon2i keys are not extractable",
                KeyAlgo::Argon2id => "Argon2id keys are not extractable",
                _ => unreachable!(),
            };
            return reject_with_dom_exception("SyntaxError", message);
        }
        (argon_algo, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "KMAC128" && format_lower == "jwk" {
        (KeyAlgo::Kmac128, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "KMAC256" && format_lower == "jwk" {
        (KeyAlgo::Kmac256, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "AES-GCM" && (format_lower == "raw" || format_lower == "jwk") {
        // AES-GCM: 128, 192, or 256-bit keys. We accept any length
        // here and let encrypt/decrypt fail loudly on mismatch.
        (KeyAlgo::AesGcm, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "AES-KW" && (format_lower == "raw" || format_lower == "jwk") {
        // AES-KW: RFC 3394 key wrapping key. The wrap/unwrap path only
        // needs the raw bytes plus a registered CryptoKey marker; key
        // length is validated by the AES-KW helper itself.
        (KeyAlgo::AesKw, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "AES-CBC" && (format_lower == "raw" || format_lower == "jwk") {
        (KeyAlgo::AesCbc, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "AES-CTR" && (format_lower == "raw" || format_lower == "jwk") {
        (KeyAlgo::AesCtr, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "CHACHA20-POLY1305" && format_lower == "jwk" {
        (KeyAlgo::ChaCha20Poly1305, HashAlgo::Sha256, KeyKind::Secret)
    } else if algo_upper == "ECDSA" && (format_lower == "raw" || format_lower == "jwk") {
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve")
            .and_then(|c| parse_ec_named_curve(&c))
        {
            Some(curve) => curve,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let kind =
            if format_lower == "jwk" && object_field_string(key_bits.to_bits(), b"d").is_some() {
                KeyKind::Private
            } else {
                KeyKind::Public
            };
        (ecdsa_key_algo_for_curve(curve), ec_curve_hash(curve), kind)
    } else if algo_upper == "ECDH" && (format_lower == "raw" || format_lower == "jwk") {
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve")
            .and_then(|c| parse_ec_named_curve(&c))
        {
            Some(curve) => curve,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let kind =
            if format_lower == "jwk" && object_field_string(key_bits.to_bits(), b"d").is_some() {
                KeyKind::Private
            } else {
                KeyKind::Public
            };
        (ecdh_key_algo_for_curve(curve), HashAlgo::Sha256, kind)
    } else if algo_upper == "ED25519" && (format_lower == "raw" || format_lower == "jwk") {
        let kind =
            if format_lower == "jwk" && object_field_string(key_bits.to_bits(), b"d").is_some() {
                KeyKind::Private
            } else {
                KeyKind::Public
            };
        (KeyAlgo::Ed25519, HashAlgo::Sha256, kind)
    } else if algo_upper == "X25519" && (format_lower == "raw" || format_lower == "jwk") {
        let kind =
            if format_lower == "jwk" && object_field_string(key_bits.to_bits(), b"d").is_some() {
                KeyKind::Private
            } else {
                KeyKind::Public
            };
        (KeyAlgo::X25519, HashAlgo::Sha256, kind)
    } else if (algo_upper == "RSA-OAEP"
        || algo_upper == "RSASSA-PKCS1-V1_5"
        || algo_upper == "RSA-PSS")
        && (format_lower == "spki" || format_lower == "pkcs8" || format_lower == "jwk")
    {
        let hash = extract_algorithm_hash(algo_bits.to_bits(), HashAlgo::Sha1);
        let kind = if format_lower == "spki" {
            KeyKind::Public
        } else if format_lower == "pkcs8" {
            KeyKind::Private
        } else if object_field_string(key_bits.to_bits(), b"d").is_some() {
            KeyKind::Private
        } else {
            KeyKind::Public
        };
        let key_algo = match algo_upper.as_str() {
            "RSA-OAEP" => KeyAlgo::RsaOaep,
            "RSASSA-PKCS1-V1_5" => KeyAlgo::RsassaPkcs1,
            "RSA-PSS" => KeyAlgo::RsaPss,
            _ => unreachable!(),
        };
        (key_algo, hash, kind)
    } else {
        return reject_with_dom_exception(
            "NotSupportedError",
            "Unsupported algorithm for the given key format",
        );
    };

    let empty_allowed = kind == KeyKind::Public;
    let empty_message = if kind == KeyKind::Secret {
        "Usages cannot be empty when importing a secret key."
    } else {
        "Usages cannot be empty when importing a private key."
    };
    let bad_message = match key_algo {
        KeyAlgo::Hmac => "Unsupported key usage for HMAC key",
        KeyAlgo::Argon2d => "Unsupported key usage for a Argon2d key",
        KeyAlgo::Argon2i => "Unsupported key usage for a Argon2i key",
        KeyAlgo::Argon2id => "Unsupported key usage for a Argon2id key",
        KeyAlgo::Kmac128 => "Unsupported key usage for KMAC128 key",
        KeyAlgo::Kmac256 => "Unsupported key usage for KMAC256 key",
        _ => "Unsupported key usage for the requested algorithm",
    };
    let usages = match validate_key_usages(
        key_algo,
        kind,
        usages_bits.to_bits(),
        empty_allowed,
        empty_message,
        bad_message,
    ) {
        Ok(u) => u,
        Err((name, message)) => return reject_with_dom_exception(name, message),
    };

    let key_bytes = if format_lower == "jwk" {
        jwk_import_key_bytes(key_bits.to_bits(), key_algo, kind).unwrap_or_else(|| Vec::new())
    } else {
        bytes_from_jsvalue(key_bits.to_bits())
    };
    if key_algo == KeyAlgo::ChaCha20Poly1305 && key_bytes.len() != 32 {
        return reject_with_dom_exception("DataError", "Invalid key length");
    }
    if key_bytes.is_empty() && matches!(key_algo, KeyAlgo::Kmac128 | KeyAlgo::Kmac256) {
        return reject_with_dom_exception("DataError", "Zero-length key is not supported");
    }
    if key_bytes.is_empty()
        && !matches!(
            key_algo,
            KeyAlgo::Hkdf
                | KeyAlgo::Pbkdf2
                | KeyAlgo::Argon2d
                | KeyAlgo::Argon2i
                | KeyAlgo::Argon2id
        )
    {
        return reject_with_dom_exception("DataError", "Key data is empty or could not be read");
    }
    if is_ec_key_algo(key_algo) {
        let ok = match (ec_curve_for_key_algo(key_algo), kind) {
            (Some(EcNamedCurve::P256), KeyKind::Public) => {
                P256PublicKey::from_sec1_bytes(&key_bytes).is_ok()
            }
            (Some(EcNamedCurve::P256), _) => P256SecretKey::from_slice(&key_bytes).is_ok(),
            (Some(EcNamedCurve::P384), KeyKind::Public) => {
                P384PublicKey::from_sec1_bytes(&key_bytes).is_ok()
            }
            (Some(EcNamedCurve::P384), _) => P384SecretKey::from_slice(&key_bytes).is_ok(),
            (Some(EcNamedCurve::P521), KeyKind::Public) => {
                P521PublicKey::from_sec1_bytes(&key_bytes).is_ok()
            }
            (Some(EcNamedCurve::P521), _) => P521SecretKey::from_slice(&key_bytes).is_ok(),
            _ => false,
        };
        if !ok {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
    }
    if matches!(key_algo, KeyAlgo::Ed25519 | KeyAlgo::X25519) {
        if key_bytes.len() != 32 {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        if key_algo == KeyAlgo::Ed25519 {
            let ok = if kind == KeyKind::Private {
                let secret: Option<[u8; 32]> = key_bytes.as_slice().try_into().ok();
                secret
                    .map(|s| ed25519_dalek::SigningKey::from_bytes(&s))
                    .is_some()
            } else {
                let public: Option<[u8; 32]> = key_bytes.as_slice().try_into().ok();
                public
                    .and_then(|p| ed25519_dalek::VerifyingKey::from_bytes(&p).ok())
                    .is_some()
            };
            if !ok {
                return reject_with_dom_exception("OperationError", "The operation failed");
            }
        }
    }
    if matches!(
        key_algo,
        KeyAlgo::RsaOaep | KeyAlgo::RsassaPkcs1 | KeyAlgo::RsaPss
    ) {
        let ok = if kind == KeyKind::Public {
            RsaPublicKey::from_public_key_der(&key_bytes).is_ok()
        } else {
            RsaPrivateKey::from_pkcs8_der(&key_bytes).is_ok()
        };
        if !ok {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
    }
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial::new(key_algo, hash, kind, extractable, usages),
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}

/// `crypto.subtle.exportKey("raw" | "spki" | "pkcs8", key)` → Promise<Uint8Array>.
///
/// The exported representation is the key byte buffer Perry uses
/// internally: raw secret bytes / SEC1 public points, SPKI public DER,
/// or PKCS#8 private DER.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_export_key(format_bits: f64, key_bits: f64) -> *mut Promise {
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to execute 'exportKey' on 'SubtleCrypto': format must be a string",
            )
        }
    };
    let format_lower = format.to_ascii_lowercase();
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    if !mat.extractable {
        return reject_with_dom_exception("InvalidAccessError", "key is not extractable");
    }
    if format_lower == "raw" && mat.algo == KeyAlgo::ChaCha20Poly1305 {
        return reject_with_dom_exception(
            "NotSupportedError",
            "Unable to export ChaCha20-Poly1305 secret key using raw format",
        );
    }
    if format_lower == "raw" && mat.kind == KeyKind::Private {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    if format_lower == "raw" && matches!(mat.algo, KeyAlgo::Kmac128 | KeyAlgo::Kmac256) {
        let name = if mat.algo == KeyAlgo::Kmac128 {
            "KMAC128"
        } else {
            "KMAC256"
        };
        let message = format!("Unable to export {name} secret key using raw format");
        return reject_with_dom_exception("NotSupportedError", &message);
    }
    if format_lower == "jwk"
        && mat.kind != KeyKind::Secret
        && !matches!(
            mat.algo,
            KeyAlgo::RsaOaep
                | KeyAlgo::RsassaPkcs1
                | KeyAlgo::RsaPss
                | KeyAlgo::EcdsaP256
                | KeyAlgo::EcdhP256
                | KeyAlgo::EcdsaP384
                | KeyAlgo::EcdhP384
                | KeyAlgo::EcdsaP521
                | KeyAlgo::EcdhP521
                | KeyAlgo::Ed25519
                | KeyAlgo::X25519
        )
    {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    if format_lower == "spki" && mat.kind != KeyKind::Public {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    if format_lower == "pkcs8" && mat.kind != KeyKind::Private {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    if format_lower != "raw"
        && format_lower != "spki"
        && format_lower != "pkcs8"
        && format_lower != "jwk"
    {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    if format_lower == "jwk" {
        if mat.kind == KeyKind::Secret {
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&key_bytes);
            let field_count = if matches!(
                mat.algo,
                KeyAlgo::ChaCha20Poly1305 | KeyAlgo::Kmac128 | KeyAlgo::Kmac256
            ) {
                3
            } else {
                2
            };
            let obj = js_object_alloc(0, field_count);
            if obj.is_null() {
                return reject_with_dom_exception("OperationError", "The operation failed");
            }
            set_object_string_field(obj, b"kty", "oct");
            if mat.algo == KeyAlgo::ChaCha20Poly1305 {
                set_object_string_field(obj, b"alg", "C20P");
            }
            set_object_string_field(obj, b"k", &encoded);
            if mat.algo == KeyAlgo::Kmac128 {
                set_object_string_field(obj, b"alg", "K128");
            } else if mat.algo == KeyAlgo::Kmac256 {
                set_object_string_field(obj, b"alg", "K256");
            }
            return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
        }
        if matches!(
            mat.algo,
            KeyAlgo::RsaOaep | KeyAlgo::RsassaPkcs1 | KeyAlgo::RsaPss
        ) {
            let obj = match rsa_jwk_export_object(&key_bytes, mat) {
                Some(o) => o,
                None => return reject_with_dom_exception("OperationError", "The operation failed"),
            };
            return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
        }
        if is_ec_key_algo(mat.algo) {
            let obj = match ec_jwk_export_object(&key_bytes, mat) {
                Some(o) => o,
                None => return reject_with_dom_exception("OperationError", "The operation failed"),
            };
            return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
        }
        if matches!(mat.algo, KeyAlgo::Ed25519 | KeyAlgo::X25519) {
            let obj = match okp_jwk_export_object(&key_bytes, mat) {
                Some(o) => o,
                None => return reject_with_dom_exception("OperationError", "The operation failed"),
            };
            return resolve_with_bits(JSValue::pointer(obj as *const u8).bits());
        }
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    resolve_with_bytes(&key_bytes)
}

pub(super) fn b64u_uint(n: &RsaBigUint) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(n.to_bytes_be())
}

pub(super) fn b64u_decode_uint(s: &str) -> Option<RsaBigUint> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()?;
    Some(RsaBigUint::from_bytes_be(&bytes))
}

pub(super) unsafe fn jwk_uint_field(obj_bits: u64, name: &[u8]) -> Option<RsaBigUint> {
    let value = object_field_string(obj_bits, name)?;
    b64u_decode_uint(&value)
}

pub(super) fn rsa_jwk_alg(algo: KeyAlgo, hash: HashAlgo) -> &'static str {
    match (algo, hash) {
        (KeyAlgo::RsaOaep, HashAlgo::Sha1) => "RSA-OAEP",
        (KeyAlgo::RsaOaep, HashAlgo::Sha256) => "RSA-OAEP-256",
        (KeyAlgo::RsaOaep, HashAlgo::Sha384) => "RSA-OAEP-384",
        (KeyAlgo::RsaOaep, HashAlgo::Sha512) => "RSA-OAEP-512",
        (KeyAlgo::RsassaPkcs1, HashAlgo::Sha1) => "RS1",
        (KeyAlgo::RsassaPkcs1, HashAlgo::Sha256) => "RS256",
        (KeyAlgo::RsassaPkcs1, HashAlgo::Sha384) => "RS384",
        (KeyAlgo::RsassaPkcs1, HashAlgo::Sha512) => "RS512",
        (KeyAlgo::RsaPss, HashAlgo::Sha1) => "PS1",
        (KeyAlgo::RsaPss, HashAlgo::Sha256) => "PS256",
        (KeyAlgo::RsaPss, HashAlgo::Sha384) => "PS384",
        (KeyAlgo::RsaPss, HashAlgo::Sha512) => "PS512",
        _ => "",
    }
}

pub(super) unsafe fn jwk_ec_bytes(
    obj_bits: u64,
    key_algo: KeyAlgo,
    kind: KeyKind,
) -> Option<Vec<u8>> {
    let curve = ec_curve_for_key_algo(key_algo)?;
    let kty = object_field_string(obj_bits, b"kty")?;
    let crv = object_field_string(obj_bits, b"crv")?;
    if kty != "EC" || crv != ec_curve_name(curve) {
        return None;
    }
    let coord_len = ec_curve_private_len(curve);
    if kind == KeyKind::Private {
        let d = object_field_string(obj_bits, b"d")?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(d.as_bytes())
            .ok()?;
        return if bytes.len() == coord_len {
            Some(bytes)
        } else {
            None
        };
    }
    let x = object_field_string(obj_bits, b"x")?;
    let y = object_field_string(obj_bits, b"y")?;
    let x_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(x.as_bytes())
        .ok()?;
    let y_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(y.as_bytes())
        .ok()?;
    if x_bytes.len() != coord_len || y_bytes.len() != coord_len {
        return None;
    }
    let mut sec1 = Vec::with_capacity(ec_curve_public_len(curve));
    sec1.push(0x04);
    sec1.extend_from_slice(&x_bytes);
    sec1.extend_from_slice(&y_bytes);
    Some(sec1)
}

pub(super) unsafe fn jwk_okp_bytes(
    obj_bits: u64,
    key_algo: KeyAlgo,
    kind: KeyKind,
) -> Option<Vec<u8>> {
    let kty = object_field_string(obj_bits, b"kty")?;
    let crv = object_field_string(obj_bits, b"crv")?;
    let expected_crv = match key_algo {
        KeyAlgo::Ed25519 => "Ed25519",
        KeyAlgo::X25519 => "X25519",
        _ => return None,
    };
    if kty != "OKP" || crv != expected_crv {
        return None;
    }
    let field = if kind == KeyKind::Private { b"d" } else { b"x" };
    let value = object_field_string(obj_bits, field)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .ok()?;
    if bytes.len() == 32 {
        Some(bytes)
    } else {
        None
    }
}

pub(super) unsafe fn jwk_import_key_bytes(
    obj_bits: u64,
    key_algo: KeyAlgo,
    kind: KeyKind,
) -> Option<Vec<u8>> {
    let kty = object_field_string(obj_bits, b"kty")?;
    if is_ec_key_algo(key_algo) {
        return jwk_ec_bytes(obj_bits, key_algo, kind);
    }
    if matches!(key_algo, KeyAlgo::Ed25519 | KeyAlgo::X25519) {
        return jwk_okp_bytes(obj_bits, key_algo, kind);
    }
    if matches!(
        key_algo,
        KeyAlgo::Hmac
            | KeyAlgo::Kmac128
            | KeyAlgo::Kmac256
            | KeyAlgo::AesGcm
            | KeyAlgo::AesKw
            | KeyAlgo::AesCbc
            | KeyAlgo::AesCtr
            | KeyAlgo::ChaCha20Poly1305
    ) {
        if kty != "oct" {
            return None;
        }
        if let Some(alg) = object_field_string(obj_bits, b"alg") {
            let expected_alg = match key_algo {
                KeyAlgo::Kmac128 => Some("K128"),
                KeyAlgo::Kmac256 => Some("K256"),
                _ => None,
            };
            if let Some(expected) = expected_alg {
                if alg != expected {
                    return None;
                }
            }
        }
        let k = object_field_string(obj_bits, b"k")?;
        return base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(k.as_bytes())
            .ok();
    }
    if !matches!(
        key_algo,
        KeyAlgo::RsaOaep | KeyAlgo::RsassaPkcs1 | KeyAlgo::RsaPss
    ) || kty != "RSA"
    {
        return None;
    }
    let n = jwk_uint_field(obj_bits, b"n")?;
    let e = jwk_uint_field(obj_bits, b"e")?;
    if kind == KeyKind::Private {
        let d = jwk_uint_field(obj_bits, b"d")?;
        let p = jwk_uint_field(obj_bits, b"p")?;
        let q = jwk_uint_field(obj_bits, b"q")?;
        let private_key = RsaPrivateKey::from_components(n, e, d, vec![p, q]).ok()?;
        let der = private_key.to_pkcs8_der().ok()?;
        Some(der.as_bytes().to_vec())
    } else {
        let public_key = RsaPublicKey::new(n, e).ok()?;
        let der = public_key.to_public_key_der().ok()?;
        Some(der.as_bytes().to_vec())
    }
}

pub(super) unsafe fn rsa_jwk_export_object(
    key_bytes: &[u8],
    mat: CryptoKeyMaterial,
) -> Option<*mut perry_runtime::ObjectHeader> {
    if mat.kind == KeyKind::Public {
        let public_key = RsaPublicKey::from_public_key_der(key_bytes).ok()?;
        let obj = js_object_alloc(0, 4);
        if obj.is_null() {
            return None;
        }
        set_object_string_field(obj, b"kty", "RSA");
        set_object_string_field(obj, b"alg", rsa_jwk_alg(mat.algo, mat.hash));
        set_object_string_field(obj, b"n", &b64u_uint(public_key.n()));
        set_object_string_field(obj, b"e", &b64u_uint(public_key.e()));
        return Some(obj);
    }

    let private_key = RsaPrivateKey::from_pkcs8_der(key_bytes).ok()?;
    let primes = private_key.primes();
    if primes.len() < 2 {
        return None;
    }
    let p = &primes[0];
    let q = &primes[1];
    let one = RsaBigUint::from(1u8);
    let dp = private_key
        .dp()
        .cloned()
        .unwrap_or_else(|| private_key.d() % (p - &one));
    let dq = private_key
        .dq()
        .cloned()
        .unwrap_or_else(|| private_key.d() % (q - &one));
    let qi = private_key.qinv()?.to_biguint()?;
    let obj = js_object_alloc(0, 10);
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "RSA");
    set_object_string_field(obj, b"alg", rsa_jwk_alg(mat.algo, mat.hash));
    set_object_string_field(obj, b"n", &b64u_uint(private_key.n()));
    set_object_string_field(obj, b"e", &b64u_uint(private_key.e()));
    set_object_string_field(obj, b"d", &b64u_uint(private_key.d()));
    set_object_string_field(obj, b"p", &b64u_uint(p));
    set_object_string_field(obj, b"q", &b64u_uint(q));
    set_object_string_field(obj, b"dp", &b64u_uint(&dp));
    set_object_string_field(obj, b"dq", &b64u_uint(&dq));
    set_object_string_field(obj, b"qi", &b64u_uint(&qi));
    Some(obj)
}

pub(super) unsafe fn okp_jwk_export_object(
    key_bytes: &[u8],
    mat: CryptoKeyMaterial,
) -> Option<*mut perry_runtime::ObjectHeader> {
    if key_bytes.len() != 32 {
        return None;
    }
    let crv = match mat.algo {
        KeyAlgo::Ed25519 => "Ed25519",
        KeyAlgo::X25519 => "X25519",
        _ => return None,
    };
    let public_bytes = if mat.kind == KeyKind::Private {
        match mat.algo {
            KeyAlgo::Ed25519 => {
                let secret: [u8; 32] = key_bytes.try_into().ok()?;
                ed25519_dalek::SigningKey::from_bytes(&secret)
                    .verifying_key()
                    .to_bytes()
                    .to_vec()
            }
            KeyAlgo::X25519 => {
                let secret: [u8; 32] = key_bytes.try_into().ok()?;
                let secret = x25519_dalek::StaticSecret::from(secret);
                x25519_dalek::PublicKey::from(&secret).to_bytes().to_vec()
            }
            _ => return None,
        }
    } else {
        key_bytes.to_vec()
    };
    let obj = js_object_alloc(0, if mat.kind == KeyKind::Private { 4 } else { 3 });
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "OKP");
    set_object_string_field(obj, b"crv", crv);
    set_object_string_field(
        obj,
        b"x",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_bytes),
    );
    if mat.kind == KeyKind::Private {
        set_object_string_field(
            obj,
            b"d",
            &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_bytes),
        );
    }
    Some(obj)
}

pub(super) unsafe fn ec_jwk_export_object(
    key_bytes: &[u8],
    mat: CryptoKeyMaterial,
) -> Option<*mut perry_runtime::ObjectHeader> {
    let curve = ec_curve_for_key_algo(mat.algo)?;
    let (public_bytes, private_d) = match (curve, mat.kind) {
        (EcNamedCurve::P256, KeyKind::Private) => {
            let secret = P256SecretKey::from_slice(key_bytes).ok()?;
            let public = secret
                .public_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec();
            (public, Some(key_bytes.to_vec()))
        }
        (EcNamedCurve::P256, _) => {
            let public = P256PublicKey::from_sec1_bytes(key_bytes).ok()?;
            (public.to_encoded_point(false).as_bytes().to_vec(), None)
        }
        (EcNamedCurve::P384, KeyKind::Private) => {
            let secret = P384SecretKey::from_slice(key_bytes).ok()?;
            let public = secret
                .public_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec();
            (public, Some(key_bytes.to_vec()))
        }
        (EcNamedCurve::P384, _) => {
            let public = P384PublicKey::from_sec1_bytes(key_bytes).ok()?;
            (public.to_encoded_point(false).as_bytes().to_vec(), None)
        }
        (EcNamedCurve::P521, KeyKind::Private) => {
            let secret = P521SecretKey::from_slice(key_bytes).ok()?;
            let public = secret
                .public_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec();
            (public, Some(key_bytes.to_vec()))
        }
        (EcNamedCurve::P521, _) => {
            let public = P521PublicKey::from_sec1_bytes(key_bytes).ok()?;
            (public.to_encoded_point(false).as_bytes().to_vec(), None)
        }
    };
    let coord_len = ec_curve_private_len(curve);
    if public_bytes.len() != ec_curve_public_len(curve) || public_bytes[0] != 0x04 {
        return None;
    }
    let obj = js_object_alloc(0, if private_d.is_some() { 5 } else { 4 });
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "EC");
    set_object_string_field(obj, b"crv", ec_curve_name(curve));
    set_object_string_field(
        obj,
        b"x",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_bytes[1..1 + coord_len]),
    );
    set_object_string_field(
        obj,
        b"y",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(&public_bytes[1 + coord_len..1 + 2 * coord_len]),
    );
    if let Some(d) = private_d {
        set_object_string_field(
            obj,
            b"d",
            &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&d),
        );
    }
    Some(obj)
}

/// Extract the algorithm name from a `string | { name }` argument.
/// Used by importKey / encrypt / decrypt where jose passes the shorthand
/// `"AES-GCM"` to importKey but a full `{ name: "AES-GCM", iv: ... }`
/// at encrypt time.
pub(super) unsafe fn extract_algo_name(bits: u64) -> Option<String> {
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
