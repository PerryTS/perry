//! Crypto module
//!
//! Native implementation of Node.js crypto module functions.
//! Provides hashing (sha256, md5), random byte generation, AES encryption,
//! and key derivation (pbkdf2, scrypt).

use crate::common::handle::{get_handle_mut, register_handle, Handle};
use aes::{Aes128, Aes192, Aes256};
use base64::Engine as _;
use cbc::{
    cipher::{
        block_padding::{NoPadding, Pkcs7},
        BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit,
    },
    Decryptor, Encryptor,
};
use hkdf::Hkdf;
use md5::{Digest as Md5Digest, Md5};
use p256::ecdh::diffie_hellman as p256_diffie_hellman;
use p256::ecdsa::{Signature as P256EcdsaSignature, SigningKey as P256EcdsaSigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::{
    EncodePrivateKey as P256EncodePrivateKey, EncodePublicKey as P256EncodePublicKey,
};
use p256::{PublicKey as P256PublicKey, SecretKey as P256SecretKey};
use perry_runtime::{
    js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name,
    js_string_from_bytes, JSValue, ObjectHeader, StringHeader,
};
use rand::{Rng, RngCore};
use rsa::pkcs1v15::{Pkcs1v15Sign, Signature as RsaPkcs1v15Signature, SigningKey, VerifyingKey};
use rsa::pss::{
    Signature as RsaPssSignature, SigningKey as RsaPssSigningKey,
    VerifyingKey as RsaPssVerifyingKey,
};
use rsa::sha2::{Sha256 as RsaSha256, Sha384 as RsaSha384, Sha512 as RsaSha512};
use rsa::signature::{RandomizedSigner, SignatureEncoding, Signer, Verifier};
use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::Oaep;
use rsa::{BigUint as RsaBigUint, RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;
use sha2::{Digest as Sha256Digest, Sha224, Sha256, Sha384, Sha512, Sha512_256};
use sha3::{
    digest::{ExtendableOutput, XofReader},
    Shake128, Shake256,
};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(bytes.to_vec())
}

/// Extract the raw bytes from a pointer that might be a Buffer, a
/// StringHeader, or anything that uses the `[u32 byte-length prefix][bytes]`
/// layout. StringHeader has `utf16_len` at offset 0 and `byte_len` at
/// offset 4; BufferHeader has `length` at offset 0 and `capacity` at
/// offset 4. Both have the payload bytes immediately after the 8-byte
/// header, and both store the byte count (in UTF-8 / as raw bytes) in
/// the same u32 slot for our purposes — but we pick the correct field
/// based on whether the pointer is a registered Buffer.
unsafe fn bytes_from_ptr(ptr: i64) -> Vec<u8> {
    let addr = ptr as usize;
    if addr < 0x1000 {
        return Vec::new();
    }
    if perry_runtime::buffer::is_registered_buffer(addr) {
        let buf = ptr as *const perry_runtime::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data =
            (buf as *const u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>());
        return std::slice::from_raw_parts(data, len).to_vec();
    }
    // Fall back to StringHeader layout — the common case for literal
    // strings passed to crypto functions.
    let hdr = ptr as *const StringHeader;
    let len = (*hdr).byte_len as usize;
    let data = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
    std::slice::from_raw_parts(data, len).to_vec()
}

/// Allocate a new Buffer, copy `bytes` into it, return the registered pointer.
unsafe fn alloc_buffer_from_slice(bytes: &[u8]) -> *mut perry_runtime::buffer::BufferHeader {
    let buf = perry_runtime::buffer::buffer_alloc(bytes.len() as u32);
    if buf.is_null() {
        return buf;
    }
    (*buf).length = bytes.len() as u32;
    let dst = perry_runtime::buffer::buffer_data_mut(buf);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    buf
}

#[derive(Clone, Copy)]
enum RsaDigestKind {
    Sha256,
    Sha384,
    Sha512,
}

fn normalize_sign_algorithm(algorithm: &[u8]) -> Option<RsaDigestKind> {
    let alg = std::str::from_utf8(algorithm)
        .unwrap_or("")
        .to_ascii_lowercase();
    match alg.as_str() {
        "rsa-sha256" | "sha256" | "sha256withrsaencryption" => Some(RsaDigestKind::Sha256),
        "rsa-sha384" | "sha384" | "sha384withrsaencryption" => Some(RsaDigestKind::Sha384),
        "rsa-sha512" | "sha512" | "sha512withrsaencryption" => Some(RsaDigestKind::Sha512),
        _ => None,
    }
}

fn parse_rsa_private_key_pem(pem: &str) -> Option<RsaPrivateKey> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .ok()
}

fn parse_rsa_public_key_pem(pem: &str) -> Option<RsaPublicKey> {
    use rsa::pkcs1::DecodeRsaPublicKey;
    use rsa::pkcs8::DecodePublicKey;

    RsaPublicKey::from_public_key_pem(pem)
        .or_else(|_| RsaPublicKey::from_pkcs1_pem(pem))
        .ok()
        .or_else(|| parse_rsa_private_key_pem(pem).map(RsaPublicKey::from))
}

fn rsa_public_key_to_pem(public_key: &RsaPublicKey) -> Option<String> {
    use rsa::pkcs1::EncodeRsaPublicKey;
    use rsa::pkcs8::EncodePublicKey;

    public_key
        .to_public_key_pem(Default::default())
        .ok()
        .or_else(|| public_key.to_pkcs1_pem(Default::default()).ok())
        .map(|pem| pem.to_string())
}

fn parse_p256_signing_key_pem(pem: &str) -> Option<P256EcdsaSigningKey> {
    use p256::pkcs8::DecodePrivateKey;

    P256EcdsaSigningKey::from_pkcs8_pem(pem).ok()
}

fn parse_p256_verifying_key_pem(pem: &str) -> Option<p256::ecdsa::VerifyingKey> {
    use p256::pkcs8::{DecodePrivateKey, DecodePublicKey};

    p256::ecdsa::VerifyingKey::from_public_key_pem(pem)
        .ok()
        .or_else(|| {
            P256EcdsaSigningKey::from_pkcs8_pem(pem)
                .ok()
                .map(|key| *key.verifying_key())
        })
}

const ED25519_PRIVATE_PREFIX: &str = "PERRY-ED25519-PRIVATE:";
const ED25519_PUBLIC_PREFIX: &str = "PERRY-ED25519-PUBLIC:";
const X25519_PRIVATE_PREFIX: &str = "PERRY-X25519-PRIVATE:";
const X25519_PUBLIC_PREFIX: &str = "PERRY-X25519-PUBLIC:";

fn ed25519_private_surrogate(key: &ed25519_dalek::SigningKey) -> String {
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.to_bytes());
    let public =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes());
    format!("{ED25519_PRIVATE_PREFIX}{secret}.{public}")
}

fn ed25519_public_surrogate(key: &ed25519_dalek::VerifyingKey) -> String {
    let public = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.to_bytes());
    format!("{ED25519_PUBLIC_PREFIX}{public}")
}

fn parse_ed25519_private_surrogate(value: &str) -> Option<ed25519_dalek::SigningKey> {
    let rest = value.strip_prefix(ED25519_PRIVATE_PREFIX)?;
    let secret_b64 = rest.split('.').next()?;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(secret_b64.as_bytes())
        .ok()?;
    let secret: [u8; 32] = secret.as_slice().try_into().ok()?;
    Some(ed25519_dalek::SigningKey::from_bytes(&secret))
}

fn parse_ed25519_public_surrogate(value: &str) -> Option<ed25519_dalek::VerifyingKey> {
    if let Some(private) = parse_ed25519_private_surrogate(value) {
        return Some(private.verifying_key());
    }
    let rest = value.strip_prefix(ED25519_PUBLIC_PREFIX)?;
    let public = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(rest.as_bytes())
        .ok()?;
    let public: [u8; 32] = public.as_slice().try_into().ok()?;
    ed25519_dalek::VerifyingKey::from_bytes(&public).ok()
}

fn x25519_private_surrogate(secret: &[u8; 32]) -> String {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret);
    format!("{X25519_PRIVATE_PREFIX}{encoded}")
}

fn x25519_public_surrogate(public: &[u8; 32]) -> String {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public);
    format!("{X25519_PUBLIC_PREFIX}{encoded}")
}

fn parse_x25519_private_surrogate(value: &str) -> Option<[u8; 32]> {
    let rest = value.strip_prefix(X25519_PRIVATE_PREFIX)?;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(rest.as_bytes())
        .ok()?;
    secret.as_slice().try_into().ok()
}

fn parse_x25519_public_surrogate(value: &str) -> Option<[u8; 32]> {
    if let Some(secret) = parse_x25519_private_surrogate(value) {
        let secret = x25519_dalek::StaticSecret::from(secret);
        let public = x25519_dalek::PublicKey::from(&secret);
        return Some(public.to_bytes());
    }
    let rest = value.strip_prefix(X25519_PUBLIC_PREFIX)?;
    let public = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(rest.as_bytes())
        .ok()?;
    public.as_slice().try_into().ok()
}

fn js_bool(b: bool) -> f64 {
    f64::from_bits(JSValue::bool(b).bits())
}

fn js_truthy(v: f64) -> bool {
    let js = JSValue::from_bits(v.to_bits());
    if js.is_bool() {
        return js.as_bool();
    }
    if js.is_undefined() || js.is_null() {
        return false;
    }
    if js.is_number() {
        return v != 0.0 && !v.is_nan();
    }
    true
}

fn nanbox_str(ptr: *mut StringHeader) -> f64 {
    f64::from_bits(0x7FFF_0000_0000_0000u64 | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
}

unsafe fn mark_keyobject_string(ptr: *mut StringHeader, kind: KeyKind, asym_type: u8) {
    if ptr.is_null() {
        return;
    }
    let kind_id = match kind {
        KeyKind::Public => 1,
        KeyKind::Private => 2,
    };
    perry_runtime::buffer::mark_as_asymmetric_key(ptr as usize, kind_id, asym_type);
}

#[derive(Clone, Copy)]
enum KeyKind {
    Private,
    Public,
}

fn classify_private_key_surrogate(pem: &str) -> Option<u8> {
    if parse_rsa_private_key_pem(pem).is_some() {
        Some(1)
    } else if parse_p256_signing_key_pem(pem).is_some() {
        Some(2)
    } else if parse_ed25519_private_surrogate(pem).is_some() {
        Some(3)
    } else if parse_x25519_private_surrogate(pem).is_some() {
        Some(4)
    } else {
        None
    }
}

fn classify_public_key_surrogate(pem: &str) -> Option<u8> {
    if parse_rsa_public_key_pem(pem).is_some() {
        Some(1)
    } else if parse_p256_verifying_key_pem(pem).is_some() {
        Some(2)
    } else if parse_ed25519_public_surrogate(pem).is_some() {
        Some(3)
    } else if parse_x25519_public_surrogate(pem).is_some() {
        Some(4)
    } else {
        None
    }
}

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
    let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
    if (raw as usize) < 0x1000 {
        return None;
    }
    let len = (*raw).byte_len as usize;
    let data = (raw as *const u8).add(std::mem::size_of::<StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(str::to_string)
}

unsafe fn object_field_bits(obj_bits: u64, name: &[u8]) -> Option<u64> {
    if (obj_bits >> 48) as u16 != 0x7FFD {
        return None;
    }
    let obj_ptr = (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = js_object_get_field_by_name(obj_ptr, key_ptr);
    let bits = val.bits();
    if (bits >> 48) as u16 == 0x7FFC {
        None
    } else {
        Some(bits)
    }
}

unsafe fn object_field_string(obj_bits: u64, name: &[u8]) -> Option<String> {
    string_from_jsvalue(object_field_bits(obj_bits, name)?)
}

fn b64u_decode_uint(s: &str) -> Option<RsaBigUint> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()?;
    Some(RsaBigUint::from_bytes_be(&bytes))
}

fn b64u_uint(value: &RsaBigUint) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_bytes_be())
}

unsafe fn jwk_uint_field(obj_bits: u64, name: &[u8]) -> Option<RsaBigUint> {
    b64u_decode_uint(&object_field_string(obj_bits, name)?)
}

unsafe fn set_object_string_field(obj: *mut ObjectHeader, name: &[u8], value: &str) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_object_set_field_by_name(obj, key, nanbox_str(val));
}

