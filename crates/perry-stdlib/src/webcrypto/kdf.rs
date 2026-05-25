/// `crypto.subtle.deriveBits({ name: "ECDH", public }, privateKey, length)`
/// → Promise<Uint8Array>. Initial asymmetric-derive coverage implements
/// P-256 ECDH, matching Node/Bun WebCrypto suites.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_derive_bits(
    algo_bits: f64,
    base_key_bits: f64,
    length_bits: f64,
) -> *mut Promise {
    let bit_len = match number_from_bits(length_bits.to_bits()) {
        Some(n) => n,
        None => return resolve_undefined(),
    };
    if bit_len % 8 != 0 {
        return resolve_undefined();
    }
    let byte_len = (bit_len / 8) as usize;
    if let Some(bytes) = kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len) {
        return resolve_with_bytes(&bytes);
    }
    if bit_len > 256 {
        return resolve_undefined();
    }
    let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    resolve_with_bytes(&shared[..byte_len])
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_derive_key(
    algo_bits: f64,
    base_key_bits: f64,
    derived_algo_bits: f64,
    _extractable_bits: f64,
    _usages_bits: f64,
) -> *mut Promise {
    let derived_name = match extract_algo_name(derived_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized derived-key algorithm name",
            )
        }
    };
    let derived_upper = derived_name.to_ascii_uppercase();
    let (key_algo, hash, bit_len) = if derived_upper == "HMAC" {
        let hash = match extract_hmac_hash(derived_algo_bits.to_bits()) {
            Some(h) => h,
            None => return resolve_undefined(),
        };
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::Hmac, hash, length)
    } else if derived_upper == "AES-GCM" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesGcm, HashAlgo::Sha256, length)
    } else if derived_upper == "AES-KW" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesKw, HashAlgo::Sha256, length)
    } else if derived_upper == "AES-CBC" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesCbc, HashAlgo::Sha256, length)
    } else if derived_upper == "AES-CTR" {
        let length = object_field_number(derived_algo_bits.to_bits(), b"length").unwrap_or(256);
        (KeyAlgo::AesCtr, HashAlgo::Sha256, length)
    } else {
        return resolve_undefined();
    };
    if bit_len % 8 != 0 || bit_len == 0 || bit_len > 256 {
        return resolve_undefined();
    }
    let byte_len = (bit_len / 8) as usize;
    let key_bytes = if let Some(bytes) =
        kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len)
    {
        bytes
    } else {
        let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
            Some(s) => s,
            None => return resolve_undefined(),
        };
        if byte_len > shared.len() {
            return resolve_undefined();
        }
        shared[..byte_len].to_vec()
    };
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return resolve_undefined();
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: key_algo,
            hash,
            kind: KeyKind::Secret,
        },
    );
    resolve_with_bits(JSValue::pointer(buf as *const u8).bits())
}

// =====================================================================
// AES-GCM encrypt / decrypt
//
// jose's `gcmEncrypt` / `gcmDecrypt` pass:
//   { name: 'AES-GCM', iv: <Uint8Array>, additionalData?: <Uint8Array>,
//     tagLength?: 128 }, key, data
// The IV is a 12-byte nonce (the only length the underlying `aes-gcm`
// crate's `Nonce` type accepts); we surface a clean "undefined" reject
// for other lengths rather than panicking.
//
// The output of encrypt is `ciphertext || tag` (the WebCrypto spec
// appends the 16-byte GCM tag); decrypt expects the same layout.
// =====================================================================

/// Read an optional object field by name and return its raw bytes, or
/// `None` if the field is absent / not a buffer-like value.
unsafe fn object_field_bytes(obj_bits: u64, name: &[u8]) -> Option<Vec<u8>> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    let bytes = bytes_from_jsvalue(val.bits());
    if bytes.is_empty() {
        // Distinguish "field missing" from "field present but empty":
        // for our callers an empty AAD / IV is semantically equivalent
        // to "missing", and the caller's defaulting path is fine.
        None
    } else {
        Some(bytes)
    }
}

unsafe fn object_field_bits(obj_bits: u64, name: &[u8]) -> Option<u64> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    let bits = val.bits();
    if (bits >> 48) as u16 == 0x7FFC {
        None
    } else {
        Some(bits)
    }
}

/// Read an optional string field from an algorithm object.
unsafe fn object_field_string(obj_bits: u64, name: &[u8]) -> Option<String> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    string_from_jsvalue(val.bits())
}

unsafe fn set_object_string_field(obj: *mut perry_runtime::ObjectHeader, name: &[u8], value: &str) {
    let key = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(STRING_TAG | ((val as u64) & POINTER_MASK)),
    );
}

