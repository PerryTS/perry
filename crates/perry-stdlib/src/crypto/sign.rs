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

const KO_USAGE_ENCRYPT: u32 = 1 << 0;
const KO_USAGE_DECRYPT: u32 = 1 << 1;
const KO_USAGE_SIGN: u32 = 1 << 2;
const KO_USAGE_VERIFY: u32 = 1 << 3;
const KO_USAGE_DERIVE_KEY: u32 = 1 << 4;
const KO_USAGE_DERIVE_BITS: u32 = 1 << 5;
const KO_USAGE_WRAP_KEY: u32 = 1 << 6;
const KO_USAGE_UNWRAP_KEY: u32 = 1 << 7;

unsafe fn keyobject_string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

unsafe fn throw_keyobject_dom_exception(name: &str, message: &str) -> ! {
    let err = perry_runtime::event_target::js_dom_exception_new(
        keyobject_string_value(message),
        keyobject_string_value(name),
    );
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

unsafe fn throw_keyobject_usage_type_error() -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(
        "Value cannot be converted to sequence.",
        "ERR_INVALID_ARG_TYPE",
    )
}

unsafe fn keyobject_algorithm_name(bits: u64) -> Option<String> {
    string_from_jsvalue(bits).or_else(|| object_field_string(bits, b"name"))
}

fn keyobject_hash_id(name: &str) -> Option<u8> {
    let normalized = name
        .chars()
        .filter(|ch| *ch != '-')
        .flat_map(char::to_uppercase)
        .collect::<String>();
    match normalized.as_str() {
        "SHA1" => Some(1),
        "SHA256" => Some(2),
        "SHA384" => Some(3),
        "SHA512" => Some(4),
        _ => None,
    }
}

unsafe fn keyobject_algorithm_hash(bits: u64) -> Option<u8> {
    let hash_bits = object_field_bits(bits, b"hash")?;
    if let Some(hash) = string_from_jsvalue(hash_bits) {
        return keyobject_hash_id(&hash);
    }
    object_field_string(hash_bits, b"name").and_then(|hash| keyobject_hash_id(&hash))
}

unsafe fn keyobject_named_curve_is_p256(bits: u64) -> bool {
    let Some(curve) = object_field_string(bits, b"namedCurve") else {
        return false;
    };
    let upper = curve.to_ascii_uppercase();
    matches!(upper.as_str(), "P-256" | "PRIME256V1" | "SECP256R1")
}

fn keyobject_usage_bit(name: &str) -> Option<u32> {
    match name {
        "encrypt" => Some(KO_USAGE_ENCRYPT),
        "decrypt" => Some(KO_USAGE_DECRYPT),
        "sign" => Some(KO_USAGE_SIGN),
        "verify" => Some(KO_USAGE_VERIFY),
        "deriveKey" => Some(KO_USAGE_DERIVE_KEY),
        "deriveBits" => Some(KO_USAGE_DERIVE_BITS),
        "wrapKey" => Some(KO_USAGE_WRAP_KEY),
        "unwrapKey" => Some(KO_USAGE_UNWRAP_KEY),
        _ => None,
    }
}

unsafe fn keyobject_usage_bits(bits: u64) -> Option<u32> {
    if perry_runtime::array::js_array_is_array(f64::from_bits(bits)).to_bits()
        != JSValue::bool(true).bits()
    {
        return None;
    }
    let arr_addr = bits & 0x0000_FFFF_FFFF_FFFF;
    if arr_addr < 0x1000 {
        return None;
    }
    let arr = arr_addr as *const perry_runtime::ArrayHeader;
    let len = perry_runtime::array::js_array_length(arr);
    let mut usages = 0u32;
    for i in 0..len {
        let item = perry_runtime::array::js_array_get(arr, i);
        let name = string_from_jsvalue(item.bits())?;
        usages |= keyobject_usage_bit(&name)?;
    }
    Some(usages)
}

fn keyobject_supported_usages(algo_id: u8, kind_id: u8) -> u32 {
    match (algo_id, kind_id) {
        (8 | 12 | 14, 2) => KO_USAGE_SIGN,
        (8 | 12 | 14, 3) => KO_USAGE_VERIFY,
        (9, 2) => KO_USAGE_DERIVE_KEY | KO_USAGE_DERIVE_BITS,
        (9, 3) => 0,
        (13, 2) => KO_USAGE_DECRYPT | KO_USAGE_UNWRAP_KEY,
        (13, 3) => KO_USAGE_ENCRYPT | KO_USAGE_WRAP_KEY,
        _ => 0,
    }
}

