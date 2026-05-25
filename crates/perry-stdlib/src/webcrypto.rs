//! Web Crypto API: `crypto.subtle.digest` / `importKey` / `sign` / `verify`
//! / `encrypt` / `decrypt`.
//!
//! Issue #561 — sigv4 / JWT / web-push consumers (s3-lite-client,
//! aws4fetch, jose, oidc-client-ts, web-push) all route through
//! `crypto.subtle`. This module covers the symmetric subset needed for
//! those use cases:
//!
//! - `digest("SHA-1" | "SHA-256" | "SHA-384" | "SHA-512", data)` →
//!   Promise<Uint8Array>
//! - `importKey("raw", key, { name: "HMAC", hash: { name: "SHA-256" } },
//!   ...)` → Promise<CryptoKey>  (HMAC and AES-GCM both supported)
//! - `sign("HMAC", key, data)` → Promise<Uint8Array>
//! - `verify("HMAC", key, signature, data)` → Promise<boolean>
//! - `encrypt({ name: "AES-GCM", iv, additionalData?, tagLength? }, key, data)`
//!   → Promise<Uint8Array> (jose `gcmEncrypt`)
//! - `decrypt({ name: "AES-GCM", iv, additionalData?, tagLength? }, key, data)`
//!   → Promise<Uint8Array>
//!
//! AES-CBC, AES-CTR, RSA-OAEP encrypt/decrypt, ECDH deriveBits, and
//! RSA-PSS/RSASSA remain TODO follow-ups.
//!
//! `CryptoKey` is represented as a Buffer holding the raw key bytes,
//! with an entry in `CRYPTO_KEY_REGISTRY` recording `(algo, hash, kind)`
//! so `sign` / `verify` can route to the correct primitive.
//!
//! The async aspect is decorative — these primitives are CPU-bound and
//! resolve synchronously inside the returned Promise (the issue's
//! implementation note explicitly calls this out).

use std::collections::HashMap;
use std::sync::Mutex;

use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit as AesBlockKeyInit};
use aes::{Aes128, Aes192, Aes256};
use base64::Engine as _;
use cbc::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit},
    Decryptor, Encryptor,
};
use hmac::{Hmac, KeyInit, Mac};
use once_cell::sync::Lazy;
use p256::ecdh::diffie_hellman as p256_diffie_hellman;
use p256::ecdsa::signature::{Signer as EcdsaSigner, Verifier as EcdsaVerifier};
use p256::ecdsa::{
    Signature as P256EcdsaSignature, SigningKey as P256EcdsaSigningKey,
    VerifyingKey as P256EcdsaVerifyingKey,
};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey as P256PublicKey, SecretKey as P256SecretKey};
use rsa::pkcs1v15::{
    Signature as RsaPkcs1v15Signature, SigningKey as RsaPkcs1v15SigningKey,
    VerifyingKey as RsaPkcs1v15VerifyingKey,
};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::pss::{
    Signature as RsaPssSignature, SigningKey as RsaPssSigningKey,
    VerifyingKey as RsaPssVerifyingKey,
};
use rsa::sha2::{Sha256 as RsaSha256, Sha384 as RsaSha384, Sha512 as RsaSha512};
use rsa::signature::{
    RandomizedSigner as RsaRandomizedSigner, SignatureEncoding as RsaSignatureEncoding,
};
use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::{BigUint as RsaBigUint, Oaep, RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;
use sha2::{Digest as Sha2Digest, Sha256, Sha384, Sha512};

use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data_mut, is_registered_buffer, mark_as_uint8array, BufferHeader,
    },
    js_object_alloc, js_object_set_field_by_name, js_promise_resolved, JSValue, Promise,
    StringHeader,
};

/// `buffer_data` is private to perry-runtime — open-code the same offset.
#[inline]
unsafe fn buffer_payload(buf: *const BufferHeader) -> *const u8 {
    (buf as *const u8).add(std::mem::size_of::<BufferHeader>())
}

