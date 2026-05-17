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
//! AES-CBC, AES-CTR, and RSA-OAEP encrypt/decrypt remain TODO follow-
//! ups. `generateKey`, `wrapKey`, `unwrapKey`, `deriveKey`, and
//! asymmetric signing (RSA / ECDSA) are still out of scope per the
//! issue.
//!
//! `CryptoKey` is represented as a Buffer holding the raw key bytes,
//! with an entry in `CRYPTO_KEY_REGISTRY` recording `(algo, hash)` so
//! `sign` / `verify` can route to the correct primitive.
//!
//! The async aspect is decorative — these primitives are CPU-bound and
//! resolve synchronously inside the returned Promise (the issue's
//! implementation note explicitly calls this out).

use std::collections::HashMap;
use std::sync::Mutex;

use hmac::{Hmac, Mac};
use once_cell::sync::Lazy;
use sha1::Sha1;
use sha2::{Digest as Sha2Digest, Sha256, Sha384, Sha512};

use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data_mut, is_registered_buffer, mark_as_uint8array, BufferHeader,
    },
    js_promise_resolved, JSValue, Promise, StringHeader,
};

/// `buffer_data` is private to perry-runtime — open-code the same offset.
#[inline]
unsafe fn buffer_payload(buf: *const BufferHeader) -> *const u8 {
    (buf as *const u8).add(std::mem::size_of::<BufferHeader>())
}

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
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
    AesGcm,
}

#[derive(Copy, Clone, Debug)]
struct CryptoKeyMaterial {
    algo: KeyAlgo,
    /// For HMAC: the underlying hash. For AES-GCM the hash slot is
    /// unused (we keep `HashAlgo::Sha256` as a harmless placeholder so
    /// the struct stays `Copy`).
    hash: HashAlgo,
}

static CRYPTO_KEY_REGISTRY: Lazy<Mutex<HashMap<usize, CryptoKeyMaterial>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn register_crypto_key(buf_addr: usize, mat: CryptoKeyMaterial) {
    CRYPTO_KEY_REGISTRY.lock().unwrap().insert(buf_addr, mat);
}