/// AES-GCM encrypt. Returns ciphertext || tag (matches WebCrypto spec).
fn aes_gcm_encrypt(key: &[u8], iv: &[u8], aad: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
    type Aes192Gcm = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12>;

    if iv.len() != 12 {
        return None;
    }
    let nonce = Nonce::from_slice(iv);
    let payload = Payload {
        msg: plaintext,
        aad,
    };
    match key.len() {
        16 => {
            let cipher = Aes128Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        24 => {
            let cipher = Aes192Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        32 => {
            let cipher = Aes256Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        _ => None,
    }
}

/// AES-GCM decrypt. Expects `ciphertext || tag` per the WebCrypto spec.
fn aes_gcm_decrypt(key: &[u8], iv: &[u8], aad: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
    type Aes192Gcm = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12>;

    if iv.len() != 12 {
        return None;
    }
    let nonce = Nonce::from_slice(iv);
    let payload = Payload {
        msg: ciphertext,
        aad,
    };
    match key.len() {
        16 => {
            let cipher = Aes128Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        24 => {
            let cipher = Aes192Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        32 => {
            let cipher = Aes256Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        _ => None,
    }
}

type Aes128CbcEnc = Encryptor<Aes128>;
type Aes192CbcEnc = Encryptor<Aes192>;
type Aes256CbcEnc = Encryptor<Aes256>;
type Aes128CbcDec = Decryptor<Aes128>;
type Aes192CbcDec = Decryptor<Aes192>;
type Aes256CbcDec = Decryptor<Aes256>;

fn aes_cbc_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    if iv.len() != 16 {
        return None;
    }
    let padded_len = ((plaintext.len() / 16) + 1) * 16;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    let out = match key.len() {
        16 => Aes128CbcEnc::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .ok()?,
        24 => Aes192CbcEnc::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .ok()?,
        32 => Aes256CbcEnc::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .ok()?,
        _ => return None,
    };
    Some(out.to_vec())
}

fn aes_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if iv.len() != 16 || ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return None;
    }
    let mut buf = ciphertext.to_vec();
    let out = match key.len() {
        16 => Aes128CbcDec::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .ok()?,
        24 => Aes192CbcDec::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .ok()?,
        32 => Aes256CbcDec::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .ok()?,
        _ => return None,
    };
    Some(out.to_vec())
}

unsafe fn extract_aes_cbc_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-CBC") {
        return None;
    }
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesCbc {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, iv, data_bytes))
}

/// Shared AES-GCM arg-extraction for encrypt / decrypt: pulls the
/// algorithm-name + iv (+ optional aad) from the algorithm object, plus
/// the raw key bytes (validating they came from an AES-GCM importKey)
/// and the data bytes. Returns `None` if any required piece is missing.

fn increment_ctr_counter(counter: &mut [u8; 16], length: u32) {
    let n = u128::from_be_bytes(*counter);
    let mask = if length == 128 {
        u128::MAX
    } else {
        (1u128 << length) - 1
    };
    let prefix = n & !mask;
    let next = ((n & mask).wrapping_add(1)) & mask;
    *counter = (prefix | next).to_be_bytes();
}

fn aes_ctr_apply(key: &[u8], counter: &[u8], length: u32, data: &[u8]) -> Option<Vec<u8>> {
    if counter.len() != 16 || length == 0 || length > 128 {
        return None;
    }
    let mut ctr = [0u8; 16];
    ctr.copy_from_slice(counter);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = GenericArray::clone_from_slice(&ctr);
        match key.len() {
            16 => Aes128::new_from_slice(key).ok()?.encrypt_block(&mut block),
            24 => Aes192::new_from_slice(key).ok()?.encrypt_block(&mut block),
            32 => Aes256::new_from_slice(key).ok()?.encrypt_block(&mut block),
            _ => return None,
        }
        out.extend(chunk.iter().zip(block.iter()).map(|(a, b)| a ^ b));
        increment_ctr_counter(&mut ctr, length);
    }
    Some(out)
}

unsafe fn extract_aes_ctr_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, u32, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-CTR") {
        return None;
    }
    let counter = object_field_bytes(algo_bits, b"counter")?;
    let length = object_field_number(algo_bits, b"length")?;
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesCtr {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, counter, length, data_bytes))
}

unsafe fn extract_aes_gcm_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-GCM") {
        return None;
    }
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let aad = object_field_bytes(algo_bits, b"additionalData").unwrap_or_default();
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesGcm {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, iv, aad, data_bytes))
}