unsafe fn set_object_value_field(obj: *mut ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

fn nanbox_pointer(ptr: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

unsafe fn rsa_public_jwk_object(public_key: &RsaPublicKey) -> Option<*mut ObjectHeader> {
    let obj = js_object_alloc(0, 3);
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "RSA");
    set_object_string_field(obj, b"n", &b64u_uint(public_key.n()));
    set_object_string_field(obj, b"e", &b64u_uint(public_key.e()));
    Some(obj)
}

unsafe fn rsa_private_jwk_object(private_key: &RsaPrivateKey) -> Option<*mut ObjectHeader> {
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
    let obj = js_object_alloc(0, 9);
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "RSA");
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

unsafe fn ec_p256_public_jwk_object(public_key: &P256PublicKey) -> Option<*mut ObjectHeader> {
    let point = public_key.to_encoded_point(false);
    let bytes = point.as_bytes();
    if bytes.len() != 65 || bytes[0] != 0x04 {
        return None;
    }
    let obj = js_object_alloc(0, 4);
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "EC");
    set_object_string_field(obj, b"crv", "P-256");
    set_object_string_field(
        obj,
        b"x",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes[1..33]),
    );
    set_object_string_field(
        obj,
        b"y",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes[33..65]),
    );
    Some(obj)
}

unsafe fn ec_p256_private_jwk_object(private_key: &P256SecretKey) -> Option<*mut ObjectHeader> {
    let public = private_key.public_key();
    let obj = ec_p256_public_jwk_object(&public)?;
    let d = private_key.to_bytes();
    set_object_string_field(
        obj,
        b"d",
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(d.as_slice()),
    );
    Some(obj)
}

unsafe fn jwk_rsa_private_to_pem(jwk_bits: u64) -> Option<String> {
    use rsa::pkcs8::EncodePrivateKey;
    let kty = object_field_string(jwk_bits, b"kty")?;
    if kty != "RSA" {
        return None;
    }
    let n = jwk_uint_field(jwk_bits, b"n")?;
    let e = jwk_uint_field(jwk_bits, b"e")?;
    let d = jwk_uint_field(jwk_bits, b"d")?;
    let p = jwk_uint_field(jwk_bits, b"p")?;
    let q = jwk_uint_field(jwk_bits, b"q")?;
    let key = RsaPrivateKey::from_components(n, e, d, vec![p, q]).ok()?;
    key.to_pkcs8_pem(Default::default())
        .ok()
        .map(|pem| pem.to_string())
}

unsafe fn jwk_rsa_public_to_pem(jwk_bits: u64) -> Option<String> {
    let kty = object_field_string(jwk_bits, b"kty")?;
    if kty != "RSA" {
        return None;
    }
    let n = jwk_uint_field(jwk_bits, b"n")?;
    let e = jwk_uint_field(jwk_bits, b"e")?;
    let key = RsaPublicKey::new(n, e).ok()?;
    rsa_public_key_to_pem(&key)
}

unsafe fn jwk_ec_private_to_pem(jwk_bits: u64) -> Option<String> {
    let kty = object_field_string(jwk_bits, b"kty")?;
    let crv = object_field_string(jwk_bits, b"crv")?;
    if kty != "EC" || crv != "P-256" {
        return None;
    }
    let d = object_field_string(jwk_bits, b"d")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(d.as_bytes())
        .ok()?;
    let key = P256SecretKey::from_slice(&bytes).ok()?;
    key.to_pkcs8_pem(Default::default())
        .ok()
        .map(|pem| pem.to_string())
}

unsafe fn jwk_ec_public_to_pem(jwk_bits: u64) -> Option<String> {
    let kty = object_field_string(jwk_bits, b"kty")?;
    let crv = object_field_string(jwk_bits, b"crv")?;
    if kty != "EC" || crv != "P-256" {
        return None;
    }
    let x = object_field_string(jwk_bits, b"x")?;
    let y = object_field_string(jwk_bits, b"y")?;
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
    let public = P256PublicKey::from_sec1_bytes(&sec1).ok()?;
    public.to_public_key_pem(Default::default()).ok()
}

unsafe fn crypto_key_input_to_private_pem(value_bits: u64) -> Option<String> {
    let format = object_field_string(value_bits, b"format");
    if let Some(key_bits) = object_field_bits(value_bits, b"key") {
        if matches!(format.as_deref(), Some(f) if f.eq_ignore_ascii_case("jwk")) {
            return jwk_rsa_private_to_pem(key_bits).or_else(|| jwk_ec_private_to_pem(key_bits));
        }
        return crypto_key_input_to_private_pem(key_bits);
    }
    if matches!(format.as_deref(), Some(f) if f.eq_ignore_ascii_case("jwk")) {
        return jwk_rsa_private_to_pem(value_bits).or_else(|| jwk_ec_private_to_pem(value_bits));
    }
    let ptr = (value_bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    String::from_utf8(bytes_from_ptr(ptr)).ok()
}

unsafe fn crypto_key_input_to_public_pem(value_bits: u64) -> Option<String> {
    let format = object_field_string(value_bits, b"format");
    if let Some(key_bits) = object_field_bits(value_bits, b"key") {
        if matches!(format.as_deref(), Some(f) if f.eq_ignore_ascii_case("jwk")) {
            if object_field_string(key_bits, b"d").is_some() {
                if let Some(private_pem) = jwk_rsa_private_to_pem(key_bits) {
                    return parse_rsa_private_key_pem(&private_pem)
                        .and_then(|k| rsa_public_key_to_pem(&RsaPublicKey::from(&k)));
                }
                if let Some(private_pem) = jwk_ec_private_to_pem(key_bits) {
                    if let Some(v) = parse_p256_verifying_key_pem(&private_pem) {
                        return v.to_public_key_pem(Default::default()).ok();
                    }
                }
            }
            return jwk_rsa_public_to_pem(key_bits).or_else(|| jwk_ec_public_to_pem(key_bits));
        }
        return crypto_key_input_to_public_pem(key_bits);
    }
    if matches!(format.as_deref(), Some(f) if f.eq_ignore_ascii_case("jwk")) {
        if object_field_string(value_bits, b"d").is_some() {
            if let Some(private_pem) = jwk_rsa_private_to_pem(value_bits) {
                return parse_rsa_private_key_pem(&private_pem)
                    .and_then(|k| rsa_public_key_to_pem(&RsaPublicKey::from(&k)));
            }
            if let Some(private_pem) = jwk_ec_private_to_pem(value_bits) {
                if let Some(v) = parse_p256_verifying_key_pem(&private_pem) {
                    return v.to_public_key_pem(Default::default()).ok();
                }
            }
        }
        return jwk_rsa_public_to_pem(value_bits).or_else(|| jwk_ec_public_to_pem(value_bits));
    }
    let ptr = (value_bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    let pem = String::from_utf8(bytes_from_ptr(ptr)).ok()?;
    if let Some(v) = parse_p256_verifying_key_pem(&pem) {
        return v.to_public_key_pem(Default::default()).ok();
    }
    if let Some(v) = parse_ed25519_public_surrogate(&pem) {
        return Some(ed25519_public_surrogate(&v));
    }
    if let Some(v) = parse_x25519_public_surrogate(&pem) {
        return Some(x25519_public_surrogate(&v));
    }
    parse_rsa_public_key_pem(&pem).and_then(|key| rsa_public_key_to_pem(&key))
}

unsafe fn key_input_uses_ieee_p1363(value_bits: u64) -> bool {
    matches!(
        object_field_string(value_bits, b"dsaEncoding").as_deref(),
        Some(enc) if enc.eq_ignore_ascii_case("ieee-p1363")
    )
}

unsafe fn key_input_uses_rsa_pss(value_bits: u64) -> bool {
    matches!(object_field_bits(value_bits, b"padding"), Some(v) if f64::from_bits(v) as i32 == 6)
}

unsafe fn key_input_pss_salt_len(value_bits: u64, alg: RsaDigestKind) -> usize {
    if let Some(v) = object_field_bits(value_bits, b"saltLength") {
        let n = f64::from_bits(v) as i32;
        if n > 0 {
            return n as usize;
        }
    }
    match alg {
        RsaDigestKind::Sha256 => 32,
        RsaDigestKind::Sha384 => 48,
        RsaDigestKind::Sha512 => 64,
    }
}

unsafe fn keygen_encoding_wants_jwk(options_bits: u64, field: &[u8]) -> bool {
    let Some(encoding_bits) = object_field_bits(options_bits, field) else {
        return false;
    };
    matches!(
        object_field_string(encoding_bits, b"format").as_deref(),
        Some(format) if format.eq_ignore_ascii_case("jwk")
    )
}

fn sign_rsa_data(alg: RsaDigestKind, private_key: RsaPrivateKey, data: &[u8]) -> Vec<u8> {
    match alg {
        RsaDigestKind::Sha256 => SigningKey::<RsaSha256>::new(private_key)
            .sign(data)
            .to_bytes()
            .to_vec(),
        RsaDigestKind::Sha384 => SigningKey::<RsaSha384>::new(private_key)
            .sign(data)
            .to_bytes()
            .to_vec(),
        RsaDigestKind::Sha512 => SigningKey::<RsaSha512>::new(private_key)
            .sign(data)
            .to_bytes()
            .to_vec(),
    }
}

fn sign_rsa_pss_data(
    alg: RsaDigestKind,
    private_key: RsaPrivateKey,
    data: &[u8],
    salt_len: usize,
) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    match alg {
        RsaDigestKind::Sha256 => {
            RsaPssSigningKey::<RsaSha256>::new_with_salt_len(private_key, salt_len)
                .sign_with_rng(&mut rng, data)
                .to_bytes()
                .to_vec()
        }
        RsaDigestKind::Sha384 => {
            RsaPssSigningKey::<RsaSha384>::new_with_salt_len(private_key, salt_len)
                .sign_with_rng(&mut rng, data)
                .to_bytes()
                .to_vec()
        }
        RsaDigestKind::Sha512 => {
            RsaPssSigningKey::<RsaSha512>::new_with_salt_len(private_key, salt_len)
                .sign_with_rng(&mut rng, data)
                .to_bytes()
                .to_vec()
        }
    }
}

fn verify_rsa_data(
    alg: RsaDigestKind,
    public_key: RsaPublicKey,
    data: &[u8],
    signature: &RsaPkcs1v15Signature,
) -> bool {
    match alg {
        RsaDigestKind::Sha256 => VerifyingKey::<RsaSha256>::new(public_key)
            .verify(data, signature)
            .is_ok(),
        RsaDigestKind::Sha384 => VerifyingKey::<RsaSha384>::new(public_key)
            .verify(data, signature)
            .is_ok(),
        RsaDigestKind::Sha512 => VerifyingKey::<RsaSha512>::new(public_key)
            .verify(data, signature)
            .is_ok(),
    }
}

fn verify_rsa_pss_data(
    alg: RsaDigestKind,
    public_key: RsaPublicKey,
    data: &[u8],
    signature: &RsaPssSignature,
    salt_len: usize,
) -> bool {
    match alg {
        RsaDigestKind::Sha256 => {
            RsaPssVerifyingKey::<RsaSha256>::new_with_salt_len(public_key, salt_len)
                .verify(data, signature)
                .is_ok()
        }
        RsaDigestKind::Sha384 => {
            RsaPssVerifyingKey::<RsaSha384>::new_with_salt_len(public_key, salt_len)
                .verify(data, signature)
                .is_ok()
        }
        RsaDigestKind::Sha512 => {
            RsaPssVerifyingKey::<RsaSha512>::new_with_salt_len(public_key, salt_len)
                .verify(data, signature)
                .is_ok()
        }
    }
}

fn rsa_public_unpad_pkcs1_type1(public_key: &RsaPublicKey, encrypted: &[u8]) -> Option<Vec<u8>> {
    let sig = RsaBigUint::from_bytes_be(encrypted);
    if &sig >= public_key.n() || encrypted.len() != public_key.size() {
        return None;
    }
    let m = sig.modpow(public_key.e(), public_key.n());
    let mut em = m.to_bytes_be();
    if em.len() > public_key.size() {
        return None;
    }
    if em.len() < public_key.size() {
        let mut padded = vec![0u8; public_key.size() - em.len()];
        padded.extend_from_slice(&em);
        em = padded;
    }
    if em.len() < 11 || em[0] != 0 || em[1] != 1 {
        return None;
    }
    let mut idx = 2;
    while idx < em.len() && em[idx] == 0xff {
        idx += 1;
    }
    if idx < 10 || idx >= em.len() || em[idx] != 0 {
        return None;
    }
    Some(em[idx + 1..].to_vec())
}

unsafe fn string_array(items: &[&str]) -> *mut perry_runtime::array::ArrayHeader {
    let mut arr = perry_runtime::js_array_alloc(items.len() as u32);
    for item in items {
        let s = js_string_from_bytes(item.as_ptr(), item.len() as u32);
        arr = perry_runtime::js_array_push(arr, JSValue::string_ptr(s));
    }
    arr
}

/// Create SHA256 hash of data
/// crypto.createHash('sha256').update(data).digest('hex') -> string
#[no_mangle]
pub unsafe extern "C" fn js_crypto_sha256(data_ptr: *const StringHeader) -> *mut StringHeader {
    let data = match string_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex_str = hex::encode(result);

    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// SHA256 over arbitrary bytes. Input can be a Buffer or a string (both
/// share the same `[u32 len][u32 cap_or_utf16_len][bytes...]` header
/// layout up to the data pointer offset). Output is a Buffer holding the
/// 32-byte digest. Used by `.digest()` (no arg) — the SCRAM path in
/// `@perry/postgres` relies on this.
///
/// Pointer is passed as `i64` so the codegen can feed either a NaN-unboxed
/// Buffer handle or a StringHeader pointer through the same FFI slot.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_sha256_bytes(
    data_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let bytes = bytes_from_ptr(data_ptr);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    alloc_buffer_from_slice(&digest)
}

/// Verify an Ed25519 signature.
///
/// `msg_ptr`, `sig_ptr`, `pk_ptr` are i64 NaN-unboxed pointers that may point at
/// either a Buffer or a StringHeader (we read raw bytes from either layout).
/// Used by the auto-updater to verify the signature on the SHA-256 digest of a
/// downloaded binary against the developer's public key.
///
/// Signature must be exactly 64 bytes; public key must be exactly 32 bytes.
/// Returns 1 on valid signature, 0 on any error (size mismatch, malformed key,
/// signature mismatch).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_ed25519_verify(msg_ptr: i64, sig_ptr: i64, pk_ptr: i64) -> i32 {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let msg = bytes_from_ptr(msg_ptr);
    let sig_bytes = bytes_from_ptr(sig_ptr);
    let pk_bytes = bytes_from_ptr(pk_ptr);

    if sig_bytes.len() != 64 || pk_bytes.len() != 32 {
        return 0;
    }

    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = Signature::from_bytes(&sig_arr);

    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&pk_bytes);
    let verifying_key = match VerifyingKey::from_bytes(&pk_arr) {
        Ok(k) => k,
        Err(_) => return 0,
    };

    match verifying_key.verify(&msg, &signature) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Create MD5 hash of data