fn lookup_crypto_key(buf_addr: usize) -> Option<CryptoKeyMaterial> {
    CRYPTO_KEY_REGISTRY.lock().unwrap().get(&buf_addr).copied()
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

fn resolve_undefined() -> *mut Promise {
    js_promise_resolved(f64::from_bits(0x7FFC_0000_0000_0001))
}

/// Resolve a Promise with a Uint8Array view of `bytes`.
unsafe fn resolve_with_bytes(bytes: &[u8]) -> *mut Promise {
    let buf = alloc_uint8array_from_slice(bytes);
    if buf.is_null() {
        return resolve_undefined();
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
        None => return resolve_undefined(),
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
    // Only "raw" format is supported.
    let format = match string_from_jsvalue(format_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    if format != "raw" {
        return resolve_undefined();
    }
    // Algorithm name — accepts string shorthand ("AES-GCM") or
    // `{ name: "..." }` object form.
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    let (key_algo, hash) = if algo_upper == "HMAC" {
        let hash = match extract_hmac_hash(algo_bits.to_bits()) {
            Some(h) => h,
            None => return resolve_undefined(),
        };
        (KeyAlgo::Hmac, hash)
    } else if algo_upper == "AES-GCM" {
        // AES-GCM: 128, 192, or 256-bit keys. We accept any length
        // here and let encrypt/decrypt fail loudly on mismatch.
        (KeyAlgo::AesGcm, HashAlgo::Sha256)
    } else {
        return resolve_undefined();
    };

    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return resolve_undefined();
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: key_algo,
            hash,
        },
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
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
/// Only `algorithm == "HMAC"` is supported. The hash is read from the
/// CryptoKey's stored material (set at importKey time).
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_sign(
    algo_bits: f64,
    key_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_hmac_or_hash(algo_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    if algo_name.to_ascii_uppercase() != "HMAC" {
        return resolve_undefined();
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => return resolve_undefined(),
    };
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let data_bytes = bytes_from_jsvalue(data_bits.to_bits());
    let sig = match compute_hmac(mat.hash, &key_bytes, &data_bytes) {
        Some(s) => s,
        None => return resolve_undefined(),
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
        None => return resolve_undefined(),
    };
    if algo_name.to_ascii_uppercase() != "HMAC" {
        return resolve_undefined();
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => return resolve_undefined(),
    };
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let data_bytes = bytes_from_jsvalue(data_bits.to_bits());
    let expected_sig = match compute_hmac(mat.hash, &key_bytes, &data_bytes) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    let provided_sig = bytes_from_jsvalue(sig_bits.to_bits());
    let ok = constant_time_eq(&expected_sig, &provided_sig);
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

/// AES-GCM encrypt. Returns ciphertext || tag (matches WebCrypto spec).
fn aes_gcm_encrypt(key: &[u8], iv: &[u8], aad: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};

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
        32 => {
            let cipher = Aes256Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        _ => None, // 192-bit AES-GCM not in the aes-gcm 0.10 type set.
    }
}

/// AES-GCM decrypt. Expects `ciphertext || tag` per the WebCrypto spec.
fn aes_gcm_decrypt(key: &[u8], iv: &[u8], aad: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};

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
        32 => {
            let cipher = Aes256Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        _ => None,
    }
}

/// Shared AES-GCM arg-extraction for encrypt / decrypt: pulls the
/// algorithm-name + iv (+ optional aad) from the algorithm object, plus
/// the raw key bytes (validating they came from an AES-GCM importKey)
/// and the data bytes. Returns `None` if any required piece is missing.
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

/// `crypto.subtle.generateKey(algorithm, extractable, keyUsages)` →
/// Promise<CryptoKey>
///
/// Supported `algorithm` shapes:
/// - `{ name: "AES-GCM", length: 128 | 256 }` — generates a random
///   AES key. (192-bit is rejected: the `aes-gcm` 0.10 crate doesn't
///   ship `Aes192Gcm`, matching the existing encrypt/decrypt path.)
/// - String shorthand `"AES-GCM"` defaults to 256-bit per the WebCrypto
///   convention (jose's `generateSecret('A256GCM')` reaches this).
///
/// Asymmetric algorithms (RSA-OAEP, RSA-PSS, ECDSA, ECDH) and HMAC
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
        None => return resolve_undefined(),
    };
    let algo_upper = algo_name.to_ascii_uppercase();
    if algo_upper != "AES-GCM" {
        // Other algorithms (HMAC, RSA-*, ECDSA, ECDH) are not yet
        // implemented; reject with an undefined-resolved Promise so the
        // caller sees a clear "TypeError" downstream.
        return resolve_undefined();
    }
    // Read `length` from the algorithm object; default to 256 for the
    // string-shorthand form.
    let length = object_field_number(algo_bits.to_bits(), b"length").unwrap_or(256);
    let byte_len = match length {
        128 => 16,
        256 => 32,
        // 192 intentionally rejected — see encrypt/decrypt path above.
        _ => return resolve_undefined(),
    };
    // Pull cryptographically strong random bytes for the key.
    let mut key_bytes = vec![0u8; byte_len];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);

    // Allocate the CryptoKey-shaped buffer + register it as AES-GCM so
    // the importKey/encrypt/decrypt path works on the result.
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return resolve_undefined();
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: KeyAlgo::AesGcm,
            hash: HashAlgo::Sha256,
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
        None => return resolve_undefined(),
    };
    if format != "raw" {
        return resolve_undefined();
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    if lookup_crypto_key(key_addr).is_none() {
        return resolve_undefined();
    }
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let wrapping_key_addr = strip_ptr(wrapping_key_bits.to_bits());
    if lookup_crypto_key(wrapping_key_addr).is_none() {
        return resolve_undefined();
    }
    let wrapping_key_bytes = bytes_from_jsvalue(wrapping_key_bits.to_bits());

    let upper = match wrap_algo_name(wrap_algo_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    let wrapped = if upper == "AES-KW" {
        match aes_kw_wrap(&wrapping_key_bytes, &key_bytes) {
            Some(w) => w,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-GCM" {
        let (iv, aad) = match resolve_aes_gcm_iv_aad(wrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        match aes_gcm_encrypt(&wrapping_key_bytes, &iv, &aad, &key_bytes) {
            Some(c) => c,
            None => return resolve_undefined(),
        }
    } else {
        return resolve_undefined();
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
        None => return resolve_undefined(),
    };
    if format != "raw" {
        return resolve_undefined();
    }
    let wrapped_bytes = bytes_from_jsvalue(wrapped_key_bits.to_bits());
    let unwrapping_key_addr = strip_ptr(unwrapping_key_bits.to_bits());
    if lookup_crypto_key(unwrapping_key_addr).is_none() {
        return resolve_undefined();
    }
    let unwrapping_key_bytes = bytes_from_jsvalue(unwrapping_key_bits.to_bits());

    let upper = match wrap_algo_name(unwrap_algo_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    let recovered = if upper == "AES-KW" {
        match aes_kw_unwrap(&unwrapping_key_bytes, &wrapped_bytes) {
            Some(r) => r,
            None => return resolve_undefined(),
        }
    } else if upper == "AES-GCM" {
        let (iv, aad) = match resolve_aes_gcm_iv_aad(unwrap_algo_bits.to_bits()) {
            Some(t) => t,
            None => return resolve_undefined(),
        };
        match aes_gcm_decrypt(&unwrapping_key_bytes, &iv, &aad, &wrapped_bytes) {
            Some(p) => p,
            None => return resolve_undefined(),
        }
    } else {
        return resolve_undefined();
    };

    // Register the recovered bytes as a CryptoKey under the
    // unwrappedKeyAlgorithm. Only AES-GCM is honored today — HMAC
    // and others would need their hash-from-algo extraction wired
    // through here (TODO follow-up; jose only round-trips AES-GCM
    // through wrap/unwrap).
    let unwrapped_name = match wrap_algo_name(unwrapped_algo_bits.to_bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    let key_algo = if unwrapped_name == "AES-GCM" {
        KeyAlgo::AesGcm
    } else {
        return resolve_undefined();
    };
    let buf = alloc_uint8array_from_slice(&recovered);
    if buf.is_null() {
        return resolve_undefined();
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: key_algo,
            hash: HashAlgo::Sha256,
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
    fn aes_gcm_rejects_192_bit_key() {
        // 192-bit AES-GCM is intentionally not in the aes-gcm 0.10
        // type set; document the rejection so we notice if it changes.
        let key = [0u8; 24];
        let iv = [0u8; 12];
        assert!(aes_gcm_encrypt(&key, &iv, b"", b"x").is_none());
    }
}
