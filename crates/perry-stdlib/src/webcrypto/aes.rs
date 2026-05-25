/// `crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData? }, key, data)`
/// → Promise<Uint8Array>
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_encrypt(
    algo_bits: f64,
    key_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    if algo_name.eq_ignore_ascii_case("RSA-OAEP") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => return resolve_undefined(),
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Public {
            return resolve_undefined();
        }
        let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
        let public_key = match RsaPublicKey::from_public_key_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return resolve_undefined(),
        };
        let data = bytes_from_jsvalue(data_bits.to_bits());
        let ciphertext = match rsa_oaep_encrypt(mat.hash, &public_key, &data) {
            Some(c) => c,
            None => return resolve_undefined(),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CBC") {
        let (key, iv, data) = match extract_aes_cbc_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        let ciphertext = match aes_cbc_encrypt(&key, &iv, &data) {
            Some(c) => c,
            None => return resolve_undefined(),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CTR") {
        let (key, counter, length, data) = match extract_aes_ctr_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        let ciphertext = match aes_ctr_apply(&key, &counter, length, &data) {
            Some(c) => c,
            None => return resolve_undefined(),
        };
        return resolve_with_bytes(&ciphertext);
    }
    let (key, iv, aad, data) =
        match extract_aes_gcm_args(algo_bits.to_bits(), key_bits.to_bits(), data_bits.to_bits()) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
    let ciphertext = match aes_gcm_encrypt(&key, &iv, &aad, &data) {
        Some(c) => c,
        None => return resolve_undefined(),
    };
    resolve_with_bytes(&ciphertext)
}

/// `crypto.subtle.decrypt({ name: "AES-GCM", iv, additionalData? }, key, data)`
/// → Promise<Uint8Array>
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_decrypt(
    algo_bits: f64,
    key_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    if algo_name.eq_ignore_ascii_case("RSA-OAEP") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => return resolve_undefined(),
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Private {
            return resolve_undefined();
        }
        let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
        let private_key = match RsaPrivateKey::from_pkcs8_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return resolve_undefined(),
        };
        let data = bytes_from_jsvalue(data_bits.to_bits());
        let plaintext = match rsa_oaep_decrypt(mat.hash, &private_key, &data) {
            Some(p) => p,
            None => return resolve_undefined(),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CBC") {
        let (key, iv, data) = match extract_aes_cbc_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        let plaintext = match aes_cbc_decrypt(&key, &iv, &data) {
            Some(p) => p,
            None => return resolve_undefined(),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CTR") {
        let (key, counter, length, data) = match extract_aes_ctr_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        let plaintext = match aes_ctr_apply(&key, &counter, length, &data) {
            Some(p) => p,
            None => return resolve_undefined(),
        };
        return resolve_with_bytes(&plaintext);
    }
    let (key, iv, aad, data) =
        match extract_aes_gcm_args(algo_bits.to_bits(), key_bits.to_bits(), data_bits.to_bits()) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
    let plaintext = match aes_gcm_decrypt(&key, &iv, &aad, &data) {
        Some(p) => p,
        None => return resolve_undefined(),
    };
    resolve_with_bytes(&plaintext)
}

/// Read a numeric field from an algorithm object (`{ name, length }`).
/// Returns `None` if the field is absent or not a number. Required by
/// `generateKey({ name: 'AES-GCM', length: 256 }, ...)` — the spec
/// allows 128, 192, or 256 here but we only honor 128 and 256 (the
/// `aes-gcm` 0.10 crate doesn't ship a 192-bit type, matching the
/// existing encrypt/decrypt rejection at line ~547).
unsafe fn object_field_number(obj_bits: u64, name: &[u8]) -> Option<u32> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    let bits = val.bits();
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFE {
        // INT32_TAG — lower 32 bits as a signed int.
        let raw = (bits & 0xFFFF_FFFF) as i32;
        if raw >= 0 {
            return Some(raw as u32);
        }
        return None;
    }
    // Treat as f64. NaN-boxed primitives (undef, null) have non-finite
    // bits — reject them explicitly so callers fall back to the default.
    let f = f64::from_bits(bits);
    if f.is_finite() && f >= 0.0 && f <= u32::MAX as f64 {
        Some(f as u32)
    } else {
        None
    }
}