unsafe fn keyobject_validate_usages(algo_name: &str, algo_id: u8, kind_id: u8, bits: u64) -> u32 {
    let usages = match keyobject_usage_bits(bits) {
        Some(usages) => usages,
        None => throw_keyobject_usage_type_error(),
    };
    let supported = keyobject_supported_usages(algo_id, kind_id);
    if usages & !supported != 0 {
        let article = if algo_name.starts_with("RSA") || algo_name.starts_with("RSASSA") {
            "an"
        } else {
            "a"
        };
        let message = format!("Unsupported key usage for {article} {algo_name} key");
        throw_keyobject_dom_exception("SyntaxError", &message);
    }
    if usages == 0 && kind_id == 2 {
        throw_keyobject_dom_exception(
            "SyntaxError",
            "Usages cannot be empty when importing a private key.",
        );
    }
    usages
}

fn keyobject_rsa_der(pem: &str, kind_id: u8) -> Option<Vec<u8>> {
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};

    if kind_id == 2 {
        return parse_rsa_private_key_pem(pem)
            .and_then(|key| key.to_pkcs8_der().ok().map(|der| der.as_bytes().to_vec()));
    }
    parse_rsa_public_key_pem(pem).and_then(|key| {
        key.to_public_key_der()
            .ok()
            .map(|der| der.as_bytes().to_vec())
    })
}

fn keyobject_p256_bytes(pem: &str, kind_id: u8) -> Option<Vec<u8>> {
    if kind_id == 2 {
        return parse_p256_signing_key_pem(pem).map(|key| key.to_bytes().as_slice().to_vec());
    }
    parse_p256_verifying_key_pem(pem).map(|key| key.to_encoded_point(false).as_bytes().to_vec())
}

/// Private runtime hook for asymmetric `KeyObject#toCryptoKey()`.
///
/// The runtime owns method dispatch for string-backed KeyObject surrogates,
/// while stdlib owns key parsing and DER/SEC1 encoding. The receiver arrives
/// as `key_bits`; the remaining args match Node's
/// `toCryptoKey(algorithm, extractable, keyUsages)`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_keyobject_to_crypto_key(
    key_bits: f64,
    algorithm_bits: f64,
    extractable_bits: f64,
    usages_bits: f64,
) -> f64 {
    let key_ptr = perry_runtime::js_get_string_pointer_unified(key_bits) as *const StringHeader;
    if key_ptr.is_null() || (key_ptr as usize) < 0x1000 {
        throw_keyobject_dom_exception("InvalidAccessError", "Key is not a valid KeyObject");
    }
    let Some((asym_kind_id, asym_type)) =
        perry_runtime::buffer::asymmetric_key_meta(key_ptr as usize)
    else {
        throw_keyobject_dom_exception("InvalidAccessError", "Key is not a valid KeyObject");
    };
    let crypto_kind_id = if asym_kind_id == 2 { 2 } else { 3 };

    let Some(algo_name) = keyobject_algorithm_name(algorithm_bits.to_bits()) else {
        throw_keyobject_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    };
    let upper = algo_name.to_ascii_uppercase();
    let (algo_id, hash_id) = match upper.as_str() {
        "RSASSA-PKCS1-V1_5" => (12, keyobject_algorithm_hash(algorithm_bits.to_bits())),
        "RSA-OAEP" => (13, keyobject_algorithm_hash(algorithm_bits.to_bits())),
        "RSA-PSS" => (14, keyobject_algorithm_hash(algorithm_bits.to_bits())),
        "ECDSA" if keyobject_named_curve_is_p256(algorithm_bits.to_bits()) => (8, Some(2)),
        "ECDH" if keyobject_named_curve_is_p256(algorithm_bits.to_bits()) => (9, Some(2)),
        "ECDSA" | "ECDH" => {
            throw_keyobject_dom_exception("DataError", "Named curve mismatch");
        }
        _ => throw_keyobject_dom_exception("NotSupportedError", "Unrecognized algorithm name"),
    };
    let Some(hash_id) = hash_id else {
        perry_runtime::fs::validate::throw_type_error_with_code(
            "Failed to normalize algorithm: missing hash",
            "ERR_MISSING_OPTION",
        );
    };

    if (matches!(algo_id, 12 | 13 | 14) && asym_type != 1)
        || (matches!(algo_id, 8 | 9) && asym_type != 2)
    {
        throw_keyobject_dom_exception("DataError", "Key algorithm mismatch");
    }

    let usages = keyobject_validate_usages(
        crypto_key_algorithm_name_for_id(algo_id),
        algo_id,
        crypto_kind_id,
        usages_bits.to_bits(),
    );
    let pem = match String::from_utf8(bytes_from_ptr(key_ptr as i64)) {
        Ok(pem) => pem,
        Err(_) => throw_keyobject_dom_exception("DataError", "Key data is invalid"),
    };
    let key_bytes = if asym_type == 1 {
        keyobject_rsa_der(&pem, crypto_kind_id)
    } else {
        keyobject_p256_bytes(&pem, crypto_kind_id)
    };
    let Some(key_bytes) = key_bytes else {
        throw_keyobject_dom_exception("OperationError", "The operation failed");
    };

    let buf = alloc_buffer_from_slice(&key_bytes);
    if buf.is_null() {
        throw_keyobject_dom_exception("OperationError", "The operation failed");
    }
    perry_runtime::buffer::js_buffer_mark_as_crypto_key_external(
        buf as usize,
        algo_id,
        hash_id,
        crypto_kind_id,
        u8::from(js_truthy(extractable_bits)),
        usages,
    );
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

fn crypto_key_algorithm_name_for_id(algo_id: u8) -> &'static str {
    match algo_id {
        8 => "ECDSA",
        9 => "ECDH",
        12 => "RSASSA-PKCS1-v1_5",
        13 => "RSA-OAEP",
        14 => "RSA-PSS",
        _ => "",
    }
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

unsafe fn nanbox_buffer_from_slice(bytes: &[u8]) -> f64 {
    let buf = alloc_buffer_from_slice(bytes);
    if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    }
}

