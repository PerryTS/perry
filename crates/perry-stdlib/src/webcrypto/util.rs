pub(super) use std::collections::HashMap;
pub(super) use std::sync::Mutex;

pub(super) use aes::cipher::{
    generic_array::GenericArray, BlockEncrypt, KeyInit as AesBlockKeyInit,
};
pub(super) use aes::{Aes128, Aes192, Aes256};
pub(super) use base64::Engine as _;
pub(super) use cbc::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit},
    Decryptor, Encryptor,
};
pub(super) use hmac::{Hmac, KeyInit, Mac};
pub(super) use once_cell::sync::Lazy;
pub(super) use p256::ecdh::diffie_hellman as p256_diffie_hellman;
pub(super) use p256::ecdsa::signature::{Signer as EcdsaSigner, Verifier as EcdsaVerifier};
pub(super) use p256::ecdsa::{
    Signature as P256EcdsaSignature, SigningKey as P256EcdsaSigningKey,
    VerifyingKey as P256EcdsaVerifyingKey,
};
pub(super) use p256::elliptic_curve::sec1::ToEncodedPoint;
pub(super) use p256::{PublicKey as P256PublicKey, SecretKey as P256SecretKey};
pub(super) use rsa::pkcs1v15::{
    Signature as RsaPkcs1v15Signature, SigningKey as RsaPkcs1v15SigningKey,
    VerifyingKey as RsaPkcs1v15VerifyingKey,
};
pub(super) use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
pub(super) use rsa::pss::{
    Signature as RsaPssSignature, SigningKey as RsaPssSigningKey,
    VerifyingKey as RsaPssVerifyingKey,
};
pub(super) use rsa::sha2::{Sha256 as RsaSha256, Sha384 as RsaSha384, Sha512 as RsaSha512};
pub(super) use rsa::signature::{
    RandomizedSigner as RsaRandomizedSigner, SignatureEncoding as RsaSignatureEncoding,
};
pub(super) use rsa::traits::{PrivateKeyParts, PublicKeyParts};
pub(super) use rsa::{BigUint as RsaBigUint, Oaep, RsaPrivateKey, RsaPublicKey};
pub(super) use sha1::Sha1;
pub(super) use sha2::{Digest as Sha2Digest, Sha256, Sha384, Sha512};

pub(super) use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data_mut, is_registered_buffer, mark_as_uint8array, BufferHeader,
    },
    js_object_alloc, js_object_set_field_by_name, js_promise_resolved, JSValue, Promise,
    StringHeader,
};

/// `buffer_data` is private to perry-runtime — open-code the same offset.
#[inline]
pub(super) unsafe fn buffer_payload(buf: *const BufferHeader) -> *const u8 {
    (buf as *const u8).add(std::mem::size_of::<BufferHeader>())
}