// #854: NaN-boxing tag contract — see CLAUDE.md. `POINTER_TAG`,
// `STRING_TAG`, and `SHORT_STRING_TAG` aren't directly consulted in this
// file but are part of the documented set of tag prefixes; kept for
// reference next to the masks/values that this module does use, so a
// future caller editing here can see the full encoding contract at the
// top of the file.
#[allow(dead_code)]
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
#[allow(dead_code)]
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
#[allow(dead_code)]
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HashAlgo {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum KeyAlgo {
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
enum KeyKind {
    Secret,
    Private,
    Public,
}

#[derive(Copy, Clone, Debug)]
struct CryptoKeyMaterial {
    algo: KeyAlgo,
    /// For HMAC: the underlying hash. For AES-GCM the hash slot is
    /// unused (we keep `HashAlgo::Sha256` as a harmless placeholder so
    /// the struct stays `Copy`).
    hash: HashAlgo,
    kind: KeyKind,
}

static CRYPTO_KEY_REGISTRY: Lazy<Mutex<HashMap<usize, CryptoKeyMaterial>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn register_crypto_key(buf_addr: usize, mat: CryptoKeyMaterial) {
    CRYPTO_KEY_REGISTRY.lock().unwrap().insert(buf_addr, mat);
}

fn lookup_crypto_key(buf_addr: usize) -> Option<CryptoKeyMaterial> {
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
fn strip_ptr(bits: u64) -> usize {
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
unsafe fn bytes_from_jsvalue(bits: u64) -> Vec<u8> {
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
unsafe fn string_from_jsvalue(bits: u64) -> Option<String> {
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

fn parse_hash_alg(name: &str) -> Option<HashAlgo> {
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
unsafe fn extract_hash_algo(bits: u64) -> Option<HashAlgo> {
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
unsafe fn extract_hmac_hash(algo_bits: u64) -> Option<HashAlgo> {
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

unsafe fn extract_algorithm_hash(algo_bits: u64, default: HashAlgo) -> HashAlgo {
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
unsafe fn alloc_uint8array_from_slice(bytes: &[u8]) -> *mut BufferHeader {
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
fn resolve_with_bits(bits: u64) -> *mut Promise {
    js_promise_resolved(f64::from_bits(bits))
}

/// Construct a DOMException-shaped object (`{ name, message, stack: "" }`)
/// and return a rejected Promise carrying it. WebCrypto spec demands
/// `DOMException` instances on subtle.* error paths (`OperationError`,
/// `NotSupportedError`, `InvalidAccessError`, `DataError`, `SyntaxError`),
/// and consumers (`.catch(e => e.name === "...")`) match on `.name` —
/// we model that shape rather than the full DOM `code` lookup table.
/// Issue #1431.
unsafe fn reject_with_dom_exception(name: &str, message: &str) -> *mut Promise {
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
unsafe fn resolve_with_bytes(bytes: &[u8]) -> *mut Promise {
    let buf = alloc_uint8array_from_slice(bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}

unsafe fn resolve_with_bool(b: bool) -> *mut Promise {
    let bits = if b { TAG_TRUE } else { TAG_FALSE };
    resolve_with_bits(bits)
}

fn compute_digest(algo: HashAlgo, data: &[u8]) -> Vec<u8> {
    match algo {
        HashAlgo::Sha1 => Sha1::digest(data).to_vec(),
        HashAlgo::Sha256 => Sha256::digest(data).to_vec(),
        HashAlgo::Sha384 => Sha384::digest(data).to_vec(),
        HashAlgo::Sha512 => Sha512::digest(data).to_vec(),
    }
}

fn compute_hmac(hash: HashAlgo, key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
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

fn generate_p256_signing_key() -> Option<P256EcdsaSigningKey> {
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

fn generate_p256_secret_key() -> Option<P256SecretKey> {
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

fn rsa_oaep_encrypt(hash: HashAlgo, key: &RsaPublicKey, data: &[u8]) -> Option<Vec<u8>> {
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

fn rsa_oaep_decrypt(hash: HashAlgo, key: &RsaPrivateKey, data: &[u8]) -> Option<Vec<u8>> {
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

fn rsa_pkcs1_sign(hash: HashAlgo, key: RsaPrivateKey, data: &[u8]) -> Option<Vec<u8>> {
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

fn rsa_pkcs1_verify(hash: HashAlgo, key: RsaPublicKey, data: &[u8], sig: &[u8]) -> Option<bool> {
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

fn rsa_pss_sign(
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

fn rsa_pss_verify(
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

/// `crypto.subtle.digest(algorithm, data)` → Promise<Uint8Array>
///
/// `algorithm` is "SHA-1" / "SHA-256" / "SHA-384" / "SHA-512" (string)
/// or `{ name: "SHA-256" }`. Unknown algorithms reject with a TypeError.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_digest(algo_bits: f64, data_bits: f64) -> *mut Promise {
    let algo = match extract_hash_algo(algo_bits.to_bits()) {
        Some(a) => a,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let bytes = bytes_from_jsvalue(data_bits.to_bits());
    let digest = compute_digest(algo, &bytes);
    resolve_with_bytes(&digest)
}

/// `crypto.subtle.importKey("raw", keyBytes, algorithm, extractable, keyUsages)`
/// → Promise<CryptoKey>
///
/// `format == "raw"` only. Supported algorithms:
/// - `{ name: "HMAC", hash: "SHA-256" }` (and SHA-1/384/512)
/// - `"AES-GCM"` or `{ name: "AES-GCM" }` — keyed by 128/192/256-bit
///   bytes; the IV / additionalData come in at encrypt/decrypt time.
///
/// `extractable` and `keyUsages` are accepted but not enforced —
/// perry's threat model treats them as documentation. Unsupported
/// shapes resolve to undefined (callers that then pass that into
/// `sign`/`encrypt` will reject there with a clear error).
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_import_key(
    format_bits: f64,
    key_bits: f64,
    algo_bits: f64,
    _extractable_bits: f64,
    _usages_bits: f64,
) -> *mut Promise {
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
    } else if algo_upper == "ECDSA" && (format_lower == "raw" || format_lower == "jwk") {
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve") {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let curve_upper = curve.to_ascii_uppercase();
        if curve_upper != "P-256" && curve_upper != "PRIME256V1" && curve_upper != "SECP256R1" {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let kind =
            if format_lower == "jwk" && object_field_string(key_bits.to_bits(), b"d").is_some() {
                KeyKind::Private
            } else {
                KeyKind::Public
            };
        (KeyAlgo::EcdsaP256, HashAlgo::Sha256, kind)
    } else if algo_upper == "ECDH" && (format_lower == "raw" || format_lower == "jwk") {
        let curve = match object_field_string(algo_bits.to_bits(), b"namedCurve") {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let curve_upper = curve.to_ascii_uppercase();
        if curve_upper != "P-256" && curve_upper != "PRIME256V1" && curve_upper != "SECP256R1" {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        let kind =
            if format_lower == "jwk" && object_field_string(key_bits.to_bits(), b"d").is_some() {
                KeyKind::Private
            } else {
                KeyKind::Public
            };
        (KeyAlgo::EcdhP256, HashAlgo::Sha256, kind)
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

    let key_bytes = if format_lower == "jwk" {
        jwk_import_key_bytes(key_bits.to_bits(), key_algo, kind).unwrap_or_else(|| Vec::new())
    } else {
        bytes_from_jsvalue(key_bits.to_bits())
    };
    if key_bytes.is_empty() && !matches!(key_algo, KeyAlgo::Hkdf | KeyAlgo::Pbkdf2) {
        return reject_with_dom_exception("DataError", "Key data is empty or could not be read");
    }
    if matches!(key_algo, KeyAlgo::EcdsaP256 | KeyAlgo::EcdhP256) {
        let ok = if kind == KeyKind::Public {
            P256PublicKey::from_sec1_bytes(&key_bytes).is_ok()
        } else {
            P256SecretKey::from_slice(&key_bytes).is_ok()
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
        CryptoKeyMaterial {
            algo: key_algo,
            hash,
            kind,
        },
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
    if format_lower == "raw" && mat.kind == KeyKind::Private {
        return reject_with_dom_exception("OperationError", "The operation failed");
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
            let obj = js_object_alloc(0, 2);
            if obj.is_null() {
                return reject_with_dom_exception("OperationError", "The operation failed");
            }
            set_object_string_field(obj, b"kty", "oct");
            set_object_string_field(obj, b"k", &encoded);
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
        if matches!(mat.algo, KeyAlgo::EcdsaP256 | KeyAlgo::EcdhP256) {
            let obj = match ec_p256_jwk_export_object(&key_bytes, mat) {
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

fn b64u_uint(n: &RsaBigUint) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(n.to_bytes_be())
}

fn b64u_decode_uint(s: &str) -> Option<RsaBigUint> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()?;
    Some(RsaBigUint::from_bytes_be(&bytes))
}

unsafe fn jwk_uint_field(obj_bits: u64, name: &[u8]) -> Option<RsaBigUint> {
    let value = object_field_string(obj_bits, name)?;
    b64u_decode_uint(&value)
}

fn rsa_jwk_alg(algo: KeyAlgo, hash: HashAlgo) -> &'static str {
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

unsafe fn jwk_ec_bytes(obj_bits: u64, kind: KeyKind) -> Option<Vec<u8>> {
    let kty = object_field_string(obj_bits, b"kty")?;
    let crv = object_field_string(obj_bits, b"crv")?;
    if kty != "EC" || crv != "P-256" {
        return None;
    }
    if kind == KeyKind::Private {
        let d = object_field_string(obj_bits, b"d")?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(d.as_bytes())
            .ok()?;
        return if bytes.len() == 32 { Some(bytes) } else { None };
    }
    let x = object_field_string(obj_bits, b"x")?;
    let y = object_field_string(obj_bits, b"y")?;
    let x_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(x.as_bytes())
        .ok()?;
    let y_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(y.as_bytes())
        .ok()?;
    if x_bytes.len() != 32 || y_bytes.len() != 32 {
        return None;
    }
    let mut sec1 = Vec::with_capacity(65);
    sec1.push(0x04);
    sec1.extend_from_slice(&x_bytes);
    sec1.extend_from_slice(&y_bytes);
    Some(sec1)
}

unsafe fn jwk_okp_bytes(obj_bits: u64, key_algo: KeyAlgo, kind: KeyKind) -> Option<Vec<u8>> {
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

unsafe fn jwk_import_key_bytes(obj_bits: u64, key_algo: KeyAlgo, kind: KeyKind) -> Option<Vec<u8>> {
    let kty = object_field_string(obj_bits, b"kty")?;
    if matches!(key_algo, KeyAlgo::EcdsaP256 | KeyAlgo::EcdhP256) {
        return jwk_ec_bytes(obj_bits, kind);
    }
    if matches!(key_algo, KeyAlgo::Ed25519 | KeyAlgo::X25519) {
        return jwk_okp_bytes(obj_bits, key_algo, kind);
    }
    if matches!(
        key_algo,
        KeyAlgo::Hmac | KeyAlgo::AesGcm | KeyAlgo::AesKw | KeyAlgo::AesCbc | KeyAlgo::AesCtr
    ) {
        if kty != "oct" {
            return None;
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

unsafe fn rsa_jwk_export_object(
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

unsafe fn okp_jwk_export_object(
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

unsafe fn ec_p256_jwk_export_object(
    key_bytes: &[u8],
    mat: CryptoKeyMaterial,
) -> Option<*mut perry_runtime::ObjectHeader> {
    let (public_bytes, private_d) = if mat.kind == KeyKind::Private {
        let secret = P256SecretKey::from_slice(key_bytes).ok()?;
        let public = secret
            .public_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        (public, Some(key_bytes.to_vec()))
    } else {
        let public = P256PublicKey::from_sec1_bytes(key_bytes).ok()?;
        (public.to_encoded_point(false).as_bytes().to_vec(), None)
    };
    if public_bytes.len() != 65 || public_bytes[0] != 0x04 {
        return None;
    }
    let obj = js_object_alloc(0, if private_d.is_some() { 5 } else { 4 });
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "EC");
    set_object_string_field(obj, b"crv", "P-256");
    set_object_string_field(
        obj,
        b"x",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_bytes[1..33]),
    );
    set_object_string_field(
        obj,
        b"y",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_bytes[33..65]),
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
unsafe fn extract_algo_name(bits: u64) -> Option<String> {
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

/// `crypto.subtle.sign(algorithm, key, data)` → Promise<Uint8Array>
///
/// Supports HMAC and ECDSA/P-256. HMAC reads the hash from the
/// CryptoKey's stored material; ECDSA expects a private P-256 key
/// produced by `generateKey`.
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
        match compute_hmac(mat.hash, &key_bytes, &data_bytes) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if algo_upper == "ECDSA" {
        if mat.algo != KeyAlgo::EcdsaP256 || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
        }
        let signing_key = match P256EcdsaSigningKey::from_slice(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let sig: P256EcdsaSignature = signing_key.sign(&data_bytes);
        sig.to_bytes().as_slice().to_vec()
    } else if algo_upper == "ED25519" {
        if mat.algo != KeyAlgo::Ed25519 || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to sign",
            );
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
        let expected_sig = match compute_hmac(mat.hash, &key_bytes, &data_bytes) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        constant_time_eq(&expected_sig, &provided_sig)
    } else if algo_upper == "ECDSA" {
        if mat.algo != KeyAlgo::EcdsaP256 || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
        }
        let verifying_key = match P256EcdsaVerifyingKey::from_sec1_bytes(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let sig = match P256EcdsaSignature::from_slice(&provided_sig) {
            Ok(s) => s,
            Err(_) => return resolve_with_bool(false),
        };
        verifying_key.verify(&data_bytes, &sig).is_ok()
    } else if algo_upper == "ED25519" {
        if mat.algo != KeyAlgo::Ed25519 || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unable to use this key to verify",
            );
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
unsafe fn extract_hmac_or_hash(bits: u64) -> Option<String> {
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
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn number_from_bits(bits: u64) -> Option<u32> {
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

unsafe fn ecdh_shared_secret_bytes(algo_bits: u64, base_key_bits: u64) -> Option<Vec<u8>> {
    let algo_name = extract_algo_name(algo_bits)?;
    let algo_upper = algo_name.to_ascii_uppercase();
    if algo_upper != "ECDH" && algo_upper != "X25519" {
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
    if public_mat.algo != KeyAlgo::EcdhP256 || base_mat.algo != KeyAlgo::EcdhP256 {
        return None;
    }
    let private_key = P256SecretKey::from_slice(&private_bytes).ok()?;
    let public_key = P256PublicKey::from_sec1_bytes(&public_bytes).ok()?;
    let secret = p256_diffie_hellman(private_key.to_nonzero_scalar(), public_key.as_affine());
    Some(secret.raw_secret_bytes().to_vec())
}

fn hkdf_expand(hash: HashAlgo, ikm: &[u8], salt: &[u8], info: &[u8], out: &mut [u8]) -> bool {
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

fn pbkdf2_derive(hash: HashAlgo, pass: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    match hash {
        HashAlgo::Sha1 => pbkdf2::pbkdf2_hmac::<Sha1>(pass, salt, iterations, out),
        HashAlgo::Sha256 => pbkdf2::pbkdf2_hmac::<Sha256>(pass, salt, iterations, out),
        HashAlgo::Sha384 => pbkdf2::pbkdf2_hmac::<Sha384>(pass, salt, iterations, out),
        HashAlgo::Sha512 => pbkdf2::pbkdf2_hmac::<Sha512>(pass, salt, iterations, out),
    }
}

unsafe fn kdf_derive_bytes(algo_bits: u64, base_key_bits: u64, byte_len: usize) -> Option<Vec<u8>> {
    let algo_name = extract_algo_name(algo_bits)?;
    let algo_upper = algo_name.to_ascii_uppercase();
    let base_key_addr = strip_ptr(base_key_bits);
    let base_mat = lookup_crypto_key(base_key_addr)?;
    if base_mat.kind != KeyKind::Secret {
        return None;
    }
    let base_key = bytes_from_jsvalue(base_key_bits);
    let mut out = vec![0u8; byte_len];
    if algo_upper == "HKDF" {
        if base_mat.algo != KeyAlgo::Hkdf {
            return None;
        }
        let hash = extract_algorithm_hash(algo_bits, base_mat.hash);
        let salt = object_field_bytes(algo_bits, b"salt").unwrap_or_default();
        let info = object_field_bytes(algo_bits, b"info").unwrap_or_default();
        if hkdf_expand(hash, &base_key, &salt, &info, &mut out) {
            return Some(out);
        }
        return None;
    }
    if algo_upper == "PBKDF2" {
        if base_mat.algo != KeyAlgo::Pbkdf2 {
            return None;
        }
        let hash = extract_algorithm_hash(algo_bits, base_mat.hash);
        let salt = object_field_bytes(algo_bits, b"salt").unwrap_or_default();
        let iterations = object_field_number(algo_bits, b"iterations")?;
        if iterations == 0 {
            return None;
        }
        pbkdf2_derive(hash, &base_key, &salt, iterations, &mut out);
        return Some(out);
    }
    None
}

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
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    if bit_len % 8 != 0 {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let byte_len = (bit_len / 8) as usize;
    if let Some(bytes) = kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len) {
        return resolve_with_bytes(&bytes);
    }
    if bit_len > 256 {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
        Some(s) => s,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
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
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
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
        return reject_with_dom_exception("OperationError", "The operation failed");
    };
    if bit_len % 8 != 0 || bit_len == 0 || bit_len > 256 {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let byte_len = (bit_len / 8) as usize;
    let key_bytes = if let Some(bytes) =
        kdf_derive_bytes(algo_bits.to_bits(), base_key_bits.to_bits(), byte_len)
    {
        bytes
    } else {
        let shared = match ecdh_shared_secret_bytes(algo_bits.to_bits(), base_key_bits.to_bits()) {
            Some(s) => s,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        if byte_len > shared.len() {
            return reject_with_dom_exception("OperationError", "The operation failed");
        }
        shared[..byte_len].to_vec()
    };
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
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
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
        let public_key = match RsaPublicKey::from_public_key_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let data = bytes_from_jsvalue(data_bits.to_bits());
        let ciphertext = match rsa_oaep_encrypt(mat.hash, &public_key, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CBC") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (key, iv, data) = match extract_aes_cbc_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let ciphertext = match aes_cbc_encrypt(&key, &iv, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CTR") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (key, counter, length, data) = match extract_aes_ctr_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let ciphertext = match aes_ctr_apply(&key, &counter, length, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if !algo_name.eq_ignore_ascii_case("AES-GCM") {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    if mat.algo != KeyAlgo::AesGcm {
        return reject_with_dom_exception(
            "InvalidAccessError",
            "The requested operation is not valid for the provided key",
        );
    }
    let (key, iv, aad, data) =
        match extract_aes_gcm_args(algo_bits.to_bits(), key_bits.to_bits(), data_bits.to_bits()) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
    let ciphertext = match aes_gcm_encrypt(&key, &iv, &aad, &data) {
        Some(c) => c,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
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
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
        let private_key = match RsaPrivateKey::from_pkcs8_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let data = bytes_from_jsvalue(data_bits.to_bits());
        let plaintext = match rsa_oaep_decrypt(mat.hash, &private_key, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CBC") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (key, iv, data) = match extract_aes_cbc_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let plaintext = match aes_cbc_decrypt(&key, &iv, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CTR") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (key, counter, length, data) = match extract_aes_ctr_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let plaintext = match aes_ctr_apply(&key, &counter, length, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if !algo_name.eq_ignore_ascii_case("AES-GCM") {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    if mat.algo != KeyAlgo::AesGcm {
        return reject_with_dom_exception(
            "InvalidAccessError",
            "The requested operation is not valid for the provided key",
        );
    }
    let (key, iv, aad, data) =
        match extract_aes_gcm_args(algo_bits.to_bits(), key_bits.to_bits(), data_bits.to_bits()) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
    let plaintext = match aes_gcm_decrypt(&key, &iv, &aad, &data) {
        Some(p) => p,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
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

// =====================================================================
// subtle.wrapKey / subtle.unwrapKey
//
// jose reaches for these to ship key material between A256GCMKW
// (AES-GCM wrap) and the symmetric encrypted-payload flow. We
// support two wrap algorithms:
//
//   - `{ name: "AES-KW" }`  (RFC 3394) — the wrappingKey is an
//     AES key (128/192/256-bit); wrapped output is `keyBytes` + 8.
//   - `{ name: "AES-GCM", iv, additionalData? }` — same shape the
//     existing encrypt/decrypt path takes; wrapped output is
//     `ciphertext || tag`.
//
// `format` is currently restricted to `"raw"` — the only format
// jose uses for symmetric keys. JWK / spki / pkcs8 are TODO follow-
// ups (they require an asymmetric algorithm we haven't wired yet).
// =====================================================================

/// AES-KW wrap — RFC 3394. Returns the wrapped key (8 bytes longer
/// than `plaintext_key`). `aes-kw` 0.3 ships
/// `KwAes128/192/256`; we support all three lengths the WebCrypto
/// spec allows for AES-KW.
fn aes_kw_wrap(wrapping_key: &[u8], plaintext_key: &[u8]) -> Option<Vec<u8>> {
    use aes_kw::{KeyInit, KwAes128, KwAes192, KwAes256};
    let mut buf = vec![0u8; plaintext_key.len() + 8];
    match wrapping_key.len() {
        16 => {
            let key_arr: [u8; 16] = wrapping_key.try_into().ok()?;
            let kek = KwAes128::new(&key_arr.into());
            kek.wrap_key(plaintext_key, &mut buf).ok()?;
        }
        24 => {
            let key_arr: [u8; 24] = wrapping_key.try_into().ok()?;
            let kek = KwAes192::new(&key_arr.into());
            kek.wrap_key(plaintext_key, &mut buf).ok()?;
        }
        32 => {
            let key_arr: [u8; 32] = wrapping_key.try_into().ok()?;
            let kek = KwAes256::new(&key_arr.into());
            kek.wrap_key(plaintext_key, &mut buf).ok()?;
        }
        _ => return None,
    }
    Some(buf)
}

/// AES-KW unwrap — RFC 3394.
fn aes_kw_unwrap(wrapping_key: &[u8], wrapped_key: &[u8]) -> Option<Vec<u8>> {
    use aes_kw::{KeyInit, KwAes128, KwAes192, KwAes256};
    if wrapped_key.len() < 8 {
        return None;
    }
    let mut buf = vec![0u8; wrapped_key.len() - 8];
    match wrapping_key.len() {
        16 => {
            let key_arr: [u8; 16] = wrapping_key.try_into().ok()?;
            let kek = KwAes128::new(&key_arr.into());
            kek.unwrap_key(wrapped_key, &mut buf).ok()?;
        }
        24 => {
            let key_arr: [u8; 24] = wrapping_key.try_into().ok()?;
            let kek = KwAes192::new(&key_arr.into());
            kek.unwrap_key(wrapped_key, &mut buf).ok()?;
        }
        32 => {
            let key_arr: [u8; 32] = wrapping_key.try_into().ok()?;
            let kek = KwAes256::new(&key_arr.into());
            kek.unwrap_key(wrapped_key, &mut buf).ok()?;
        }
        _ => return None,
    }
    Some(buf)
}

/// Resolve the AES-GCM IV / AAD pair from a wrap-algorithm object.
/// Returns `None` if the IV is missing (the only mandatory field).
unsafe fn resolve_aes_gcm_iv_aad(algo_bits: u64) -> Option<(Vec<u8>, Vec<u8>)> {
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let aad = object_field_bytes(algo_bits, b"additionalData").unwrap_or_default();
    Some((iv, aad))
}

/// Read the canonical algorithm-name from an algorithm arg (string or
/// `{ name }` object), upper-cased for matching.
unsafe fn wrap_algo_name(algo_bits: u64) -> Option<String> {
    extract_algo_name(algo_bits).map(|s| s.to_ascii_uppercase())
}

/// `crypto.subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm)` →
/// Promise<Uint8Array>
///
/// Supported `format`: `"raw"`. Supported `wrapAlgorithm`:
/// - `{ name: "AES-KW" }` — RFC 3394 (wrappingKey is AES-128/256).
/// - `{ name: "AES-GCM", iv, additionalData? }` — same shape as
///   the existing encrypt path; wrapped output is `ciphertext || tag`.
///
/// Returns a Uint8Array of the wrapped key bytes. Errors (unsupported
/// format, missing IV for AES-GCM, key-length mismatch) resolve to
/// undefined so the caller's `await` rejects with a TypeError.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_wrap_key(
    format_bits: f64,
    key_bits: f64,
    wrapping_key_bits: f64,
    wrap_algo_bits: f64,
) -> *mut Promise {
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to execute 'wrapKey' on 'SubtleCrypto': format must be a string",
            )
        }
    };
    if format != "raw" {
        return reject_with_dom_exception("NotSupportedError", "Unsupported key format");
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    if lookup_crypto_key(key_addr).is_none() {
        return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey");
    }
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let wrapping_key_addr = strip_ptr(wrapping_key_bits.to_bits());
    let wrapping_mat = match lookup_crypto_key(wrapping_key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Wrapping key is not a valid CryptoKey",
            )
        }
    };
    let wrapping_key_bytes = bytes_from_jsvalue(wrapping_key_bits.to_bits());

    let upper = match wrap_algo_name(wrap_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized wrap-algorithm name",
            )
        }
    };
    let wrapped = if upper == "AES-KW" {
        if wrapping_mat.algo != KeyAlgo::AesKw {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        match aes_kw_wrap(&wrapping_key_bytes, &key_bytes) {
            Some(w) => w,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-GCM" {
        if wrapping_mat.algo != KeyAlgo::AesGcm {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (iv, aad) = match resolve_aes_gcm_iv_aad(wrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        match aes_gcm_encrypt(&wrapping_key_bytes, &iv, &aad, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CBC" {
        if wrapping_mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let iv = match object_field_bytes(wrap_algo_bits.to_bits(), b"iv") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        match aes_cbc_encrypt(&wrapping_key_bytes, &iv, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CTR" {
        if wrapping_mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let counter = match object_field_bytes(wrap_algo_bits.to_bits(), b"counter") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        let length = match object_field_number(wrap_algo_bits.to_bits(), b"length") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize wrap algorithm parameters",
                )
            }
        };
        match aes_ctr_apply(&wrapping_key_bytes, &counter, length, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "RSA-OAEP" {
        if wrapping_mat.algo != KeyAlgo::RsaOaep || wrapping_mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let public_key = match RsaPublicKey::from_public_key_der(&wrapping_key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        match rsa_oaep_encrypt(wrapping_mat.hash, &public_key, &key_bytes) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized wrap-algorithm name");
    };
    resolve_with_bytes(&wrapped)
}

/// `crypto.subtle.unwrapKey(format, wrappedKey, unwrappingKey,
///   unwrapAlgorithm, unwrappedKeyAlgorithm, extractable, usages)` →
/// Promise<CryptoKey>
///
/// Inverts `wrapKey`. The recovered raw bytes are wrapped in a fresh
/// Buffer + registered with `unwrappedKeyAlgorithm` (currently
/// AES-GCM) so subsequent `encrypt`/`decrypt` calls find the right
/// `CryptoKeyMaterial`.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_unwrap_key(
    format_bits: f64,
    wrapped_key_bits: f64,
    unwrapping_key_bits: f64,
    unwrap_algo_bits: f64,
    unwrapped_algo_bits: f64,
    _extractable_bits: f64,
    _usages_bits: f64,
) -> *mut Promise {
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "TypeError",
                "Failed to execute 'unwrapKey' on 'SubtleCrypto': format must be a string",
            )
        }
    };
    if format != "raw" {
        return reject_with_dom_exception("NotSupportedError", "Unsupported key format");
    }
    let wrapped_bytes = bytes_from_jsvalue(wrapped_key_bits.to_bits());
    let unwrapping_key_addr = strip_ptr(unwrapping_key_bits.to_bits());
    let unwrapping_mat = match lookup_crypto_key(unwrapping_key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "Unwrapping key is not a valid CryptoKey",
            )
        }
    };
    let unwrapping_key_bytes = bytes_from_jsvalue(unwrapping_key_bits.to_bits());

    let upper = match wrap_algo_name(unwrap_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized unwrap-algorithm name",
            )
        }
    };
    let recovered = if upper == "AES-KW" {
        if unwrapping_mat.algo != KeyAlgo::AesKw {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        match aes_kw_unwrap(&unwrapping_key_bytes, &wrapped_bytes) {
            Some(r) => r,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-GCM" {
        if unwrapping_mat.algo != KeyAlgo::AesGcm {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let (iv, aad) = match resolve_aes_gcm_iv_aad(unwrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        match aes_gcm_decrypt(&unwrapping_key_bytes, &iv, &aad, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CBC" {
        if unwrapping_mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let iv = match object_field_bytes(unwrap_algo_bits.to_bits(), b"iv") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        match aes_cbc_decrypt(&unwrapping_key_bytes, &iv, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "AES-CTR" {
        if unwrapping_mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let counter = match object_field_bytes(unwrap_algo_bits.to_bits(), b"counter") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        let length = match object_field_number(unwrap_algo_bits.to_bits(), b"length") {
            Some(v) => v,
            None => {
                return reject_with_dom_exception(
                    "TypeError",
                    "Failed to normalize unwrap algorithm parameters",
                )
            }
        };
        match aes_ctr_apply(&unwrapping_key_bytes, &counter, length, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else if upper == "RSA-OAEP" {
        if unwrapping_mat.algo != KeyAlgo::RsaOaep || unwrapping_mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        let private_key = match RsaPrivateKey::from_pkcs8_der(&unwrapping_key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        match rsa_oaep_decrypt(unwrapping_mat.hash, &private_key, &wrapped_bytes) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        }
    } else {
        return reject_with_dom_exception(
            "NotSupportedError",
            "Unrecognized unwrap-algorithm name",
        );
    };

    // Register the recovered bytes as a CryptoKey under the requested
    // unwrappedKeyAlgorithm.
    let unwrapped_name = match wrap_algo_name(unwrapped_algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception(
                "NotSupportedError",
                "Unrecognized unwrapped-key algorithm name",
            )
        }
    };
    let key_algo = match unwrapped_name.as_str() {
        "AES-GCM" => KeyAlgo::AesGcm,
        "AES-KW" => KeyAlgo::AesKw,
        "AES-CBC" => KeyAlgo::AesCbc,
        "AES-CTR" => KeyAlgo::AesCtr,
        _ => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    let buf = alloc_uint8array_from_slice(&recovered);
    if buf.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: key_algo,
            hash: HashAlgo::Sha256,
            kind: KeyKind::Secret,
        },
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hash_alg_accepts_canonical_and_aliased_forms() {
        assert_eq!(parse_hash_alg("SHA-256"), Some(HashAlgo::Sha256));
        assert_eq!(parse_hash_alg("sha-256"), Some(HashAlgo::Sha256));
        assert_eq!(parse_hash_alg("SHA256"), Some(HashAlgo::Sha256));
        assert_eq!(parse_hash_alg("SHA-1"), Some(HashAlgo::Sha1));
        assert_eq!(parse_hash_alg("SHA-384"), Some(HashAlgo::Sha384));
        assert_eq!(parse_hash_alg("SHA-512"), Some(HashAlgo::Sha512));
        assert_eq!(parse_hash_alg("MD5"), None);
        assert_eq!(parse_hash_alg(""), None);
    }

    #[test]
    fn aws_sigv4_test_vector() {
        // From the AWS SigV4 documentation:
        //   key = "AWS4" + secret_access_key
        //   k_date = HMAC-SHA-256(key, "20150830")
        // Vector at https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html
        let key = b"AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let date = b"20150830";
        let mac = compute_hmac(HashAlgo::Sha256, key, date).unwrap();
        // Expected k_date from the docs example:
        let expected =
            hex::decode("0138c7a6cbd60aa727b2f653a522567439dfb9f3e72b21f9b25941a42f04a7cd")
                .unwrap();
        assert_eq!(mac, expected);
    }

    #[test]
    fn sha256_test_vector_empty() {
        let digest = compute_digest(HashAlgo::Sha256, b"");
        assert_eq!(
            hex::encode(&digest),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_test_vector_abc() {
        let digest = compute_digest(HashAlgo::Sha256, b"abc");
        assert_eq!(
            hex::encode(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn constant_time_eq_matches() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn aes_gcm_round_trip_128() {
        let key = [0x42u8; 16];
        let iv = [0x11u8; 12];
        let aad = b"context";
        let plaintext = b"hello aes-gcm";
        let ct = aes_gcm_encrypt(&key, &iv, aad, plaintext).expect("encrypt");
        assert_eq!(ct.len(), plaintext.len() + 16); // ciphertext || tag
        let pt = aes_gcm_decrypt(&key, &iv, aad, &ct).expect("decrypt");
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_gcm_round_trip_256_no_aad() {
        let key = [0x37u8; 32];
        let iv = [0x22u8; 12];
        let plaintext = b"";
        let ct = aes_gcm_encrypt(&key, &iv, b"", plaintext).expect("encrypt");
        assert_eq!(ct.len(), 16); // empty payload + tag
        let pt = aes_gcm_decrypt(&key, &iv, b"", &ct).expect("decrypt");
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_gcm_rejects_short_iv() {
        let key = [0u8; 16];
        let iv = [0u8; 8]; // wrong length
        assert!(aes_gcm_encrypt(&key, &iv, b"", b"x").is_none());
        assert!(aes_gcm_decrypt(&key, &iv, b"", b"xxxxxxxxxxxxxxxx").is_none());
    }

    #[test]
    fn aes_gcm_round_trip_192() {
        // Node accepts AES-192-GCM in `crypto.subtle.*` even though the
        // browser WebCrypto spec only lists 128 / 256. We support it via
        // the typed `AesGcm<Aes192, U12>` alias to match Node parity
        // (see `test-parity/node-suite/crypto/webcrypto/aes-gcm-192.ts`).
        // This test was previously asserting rejection of 24-byte keys —
        // now it asserts a clean encrypt + decrypt round-trip instead.
        let key = [0u8; 24];
        let iv = [0u8; 12];
        let aad = b"aad";
        let plaintext = b"the quick brown fox";
        let ciphertext =
            aes_gcm_encrypt(&key, &iv, aad, plaintext).expect("192-bit GCM should encrypt");
        let recovered =
            aes_gcm_decrypt(&key, &iv, aad, &ciphertext).expect("192-bit GCM should decrypt");
        assert_eq!(recovered, plaintext);
    }
}