/// crypto.createHash('md5').update(data).digest('hex') -> string
#[no_mangle]
pub unsafe extern "C" fn js_crypto_md5(data_ptr: *const StringHeader) -> *mut StringHeader {
    let data = match string_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut hasher = Md5::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex_str = hex::encode(result);

    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// `crypto.createSecretKey(key, encoding?)` — produce a key Buffer
/// that jose / jsonwebtoken / etc. can use as an HS* signing key.
///
/// In Node's surface this returns a `KeyObject`, but for the V8-fallback
/// JWT path that's where Perry's JS shim lives. From native Perry the
/// shortest correct value is a `BufferHeader` of the raw key bytes,
/// marked as a `Uint8Array` so that:
///   - jose's `key instanceof Uint8Array` check passes after the native
///     -> V8 marshal turns the BufferHeader into a real `v8::Uint8Array`
///     (`bridge.rs:native_object_to_v8`),
///   - `instanceof KeyObject` is not required for HS* algorithms (jose
///     accepts Uint8Array directly per `getSignVerifyKey`).
///
/// `key_ptr` may point at a Buffer (already bytes) or a StringHeader
/// (utf8 string literal). The `encoding` arg is accepted for API parity
/// but only utf8/utf-8 is honored today; anything else is treated as
/// utf8 (so `'secret'` and `'secret', 'utf8'` produce identical bytes).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_secret_key(
    key_ptr: i64,
    encoding_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let raw = bytes_from_ptr(key_ptr);
    let encoding = if encoding_ptr >= 0x1000 {
        String::from_utf8(bytes_from_ptr(encoding_ptr))
            .unwrap_or_default()
            .to_ascii_lowercase()
    } else {
        String::new()
    };
    // Node throws on malformed encodings here; matching that exactly
    // requires plumbing js_throw through the C ABI call site, so we
    // surface failure as a null buffer (which the codegen path nanboxes
    // to a NULL POINTER_TAG) instead of silently producing nonsense key
    // bytes from an invalid hex/base64 input.
    let bytes = match encoding.as_str() {
        "hex" => match hex::decode(&raw) {
            Ok(b) => b,
            Err(_) => return std::ptr::null_mut(),
        },
        "base64" => match base64::engine::general_purpose::STANDARD.decode(&raw) {
            Ok(b) => b,
            Err(_) => return std::ptr::null_mut(),
        },
        "base64url" => match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&raw) {
            Ok(b) => b,
            Err(_) => return std::ptr::null_mut(),
        },
        _ => raw,
    };
    let buf = alloc_buffer_from_slice(&bytes);
    if !buf.is_null() {
        // Mark as Uint8Array so `instanceof Uint8Array` works, both in
        // perry-native code and after the bridge materializes a v8
        // Uint8Array on the V8 side.
        perry_runtime::buffer::mark_as_uint8array(buf as usize);
        perry_runtime::buffer::mark_as_secret_key(buf as usize);
    }
    buf
}

/// `crypto.generateKeySync("aes"|"hmac", { length })` -> secret KeyObject.
///
/// Node reports secret keys in bytes; AES requires 128/192/256-bit lengths
/// while HMAC accepts bit lengths and truncates to whole bytes.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_sync(
    alg_ptr: i64,
    options_bits: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let alg = String::from_utf8(bytes_from_ptr(alg_ptr))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let length_bits = match object_field_bits(options_bits.to_bits(), b"length") {
        Some(bits) => bits,
        None => return std::ptr::null_mut(),
    };
    let length = f64::from_bits(length_bits) as i32;
    let byte_len = match alg.as_str() {
        "aes" => match length {
            128 => 16,
            192 => 24,
            256 => 32,
            _ => return std::ptr::null_mut(),
        },
        "hmac" if length >= 8 => (length / 8) as usize,
        _ => return std::ptr::null_mut(),
    };
    let mut bytes = vec![0u8; byte_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let buf = alloc_buffer_from_slice(&bytes);
    if !buf.is_null() {
        perry_runtime::buffer::mark_as_uint8array(buf as usize);
        perry_runtime::buffer::mark_as_secret_key(buf as usize);
    }
    buf
}

unsafe fn call_node_style_callback2(callback_bits: f64, err: f64, value: f64) {
    let raw = callback_bits.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    if raw < 0x1000 {
        return;
    }
    perry_runtime::closure::js_closure_call2(
        raw as *const perry_runtime::ClosureHeader,
        err,
        value,
    );
}

unsafe fn call_node_style_callback3(callback_bits: f64, err: f64, a: f64, b: f64) {
    let raw = callback_bits.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    if raw < 0x1000 {
        return;
    }
    perry_runtime::closure::js_closure_call3(raw as *const perry_runtime::ClosureHeader, err, a, b);
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_async(
    alg_ptr: i64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let key = js_crypto_generate_key_sync(alg_ptr, options_bits);
    let value = if key.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(key as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_async(
    alg_ptr: i64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let alg = string_from_header(alg_ptr as *const StringHeader).unwrap_or_default();
    let pair = match alg.as_slice() {
        b"ec" => js_crypto_generate_key_pair_sync_ec_p256(options_bits),
        b"ed25519" => js_crypto_generate_key_pair_sync_ed25519(options_bits),
        b"x25519" => js_crypto_generate_key_pair_sync_x25519(options_bits),
        _ => js_crypto_generate_key_pair_sync_rsa(options_bits),
    };
    let null = f64::from_bits(JSValue::null().bits());
    let undefined = f64::from_bits(JSValue::undefined().bits());
    if pair.is_null() {
        call_node_style_callback3(callback_bits, null, undefined, undefined);
        return undefined;
    }
    let public_key =
        js_object_get_field_by_name(pair, js_string_from_bytes(b"publicKey".as_ptr(), 9));
    let private_key =
        js_object_get_field_by_name(pair, js_string_from_bytes(b"privateKey".as_ptr(), 10));
    call_node_style_callback3(
        callback_bits,
        null,
        f64::from_bits(public_key.bits()),
        f64::from_bits(private_key.bits()),
    );
    undefined
}

/// Generate random bytes and return as a Buffer
/// crypto.randomBytes(size) -> Buffer
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_buffer(
    size: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let size = size as usize;
    if size == 0 || size > 1024 * 1024 {
        return perry_runtime::buffer::buffer_alloc(0);
    }

    let buf = perry_runtime::buffer::buffer_alloc(size as u32);
    unsafe {
        (*buf).length = size as u32;
        let data = perry_runtime::buffer::buffer_data_mut(buf);
        let bytes = std::slice::from_raw_parts_mut(data, size);
        rand::thread_rng().fill_bytes(bytes);
    }
    buf
}

/// `crypto.randomBytes(size, callback)` — callback form.
///
/// Perry executes the callback synchronously, but preserves Node's
/// observable callback shape `(err, buffer)` for parity tests and common
/// compatibility paths.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_bytes_async(size: f64, callback_bits: f64) -> f64 {
    let buf = js_crypto_random_bytes_buffer(size);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// Generate random bytes and return as hex string
/// crypto.randomBytes(size).toString('hex') -> string
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_hex(size: f64) -> *mut StringHeader {
    let size = size as usize;
    if size == 0 || size > 1024 * 1024 {
        // Limit to 1MB
        return std::ptr::null_mut();
    }

    let mut bytes = vec![0u8; size];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex_str = hex::encode(&bytes);

    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// Generate a random UUID v4 using crypto-secure random
/// crypto.randomUUID() -> string
#[no_mangle]
pub extern "C" fn js_crypto_random_uuid() -> *mut StringHeader {
    let uuid = uuid::Uuid::new_v4();
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// `crypto.randomInt(max)` / `crypto.randomInt(min, max)` synchronous form.
/// Returns a uniformly distributed integer in `[min, max)`.
#[no_mangle]
pub extern "C" fn js_crypto_random_int(min_bits: f64, max_bits: f64) -> f64 {
    let Some(min) = nanboxed_to_i64(min_bits) else {
        return f64::NAN;
    };
    let Some(max) = nanboxed_to_i64(max_bits) else {
        return f64::NAN;
    };
    if max <= min {
        return f64::NAN;
    }
    rand::thread_rng().gen_range(min..max) as f64
}

/// `crypto.randomInt(min, max, callback)` callback form. The random value is
/// generated through the synchronous helper and delivered as `(err, n)`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_int_async(
    min_bits: f64,
    max_bits: f64,
    callback_bits: f64,
) -> f64 {
    let n = js_crypto_random_int(min_bits, max_bits);
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), n);
    f64::from_bits(JSValue::undefined().bits())
}

/// `crypto.randomFillSync(buffer, offset?, size?)` — fills the given
/// Buffer or TypedArray with cryptographically strong random bytes
/// in-place and returns the **same** NaN-boxed buffer value.
///
/// All three arguments arrive as NaN-boxed `f64`. `offset` and `size`
/// may be `undefined` (Node's defaults are `offset = 0`, `size =
/// buffer.byteLength - offset`). Bounds-clamping mirrors Node: out-of-
/// range offsets/sizes are clamped to the buffer's byte length rather
/// than throwing, so misuse degrades to "fewer bytes filled" instead
/// of an opaque crash.
///
/// Required by axios (#? jose / axios native compile) — axios's
/// `generateString` passes a `Uint32Array`; jose can pass a `Buffer`.
#[no_mangle]
pub extern "C" fn js_crypto_random_fill_sync(
    buf_bits: f64,
    offset_bits: f64,
    size_bits: f64,
) -> f64 {
    let bits = buf_bits.to_bits();
    // Accept either form the codegen can hand us:
    //   - NaN-boxed POINTER_TAG (top16 0x7FFD) → Buffer / Uint8Array
    //   - Raw heap pointer in low 48 bits (top16 == 0) → TypedArray
    //     (Uint32Array etc — see `Expr::TypedArrayNew` codegen, the
    //     pointer is `bitcast_i64_to_double` without a tag).
    let top16 = (bits >> 48) as u16;
    let raw = if top16 >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    if raw < 0x1000 {
        return buf_bits;
    }

    // Read optional numeric args; undefined / NaN → use default.
    let offset_arg = nanboxed_to_usize(offset_bits);
    let size_arg = nanboxed_to_usize(size_bits);

    unsafe {
        // TypedArrayHeader path (Uint8Array, Uint32Array, Float32Array, …).
        if perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some() {
            let ta = raw as *mut perry_runtime::typedarray::TypedArrayHeader;
            let len = (*ta).length as usize;
            let elem_size = (*ta).elem_size as usize;
            // Node interprets offset/size for TypedArray inputs in elements,
            // not bytes. Convert the resolved element range back to byte
            // offsets before filling the underlying storage.
            let (start_elem, end_elem) = resolve_range(len, offset_arg, size_arg);
            let start = start_elem.saturating_mul(elem_size);
            let end = end_elem.saturating_mul(elem_size);
            if end > start {
                let data = (raw as *mut u8).add(std::mem::size_of::<
                    perry_runtime::typedarray::TypedArrayHeader,
                >());
                let slice = std::slice::from_raw_parts_mut(data.add(start), end - start);
                rand::thread_rng().fill_bytes(slice);
            }
            return buf_bits;
        }
        // BufferHeader / Uint8Array path.
        if perry_runtime::buffer::is_registered_buffer(raw) {
            let buf = raw as *mut perry_runtime::buffer::BufferHeader;
            let total = (*buf).length as usize;
            let (start, end) = resolve_range(total, offset_arg, size_arg);
            if end > start {
                let data = perry_runtime::buffer::buffer_data_mut(buf);
                let slice = std::slice::from_raw_parts_mut(data.add(start), end - start);
                rand::thread_rng().fill_bytes(slice);
            }
            // Hand back the same NaN-boxed value the caller passed.
            return buf_bits;
        }
    }

    // Unsupported value shape — return the original (no-op) rather
    // than crashing. The HIR-level type check is "any", so the
    // compiler can't statically rule this out.
    buf_bits
}

/// `crypto.randomFill(buffer[, offset][, size], callback)` — async callback
/// form. Fills in-place using the same implementation as `randomFillSync`,
/// then invokes `(err, buffer)`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_fill_async(
    buf_bits: f64,
    offset_bits: f64,
    size_bits: f64,
    callback_bits: f64,
) -> f64 {
    let value = js_crypto_random_fill_sync(buf_bits, offset_bits, size_bits);
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

/// Best-effort extract a non-negative integer from a NaN-boxed `f64`.
/// `undefined`, `null`, NaN, negatives → `None` (caller picks default).
fn nanboxed_to_usize(bits: f64) -> Option<usize> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    // Undefined / null / false / true sentinels.
    if matches!(raw, 0x7FFC_0000_0000_0001 | 0x7FFC_0000_0000_0002) {
        return None;
    }
    // Int32 tag (0x7FFE).
    if top16 == 0x7FFE {
        let i = (raw & 0xFFFF_FFFF) as u32 as i32;
        if i < 0 {
            return None;
        }
        return Some(i as usize);
    }
    // Otherwise treat as plain f64.
    if bits.is_nan() || bits.is_sign_negative() || bits.is_infinite() {
        return None;
    }
    Some(bits as usize)
}

fn nanboxed_to_i64(bits: f64) -> Option<i64> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    if matches!(raw, 0x7FFC_0000_0000_0001 | 0x7FFC_0000_0000_0002) {
        return None;
    }
    if top16 == 0x7FFE {
        return Some((raw & 0xFFFF_FFFF) as u32 as i32 as i64);
    }
    if bits.is_nan() || bits.is_infinite() {
        return None;
    }
    Some(bits as i64)
}