// #854: NaN-boxing tag contract — see CLAUDE.md. `POINTER_TAG`,
// `STRING_TAG`, and `SHORT_STRING_TAG` aren't directly consulted in this
// file but are part of the documented set of tag prefixes; kept for
// reference next to the masks/values that this module does use, so a
// future caller editing here can see the full encoding contract at the
// top of the file.
#[allow(dead_code)]
pub(super) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(super) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
#[allow(dead_code)]
pub(super) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
#[allow(dead_code)]
pub(super) const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
pub(super) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
pub(super) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum HashAlgo {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum KeyAlgo {
    Hmac,
    Hkdf,
    Pbkdf2,
    AesGcm,
    AesKw,
    AesCbc,
    AesCtr,
    EcdsaP256,
    EcdhP256,
    Ed25519,
    X25519,
    RsaOaep,
    RsassaPkcs1,
    RsaPss,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum KeyKind {
    Secret,
    Private,
    Public,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct CryptoKeyMaterial {
    pub(super) algo: KeyAlgo,
    /// For HMAC: the underlying hash. For AES-GCM the hash slot is
    /// unused (we keep `HashAlgo::Sha256` as a harmless placeholder so
    /// the struct stays `Copy`).
    pub(super) hash: HashAlgo,
    pub(super) kind: KeyKind,
}

pub(super) static CRYPTO_KEY_REGISTRY: Lazy<Mutex<HashMap<usize, CryptoKeyMaterial>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) fn register_crypto_key(buf_addr: usize, mat: CryptoKeyMaterial) {
    CRYPTO_KEY_REGISTRY.lock().unwrap().insert(buf_addr, mat);
}

pub(super) fn lookup_crypto_key(buf_addr: usize) -> Option<CryptoKeyMaterial> {
    CRYPTO_KEY_REGISTRY
        .lock()
        .unwrap()
        .get(&buf_addr)
        .copied()
        .or_else(|| {
            let (algo, hash, kind) = perry_runtime::buffer::crypto_key_meta(buf_addr)?;
            let algo = match algo {
                1 => KeyAlgo::Hmac,
                2 => KeyAlgo::AesGcm,
                3 => KeyAlgo::AesKw,
                4 => KeyAlgo::AesCbc,
                5 => KeyAlgo::AesCtr,
                6 => KeyAlgo::Hkdf,
                7 => KeyAlgo::Pbkdf2,
                _ => return None,
            };
            let hash = match hash {
                1 => HashAlgo::Sha1,
                2 => HashAlgo::Sha256,
                3 => HashAlgo::Sha384,
                4 => HashAlgo::Sha512,
                _ => HashAlgo::Sha256,
            };
            let kind = match kind {
                1 => KeyKind::Secret,
                2 => KeyKind::Private,
                3 => KeyKind::Public,
                _ => KeyKind::Secret,
            };
            Some(CryptoKeyMaterial { algo, hash, kind })
        })
}

/// Strip POINTER_TAG / STRING_TAG from a NaN-boxed value, returning the
/// raw 48-bit pointer. Returns 0 for tagged primitives (undef/null/bool/int).
pub(super) fn strip_ptr(bits: u64) -> usize {
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFD || top16 == 0x7FFF {
        (bits & POINTER_MASK) as usize
    } else {
        0
    }
}

/// Read raw bytes from a NaN-boxed value. Accepts strings (StringHeader),
/// Buffers / Uint8Arrays (BufferHeader — perry uses one type for both),
/// and TypedArrayHeader (Uint8Array allocated via the typed-array path).
pub(super) unsafe fn bytes_from_jsvalue(bits: u64) -> Vec<u8> {
    let top16 = (bits >> 48) as u16;
    // Inline SSO short string.
    if top16 == 0x7FF9 {
        // Mirror StringHeader::short_string_to_buf — but we don't have
        // direct access to it here without going through value.rs's
        // private API. Pull the bytes out of the inline payload.
        let v = JSValue::from_bits(bits);
        let mut buf = [0u8; perry_runtime::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        return buf[..n].to_vec();
    }
    let raw = strip_ptr(bits);
    if raw < 0x1000 {
        return Vec::new();
    }
    if is_registered_buffer(raw) {
        let buf = raw as *const BufferHeader;
        let len = (*buf).length as usize;
        return std::slice::from_raw_parts(buffer_payload(buf), len).to_vec();
    }
    if let Some(_kind) = perry_runtime::typedarray::lookup_typed_array_kind(raw) {
        // TypedArrayHeader: 16-byte header, payload follows. Read raw
        // bytes — for Uint8Array this is what the caller wants. For
        // wider element kinds the caller's intent is ambiguous; we
        // return the raw byte view (length × elem_size) which matches
        // the spec ("BufferSource" can be any TypedArray and digest
        // hashes the raw underlying bytes).
        let ta = raw as *const perry_runtime::typedarray::TypedArrayHeader;
        let len = (*ta).length as usize;
        let elem_size = (*ta).elem_size as usize;
        let total = len * elem_size;
        let data = (raw as *const u8).add(std::mem::size_of::<
            perry_runtime::typedarray::TypedArrayHeader,
        >());
        return std::slice::from_raw_parts(data, total).to_vec();
    }
    if top16 == 0x7FFF {
        let hdr = raw as *const StringHeader;
        let len = (*hdr).byte_len as usize;
        let data = (raw as *const u8).add(std::mem::size_of::<StringHeader>());
        return std::slice::from_raw_parts(data, len).to_vec();
    }
    Vec::new()
}

/// Coerce a NaN-boxed value to a String. Returns None for non-string
/// primitives (we want loud failures, not "undefined" → "undefined").
pub(super) unsafe fn string_from_jsvalue(bits: u64) -> Option<String> {
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FF9 {
        let v = JSValue::from_bits(bits);
        let mut buf = [0u8; perry_runtime::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        return std::str::from_utf8(&buf[..n]).ok().map(str::to_string);
    }
    if top16 != 0x7FFF {
        return None;
    }
    let raw = (bits & POINTER_MASK) as *const StringHeader;
    if (raw as usize) < 0x1000 {
        return None;
    }
    let len = (*raw).byte_len as usize;
    let data = (raw as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

pub(super) fn parse_hash_alg(name: &str) -> Option<HashAlgo> {
    let upper: String = name.chars().flat_map(char::to_uppercase).collect();
    match upper.replace('-', "").as_str() {
        "SHA1" => Some(HashAlgo::Sha1),
        "SHA256" => Some(HashAlgo::Sha256),
        "SHA384" => Some(HashAlgo::Sha384),
        "SHA512" => Some(HashAlgo::Sha512),
        _ => None,
    }
}

/// Extract a hash algorithm name from the digest's first arg. Accepts
/// either a string ("SHA-256") or an object with a `.name` field
/// ({ name: "SHA-256" }), per the spec's `AlgorithmIdentifier` shape.
pub(super) unsafe fn extract_hash_algo(bits: u64) -> Option<HashAlgo> {
    if let Some(s) = string_from_jsvalue(bits) {
        return parse_hash_alg(&s);
    }
    // Object with `.name` — read via the runtime helper.
    let obj_ptr = strip_ptr(bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key = b"name";
    let key_ptr = perry_runtime::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let name_val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    string_from_jsvalue(name_val.bits()).and_then(|s| parse_hash_alg(&s))
}

/// Extract the HMAC hash from an algorithm object literal:
/// `{ name: "HMAC", hash: "SHA-256" }` or `{ name: "HMAC", hash: { name: "SHA-256" } }`.
pub(super) unsafe fn extract_hmac_hash(algo_bits: u64) -> Option<HashAlgo> {
    // Direct string shorthand: `importKey("raw", k, "HMAC", ...)` is not
    // spec-legal but some libraries pass it; treat it as HMAC-SHA-256
    // by default — actually no, stay strict and require the object form.
    let obj_ptr = strip_ptr(algo_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key = b"hash";
    let key_ptr = perry_runtime::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let hash_val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    extract_hash_algo(hash_val.bits())
}

pub(super) unsafe fn extract_algorithm_hash(algo_bits: u64, default: HashAlgo) -> HashAlgo {
    let obj_ptr = strip_ptr(algo_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return default;
    }
    let key = b"hash";
    let key_ptr = perry_runtime::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let hash_val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    extract_hash_algo(hash_val.bits()).unwrap_or(default)
}

/// Allocate a fresh Buffer marked as Uint8Array (so `instanceof Uint8Array`
/// is true and `new Uint8Array(buf)` memcpy's correctly), copy `bytes` in.
pub(super) unsafe fn alloc_uint8array_from_slice(bytes: &[u8]) -> *mut BufferHeader {
    let buf = buffer_alloc(bytes.len() as u32);
    if buf.is_null() {
        return buf;
    }
    (*buf).length = bytes.len() as u32;
    if !bytes.is_empty() {
        let dst = buffer_data_mut(buf);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    }
    mark_as_uint8array(buf as usize);
    buf
}

/// Wrap a heap value (NaN-boxed bits) in an already-resolved Promise.
pub(super) fn resolve_with_bits(bits: u64) -> *mut Promise {
    js_promise_resolved(f64::from_bits(bits))
}

/// Construct a DOMException-shaped object (`{ name, message, stack: "" }`)
/// and return a rejected Promise carrying it. WebCrypto spec demands
/// `DOMException` instances on subtle.* error paths (`OperationError`,
/// `NotSupportedError`, `InvalidAccessError`, `DataError`, `SyntaxError`),
/// and consumers (`.catch(e => e.name === "...")`) match on `.name` —
/// we model that shape rather than the full DOM `code` lookup table.
/// Issue #1431.
pub(super) unsafe fn reject_with_dom_exception(name: &str, message: &str) -> *mut Promise {
    let obj = js_object_alloc(0, 3);
    if obj.is_null() {
        return perry_runtime::js_promise_rejected(f64::from_bits(0x7FFC_0000_0000_0001));
    }
    let name_key = perry_runtime::js_string_from_bytes(b"name".as_ptr(), 4);
    let message_key = perry_runtime::js_string_from_bytes(b"message".as_ptr(), 7);
    let stack_key = perry_runtime::js_string_from_bytes(b"stack".as_ptr(), 5);
    let name_str = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let message_str = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let empty_str = perry_runtime::js_string_from_bytes(b"".as_ptr(), 0);
    let name_val = f64::from_bits(JSValue::string_ptr(name_str).bits());
    let message_val = f64::from_bits(JSValue::string_ptr(message_str).bits());
    let stack_val = f64::from_bits(JSValue::string_ptr(empty_str).bits());
    js_object_set_field_by_name(obj, name_key, name_val);
    js_object_set_field_by_name(obj, message_key, message_val);
    js_object_set_field_by_name(obj, stack_key, stack_val);
    let obj_val = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    perry_runtime::js_promise_rejected(obj_val)
}

/// Resolve a Promise with a Uint8Array view of `bytes`.
pub(super) unsafe fn resolve_with_bytes(bytes: &[u8]) -> *mut Promise {
    let buf = alloc_uint8array_from_slice(bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}

pub(super) unsafe fn resolve_with_bool(b: bool) -> *mut Promise {
    let bits = if b { TAG_TRUE } else { TAG_FALSE };
    resolve_with_bits(bits)
}

pub(super) fn compute_digest(algo: HashAlgo, data: &[u8]) -> Vec<u8> {
    match algo {
        HashAlgo::Sha1 => Sha1::digest(data).to_vec(),
        HashAlgo::Sha256 => Sha256::digest(data).to_vec(),
        HashAlgo::Sha384 => Sha384::digest(data).to_vec(),
        HashAlgo::Sha512 => Sha512::digest(data).to_vec(),
    }
}

pub(super) fn compute_hmac(hash: HashAlgo, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    match hash {
        HashAlgo::Sha1 => {
            let mut mac = <Hmac<Sha1>>::new_from_slice(key).ok()?;
            mac.update(data);
            Some(mac.finalize().into_bytes().to_vec())
        }
        HashAlgo::Sha256 => {
            let mut mac = <Hmac<Sha256>>::new_from_slice(key).ok()?;
            mac.update(data);
            Some(mac.finalize().into_bytes().to_vec())
        }
        HashAlgo::Sha384 => {
            let mut mac = <Hmac<Sha384>>::new_from_slice(key).ok()?;
            mac.update(data);
            Some(mac.finalize().into_bytes().to_vec())
        }
        HashAlgo::Sha512 => {
            let mut mac = <Hmac<Sha512>>::new_from_slice(key).ok()?;
            mac.update(data);
            Some(mac.finalize().into_bytes().to_vec())
        }
    }
}

pub(super) fn generate_p256_signing_key() -> Option<P256EcdsaSigningKey> {
    use rand::RngCore;
    let mut rng = rand::rngs::OsRng;
    for _ in 0..128 {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if let Ok(key) = P256EcdsaSigningKey::from_slice(&bytes) {
            return Some(key);
        }
    }
    None
}

pub(super) fn generate_p256_secret_key() -> Option<P256SecretKey> {
    use rand::RngCore;
    let mut rng = rand::rngs::OsRng;
    for _ in 0..128 {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if let Ok(key) = P256SecretKey::from_slice(&bytes) {
            return Some(key);
        }
    }
    None
}

pub(super) fn rsa_oaep_encrypt(hash: HashAlgo, key: &RsaPublicKey, data: &[u8]) -> Option<Vec<u8>> {
    let mut rng = rand::rngs::OsRng;
    match hash {
        HashAlgo::Sha1 => key
            .encrypt(&mut rng, Oaep::new::<rsa_sha1::Sha1>(), data)
            .ok(),
        HashAlgo::Sha256 => key.encrypt(&mut rng, Oaep::new::<RsaSha256>(), data).ok(),
        HashAlgo::Sha384 => key.encrypt(&mut rng, Oaep::new::<RsaSha384>(), data).ok(),
        HashAlgo::Sha512 => key.encrypt(&mut rng, Oaep::new::<RsaSha512>(), data).ok(),
    }
}

pub(super) fn rsa_oaep_decrypt(
    hash: HashAlgo,
    key: &RsaPrivateKey,
    data: &[u8],
) -> Option<Vec<u8>> {
    let mut rng = rand::rngs::OsRng;
    match hash {
        HashAlgo::Sha1 => key
            .decrypt_blinded(&mut rng, Oaep::new::<rsa_sha1::Sha1>(), data)
            .ok(),
        HashAlgo::Sha256 => key
            .decrypt_blinded(&mut rng, Oaep::new::<RsaSha256>(), data)
            .ok(),
        HashAlgo::Sha384 => key
            .decrypt_blinded(&mut rng, Oaep::new::<RsaSha384>(), data)
            .ok(),
        HashAlgo::Sha512 => key
            .decrypt_blinded(&mut rng, Oaep::new::<RsaSha512>(), data)
            .ok(),
    }
}

pub(super) fn rsa_pkcs1_sign(hash: HashAlgo, key: RsaPrivateKey, data: &[u8]) -> Option<Vec<u8>> {
    match hash {
        HashAlgo::Sha256 => Some(
            RsaPkcs1v15SigningKey::<RsaSha256>::new(key)
                .sign(data)
                .to_vec(),
        ),
        HashAlgo::Sha384 => Some(
            RsaPkcs1v15SigningKey::<RsaSha384>::new(key)
                .sign(data)
                .to_vec(),
        ),
        HashAlgo::Sha512 => Some(
            RsaPkcs1v15SigningKey::<RsaSha512>::new(key)
                .sign(data)
                .to_vec(),
        ),
        HashAlgo::Sha1 => None,
    }
}

pub(super) fn rsa_pkcs1_verify(
    hash: HashAlgo,
    key: RsaPublicKey,
    data: &[u8],
    sig: &[u8],
) -> Option<bool> {
    let sig = RsaPkcs1v15Signature::try_from(sig).ok()?;
    let ok = match hash {
        HashAlgo::Sha256 => RsaPkcs1v15VerifyingKey::<RsaSha256>::new(key)
            .verify(data, &sig)
            .is_ok(),
        HashAlgo::Sha384 => RsaPkcs1v15VerifyingKey::<RsaSha384>::new(key)
            .verify(data, &sig)
            .is_ok(),
        HashAlgo::Sha512 => RsaPkcs1v15VerifyingKey::<RsaSha512>::new(key)
            .verify(data, &sig)
            .is_ok(),
        HashAlgo::Sha1 => return None,
    };
    Some(ok)
}

pub(super) fn rsa_pss_sign(
    hash: HashAlgo,
    key: RsaPrivateKey,
    data: &[u8],
    salt_len: usize,
) -> Option<Vec<u8>> {
    let mut rng = rand::rngs::OsRng;
    match hash {
        HashAlgo::Sha256 => RsaPssSigningKey::<RsaSha256>::new_with_salt_len(key, salt_len)
            .try_sign_with_rng(&mut rng, data)
            .ok()
            .map(|s| s.to_vec()),
        HashAlgo::Sha384 => RsaPssSigningKey::<RsaSha384>::new_with_salt_len(key, salt_len)
            .try_sign_with_rng(&mut rng, data)
            .ok()
            .map(|s| s.to_vec()),
        HashAlgo::Sha512 => RsaPssSigningKey::<RsaSha512>::new_with_salt_len(key, salt_len)
            .try_sign_with_rng(&mut rng, data)
            .ok()
            .map(|s| s.to_vec()),
        HashAlgo::Sha1 => None,
    }
}

pub(super) fn rsa_pss_verify(
    hash: HashAlgo,
    key: RsaPublicKey,
    data: &[u8],
    sig: &[u8],
    salt_len: usize,
) -> Option<bool> {
    let sig = RsaPssSignature::try_from(sig).ok()?;
    let ok = match hash {
        HashAlgo::Sha256 => RsaPssVerifyingKey::<RsaSha256>::new_with_salt_len(key, salt_len)
            .verify(data, &sig)
            .is_ok(),
        HashAlgo::Sha384 => RsaPssVerifyingKey::<RsaSha384>::new_with_salt_len(key, salt_len)
            .verify(data, &sig)
            .is_ok(),
        HashAlgo::Sha512 => RsaPssVerifyingKey::<RsaSha512>::new_with_salt_len(key, salt_len)
            .verify(data, &sig)
            .is_ok(),
        HashAlgo::Sha1 => return None,
    };
    Some(ok)
}

// =====================================================================
// FFI entry points (called from codegen-emitted IR).
// All four return `*mut Promise`; codegen NaN-boxes the result with
// POINTER_TAG. Each takes `f64` for value args (NaN-boxed at the call
// site) so the ABI matches perry's standard JS-value calling convention.
// =====================================================================
