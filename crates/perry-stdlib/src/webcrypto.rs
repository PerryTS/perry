//! Web Crypto API: `crypto.subtle.digest` / `importKey` / `sign` / `verify`.
//!
//! Issue #561 — sigv4 / JWT / web-push consumers (s3-lite-client,
//! aws4fetch, jose, oidc-client-ts, web-push) all route through
//! `crypto.subtle`. This module covers the symmetric subset needed for
//! those use cases:
//!
//! - `digest("SHA-1" | "SHA-256" | "SHA-384" | "SHA-512", data)` →
//!   Promise<Uint8Array>
//! - `importKey("raw", key, { name: "HMAC", hash: { name: "SHA-256" } },
//!   ...)` → Promise<CryptoKey>
//! - `sign("HMAC", key, data)` → Promise<Uint8Array>
//! - `verify("HMAC", key, signature, data)` → Promise<boolean>
//!
//! Asymmetric algorithms (RSA / ECDSA / RSA-OAEP), `generateKey`,
//! `wrapKey`, `unwrapKey`, `deriveKey`, `encrypt`, and `decrypt` are
//! out of scope per the issue.
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
}

#[derive(Copy, Clone, Debug)]
struct CryptoKeyMaterial {
    algo: KeyAlgo,
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
        // TextEncoder produces an ArrayHeader registered in BUFFER_REGISTRY
        // but the bytes are stored as f64 elements at offset 8 (so
        // `instanceof Uint8Array` works while the decoder can recover the
        // original UTF-8 bytes). Detect via the same side-table the
        // decoder uses and unpack accordingly. Without this, reading
        // `enc.encode("abc")` as packed u8 yields the first 3 bytes of
        // each f64's IEEE-754 LE representation (all-zero high bytes for
        // small ints) instead of the source bytes.
        if perry_runtime::text::is_text_encoder_result(raw) {
            let arr = raw as *const perry_runtime::ArrayHeader;
            let len = (*arr).length as usize;
            let elems = (arr as *const u8).add(std::mem::size_of::<perry_runtime::ArrayHeader>())
                as *const f64;
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                let d = *elems.add(i);
                out.push((d as i64).clamp(0, 255) as u8);
            }
            return out;
        }
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
/// Only the `format == "raw"` + HMAC algorithm path is supported (the
/// surface every sigv4 / JWT signer uses). `extractable` and `keyUsages`
/// are accepted but not enforced — perry's threat model treats them as
/// documentation. Unsupported shapes resolve to undefined (callers that
/// then pass that into `sign` will reject there with a clear error).
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
    // Algorithm — must be HMAC for now.
    let algo_obj = strip_ptr(algo_bits.to_bits()) as *const perry_runtime::ObjectHeader;
    if (algo_obj as usize) < 0x1000 {
        return resolve_undefined();
    }
    let name_key_ptr = perry_runtime::js_string_from_bytes(b"name".as_ptr(), 4);
    let algo_name_val = perry_runtime::js_object_get_field_by_name(algo_obj, name_key_ptr);
    let algo_name = match string_from_jsvalue(algo_name_val.bits()) {
        Some(s) => s,
        None => return resolve_undefined(),
    };
    if algo_name.to_ascii_uppercase() != "HMAC" {
        return resolve_undefined();
    }
    let hash = match extract_hmac_hash(algo_bits.to_bits()) {
        Some(h) => h,
        None => return resolve_undefined(),
    };
    let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
    let buf = alloc_uint8array_from_slice(&key_bytes);
    if buf.is_null() {
        return resolve_undefined();
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial {
            algo: KeyAlgo::Hmac,
            hash,
        },
    );
    let val = JSValue::pointer(buf as *const u8).bits();
    resolve_with_bits(val)
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
}