/// `crypto.encapsulate(publicKey)` for Perry's X25519 KeyObject surrogate.
///
/// Node returns `{ sharedKey, ciphertext }` where the ciphertext is the
/// ephemeral public key. Decapsulation with the private key recomputes the
/// same X25519 shared secret.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_encapsulate(key_val: f64) -> *mut ObjectHeader {
    let public_value = match crypto_key_input_to_public_pem(key_val.to_bits()) {
        Some(value) => value,
        None => return js_object_alloc(0, 0),
    };
    let public = match parse_x25519_public_surrogate(&public_value) {
        Some(public) => public,
        None => return js_object_alloc(0, 0),
    };
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let private = x25519_dalek::StaticSecret::from(seed);
    let ephemeral_public = x25519_dalek::PublicKey::from(&private);
    let peer_public = x25519_dalek::PublicKey::from(public);
    let shared = private.diffie_hellman(&peer_public);

    let obj = js_object_alloc(0, 2);
    set_object_value_field(
        obj,
        b"sharedKey",
        nanbox_buffer_from_slice(shared.as_bytes()),
    );
    set_object_value_field(
        obj,
        b"ciphertext",
        nanbox_buffer_from_slice(ephemeral_public.as_bytes()),
    );
    obj
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_encapsulate_async(key_val: f64, callback_bits: f64) -> f64 {
    let result = js_crypto_encapsulate(key_val);
    let value = if result.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(result as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// `crypto.decapsulate(privateKey, ciphertext)` for X25519 ciphertexts.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_decapsulate(
    key_val: f64,
    ciphertext_val: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let private_value = match crypto_key_input_to_private_pem(key_val.to_bits()) {
        Some(value) => value,
        None => return alloc_buffer_from_slice(&[]),
    };
    let private = match parse_x25519_private_surrogate(&private_value) {
        Some(private) => private,
        None => return alloc_buffer_from_slice(&[]),
    };
    let ciphertext = crypto_value_bytes(ciphertext_val);
    let ciphertext: [u8; 32] = match ciphertext.as_slice().try_into() {
        Ok(bytes) => bytes,
        Err(_) => return alloc_buffer_from_slice(&[]),
    };
    let private = x25519_dalek::StaticSecret::from(private);
    let public = x25519_dalek::PublicKey::from(ciphertext);
    let secret = private.diffie_hellman(&public);
    alloc_buffer_from_slice(secret.as_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_decapsulate_async(
    key_val: f64,
    ciphertext_val: f64,
    callback_bits: f64,
) -> f64 {
    let buf = js_crypto_decapsulate(key_val, ciphertext_val);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}
