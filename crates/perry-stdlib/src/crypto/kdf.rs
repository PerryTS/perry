use super::*;

/// PBKDF2-HMAC returning a Buffer. Counterpart of
/// `crypto.pbkdf2Sync(password, salt, iterations, keylen, digest)`.
/// Accepts string or Buffer for both password and salt.
///
/// `digest_ptr` is the NaN-unboxed pointer to the digest-algorithm string
/// (`'sha256'`, `'sha512'`, …). A null/empty/unknown digest defaults to
/// SHA-256 — the algorithm SCRAM and the previous callers relied on. The
/// digest was silently ignored before, so `pbkdf2Sync(..., 'sha512')`
/// produced a SHA-256 key (#1355).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_pbkdf2_bytes(
    password_ptr: i64,
    salt_ptr: i64,
    iterations: f64,
    keylen: f64,
    digest_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    use pbkdf2::pbkdf2_hmac;
    use sha2::{Sha224, Sha384};
    let password = bytes_from_ptr(password_ptr);
    let salt = bytes_from_ptr(salt_ptr);
    let iter = iterations as u32;
    let klen = keylen as usize;
    let mut out = vec![0u8; klen];
    // Resolve the digest algorithm. `digest_ptr` may be a null/sentinel
    // pointer (no arg passed) — fall back to SHA-256 in that case.
    let digest = if (digest_ptr as usize) < 0x1000 {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes_from_ptr(digest_ptr))
            .to_ascii_lowercase()
            .replace('-', "")
    };
    match digest.as_str() {
        "sha1" => pbkdf2_hmac::<Sha1>(&password, &salt, iter, &mut out),
        "sha224" => pbkdf2_hmac::<Sha224>(&password, &salt, iter, &mut out),
        "sha384" => pbkdf2_hmac::<Sha384>(&password, &salt, iter, &mut out),
        "sha512" => pbkdf2_hmac::<Sha512>(&password, &salt, iter, &mut out),
        // After the `replace('-', "")` normalization, "sha512-256" comes
        // through as "sha512256".
        "sha512256" => pbkdf2_hmac::<Sha512_256>(&password, &salt, iter, &mut out),
        // "sha256" and the empty/unknown default.
        _ => pbkdf2_hmac::<Sha256>(&password, &salt, iter, &mut out),
    }
    alloc_buffer_from_slice(&out)
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_pbkdf2_async_alg(
    password_ptr: i64,
    salt_ptr: i64,
    iterations: f64,
    keylen: f64,
    alg_ptr: i64,
    callback_bits: f64,
) -> f64 {
    // Routes to `js_crypto_pbkdf2_bytes` (the externally-visible 5-arg
    // helper that normalizes digest names via `replace('-', "")`).
    let buf = js_crypto_pbkdf2_bytes(password_ptr, salt_ptr, iterations, keylen, alg_ptr);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// Node-compatible `crypto.hkdfSync(digest, ikm, salt, info, keylen)`.
/// Returns bytes as a Buffer. Supports the digest family Perry already
/// exposes through hash/HMAC.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_hkdf_bytes_alg(
    alg_ptr: i64,
    ikm_ptr: i64,
    salt_ptr: i64,
    info_ptr: i64,
    keylen: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes)
        .unwrap_or("sha256")
        .to_ascii_lowercase();
    let ikm = bytes_from_ptr(ikm_ptr);
    let salt = bytes_from_ptr(salt_ptr);
    let info = bytes_from_ptr(info_ptr);
    let len = keylen as usize;
    if len == 0 || len > 8160 {
        return perry_runtime::buffer::buffer_alloc(0);
    }
    let mut out = vec![0u8; len];
    let ok = match alg.as_str() {
        "sha1" | "sha-1" => Hkdf::<Sha1>::new(Some(&salt), &ikm)
            .expand(&info, &mut out)
            .is_ok(),
        "sha224" | "sha-224" => Hkdf::<Sha224>::new(Some(&salt), &ikm)
            .expand(&info, &mut out)
            .is_ok(),
        "sha384" | "sha-384" => Hkdf::<Sha384>::new(Some(&salt), &ikm)
            .expand(&info, &mut out)
            .is_ok(),
        "sha512" | "sha-512" => Hkdf::<Sha512>::new(Some(&salt), &ikm)
            .expand(&info, &mut out)
            .is_ok(),
        _ => Hkdf::<Sha256>::new(Some(&salt), &ikm)
            .expand(&info, &mut out)
            .is_ok(),
    };
    if ok {
        alloc_buffer_from_slice(&out)
    } else {
        perry_runtime::buffer::buffer_alloc(0)
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_hkdf_async_alg(
    alg_ptr: i64,
    ikm_ptr: i64,
    salt_ptr: i64,
    info_ptr: i64,
    keylen: f64,
    callback_bits: f64,
) -> f64 {
    let buf = js_crypto_hkdf_bytes_alg(alg_ptr, ikm_ptr, salt_ptr, info_ptr, keylen);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_scrypt_async(
    password_ptr: i64,
    salt_ptr: i64,
    keylen: f64,
    callback_bits: f64,
) -> f64 {
    // Routes to the 4-arg scryptSync (defined below) with no options
    // object — same default cost parameters as Node.
    let buf = js_crypto_scrypt_bytes(password_ptr, salt_ptr, keylen, 0);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// Constant-time equality for equal-length byte inputs.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_timing_safe_equal(a_ptr: i64, b_ptr: i64) -> f64 {
    let a = bytes_from_ptr(a_ptr);
    let b = bytes_from_ptr(b_ptr);
    if a.len() != b.len() {
        return f64::from_bits(JSValue::bool(false).bits());
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    f64::from_bits(JSValue::bool(diff == 0).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_hashes() -> *mut perry_runtime::array::ArrayHeader {
    string_array(&[
        "RSA-MD5",
        "RSA-RIPEMD160",
        "RSA-SHA1",
        "RSA-SHA224",
        "RSA-SHA256",
        "RSA-SHA384",
        "RSA-SHA512",
        "md5",
        "ripemd160",
        "sha1",
        "sha224",
        "sha256",
        "sha384",
        "sha512",
        "sha512-256",
        "shake128",
        "shake256",
    ])
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_ciphers() -> *mut perry_runtime::array::ArrayHeader {
    string_array(&[
        "aes-128-cbc",
        "aes-128-ecb",
        "aes-128-gcm",
        "aes128-wrap",
        "id-aes128-wrap",
        "aes-192-cbc",
        "aes-192-ecb",
        "aes-192-gcm",
        "aes192-wrap",
        "id-aes192-wrap",
        "aes-256-cbc",
        "aes-256-ecb",
        "aes-256-gcm",
        "aes256-wrap",
        "id-aes256-wrap",
    ])
}

pub(super) struct CipherInfo {
    name: &'static str,
    nid: f64,
    block_size: f64,
    iv_len: Option<f64>,
    key_len: f64,
    mode: &'static str,
}

pub(super) fn cipher_info_for_name(name: &str) -> Option<CipherInfo> {
    match name.to_ascii_lowercase().as_str() {
        "aes-128-cbc" => Some(CipherInfo {
            name: "aes-128-cbc",
            nid: 419.0,
            block_size: 16.0,
            iv_len: Some(16.0),
            key_len: 16.0,
            mode: "cbc",
        }),
        "aes-192-cbc" => Some(CipherInfo {
            name: "aes-192-cbc",
            nid: 423.0,
            block_size: 16.0,
            iv_len: Some(16.0),
            key_len: 24.0,
            mode: "cbc",
        }),
        "aes-256-cbc" => Some(CipherInfo {
            name: "aes-256-cbc",
            nid: 427.0,
            block_size: 16.0,
            iv_len: Some(16.0),
            key_len: 32.0,
            mode: "cbc",
        }),
        "aes-128-ecb" => Some(CipherInfo {
            name: "aes-128-ecb",
            nid: 418.0,
            block_size: 16.0,
            iv_len: None,
            key_len: 16.0,
            mode: "ecb",
        }),
        "aes-192-ecb" => Some(CipherInfo {
            name: "aes-192-ecb",
            nid: 422.0,
            block_size: 16.0,
            iv_len: None,
            key_len: 24.0,
            mode: "ecb",
        }),
        "aes-256-ecb" => Some(CipherInfo {
            name: "aes-256-ecb",
            nid: 426.0,
            block_size: 16.0,
            iv_len: None,
            key_len: 32.0,
            mode: "ecb",
        }),
        "id-aes128-wrap" | "aes128-wrap" => Some(CipherInfo {
            name: "id-aes128-wrap",
            nid: 788.0,
            block_size: 8.0,
            iv_len: Some(8.0),
            key_len: 16.0,
            mode: "wrap",
        }),
        "id-aes192-wrap" | "aes192-wrap" => Some(CipherInfo {
            name: "id-aes192-wrap",
            nid: 789.0,
            block_size: 8.0,
            iv_len: Some(8.0),
            key_len: 24.0,
            mode: "wrap",
        }),
        "id-aes256-wrap" | "aes256-wrap" => Some(CipherInfo {
            name: "id-aes256-wrap",
            nid: 790.0,
            block_size: 8.0,
            iv_len: Some(8.0),
            key_len: 32.0,
            mode: "wrap",
        }),
        "aes-128-gcm" | "id-aes128-gcm" => Some(CipherInfo {
            name: "id-aes128-gcm",
            nid: 895.0,
            block_size: 1.0,
            iv_len: Some(12.0),
            key_len: 16.0,
            mode: "gcm",
        }),
        "aes-192-gcm" | "id-aes192-gcm" => Some(CipherInfo {
            name: "id-aes192-gcm",
            nid: 898.0,
            block_size: 1.0,
            iv_len: Some(12.0),
            key_len: 24.0,
            mode: "gcm",
        }),
        "aes-256-gcm" | "id-aes256-gcm" => Some(CipherInfo {
            name: "id-aes256-gcm",
            nid: 901.0,
            block_size: 1.0,
            iv_len: Some(12.0),
            key_len: 32.0,
            mode: "gcm",
        }),
        _ => None,
    }
}

pub(super) fn cipher_info_for_nid(nid: i32) -> Option<CipherInfo> {
    match nid {
        419 => cipher_info_for_name("aes-128-cbc"),
        418 => cipher_info_for_name("aes-128-ecb"),
        423 => cipher_info_for_name("aes-192-cbc"),
        422 => cipher_info_for_name("aes-192-ecb"),
        427 => cipher_info_for_name("aes-256-cbc"),
        426 => cipher_info_for_name("aes-256-ecb"),
        788 => cipher_info_for_name("id-aes128-wrap"),
        789 => cipher_info_for_name("id-aes192-wrap"),
        790 => cipher_info_for_name("id-aes256-wrap"),
        895 => cipher_info_for_name("aes-128-gcm"),
        898 => cipher_info_for_name("aes-192-gcm"),
        901 => cipher_info_for_name("aes-256-gcm"),
        _ => None,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_cipher_info(alg_bits: f64, options_bits: f64) -> f64 {
    let info = if let Some(name) = string_from_jsvalue(alg_bits.to_bits()) {
        cipher_info_for_name(&name)
    } else if alg_bits.is_finite() {
        cipher_info_for_nid(alg_bits as i32)
    } else {
        None
    };
    let Some(info) = info else {
        return f64::from_bits(JSValue::undefined().bits());
    };

    if let Some(bits) = object_field_bits(options_bits.to_bits(), b"keyLength") {
        if f64::from_bits(bits) as i32 != info.key_len as i32 {
            return f64::from_bits(JSValue::undefined().bits());
        }
    }
    let mut iv_len = info.iv_len;
    if let Some(bits) = object_field_bits(options_bits.to_bits(), b"ivLength") {
        let requested = f64::from_bits(bits) as i32;
        if info.mode == "gcm" && requested > 0 {
            iv_len = Some(requested as f64);
        } else if Some(requested as f64) != info.iv_len {
            return f64::from_bits(JSValue::undefined().bits());
        }
    }

    let obj = js_object_alloc(0, 6);
    set_object_string_field(obj, b"name", info.name);
    set_object_value_field(obj, b"nid", info.nid);
    set_object_value_field(obj, b"blockSize", info.block_size);
    if let Some(iv_len) = iv_len {
        set_object_value_field(obj, b"ivLength", iv_len);
    }
    set_object_value_field(obj, b"keyLength", info.key_len);
    set_object_string_field(obj, b"mode", info.mode);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_curves() -> *mut perry_runtime::array::ArrayHeader {
    string_array(&[
        "prime256v1",
        "secp256k1",
        "secp384r1",
        "secp521r1",
        "X25519",
        "X448",
    ])
}

/// `crypto.secureHeapUsed()` — Perry does not use OpenSSL's secure heap,
/// so mirror Node's default process state without `--secure-heap`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_secure_heap_used() -> *mut ObjectHeader {
    let obj = js_object_alloc(0, 4);
    if obj.is_null() {
        return std::ptr::null_mut();
    }
    set_object_value_field(obj, b"total", 0.0);
    set_object_value_field(obj, b"used", 0.0);
    set_object_value_field(obj, b"utilization", 0.0);
    set_object_value_field(obj, b"min", 0.0);
    obj
}

// Type aliases for AES-256-CBC
pub(super) type Aes256CbcEnc = Encryptor<Aes256>;
pub(super) type Aes256CbcDec = Decryptor<Aes256>;

/// AES-256-CBC encryption
/// crypto.createCipheriv('aes-256-cbc', key, iv) -> string (base64)
///
/// # Safety
/// All pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_aes256_encrypt(
    data_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
    iv_ptr: *const StringHeader,
) -> *mut StringHeader {
    let data = match string_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };

    let iv = match string_from_header(iv_ptr) {
        Some(i) => i,
        None => return std::ptr::null_mut(),
    };

    // Key must be 32 bytes for AES-256
    if key.len() != 32 {
        return std::ptr::null_mut();
    }

    // IV must be 16 bytes
    if iv.len() != 16 {
        return std::ptr::null_mut();
    }

    // Create encryptor
    let cipher = Aes256CbcEnc::new_from_slices(&key, &iv);
    let cipher = match cipher {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    // Calculate padded buffer size (next multiple of 16)
    let block_size = 16;
    let padded_len = ((data.len() / block_size) + 1) * block_size;
    let mut buf = vec![0u8; padded_len];
    buf[..data.len()].copy_from_slice(&data);

    // Encrypt with PKCS7 padding
    let ciphertext = match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, data.len()) {
        Ok(ct) => ct,
        Err(_) => return std::ptr::null_mut(),
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(ciphertext);

    js_string_from_bytes(b64.as_ptr(), b64.len() as u32)
}

/// AES-256-CBC decryption
/// crypto.createDecipheriv('aes-256-cbc', key, iv) -> string
///
/// # Safety
/// All pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_aes256_decrypt(
    data_ptr: *const StringHeader, // base64 encoded ciphertext
    key_ptr: *const StringHeader,
    iv_ptr: *const StringHeader,
) -> *mut StringHeader {
    let data_b64 = match string_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };

    let iv = match string_from_header(iv_ptr) {
        Some(i) => i,
        None => return std::ptr::null_mut(),
    };

    // Key must be 32 bytes for AES-256
    if key.len() != 32 {
        return std::ptr::null_mut();
    }

    // IV must be 16 bytes
    if iv.len() != 16 {
        return std::ptr::null_mut();
    }

    // Decode base64 ciphertext
    let mut ciphertext = match base64::engine::general_purpose::STANDARD.decode(&data_b64) {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    // Create decryptor
    let cipher = Aes256CbcDec::new_from_slices(&key, &iv);
    let cipher = match cipher {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    // Decrypt with PKCS7 padding
    let plaintext = match cipher.decrypt_padded_mut::<Pkcs7>(&mut ciphertext) {
        Ok(p) => p,
        Err(_) => return std::ptr::null_mut(),
    };

    // Return as UTF-8 string
    let text = String::from_utf8_lossy(plaintext);
    js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

/// PBKDF2 key derivation
/// crypto.pbkdf2Sync(password, salt, iterations, keyLength, 'sha256') -> Buffer (hex string)
///
/// # Safety
/// Pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_pbkdf2(
    password_ptr: *const StringHeader,
    salt_ptr: *const StringHeader,
    iterations: f64,
    key_length: f64,
) -> *mut StringHeader {
    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let salt = match string_from_header(salt_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let iterations = iterations as u32;
    let key_length = key_length as usize;

    if key_length == 0 || key_length > 1024 {
        return std::ptr::null_mut();
    }

    // Derive key using PBKDF2 with SHA-256
    let mut output = vec![0u8; key_length];
    pbkdf2::pbkdf2_hmac::<Sha256>(&password, &salt, iterations, &mut output);

    let hex_str = hex::encode(&output);
    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// Scrypt key derivation
/// crypto.scryptSync(password, salt, keyLength) -> Buffer (hex string)
///
/// # Safety
/// Pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_scrypt(
    password_ptr: *const StringHeader,
    salt_ptr: *const StringHeader,
    key_length: f64,
) -> *mut StringHeader {
    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let salt = match string_from_header(salt_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let key_length = key_length as usize;

    if key_length == 0 || key_length > 1024 {
        return std::ptr::null_mut();
    }

    // Use recommended scrypt parameters (N=16384, r=8, p=1)
    let params = scrypt::Params::new(14, 8, 1, key_length)
        .unwrap_or_else(|_| scrypt::Params::new(14, 8, 1, 32).unwrap());

    let mut output = vec![0u8; key_length];
    if scrypt::scrypt(&password, &salt, &params, &mut output).is_err() {
        return std::ptr::null_mut();
    }

    let hex_str = hex::encode(&output);
    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// Scrypt key derivation with custom parameters
/// crypto.scryptSync(password, salt, keyLength, { N, r, p }) -> Buffer (hex string)
///
/// # Safety
/// Pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_scrypt_custom(
    password_ptr: *const StringHeader,
    salt_ptr: *const StringHeader,
    key_length: f64,
    log_n: f64, // log2(N)
    r: f64,
    p: f64,
) -> *mut StringHeader {
    let password = match string_from_header(password_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let salt = match string_from_header(salt_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    let key_length = key_length as usize;
    let log_n = log_n as u8;
    let r = r as u32;
    let p = p as u32;

    if key_length == 0 || key_length > 1024 {
        return std::ptr::null_mut();
    }

    let params = match scrypt::Params::new(log_n, r, p, key_length) {
        Ok(p) => p,
        Err(_) => return std::ptr::null_mut(),
    };

    let mut output = vec![0u8; key_length];
    if scrypt::scrypt(&password, &salt, &params, &mut output).is_err() {
        return std::ptr::null_mut();
    }

    let hex_str = hex::encode(&output);
    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// `crypto.scryptSync(password, salt, keylen[, options])` → Buffer.
///
/// Unlike `js_crypto_scrypt` (which returns a hex string), this returns a
/// Buffer to match Node's `scryptSync`, and reads password/salt via
/// `bytes_from_ptr` so Buffer inputs hash correctly. Optional cost
/// parameters are read from `options_ptr` (a NaN-unboxed object pointer, or
/// a null/sentinel for none): `N`/`cost`, `r`/`blockSize`, `p`/
/// `parallelization`. Defaults match Node: N=16384, r=8, p=1.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_scrypt_bytes(
    password_ptr: i64,
    salt_ptr: i64,
    key_length: f64,
    options_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    use perry_runtime::{js_object_get_field_by_name, ObjectHeader};
    let password = bytes_from_ptr(password_ptr);
    let salt = bytes_from_ptr(salt_ptr);
    let klen = key_length as usize;
    if klen == 0 || klen > 1024 {
        return alloc_buffer_from_slice(&[]);
    }
    // Node defaults: N=16384 (cost), r=8 (blockSize), p=1 (parallelization).
    let (mut n, mut r, mut p) = (16384u64, 8u32, 1u32);
    if (options_ptr as usize) >= 0x1000 {
        let obj = options_ptr as *const ObjectHeader;
        // Read a numeric option by primary or alias name; None if absent.
        let read = |primary: &str, alias: &str| -> Option<f64> {
            let pk = js_string_from_bytes(primary.as_ptr(), primary.len() as u32);
            let v = js_object_get_field_by_name(obj, pk);
            if !v.is_undefined() {
                return Some(v.to_number());
            }
            let ak = js_string_from_bytes(alias.as_ptr(), alias.len() as u32);
            let v = js_object_get_field_by_name(obj, ak);
            if v.is_undefined() {
                None
            } else {
                Some(v.to_number())
            }
        };
        if let Some(x) = read("N", "cost") {
            if x >= 1.0 {
                n = x as u64;
            }
        }
        if let Some(x) = read("r", "blockSize") {
            if x >= 1.0 {
                r = x as u32;
            }
        }
        if let Some(x) = read("p", "parallelization") {
            if x >= 1.0 {
                p = x as u32;
            }
        }
    }
    // `scrypt::Params` takes log2(N); Node requires N to be a power of two,
    // so trailing_zeros gives the exact exponent. A non-power-of-two or an
    // otherwise-invalid combo falls back to the Node defaults.
    let log_n = n.trailing_zeros() as u8;
    let params = scrypt::Params::new(log_n, r, p, klen)
        .unwrap_or_else(|_| scrypt::Params::new(14, 8, 1, klen).unwrap());
    let mut out = vec![0u8; klen];
    if scrypt::scrypt(&password, &salt, &params, &mut out).is_err() {
        return alloc_buffer_from_slice(&[]);
    }
    alloc_buffer_from_slice(&out)
}

/// `crypto.hkdfSync(digest, ikm, salt, info, keylen)` → ArrayBuffer.
///
/// HKDF (RFC 5869) extract-and-expand. `ikm`/`salt`/`info` are read as raw
/// bytes (string or Buffer); `digest` selects the HMAC hash (sha256 default,
/// plus sha1/sha224/sha384/sha512). Returns the derived key in a real
/// ArrayBuffer to match Node, so `Buffer.from(result)` / `new
/// Uint8Array(result)` work. An empty salt means "no salt" per the spec.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_hkdf_sync(
    digest_ptr: i64,
    ikm_ptr: i64,
    salt_ptr: i64,
    info_ptr: i64,
    keylen: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    use hkdf::Hkdf;
    let digest = String::from_utf8_lossy(&bytes_from_ptr(digest_ptr))
        .to_ascii_lowercase()
        .replace('-', "");
    let ikm = bytes_from_ptr(ikm_ptr);
    let salt = bytes_from_ptr(salt_ptr);
    let info = bytes_from_ptr(info_ptr);
    let out_len = keylen as usize;
    // Cap at the largest HKDF output across supported digests (255*64 for
    // sha512); per-digest over-length is rejected by `expand` below.
    let make = |bytes: &[u8]| -> *mut perry_runtime::buffer::BufferHeader {
        let buf = alloc_buffer_from_slice(bytes);
        if !buf.is_null() {
            perry_runtime::buffer::mark_as_array_buffer(buf as usize);
        }
        buf
    };
    if out_len == 0 || out_len > 255 * 64 {
        return make(&[]);
    }
    let salt_ref: Option<&[u8]> = if salt.is_empty() { None } else { Some(&salt) };
    let mut okm = vec![0u8; out_len];
    let ok = match digest.as_str() {
        "sha1" => Hkdf::<Sha1>::new(salt_ref, &ikm)
            .expand(&info, &mut okm)
            .is_ok(),
        "sha224" => Hkdf::<Sha224>::new(salt_ref, &ikm)
            .expand(&info, &mut okm)
            .is_ok(),
        "sha384" => Hkdf::<Sha384>::new(salt_ref, &ikm)
            .expand(&info, &mut okm)
            .is_ok(),
        "sha512" => Hkdf::<Sha512>::new(salt_ref, &ikm)
            .expand(&info, &mut okm)
            .is_ok(),
        // "sha256" and the empty/unknown default.
        _ => Hkdf::<Sha256>::new(salt_ref, &ikm)
            .expand(&info, &mut okm)
            .is_ok(),
    };
    if !ok {
        return make(&[]);
    }
    make(&okm)
}

/// `crypto.generateKeyPairSync(type, options)` → `{ publicKey, privateKey }`
/// as PEM strings (#1365). Supports `'rsa'` (modulusLength from options,
/// default 2048) and `'ec'` (NIST P-256 / `prime256v1`). The public key is
/// SPKI PEM, the private key PKCS#8 PEM — the format the overwhelming
/// majority of callers request via `publicKeyEncoding`/`privateKeyEncoding:
/// { type, format: 'pem' }`. The encoding-options object is accepted but only
/// the PEM string form is produced (KeyObjects and DER are not modeled).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_sync(type_ptr: i64, options_ptr: i64) -> f64 {
    let ktype = String::from_utf8_lossy(&bytes_from_ptr(type_ptr)).to_ascii_lowercase();

    // RSA modulus length from `options.modulusLength` (default 2048).
    let modulus_bits = read_options_number(options_ptr, "modulusLength").unwrap_or(2048.0) as usize;

    let pems: Option<(String, String)> = match ktype.as_str() {
        "rsa" => {
            use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
            let mut rng = rand::thread_rng();
            // Clamp to a sane range; Node's default is 2048.
            let bits = modulus_bits.clamp(512, 8192);
            rsa::RsaPrivateKey::new(&mut rng, bits).ok().and_then(|sk| {
                let pk = sk.to_public_key();
                let priv_pem = sk.to_pkcs8_pem(LineEnding::LF).ok()?.to_string();
                let pub_pem = pk.to_public_key_pem(LineEnding::LF).ok()?;
                Some((pub_pem, priv_pem))
            })
        }
        // 'ec' (default/prime256v1) and the explicit 'prime256v1'/'p-256'.
        "ec" | "prime256v1" | "p-256" | "p256" => {
            use p256::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
            let secret = p256::SecretKey::random(&mut rand::thread_rng());
            let priv_pem = secret
                .to_pkcs8_pem(LineEnding::LF)
                .ok()
                .map(|p| p.to_string());
            let pub_pem = secret.public_key().to_public_key_pem(LineEnding::LF).ok();
            match (pub_pem, priv_pem) {
                (Some(pb), Some(pv)) => Some((pb, pv)),
                _ => None,
            }
        }
        _ => None,
    };

    match pems {
        Some((pub_pem, priv_pem)) => build_key_pair_object(&pub_pem, &priv_pem),
        None => nanbox_undefined(),
    }
}

/// Read a numeric field from a NaN-unboxed options object pointer (0/null →
/// `None`). Shared by `generateKeyPairSync`.
pub(super) unsafe fn read_options_number(options_ptr: i64, name: &str) -> Option<f64> {
    if (options_ptr as usize) < 0x1000 {
        return None;
    }
    let obj = options_ptr as *const perry_runtime::ObjectHeader;
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let v = perry_runtime::js_object_get_field_by_name(obj, key);
    if v.is_undefined() {
        None
    } else {
        Some(v.to_number())
    }
}

/// Build a `{ publicKey, privateKey }` JS object holding the two PEM strings,
/// returned NaN-boxed as POINTER_TAG.
pub(super) unsafe fn build_key_pair_object(pub_pem: &str, priv_pem: &str) -> f64 {
    use perry_runtime::{
        js_array_alloc, js_array_push, js_object_alloc, js_object_set_field, js_object_set_keys,
        JSValue,
    };
    let obj = js_object_alloc(0, 2);
    let keys = js_array_alloc(2);
    let pub_key_name = js_string_from_bytes("publicKey".as_ptr(), 9);
    js_array_push(keys, JSValue::string_ptr(pub_key_name));
    let priv_key_name = js_string_from_bytes("privateKey".as_ptr(), 10);
    js_array_push(keys, JSValue::string_ptr(priv_key_name));
    let pub_s = js_string_from_bytes(pub_pem.as_ptr(), pub_pem.len() as u32);
    let priv_s = js_string_from_bytes(priv_pem.as_ptr(), priv_pem.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(pub_s));
    js_object_set_field(obj, 1, JSValue::string_ptr(priv_s));
    js_object_set_keys(obj, keys);
    nanbox_pointer_f64(obj as usize)
}