unsafe fn crypto_value_bytes(bits: f64) -> Vec<u8> {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    if top16 == 0x7FFE {
        let i = (raw & 0xFFFF_FFFF) as u32 as i32;
        if i < 0 {
            return Vec::new();
        }
        let n = i as u64;
        return if n <= 0xff {
            vec![n as u8]
        } else if n <= 0xffff {
            (n as u16).to_be_bytes().to_vec()
        } else if n <= 0xffff_ffff {
            (n as u32).to_be_bytes().to_vec()
        } else {
            n.to_be_bytes().to_vec()
        };
    }
    let ptr = (raw & 0x0000_FFFF_FFFF_FFFF) as i64;
    bytes_from_ptr(ptr)
}

fn bytes_to_u128(bytes: &[u8]) -> Option<u128> {
    if bytes.len() > 16 {
        return None;
    }
    let mut n = 0u128;
    for &b in bytes {
        n = (n << 8) | b as u128;
    }
    Some(n)
}

fn mod_pow_u128(mut base: u128, mut exp: u128, modu: u128) -> u128 {
    if modu == 1 {
        return 0;
    }
    let mut acc = 1u128;
    base %= modu;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = acc.wrapping_mul(base) % modu;
        }
        base = base.wrapping_mul(base) % modu;
        exp >>= 1;
    }
    acc
}

fn is_prime_u128(n: u128) -> bool {
    if n < 2 {
        return false;
    }
    for p in [2u128, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n == p {
            return true;
        }
        if n % p == 0 {
            return false;
        }
    }
    let mut d = n - 1;
    let mut s = 0u32;
    while d % 2 == 0 {
        d /= 2;
        s += 1;
    }
    for a in [2u128, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if a >= n - 2 {
            continue;
        }
        let mut x = mod_pow_u128(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        let mut composite = true;
        for _ in 1..s {
            x = x.wrapping_mul(x) % n;
            if x == n - 1 {
                composite = false;
                break;
            }
        }
        if composite {
            return false;
        }
    }
    true
}

fn prime_to_be_bytes(n: u128, bits: usize) -> Vec<u8> {
    let len = ((bits.max(1) + 7) / 8).max(1);
    let all = n.to_be_bytes();
    all[all.len() - len..].to_vec()
}

unsafe fn object_field_bool(obj_bits: u64, name: &[u8]) -> Option<bool> {
    match object_field_bits(obj_bits, name)? {
        0x7FFC_0000_0000_0004 => Some(true),
        0x7FFC_0000_0000_0003 => Some(false),
        _ => None,
    }
}

unsafe fn object_field_u128(obj_bits: u64, name: &[u8]) -> Option<u128> {
    let bits = object_field_bits(obj_bits, name)?;
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFE {
        let i = (bits & 0xFFFF_FFFF) as u32 as i32;
        return (i >= 0).then_some(i as u128);
    }
    bytes_to_u128(&crypto_value_bytes(f64::from_bits(bits)))
}

fn generate_prime_u128(
    bits: usize,
    safe: bool,
    add: Option<u128>,
    rem: Option<u128>,
) -> Option<u128> {
    if bits == 0 || bits > 64 {
        return None;
    }
    let high_bit = 1u128 << (bits - 1);
    let mask = (1u128 << bits) - 1;
    let mut rng = rand::thread_rng();
    for _ in 0..1_000_000 {
        let mut n = (rng.next_u64() as u128) & mask;
        n |= high_bit;
        n |= 1;
        if let Some(add) = add {
            if add == 0 {
                return None;
            }
            let rem = rem.unwrap_or(if safe { 3 } else { 1 });
            let cur = n % add;
            n = n.wrapping_add((rem + add - cur) % add);
            if n > mask || n < high_bit {
                continue;
            }
        }
        if !is_prime_u128(n) {
            continue;
        }
        if safe && !is_prime_u128((n - 1) / 2) {
            continue;
        }
        return Some(n);
    }
    None
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_prime_sync(
    size_bits: f64,
    options_bits: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let size = match nanboxed_to_usize(size_bits) {
        Some(s) => s,
        None => return alloc_buffer_from_slice(&[]),
    };
    let options_raw = options_bits.to_bits();
    let safe = object_field_bool(options_raw, b"safe").unwrap_or(false);
    let add = object_field_u128(options_raw, b"add");
    let rem = object_field_u128(options_raw, b"rem");
    let Some(prime) = generate_prime_u128(size, safe, add, rem) else {
        return alloc_buffer_from_slice(&[]);
    };
    alloc_buffer_from_slice(&prime_to_be_bytes(prime, size))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_check_prime_sync(
    candidate_bits: f64,
    _options_bits: f64,
) -> f64 {
    let bytes = crypto_value_bytes(candidate_bits);
    let Some(n) = bytes_to_u128(&bytes) else {
        return js_bool(false);
    };
    js_bool(is_prime_u128(n))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_prime_async(
    size_bits: f64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let buf = js_crypto_generate_prime_sync(size_bits, options_bits);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_check_prime_async(
    candidate_bits: f64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let result = js_crypto_check_prime_sync(candidate_bits, options_bits);
    call_node_style_callback2(
        callback_bits,
        f64::from_bits(JSValue::null().bits()),
        result,
    );
    f64::from_bits(JSValue::undefined().bits())
}

/// Resolve `(start, end)` byte indices from Node-style `offset` / `size`
/// arguments against a buffer of `total` bytes. Out-of-range values are
/// clamped to `[0, total]`.
fn resolve_range(total: usize, offset: Option<usize>, size: Option<usize>) -> (usize, usize) {
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
    let private_pem = private_key
        .to_pkcs8_pem(Default::default())
        .map(|pem| pem.to_string())
        .unwrap_or_default();
    let options = options_bits.to_bits();
    let public_as_jwk = keygen_encoding_wants_jwk(options, b"publicKeyEncoding");
    let private_as_jwk = keygen_encoding_wants_jwk(options, b"privateKeyEncoding");

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

pub struct SignHandle {
    alg: RsaDigestKind,
    data: std::sync::Mutex<Vec<u8>>,
}

pub struct VerifyHandle {
    alg: RsaDigestKind,
    data: std::sync::Mutex<Vec<u8>>,
}

pub struct EcdhHandle {
    private_key: std::sync::Mutex<Option<P256SecretKey>>,
}

pub struct DiffieHellmanHandle {
    prime: Vec<u8>,
    generator: Vec<u8>,
    private_key: std::sync::Mutex<Option<Vec<u8>>>,
    public_key: std::sync::Mutex<Option<Vec<u8>>>,
}

const DH_DEFAULT_PRIME_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
    "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381",
    "FFFFFFFFFFFFFFFF"
);

fn dh_default_prime() -> Vec<u8> {
    hex::decode(DH_DEFAULT_PRIME_HEX).unwrap_or_else(|_| vec![0xff; 128])
}

fn dh_default_generator() -> Vec<u8> {
    vec![2]
}

fn bigint_to_padded_bytes(n: &RsaBigUint, len: usize) -> Vec<u8> {
    let mut bytes = n.to_bytes_be();
    if bytes.len() > len {
        bytes = bytes[bytes.len() - len..].to_vec();
    } else if bytes.len() < len {
        let mut padded = vec![0u8; len - bytes.len()];
        padded.extend_from_slice(&bytes);
        bytes = padded;
    }
    bytes
}

fn dh_public_from_private(prime: &[u8], generator: &[u8], private_key: &[u8]) -> Vec<u8> {
    let p = RsaBigUint::from_bytes_be(prime);
    let g = RsaBigUint::from_bytes_be(generator);
    let x = RsaBigUint::from_bytes_be(private_key);
    let y = g.modpow(&x, &p);
    bigint_to_padded_bytes(&y, prime.len())
}

fn dh_secret(prime: &[u8], private_key: &[u8], other_public_key: &[u8]) -> Vec<u8> {
    let p = RsaBigUint::from_bytes_be(prime);
    let x = RsaBigUint::from_bytes_be(private_key);
    let y = RsaBigUint::from_bytes_be(other_public_key);
    let s = y.modpow(&x, &p);
    bigint_to_padded_bytes(&s, prime.len())
}

fn dh_random_private_key(prime: &[u8]) -> Vec<u8> {
    let p = RsaBigUint::from_bytes_be(prime);
    let two = RsaBigUint::from(2u32);
    let mut rng = rand::thread_rng();
    for _ in 0..128 {
        let mut bytes = vec![0u8; prime.len()];
        rng.fill_bytes(&mut bytes);
        let x = RsaBigUint::from_bytes_be(&bytes);
        if x > two && x < p {
            return bigint_to_padded_bytes(&x, prime.len());
        }
    }
    let fallback = RsaBigUint::from(3u32);
    bigint_to_padded_bytes(&fallback, prime.len())
}

fn nanbox_ptr<T>(ptr: *mut T) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

fn arg_ptr(arg: f64) -> i64 {
    (arg.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}

unsafe fn arg_bytes(args: &[f64], idx: usize) -> Vec<u8> {
    args.get(idx)
        .map(|arg| bytes_from_ptr(arg_ptr(*arg)))
        .unwrap_or_default()
}

unsafe fn arg_string(args: &[f64], idx: usize) -> String {
    String::from_utf8(arg_bytes(args, idx)).unwrap_or_default()
}

unsafe fn string_value(bytes: &[u8]) -> f64 {
    let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    nanbox_str(s)
}

unsafe fn ecdh_output(bytes: &[u8], encoding: Option<&str>) -> f64 {
    if matches!(encoding, Some(enc) if enc.eq_ignore_ascii_case("hex")) {
        return string_value(hex::encode(bytes).as_bytes());
    }
    if matches!(encoding, Some(enc) if enc.eq_ignore_ascii_case("base64")) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        return string_value(encoded.as_bytes());
    }
    nanbox_ptr(alloc_buffer_from_slice(bytes))
}

unsafe fn decode_ecdh_input(ptr: i64, encoding: &str) -> Vec<u8> {
    let bytes = bytes_from_ptr(ptr);
    if encoding.eq_ignore_ascii_case("hex") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return hex::decode(s).unwrap_or_default();
    }
    if encoding.eq_ignore_ascii_case("base64") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .unwrap_or_default();
    }
    bytes
}

unsafe fn decode_crypto_value(value: f64, encoding: &str) -> Vec<u8> {
    decode_ecdh_input(arg_ptr(value), encoding)
}

unsafe fn decode_hash_update_value(value: f64, encoding: &str) -> Vec<u8> {
    let bytes = bytes_from_ptr(arg_ptr(value));
    if encoding.eq_ignore_ascii_case("hex") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return hex::decode(s).unwrap_or_default();
    }
    if encoding.eq_ignore_ascii_case("base64") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .unwrap_or_default();
    }
    if encoding.eq_ignore_ascii_case("base64url") {
        let s = String::from_utf8(bytes).unwrap_or_default();
        return base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.as_bytes())
            .unwrap_or_default();
    }
    bytes
}

unsafe fn decode_dh_prime_value(value: f64, encoding: &str) -> Vec<u8> {
    if value.is_finite() {
        return dh_default_prime();
    }
    let decoded = decode_crypto_value(value, encoding);
    if decoded.is_empty() {
        dh_default_prime()
    } else {
        decoded
    }
}

unsafe fn decode_dh_generator_value(value: Option<f64>, encoding: &str) -> Vec<u8> {
    let Some(value) = value else {
        return dh_default_generator();
    };
    if value.is_finite() {
        let n = value as u64;
        if n == 0 {
            return dh_default_generator();
        }
        let bytes = RsaBigUint::from(n).to_bytes_be();
        return if bytes.is_empty() {
            dh_default_generator()
        } else {
            bytes
        };
    }
    let decoded = decode_crypto_value(value, encoding);
    if decoded.is_empty() {
        dh_default_generator()
    } else {
        decoded
    }
}

fn generate_p256_secret_key() -> Option<P256SecretKey> {
    let mut rng = rand::thread_rng();
    for _ in 0..128 {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if let Ok(key) = P256SecretKey::from_slice(&bytes) {
            return Some(key);
        }
    }
    None
}

fn p256_public_bytes(private_key: &P256SecretKey, format: &str) -> Vec<u8> {
    let compressed = format.eq_ignore_ascii_case("compressed");
    private_key
        .public_key()
        .to_encoded_point(compressed)
        .as_bytes()
        .to_vec()
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_sign(alg_ptr: i64) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = match normalize_sign_algorithm(&alg_bytes) {
        Some(alg) => alg,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let handle: Handle = register_handle(SignHandle {
        alg,
        data: std::sync::Mutex::new(Vec::new()),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_verify(alg_ptr: i64) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = match normalize_sign_algorithm(&alg_bytes) {
        Some(alg) => alg,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let handle: Handle = register_handle(VerifyHandle {
        alg,
        data: std::sync::Mutex::new(Vec::new()),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_ecdh(curve_ptr: i64) -> f64 {
    let curve = String::from_utf8(bytes_from_ptr(curve_ptr))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(curve.as_str(), "prime256v1" | "secp256r1" | "p-256") {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    let handle: Handle = register_handle(EcdhHandle {
        private_key: std::sync::Mutex::new(None),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_diffie_hellman(
    prime_val: f64,
    second_val: f64,
    third_val: f64,
) -> f64 {
    let second_string = if second_val.is_finite() {
        String::new()
    } else {
        String::from_utf8(bytes_from_ptr(arg_ptr(second_val))).unwrap_or_default()
    };
    let (prime_encoding, generator_value, generator_encoding) = if matches!(
        second_string.as_str(),
        "hex" | "base64" | "buffer" | "latin1" | "binary"
    ) {
        (
            second_string.as_str(),
            if third_val.to_bits() == JSValue::undefined().bits() {
                None
            } else {
                Some(third_val)
            },
            second_string.as_str(),
        )
    } else {
        (
            "",
            if second_val.to_bits() == JSValue::undefined().bits() {
                None
            } else {
                Some(second_val)
            },
            "",
        )
    };

    let prime = decode_dh_prime_value(prime_val, prime_encoding);
    let generator = decode_dh_generator_value(generator_value, generator_encoding);
    let handle: Handle = register_handle(DiffieHellmanHandle {
        prime,
        generator,
        private_key: std::sync::Mutex::new(None),
        public_key: std::sync::Mutex::new(None),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_diffie_hellman(_group_val: f64) -> f64 {
    let handle: Handle = register_handle(DiffieHellmanHandle {
        prime: dh_default_prime(),
        generator: dh_default_generator(),
        private_key: std::sync::Mutex::new(None),
        public_key: std::sync::Mutex::new(None),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_ecdh_convert_key(
    key_val: f64,
    curve_val: f64,
    input_encoding_val: f64,
    output_encoding_val: f64,
    format_val: f64,
) -> f64 {
    let curve_ptr = arg_ptr(curve_val);
    let curve = String::from_utf8(bytes_from_ptr(curve_ptr))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(curve.as_str(), "prime256v1" | "secp256r1" | "p-256") {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let input_encoding =
        String::from_utf8(bytes_from_ptr(arg_ptr(input_encoding_val))).unwrap_or_default();
    let output_encoding =
        String::from_utf8(bytes_from_ptr(arg_ptr(output_encoding_val))).unwrap_or_default();
    let format = String::from_utf8(bytes_from_ptr(arg_ptr(format_val))).unwrap_or_default();
    let key_bytes = decode_ecdh_input(arg_ptr(key_val), &input_encoding);
    let public = match P256PublicKey::from_sec1_bytes(&key_bytes) {
        Ok(public) => public,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let compressed = format.eq_ignore_ascii_case("compressed");
    let converted = public.to_encoded_point(compressed).as_bytes().to_vec();
    ecdh_output(
        &converted,
        (!output_encoding.is_empty()).then_some(output_encoding.as_str()),
    )
}

pub unsafe fn dispatch_sign(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<SignHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    match method {
        "update" if !args.is_empty() => {
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            h.data.lock().unwrap().extend_from_slice(&bytes);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "sign" if !args.is_empty() => {
            let key_bits = args[0].to_bits();
            let pem = match crypto_key_input_to_private_pem(key_bits) {
                Some(pem) => pem,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            if let Some(signing_key) = parse_p256_signing_key_pem(&pem) {
                let data = h.data.lock().unwrap().clone();
                let signature: P256EcdsaSignature = signing_key.sign(&data);
                if key_input_uses_ieee_p1363(key_bits) {
                    let raw = signature.to_bytes();
                    let buf = alloc_buffer_from_slice(raw.as_slice());
                    return f64::from_bits(
                        0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                    );
                }
                let der = signature.to_der();
                let buf = alloc_buffer_from_slice(der.as_bytes());
                return f64::from_bits(
                    0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                );
            }
            let private_key = match parse_rsa_private_key_pem(&pem) {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let data = h.data.lock().unwrap().clone();
            let signature = if key_input_uses_rsa_pss(key_bits) {
                let salt_len = key_input_pss_salt_len(key_bits, h.alg);
                sign_rsa_pss_data(h.alg, private_key, &data, salt_len)
            } else {
                sign_rsa_data(h.alg, private_key, &data)
            };
            let buf = alloc_buffer_from_slice(&signature);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_sign_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "sign" => b"sign",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

pub unsafe fn dispatch_ecdh(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<EcdhHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    match method {
        "generateKeys" | "dhGenerateKeys" => {
            let key = match generate_p256_secret_key() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let encoding = arg_string(args, 0);
            let format = arg_string(args, 1);
            let public = p256_public_bytes(&key, &format);
            *h.private_key.lock().unwrap() = Some(key);
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "getPublicKey" => {
            let guard = h.private_key.lock().unwrap();
            let key = match guard.as_ref() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let encoding = arg_string(args, 0);
            let format = arg_string(args, 1);
            let public = p256_public_bytes(key, &format);
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "getPrivateKey" | "dhGetPrivateKey" => {
            let guard = h.private_key.lock().unwrap();
            let key = match guard.as_ref() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let encoding = arg_string(args, 0);
            let bytes = key.to_bytes();
            ecdh_output(
                bytes.as_slice(),
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "setPrivateKey" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let mut bytes = arg_bytes(args, 0);
            if encoding.eq_ignore_ascii_case("hex") {
                let s = String::from_utf8(bytes).unwrap_or_default();
                bytes = hex::decode(s).unwrap_or_default();
            }
            match P256SecretKey::from_slice(&bytes) {
                Ok(key) => {
                    *h.private_key.lock().unwrap() = Some(key);
                    f64::from_bits(JSValue::undefined().bits())
                }
                Err(_) => f64::from_bits(0x7FFC_0000_0000_0001),
            }
        }
        "setPublicKey" => f64::from_bits(JSValue::undefined().bits()),
        "computeSecret" | "dhComputeSecret" if !args.is_empty() => {
            let guard = h.private_key.lock().unwrap();
            let key = match guard.as_ref() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let input_encoding = arg_string(args, 1);
            let output_encoding = arg_string(args, 2);
            let public_bytes = decode_ecdh_input(arg_ptr(args[0]), &input_encoding);
            let public = match P256PublicKey::from_sec1_bytes(&public_bytes) {
                Ok(public) => public,
                Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let secret = p256_diffie_hellman(key.to_nonzero_scalar(), public.as_affine());
            ecdh_output(
                secret.raw_secret_bytes().as_slice(),
                (!output_encoding.is_empty()).then_some(output_encoding.as_str()),
            )
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_ecdh_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "generateKeys" => b"generateKeys",
        "getPublicKey" => b"getPublicKey",
        "getPrivateKey" => b"dhGetPrivateKey",
        "setPrivateKey" => b"setPrivateKey",
        "setPublicKey" => b"setPublicKey",
        "computeSecret" => b"dhComputeSecret",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

pub unsafe fn dispatch_diffie_hellman(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<DiffieHellmanHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    match method {
        "generateKeys" => {
            let encoding = arg_string(args, 0);
            let private = {
                let mut private_guard = h.private_key.lock().unwrap();
                match private_guard.as_ref() {
                    Some(private) => private.clone(),
                    None => {
                        let private = dh_random_private_key(&h.prime);
                        *private_guard = Some(private.clone());
                        private
                    }
                }
            };
            let public = dh_public_from_private(&h.prime, &h.generator, &private);
            *h.public_key.lock().unwrap() = Some(public.clone());
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "computeSecret" | "dhComputeSecret" if !args.is_empty() => {
            let input_encoding = arg_string(args, 1);
            let output_encoding = arg_string(args, 2);
            let other_public = decode_crypto_value(args[0], &input_encoding);
            let private = {
                let mut private_guard = h.private_key.lock().unwrap();
                match private_guard.as_ref() {
                    Some(private) => private.clone(),
                    None => {
                        let private = dh_random_private_key(&h.prime);
                        let public = dh_public_from_private(&h.prime, &h.generator, &private);
                        *h.public_key.lock().unwrap() = Some(public);
                        *private_guard = Some(private.clone());
                        private
                    }
                }
            };
            let secret = dh_secret(&h.prime, &private, &other_public);
            ecdh_output(
                &secret,
                (!output_encoding.is_empty()).then_some(output_encoding.as_str()),
            )
        }
        "getPrime" | "dhGetPrime" => {
            let encoding = arg_string(args, 0);
            ecdh_output(
                &h.prime,
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "getGenerator" | "dhGetGenerator" => {
            let encoding = arg_string(args, 0);
            ecdh_output(
                &h.generator,
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "getPublicKey" | "dhGetPublicKey" => {
            let encoding = arg_string(args, 0);
            let public = {
                let public_guard = h.public_key.lock().unwrap();
                public_guard.as_ref().cloned()
            }
            .or_else(|| {
                h.private_key
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|private| dh_public_from_private(&h.prime, &h.generator, private))
            })
            .unwrap_or_default();
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "getPrivateKey" | "dhGetPrivateKey" => {
            let encoding = arg_string(args, 0);
            let private = h
                .private_key
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap_or_default();
            ecdh_output(
                &private,
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "setPrivateKey" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let private = decode_crypto_value(args[0], &encoding);
            *h.private_key.lock().unwrap() = Some(private);
            f64::from_bits(JSValue::undefined().bits())
        }
        "setPublicKey" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let public = decode_crypto_value(args[0], &encoding);
            *h.public_key.lock().unwrap() = Some(public);
            f64::from_bits(JSValue::undefined().bits())
        }
        "verifyError" => 0.0,
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_diffie_hellman_property(handle: i64, property: &str) -> f64 {
    if property == "verifyError" {
        return 0.0;
    }
    let name_bytes: &'static [u8] = match property {
        "generateKeys" => b"dhGenerateKeys",
        "computeSecret" => b"dhComputeSecret",
        "getPrime" => b"dhGetPrime",
        "getGenerator" => b"dhGetGenerator",
        "getPublicKey" => b"dhGetPublicKey",
        "getPrivateKey" => b"dhGetPrivateKey",
        "setPublicKey" => b"setPublicKey",
        "setPrivateKey" => b"setPrivateKey",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

pub unsafe fn dispatch_verify(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<VerifyHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    match method {
        "update" if !args.is_empty() => {
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            h.data.lock().unwrap().extend_from_slice(&bytes);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "verify" if args.len() >= 2 => {
            let key_bits = args[0].to_bits();
            let sig_ptr = (args[1].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let pem = match crypto_key_input_to_public_pem(key_bits) {
                Some(pem) => pem,
                None => return js_bool(false),
            };
            if let Some(verifying_key) = parse_p256_verifying_key_pem(&pem) {
                let sig_bytes = bytes_from_ptr(sig_ptr);
                let signature = if key_input_uses_ieee_p1363(key_bits) {
                    P256EcdsaSignature::from_slice(&sig_bytes)
                } else {
                    P256EcdsaSignature::from_der(&sig_bytes)
                };
                let signature = match signature {
                    Ok(sig) => sig,
                    Err(_) => return js_bool(false),
                };
                let data = h.data.lock().unwrap().clone();
                return js_bool(verifying_key.verify(&data, &signature).is_ok());
            }
            let public_key = match parse_rsa_public_key_pem(&pem) {
                Some(key) => key,
                None => return js_bool(false),
            };
            let sig_bytes = bytes_from_ptr(sig_ptr);
            if key_input_uses_rsa_pss(key_bits) {
                let signature = match RsaPssSignature::try_from(sig_bytes.as_slice()) {
                    Ok(sig) => sig,
                    Err(_) => return js_bool(false),
                };
                let data = h.data.lock().unwrap().clone();
                let salt_len = key_input_pss_salt_len(key_bits, h.alg);
                return js_bool(verify_rsa_pss_data(
                    h.alg, public_key, &data, &signature, salt_len,
                ));
            }
            let signature = match RsaPkcs1v15Signature::try_from(sig_bytes.as_slice()) {
                Ok(sig) => sig,
                Err(_) => return js_bool(false),
            };
            let data = h.data.lock().unwrap().clone();
            js_bool(verify_rsa_data(h.alg, public_key, &data, &signature))
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_verify_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "verify" => b"verify",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

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

struct CipherInfo {
    name: &'static str,
    nid: f64,
    block_size: f64,
    iv_len: Option<f64>,
    key_len: f64,
    mode: &'static str,
}

fn cipher_info_for_name(name: &str) -> Option<CipherInfo> {
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

fn cipher_info_for_nid(nid: i32) -> Option<CipherInfo> {
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
type Aes256CbcEnc = Encryptor<Aes256>;
type Aes256CbcDec = Decryptor<Aes256>;

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
unsafe fn read_options_number(options_ptr: i64, name: &str) -> Option<f64> {
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
unsafe fn build_key_pair_object(pub_pem: &str, priv_pem: &str) -> f64 {
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

// ---------------------------------------------------------------------------
// Hash handle — powers `const h = crypto.createHash('sha1'); h.update(x);
// h.digest()` (issue #86). The runtime-resident chain-collapse in
// `perry-codegen/src/expr.rs` only catches the literal single-expression
// form; once the user binds the hash to a local and calls update/digest on
// subsequent statements, the chain pattern no longer matches and the calls
// fall through to `js_native_call_method`. We register the hash state in
// the handle registry and the small-integer dispatch path (see
// `perry-runtime/src/object.rs` ~line 3040) routes update/digest back to
// `dispatch_hash` below.
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum HashState {
    Sha1(Sha1),
    Sha224(Sha224),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
    Sha512_256(Sha512_256),
    Shake128(Shake128),
    Shake256(Shake256),
    Md5(Md5),
}

pub struct HashHandle {
    /// `Option` so `digest()` can `take()` ownership of the hasher
    /// (sha1/sha2 `finalize()` consumes `self`).
    state: std::sync::Mutex<Option<HashState>>,
    output_len: Option<usize>,
}

/// Allocate a new Hash handle for the given algorithm. Returns the handle
/// id NaN-boxed with POINTER_TAG (0x7FFD_…). Small integers survive the
/// 48-bit POINTER_MASK, and the runtime's handle-range check in
/// `js_native_call_method` (`raw_ptr < 0x100000`) routes subsequent
/// `.update(...)` / `.digest(...)` through `HANDLE_METHOD_DISPATCH` which
/// calls `dispatch_hash` below. Unknown algorithms return undefined.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hash(alg_ptr: i64) -> f64 {
    js_crypto_create_hash_options(alg_ptr, f64::from_bits(JSValue::undefined().bits()))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hash_options(alg_ptr: i64, options_bits: f64) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes)
        .unwrap_or("")
        .to_ascii_lowercase();
    let state = match alg.as_str() {
        "sha1" | "sha-1" => HashState::Sha1(Sha1::new()),
        "sha224" | "sha-224" => HashState::Sha224(Sha224::new()),
        "sha256" | "sha-256" => HashState::Sha256(Sha256::new()),
        "sha384" | "sha-384" => HashState::Sha384(Sha384::new()),
        "sha512" | "sha-512" => HashState::Sha512(Sha512::new()),
        "sha512-256" | "sha512_256" | "sha-512-256" => HashState::Sha512_256(Sha512_256::new()),
        "shake128" | "shake-128" => HashState::Shake128(Shake128::default()),
        "shake256" | "shake-256" => HashState::Shake256(Shake256::default()),
        "md5" => HashState::Md5(Md5::new()),
        _ => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let output_len = object_field_bits(options_bits.to_bits(), b"outputLength")
        .and_then(|bits| nanboxed_to_usize(f64::from_bits(bits)));
    let handle: Handle = register_handle(HashHandle {
        state: std::sync::Mutex::new(Some(state)),
        output_len,
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Dispatch `update` / `digest` / `copy` on a HashHandle. Called from
/// `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_hash(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<HashHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    match method {
        "update" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let bytes = decode_hash_update_value(args[0], &encoding);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                match state {
                    HashState::Sha1(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha224(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha256(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha384(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha512(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha512_256(x) => Sha256Digest::update(x, &bytes),
                    HashState::Shake128(x) => sha3::digest::Update::update(x, &bytes),
                    HashState::Shake256(x) => sha3::digest::Update::update(x, &bytes),
                    HashState::Md5(x) => Md5Digest::update(x, &bytes),
                }
            }
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "digest" => {
            let state = {
                let mut guard = h.state.lock().unwrap();
                guard.take()
            };
            let arg0 = args.first().copied();
            let option_len = arg0
                .and_then(|arg| object_field_bits(arg.to_bits(), b"outputLength"))
                .and_then(|bits| nanboxed_to_usize(f64::from_bits(bits)));
            let digest: Vec<u8> = match state {
                Some(HashState::Sha1(x)) => x.finalize().to_vec(),
                Some(HashState::Sha224(x)) => x.finalize().to_vec(),
                Some(HashState::Sha256(x)) => x.finalize().to_vec(),
                Some(HashState::Sha384(x)) => x.finalize().to_vec(),
                Some(HashState::Sha512(x)) => x.finalize().to_vec(),
                Some(HashState::Sha512_256(x)) => x.finalize().to_vec(),
                Some(HashState::Shake128(x)) => {
                    let mut out = vec![0u8; option_len.or(h.output_len).unwrap_or(16)];
                    let mut reader = x.finalize_xof();
                    reader.read(&mut out);
                    out
                }
                Some(HashState::Shake256(x)) => {
                    let mut out = vec![0u8; option_len.or(h.output_len).unwrap_or(32)];
                    let mut reader = x.finalize_xof();
                    reader.read(&mut out);
                    out
                }
                Some(HashState::Md5(x)) => x.finalize().to_vec(),
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            if args.is_empty() || is_undefined_f64(args[0]) {
                let buf = alloc_buffer_from_slice(&digest);
                f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
            } else {
                let enc = if let Some(output_encoding) =
                    object_field_string(args[0].to_bits(), b"outputEncoding")
                {
                    output_encoding.to_ascii_lowercase()
                } else {
                    let enc_ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                    let enc_bytes = bytes_from_ptr(enc_ptr);
                    std::str::from_utf8(&enc_bytes)
                        .unwrap_or("hex")
                        .to_ascii_lowercase()
                };
                if enc == "buffer" {
                    let buf = alloc_buffer_from_slice(&digest);
                    return f64::from_bits(
                        0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                    );
                }
                let encoded = match enc.as_str() {
                    "hex" => hex::encode(&digest),
                    "base64" => base64::engine::general_purpose::STANDARD.encode(&digest),
                    "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest),
                    "binary" | "latin1" => String::from_utf8_lossy(&digest).into_owned(),
                    _ => hex::encode(&digest),
                };
                let s = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
                f64::from_bits(0x7FFF_0000_0000_0000u64 | ((s as u64) & 0x0000_FFFF_FFFF_FFFF))
            }
        }
        // `hash.copy()` (#1369) — return an independent Hash whose internal
        // state is a snapshot of this one, so the two can be `.update()`d and
        // `.digest()`ed separately. An already-digested hash (state taken)
        // yields undefined, mirroring the error a caller would hit using a
        // finalized hash. The optional `outputLength` arg only applies to XOF
        // hashes (shake*) — propagated via `output_len`.
        "copy" => {
            let state = {
                let guard = h.state.lock().unwrap();
                guard.clone()
            };
            let Some(state) = state else {
                return f64::from_bits(0x7FFC_0000_0000_0001);
            };
            let handle: Handle = register_handle(HashHandle {
                state: std::sync::Mutex::new(Some(state)),
                output_len: h.output_len,
            });
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_hash_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "digest" => b"digest",
        "copy" => b"copy",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

#[inline]
fn is_undefined_f64(v: f64) -> bool {
    v.to_bits() == 0x7FFC_0000_0000_0001
}

// ---------------------------------------------------------------------------
// HMAC handle — covers the same #1076 silent-empty bug shape that the hash
// handle covers for `createHash`. The chain-collapse in
// `perry-codegen/src/expr.rs` only emits the literal-`"sha256"` fast path
// for `crypto.createHmac(alg, key).update(data).digest(enc)`. When `alg`
// is a `const`-bound identifier, a for-of binding, a ternary, or anything
// else that isn't an inline `Expr::String`, the codegen falls back to
// `js_crypto_create_hmac` which returns a handle. Subsequent `.update(...)`
// and `.digest(...)` calls dispatch through `HANDLE_METHOD_DISPATCH` →
// `dispatch_hmac` below. Supports sha1, sha256, sha512, and md5 — Node's
// commonly-used HMAC algorithms. Unknown algorithms return undefined so
// the symptom (silent empty hex) becomes a real `undefined.update is not
// a function` at the call site instead of a wrong answer.
// ---------------------------------------------------------------------------

pub enum HmacState {
    Sha1(hmac::Hmac<Sha1>),
    Sha224(hmac::Hmac<Sha224>),
    Sha256(hmac::Hmac<Sha256>),
    Sha384(hmac::Hmac<Sha384>),
    Sha512(hmac::Hmac<Sha512>),
    Sha512_256(hmac::Hmac<Sha512_256>),
    Md5(hmac::Hmac<Md5>),
}

pub struct HmacHandle {
    /// `Option` so `digest()` can `take()` ownership of the MAC
    /// (`finalize()` consumes `self`).
    state: std::sync::Mutex<Option<HmacState>>,
}

/// Allocate a new HMAC handle for `(alg, key)`. Mirrors `js_crypto_create_hash`
/// in shape: returns the handle id NaN-boxed with `POINTER_TAG`. Unknown
/// algorithms return undefined.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hmac(alg_ptr: i64, key_ptr: i64) -> f64 {
    use hmac::KeyInit;
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes)
        .unwrap_or("")
        .to_ascii_lowercase();
    let key = bytes_from_ptr(key_ptr);
    let state = match alg.as_str() {
        "sha1" | "sha-1" => match hmac::Hmac::<Sha1>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha1(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        "sha224" | "sha-224" => match hmac::Hmac::<Sha224>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha224(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        "sha256" | "sha-256" => match hmac::Hmac::<Sha256>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha256(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        "sha384" | "sha-384" => match hmac::Hmac::<Sha384>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha384(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        "sha512" | "sha-512" => match hmac::Hmac::<Sha512>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha512(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        "sha512-256" | "sha512_256" | "sha-512-256" => {
            match hmac::Hmac::<Sha512_256>::new_from_slice(&key) {
                Ok(m) => HmacState::Sha512_256(m),
                Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
            }
        }
        "md5" => match hmac::Hmac::<Md5>::new_from_slice(&key) {
            Ok(m) => HmacState::Md5(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        _ => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let handle: Handle = register_handle(HmacHandle {
        state: std::sync::Mutex::new(Some(state)),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Dispatch `update` / `digest` on an HmacHandle. Called from
/// `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_hmac(handle: i64, method: &str, args: &[f64]) -> f64 {
    use hmac::Mac;
    let h = match get_handle_mut::<HmacHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    match method {
        "update" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let bytes = decode_hash_update_value(args[0], &encoding);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                match state {
                    HmacState::Sha1(x) => Mac::update(x, &bytes),
                    HmacState::Sha224(x) => Mac::update(x, &bytes),
                    HmacState::Sha256(x) => Mac::update(x, &bytes),
                    HmacState::Sha384(x) => Mac::update(x, &bytes),
                    HmacState::Sha512(x) => Mac::update(x, &bytes),
                    HmacState::Sha512_256(x) => Mac::update(x, &bytes),
                    HmacState::Md5(x) => Mac::update(x, &bytes),
                }
            }
            // Return the same handle (NaN-boxed) so the chain
            // `hmac.update(data).digest(enc)` continues against the same
            // state. Mirrors Node's behavior (`update` returns `this`).
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "digest" => {
            let state = {
                let mut guard = h.state.lock().unwrap();
                guard.take()
            };
            let digest: Vec<u8> = match state {
                Some(HmacState::Sha1(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Sha224(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Sha256(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Sha384(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Sha512(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Sha512_256(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Md5(x)) => x.finalize().into_bytes().to_vec(),
                // Node keeps Hmac.digest() idempotent in shape after the
                // first finalization: encoded digests become an empty string
                // and buffer digests become an empty Buffer instead of
                // `undefined`.
                None => Vec::new(),
            };
            if args.is_empty() || is_undefined_f64(args[0]) {
                let buf = alloc_buffer_from_slice(&digest);
                f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
            } else {
                let enc_ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                let enc_bytes = bytes_from_ptr(enc_ptr);
                let enc = std::str::from_utf8(&enc_bytes)
                    .unwrap_or("hex")
                    .to_ascii_lowercase();
                if enc == "buffer" {
                    let buf = alloc_buffer_from_slice(&digest);
                    return f64::from_bits(
                        0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                    );
                }
                let encoded = match enc.as_str() {
                    "hex" => hex::encode(&digest),
                    "base64" => base64::engine::general_purpose::STANDARD.encode(&digest),
                    "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest),
                    "binary" | "latin1" => String::from_utf8_lossy(&digest).into_owned(),
                    _ => hex::encode(&digest),
                };
                let s = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
                f64::from_bits(0x7FFF_0000_0000_0000u64 | ((s as u64) & 0x0000_FFFF_FFFF_FFFF))
            }
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_hmac_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "digest" => b"digest",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

// ---------------------------------------------------------------------------
// Cipher handle — powers `crypto.createCipheriv(alg, key, iv)` /
// `crypto.createDecipheriv(alg, key, iv)` followed by `.update(buf)` /
// `.final()` / `.getAuthTag()` / `.setAuthTag(buf)` (issue #1075).
//
// Mirrors the HashHandle shape above: `js_crypto_create_cipheriv` allocates
// a CipherHandle in the common handle registry and returns a small-integer
// handle NaN-boxed with POINTER_TAG. The runtime's small-pointer detection
// in `js_native_call_method` then routes subsequent method calls through
// HANDLE_METHOD_DISPATCH → `dispatch_cipher` below.
//
// Supported algorithms (priority order, what new code wants first):
//   - aes-256-gcm  (authenticated, 12-byte IV, 16-byte auth tag)
//   - aes-128-gcm  (authenticated, 12-byte IV, 16-byte auth tag)
//   - aes-256-cbc  (legacy/compat, 16-byte IV, PKCS7 padding)
//   - aes-128-cbc  (legacy/compat, 16-byte IV, PKCS7 padding)
//
// Buffer.update(plain).final() returns ciphertext bytes; for GCM the auth
// tag is appended to the AEAD output and split out by `getAuthTag()` once
// `final()` has run. For decrypt-side GCM, `setAuthTag(buf)` must be called
// before `final()` so the verifier can authenticate.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum CipherKind {
    Aes128Cbc,
    Aes192Cbc,
    Aes256Cbc,
    Aes128Ecb,
    Aes192Ecb,
    Aes256Ecb,
    Aes128Wrap,
    Aes192Wrap,
    Aes256Wrap,
    Aes128Gcm,
    Aes192Gcm,
    Aes256Gcm,
}

impl CipherKind {
    fn parse(alg: &str) -> Option<Self> {
        match alg.to_ascii_lowercase().as_str() {
            "aes-128-cbc" => Some(Self::Aes128Cbc),
            "aes-192-cbc" => Some(Self::Aes192Cbc),
            "aes-256-cbc" => Some(Self::Aes256Cbc),
            "aes-128-ecb" => Some(Self::Aes128Ecb),
            "aes-192-ecb" => Some(Self::Aes192Ecb),
            "aes-256-ecb" => Some(Self::Aes256Ecb),
            "id-aes128-wrap" | "aes128-wrap" => Some(Self::Aes128Wrap),
            "id-aes192-wrap" | "aes192-wrap" => Some(Self::Aes192Wrap),
            "id-aes256-wrap" | "aes256-wrap" => Some(Self::Aes256Wrap),
            "aes-128-gcm" => Some(Self::Aes128Gcm),
            "aes-192-gcm" => Some(Self::Aes192Gcm),
            "aes-256-gcm" => Some(Self::Aes256Gcm),
            _ => None,
        }
    }

    fn key_len(self) -> usize {
        match self {
            Self::Aes128Cbc | Self::Aes128Ecb | Self::Aes128Wrap | Self::Aes128Gcm => 16,
            Self::Aes192Cbc | Self::Aes192Ecb | Self::Aes192Wrap | Self::Aes192Gcm => 24,
            Self::Aes256Cbc | Self::Aes256Ecb | Self::Aes256Wrap | Self::Aes256Gcm => 32,
        }
    }

    fn is_gcm(self) -> bool {
        matches!(self, Self::Aes128Gcm | Self::Aes192Gcm | Self::Aes256Gcm)
    }

    fn is_ecb(self) -> bool {
        matches!(self, Self::Aes128Ecb | Self::Aes192Ecb | Self::Aes256Ecb)
    }

    fn is_wrap(self) -> bool {
        matches!(self, Self::Aes128Wrap | Self::Aes192Wrap | Self::Aes256Wrap)
    }
}

// CBC type aliases (Aes256CbcEnc/Dec already exist above for aes-256-cbc).
type Aes128CbcEnc = Encryptor<Aes128>;
type Aes128CbcDec = Decryptor<Aes128>;
type Aes192CbcEnc = Encryptor<Aes192>;
type Aes192CbcDec = Decryptor<Aes192>;
type Aes128EcbEnc = ecb::Encryptor<Aes128>;
type Aes128EcbDec = ecb::Decryptor<Aes128>;
type Aes192EcbEnc = ecb::Encryptor<Aes192>;
type Aes192EcbDec = ecb::Decryptor<Aes192>;
type Aes256EcbEnc = ecb::Encryptor<Aes256>;
type Aes256EcbDec = ecb::Decryptor<Aes256>;
type Aes192Gcm = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12>;
type Aes128Gcm12 = aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U12>;
type Aes128Gcm13 = aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U13>;
type Aes128Gcm14 = aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U14>;
type Aes128Gcm15 = aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U15>;
type Aes192Gcm12 = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U12>;
type Aes192Gcm13 = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U13>;
type Aes192Gcm14 = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U14>;
type Aes192Gcm15 = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U15>;
type Aes256Gcm12 = aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U12>;
type Aes256Gcm13 = aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U13>;
type Aes256Gcm14 = aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U14>;
type Aes256Gcm15 = aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U15>;

fn decrypt_gcm128_with_tag_len(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Nonce};
    let nonce = Nonce::from_slice(iv);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    match tag.len() {
        12 => Aes128Gcm12::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        13 => Aes128Gcm13::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        14 => Aes128Gcm14::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        15 => Aes128Gcm15::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        16 => Aes128Gcm::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        _ => None,
    }
}

fn decrypt_gcm192_with_tag_len(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::Nonce;
    let nonce = Nonce::from_slice(iv);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    match tag.len() {
        12 => Aes192Gcm12::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        13 => Aes192Gcm13::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        14 => Aes192Gcm14::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        15 => Aes192Gcm15::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        16 => Aes192Gcm::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        _ => None,
    }
}

fn decrypt_gcm256_with_tag_len(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let nonce = Nonce::from_slice(iv);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    match tag.len() {
        12 => Aes256Gcm12::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        13 => Aes256Gcm13::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        14 => Aes256Gcm14::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        15 => Aes256Gcm15::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        16 => Aes256Gcm::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        _ => None,
    }
}

/// Per-handle cipher state. CBC ciphers accumulate plaintext (or
/// ciphertext, on decrypt) in `buffer` until `.final()` runs the
/// single-shot encryptor/decryptor — block ciphers can't safely emit
/// partial output without buffering the trailing fragment for PKCS7
/// padding anyway, and bouncing through the `_padded_mut` API keeps
/// the implementation small. GCM uses `aes_gcm::Aes256Gcm::encrypt`
/// / `decrypt` which are one-shot AEAD ops, so the same "buffer and
/// flush on final" shape applies there.
pub struct CipherHandle {
    state: std::sync::Mutex<CipherState>,
}

struct CipherState {
    kind: CipherKind,
    encrypt: bool,
    key: Vec<u8>,
    iv: Vec<u8>,
    auth_tag_len: usize,
    buffer: Vec<u8>,
    /// For GCM encrypt: filled in by `.final()`, read by `.getAuthTag()`.
    /// For GCM decrypt: set by `.setAuthTag(tag)` and consumed at `.final()`.
    auth_tag: Option<Vec<u8>>,
    aad: Vec<u8>,
    auto_padding: bool,
    finished: bool,
}

#[inline]
fn nanbox_pointer_f64(ptr: usize) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[inline]
fn nanbox_undefined() -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

unsafe fn create_cipher_handle(
    alg_ptr: i64,
    key_ptr: i64,
    iv_ptr: i64,
    options_bits: f64,
    encrypt: bool,
) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes).unwrap_or("");
    let kind = match CipherKind::parse(alg) {
        Some(k) => k,
        None => return nanbox_undefined(),
    };
    let key = bytes_from_ptr(key_ptr);
    let iv = bytes_from_ptr(iv_ptr);
    if key.len() != kind.key_len() {
        return nanbox_undefined();
    }
    // GCM accepts a 12-byte nonce (recommended) or any non-empty IV; we
    // require 12 to match what Node verifies against the standard AES-GCM
    // implementations. CBC requires exactly 16 (one block).
    if kind.is_gcm() {
        if iv.is_empty() {
            return nanbox_undefined();
        }
    } else if kind.is_ecb() {
        if !iv.is_empty() {
            return nanbox_undefined();
        }
    } else if kind.is_wrap() {
        if iv.len() != 8 {
            return nanbox_undefined();
        }
    } else if iv.len() != 16 {
        return nanbox_undefined();
    }
    // Node permits GCM auth-tag lengths {4, 8, 12, 13, 14, 15, 16}; values
    // outside that set throw. The RustCrypto Aes*Gcm backend only natively
    // produces 12-16 byte tags, but the cipher state truncates the tag
    // down to `auth_tag_len` before `getAuthTag()` returns, so a request
    // for 4 / 8 still produces a tag with the expected length. Filter to
    // 1..=16 (Node-superset; out-of-range falls through to the default).
    let auth_tag_len = if kind.is_gcm() {
        object_field_bits(options_bits.to_bits(), b"authTagLength")
            .and_then(|bits| nanboxed_to_usize(f64::from_bits(bits)))
            .filter(|len| (1..=16).contains(len))
            .unwrap_or(16)
    } else {
        0
    };
    let handle: Handle = register_handle(CipherHandle {
        state: std::sync::Mutex::new(CipherState {
            kind,
            encrypt,
            key,
            iv,
            auth_tag_len,
            buffer: Vec::new(),
            auth_tag: None,
            aad: Vec::new(),
            auto_padding: true,
            finished: false,
        }),
    });
    nanbox_pointer_f64(handle as usize)
}

/// `crypto.createCipheriv(alg, key, iv)` — register a CipherHandle for
/// encryption and return its handle NaN-boxed as POINTER_TAG.
///
/// # Safety
/// Pointers must point at a Buffer or StringHeader (both layouts are
/// handled by `bytes_from_ptr`).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_cipheriv(
    alg_ptr: i64,
    key_ptr: i64,
    iv_ptr: i64,
    options_bits: f64,
) -> f64 {
    create_cipher_handle(alg_ptr, key_ptr, iv_ptr, options_bits, true)
}

/// `crypto.createDecipheriv(alg, key, iv)` — register a CipherHandle for
/// decryption and return its handle NaN-boxed as POINTER_TAG.
///
/// # Safety
/// Pointers must point at a Buffer or StringHeader (both layouts are
/// handled by `bytes_from_ptr`).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_decipheriv(
    alg_ptr: i64,
    key_ptr: i64,
    iv_ptr: i64,
    options_bits: f64,
) -> f64 {
    create_cipher_handle(alg_ptr, key_ptr, iv_ptr, options_bits, false)
}

/// Dispatch `update` / `final` / `getAuthTag` / `setAuthTag` on a
/// CipherHandle. Called from `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_cipher(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<CipherHandle>(handle) {
        Some(h) => h,
        None => return nanbox_undefined(),
    };
    let mut guard = h.state.lock().unwrap();
    let state = &mut *guard;
    match method {
        // `.update(buf)` — accumulate plaintext / ciphertext. Node returns
        // an incremental chunk here; for CBC/GCM we can safely return an
        // empty Buffer and emit everything at `.final()` because
        // `Buffer.concat([cipher.update(x), cipher.final()])` is what the
        // overwhelming majority of callers do. This matches `Buffer.concat`
        // length-wise (empty + total == total) and avoids the partial-block
        // bookkeeping that streaming CBC would need.
        "update" => {
            if state.finished {
                return nanbox_undefined();
            }
            if args.is_empty() {
                let buf = alloc_buffer_from_slice(&[]);
                return nanbox_pointer_f64(buf as usize);
            }
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            state.buffer.extend_from_slice(&bytes);
            let buf = alloc_buffer_from_slice(&[]);
            nanbox_pointer_f64(buf as usize)
        }
        // `.final()` — runs the actual encrypt/decrypt and returns the
        // full output. For GCM-encrypt this also stashes the 16-byte auth
        // tag in `auth_tag` for a subsequent `.getAuthTag()` call.
        "final" => {
            if state.finished {
                let buf = alloc_buffer_from_slice(&[]);
                return nanbox_pointer_f64(buf as usize);
            }
            state.finished = true;
            let plaintext_or_ct = std::mem::take(&mut state.buffer);
            let output: Vec<u8> = match (state.kind, state.encrypt) {
                (CipherKind::Aes256Cbc, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes256CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Cbc, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes128CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Cbc, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes192CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes256Ecb, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher =
                        Aes256EcbEnc::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Ecb, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher =
                        Aes192EcbEnc::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Ecb, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher =
                        Aes128EcbEnc::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Wrap, true) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes128};
                    let kw =
                        KwAes128::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len() + 8];
                    match kw.wrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes192Wrap, true) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes192};
                    let kw =
                        KwAes192::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len() + 8];
                    match kw.wrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Wrap, true) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes256};
                    let kw =
                        KwAes256::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len() + 8];
                    match kw.wrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes256CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes128CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes192CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes256Ecb, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher =
                        Aes256EcbDec::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Ecb, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher =
                        Aes192EcbDec::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Ecb, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher =
                        Aes128EcbDec::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Wrap, false) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes128};
                    let kw =
                        KwAes128::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len().saturating_sub(8)];
                    match kw.unwrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes192Wrap, false) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes192};
                    let kw =
                        KwAes192::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len().saturating_sub(8)];
                    match kw.unwrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Wrap, false) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes256};
                    let kw =
                        KwAes256::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len().saturating_sub(8)];
                    match kw.unwrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit, Payload};
                    use aes_gcm::{Aes256Gcm, Nonce};
                    let cipher = match Aes256Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let payload = Payload {
                        msg: plaintext_or_ct.as_ref(),
                        aad: state.aad.as_ref(),
                    };
                    let mut ct = match cipher.encrypt(nonce, payload) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    // aes-gcm appends the 16-byte tag to the ciphertext.
                    // Node's createCipheriv splits these: update/final
                    // produces just the ciphertext, getAuthTag returns
                    // the tag separately.
                    let tag = ct.split_off(ct.len().saturating_sub(16));
                    state.auth_tag = Some(tag[..state.auth_tag_len.min(tag.len())].to_vec());
                    ct
                }
                (CipherKind::Aes128Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit, Payload};
                    use aes_gcm::{Aes128Gcm, Nonce};
                    let cipher = match Aes128Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let payload = Payload {
                        msg: plaintext_or_ct.as_ref(),
                        aad: state.aad.as_ref(),
                    };
                    let mut ct = match cipher.encrypt(nonce, payload) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    let tag = ct.split_off(ct.len().saturating_sub(16));
                    state.auth_tag = Some(tag[..state.auth_tag_len.min(tag.len())].to_vec());
                    ct
                }
                (CipherKind::Aes192Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit, Payload};
                    use aes_gcm::Nonce;
                    let cipher = match Aes192Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let payload = Payload {
                        msg: plaintext_or_ct.as_ref(),
                        aad: state.aad.as_ref(),
                    };
                    let mut ct = match cipher.encrypt(nonce, payload) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    let tag = ct.split_off(ct.len().saturating_sub(16));
                    state.auth_tag = Some(tag[..state.auth_tag_len.min(tag.len())].to_vec());
                    ct
                }
                (CipherKind::Aes256Gcm, false) => {
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == state.auth_tag_len => t.clone(),
                        _ => return nanbox_undefined(), // GCM decrypt needs tag
                    };
                    match decrypt_gcm256_with_tag_len(
                        &state.key,
                        &state.iv,
                        &state.aad,
                        &plaintext_or_ct,
                        &tag,
                    ) {
                        Some(pt) => pt,
                        None => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes192Gcm, false) => {
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == state.auth_tag_len => t.clone(),
                        _ => return nanbox_undefined(),
                    };
                    match decrypt_gcm192_with_tag_len(
                        &state.key,
                        &state.iv,
                        &state.aad,
                        &plaintext_or_ct,
                        &tag,
                    ) {
                        Some(pt) => pt,
                        None => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes128Gcm, false) => {
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == state.auth_tag_len => t.clone(),
                        _ => return nanbox_undefined(),
                    };
                    match decrypt_gcm128_with_tag_len(
                        &state.key,
                        &state.iv,
                        &state.aad,
                        &plaintext_or_ct,
                        &tag,
                    ) {
                        Some(pt) => pt,
                        None => return nanbox_undefined(),
                    }
                }
            };
            let buf = alloc_buffer_from_slice(&output);
            nanbox_pointer_f64(buf as usize)
        }
        // `.getAuthTag()` — GCM-encrypt only. Returns the 16-byte tag
        // that `.final()` stashed. Calling this before `.final()` (or on
        // a non-GCM cipher) yields undefined.
        "getAuthTag" => match state.auth_tag.as_ref() {
            Some(tag) => {
                let buf = alloc_buffer_from_slice(tag);
                nanbox_pointer_f64(buf as usize)
            }
            None => nanbox_undefined(),
        },
        // `.setAuthTag(tag)` — GCM-decrypt only. Stores the tag so
        // `.final()` can authenticate. Returns the handle (Node returns
        // `this`); the chain-call surface in Perry doesn't rely on the
        // return shape, but mirroring Node's API matters for the rare
        // chained `d.setAuthTag(t).update(x).final()` case.
        "setAuthTag" => {
            if args.is_empty() {
                return nanbox_undefined();
            }
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let tag = bytes_from_ptr(ptr);
            state.auth_tag = Some(tag);
            nanbox_pointer_f64(handle as usize)
        }
        // `.setAAD(buf)` — bind additional authenticated data for GCM.
        "setAAD" => {
            if args.is_empty() {
                state.aad.clear();
            } else {
                let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                state.aad = bytes_from_ptr(ptr);
            }
            nanbox_pointer_f64(handle as usize)
        }
        // `.setAutoPadding([autoPadding])` — Node defaults to PKCS#7
        // padding for CBC/ECB and allows callers to disable it for exact
        // block-size inputs. Return `this` for chaining.
        "setAutoPadding" => {
            state.auto_padding = args.first().copied().map(js_truthy).unwrap_or(true);
            nanbox_pointer_f64(handle as usize)
        }
        _ => nanbox_undefined(),
    }
}

/// Property reads on a CipherHandle — `c.getAuthTag` / `c.setAuthTag` /
/// `c.update` / `c.final` / `c.setAAD`. Issue #1111: without this,
/// `c.getAuthTag?.()` short-circuited because the property access
/// returned undefined (small handles have no field storage), so the
/// `?.` lowering's `c.getAuthTag == null` check fired and the call
/// never happened.
///
/// Each known method name returns a bound-method closure (via
/// `js_class_method_bind`) whose `this` is the POINTER_TAG-NaN-boxed
/// handle. When invoked the closure routes through
/// `js_native_call_method` → `HANDLE_METHOD_DISPATCH` → `dispatch_cipher`,
/// the exact path `c.method(args)` takes when called inline. So
/// `typeof c.getAuthTag === "function"` and `const g = c.getAuthTag; g()`
/// both work, mirroring Node's `Cipher` shape.
pub unsafe fn dispatch_cipher_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "final" => b"final",
        "getAuthTag" => b"getAuthTag",
        "setAuthTag" => b"setAuthTag",
        "setAAD" => b"setAAD",
        "setAutoPadding" => b"setAutoPadding",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}
