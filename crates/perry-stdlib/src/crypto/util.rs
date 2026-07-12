pub(super) use crate::common::handle::{get_handle_mut, register_handle, Handle};
pub(super) use aes::{Aes128, Aes192, Aes256};
pub(super) use base64::Engine as _;
pub(super) use cbc::{
    cipher::{
        block_padding::{NoPadding, Pkcs7},
        BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit,
    },
    Decryptor, Encryptor,
};
pub(super) use hkdf::Hkdf;
pub(super) use md5::{Digest as Md5Digest, Md5};
pub(super) use p256::ecdh::diffie_hellman as p256_diffie_hellman;
pub(super) use p256::ecdsa::{Signature as P256EcdsaSignature, SigningKey as P256EcdsaSigningKey};
pub(super) use p256::elliptic_curve::sec1::ToEncodedPoint;
pub(super) use p256::pkcs8::{
    EncodePrivateKey as P256EncodePrivateKey, EncodePublicKey as P256EncodePublicKey,
};
pub(super) use p256::{PublicKey as P256PublicKey, SecretKey as P256SecretKey};
pub(super) use perry_runtime::{
    js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name,
    js_string_from_bytes, JSValue, ObjectHeader, StringHeader,
};
pub(super) use rand::{Rng, RngCore};
pub(super) use rsa::pkcs1v15::{
    Pkcs1v15Sign, Signature as RsaPkcs1v15Signature, SigningKey, VerifyingKey,
};
pub(super) use rsa::pss::{
    Signature as RsaPssSignature, SigningKey as RsaPssSigningKey,
    VerifyingKey as RsaPssVerifyingKey,
};
pub(super) use rsa::sha2::{Sha256 as RsaSha256, Sha384 as RsaSha384, Sha512 as RsaSha512};
pub(super) use rsa::signature::{RandomizedSigner, SignatureEncoding, Signer, Verifier};
pub(super) use rsa::traits::{PrivateKeyParts, PublicKeyParts};
pub(super) use rsa::Oaep;
pub(super) use rsa::{BigUint as RsaBigUint, RsaPrivateKey, RsaPublicKey};
pub(super) use sha1::Sha1;
pub(super) use sha2::{Digest as Sha256Digest, Sha224, Sha256, Sha384, Sha512, Sha512_256};
// SHAKE128/256 moved out of `sha3`'s root into the separate `shake` crate
// as of sha3 0.12 (RustCrypto/hashes#869).
pub(super) use shake::{ExtendableOutput, Shake128, Shake256, XofReader};

/// Helper to extract string from StringHeader pointer
pub(super) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
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
pub(super) unsafe fn bytes_from_ptr(ptr: i64) -> Vec<u8> {
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
pub(super) unsafe fn alloc_buffer_from_slice(
    bytes: &[u8],
) -> *mut perry_runtime::buffer::BufferHeader {
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
pub(super) enum RsaDigestKind {
    Sha256,
    Sha384,
    Sha512,
}

pub(super) fn normalize_sign_algorithm(algorithm: &[u8]) -> Option<RsaDigestKind> {
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

pub(crate) fn parse_rsa_private_key_pem(pem: &str) -> Option<RsaPrivateKey> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .ok()
}

pub(super) fn parse_rsa_encrypted_private_key_pem(
    pem: &str,
    passphrase: &[u8],
) -> Option<RsaPrivateKey> {
    use rsa::pkcs8::DecodePrivateKey;

    RsaPrivateKey::from_pkcs8_encrypted_pem(pem, passphrase).ok()
}

pub(super) fn rsa_private_key_to_pem(private_key: &RsaPrivateKey) -> Option<String> {
    use rsa::pkcs8::EncodePrivateKey;

    private_key
        .to_pkcs8_pem(Default::default())
        .ok()
        .map(|pem| pem.to_string())
}

pub(crate) fn parse_rsa_public_key_pem(pem: &str) -> Option<RsaPublicKey> {
    use rsa::pkcs1::DecodeRsaPublicKey;
    use rsa::pkcs8::DecodePublicKey;

    RsaPublicKey::from_public_key_pem(pem)
        .or_else(|_| RsaPublicKey::from_pkcs1_pem(pem))
        .ok()
        .or_else(|| parse_rsa_private_key_pem(pem).map(RsaPublicKey::from))
}

pub(super) fn rsa_public_key_to_pem(public_key: &RsaPublicKey) -> Option<String> {
    use rsa::pkcs1::EncodeRsaPublicKey;
    use rsa::pkcs8::EncodePublicKey;

    public_key
        .to_public_key_pem(Default::default())
        .ok()
        .or_else(|| public_key.to_pkcs1_pem(Default::default()).ok())
        .map(|pem| pem.to_string())
}

pub(crate) fn parse_p256_signing_key_pem(pem: &str) -> Option<P256EcdsaSigningKey> {
    use p256::pkcs8::DecodePrivateKey;

    P256EcdsaSigningKey::from_pkcs8_pem(pem).ok()
}

pub(crate) fn parse_p256_verifying_key_pem(pem: &str) -> Option<p256::ecdsa::VerifyingKey> {
    use p256::pkcs8::{DecodePrivateKey, DecodePublicKey};

    p256::ecdsa::VerifyingKey::from_public_key_pem(pem)
        .ok()
        .or_else(|| {
            P256EcdsaSigningKey::from_pkcs8_pem(pem)
                .ok()
                .map(|key| *key.verifying_key())
        })
}

pub(super) const ED25519_PRIVATE_PREFIX: &str = "PERRY-ED25519-PRIVATE:";
pub(super) const ED25519_PUBLIC_PREFIX: &str = "PERRY-ED25519-PUBLIC:";
pub(super) const X25519_PRIVATE_PREFIX: &str = "PERRY-X25519-PRIVATE:";
pub(super) const X25519_PUBLIC_PREFIX: &str = "PERRY-X25519-PUBLIC:";

/// `pub(crate)` (not `pub(super)`): the WebCrypto bridge builds the very same
/// surrogate strings when `KeyObject.from()` converts an asymmetric CryptoKey
/// back into a KeyObject (#6302), and both sides must agree byte-for-byte or
/// the `parse_*_surrogate` readers below reject the result.
pub(crate) fn ed25519_private_surrogate(key: &ed25519_dalek::SigningKey) -> String {
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.to_bytes());
    let public =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes());
    format!("{ED25519_PRIVATE_PREFIX}{secret}.{public}")
}

pub(crate) fn ed25519_public_surrogate(key: &ed25519_dalek::VerifyingKey) -> String {
    let public = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.to_bytes());
    format!("{ED25519_PUBLIC_PREFIX}{public}")
}

pub(crate) fn parse_ed25519_private_surrogate(value: &str) -> Option<ed25519_dalek::SigningKey> {
    let rest = value.strip_prefix(ED25519_PRIVATE_PREFIX)?;
    let secret_b64 = rest.split('.').next()?;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(secret_b64.as_bytes())
        .ok()?;
    let secret: [u8; 32] = secret.as_slice().try_into().ok()?;
    Some(ed25519_dalek::SigningKey::from_bytes(&secret))
}

pub(crate) fn parse_ed25519_public_surrogate(value: &str) -> Option<ed25519_dalek::VerifyingKey> {
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

pub(crate) fn x25519_private_surrogate(secret: &[u8; 32]) -> String {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret);
    format!("{X25519_PRIVATE_PREFIX}{encoded}")
}

pub(crate) fn x25519_public_surrogate(public: &[u8; 32]) -> String {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public);
    format!("{X25519_PUBLIC_PREFIX}{encoded}")
}

pub(crate) fn parse_x25519_private_surrogate(value: &str) -> Option<[u8; 32]> {
    let rest = value.strip_prefix(X25519_PRIVATE_PREFIX)?;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(rest.as_bytes())
        .ok()?;
    secret.as_slice().try_into().ok()
}

pub(crate) fn parse_x25519_public_surrogate(value: &str) -> Option<[u8; 32]> {
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

/// Buffer-style string encoding tag, matching `js_buffer_from_string` /
/// `js_buffer_to_string`:
/// 0 = utf8, 1 = hex, 2 = base64, 3 = base64url, 4 = latin1/binary,
/// 5 = ascii, 6 = utf16le/ucs2. Used by `crypto.createSecretKey(str, enc)`
/// (#2954) and `cipher.update(str, inputEnc, outputEnc)` / `cipher.final(enc)`
/// (#3381) so crypto reuses the exact same Node-compatible string decoding /
/// encoding rules as `Buffer`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct EncodingTag(pub i32);

/// Map a lowercase encoding name to its Buffer tag, or `None` for an unknown
/// name. Node throws `TypeError [ERR_UNKNOWN_ENCODING]` for unknown names.
pub(super) fn encoding_tag_from_name(name: &str) -> Option<EncodingTag> {
    let tag = match name {
        "utf8" | "utf-8" => 0,
        "hex" => 1,
        "base64" => 2,
        "base64url" => 3,
        "latin1" | "binary" => 4,
        "ascii" => 5,
        "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => 6,
        _ => return None,
    };
    Some(EncodingTag(tag))
}

/// Throw Node's `TypeError [ERR_UNKNOWN_ENCODING]: Unknown encoding: <name>`.
pub(super) unsafe fn throw_unknown_encoding(name: &str) -> ! {
    let message = format!("Unknown encoding: {name}");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, "ERR_UNKNOWN_ENCODING");
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

/// Decode `str_bytes` (a JS string's UTF-8 bytes) into raw bytes per `tag`,
/// reusing the runtime's Node-lenient hex/base64 decoders and latin1/utf16le
/// transforms.
pub(super) fn decode_string_bytes_with_tag(str_bytes: &[u8], tag: EncodingTag) -> Vec<u8> {
    match tag.0 {
        1 => perry_runtime::buffer::decode_hex(str_bytes),
        2 | 3 => perry_runtime::buffer::decode_base64(str_bytes),
        4 | 5 => {
            // latin1 / ascii input: low byte of each code point.
            let decoded = String::from_utf8_lossy(str_bytes);
            decoded.chars().map(|ch| (ch as u32 & 0xFF) as u8).collect()
        }
        6 => {
            // utf16le / ucs2 input.
            let decoded = String::from_utf8_lossy(str_bytes);
            let mut out = Vec::with_capacity(decoded.len() * 2);
            for unit in decoded.encode_utf16() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            out
        }
        _ => str_bytes.to_vec(),
    }
}

/// Encode raw `bytes` into a JS string per `tag`, reusing the runtime's
/// `Buffer#toString(encoding)` codec. Returns a NaN-boxed STRING_TAG value.
pub(super) unsafe fn encode_bytes_with_tag(bytes: &[u8], tag: EncodingTag) -> f64 {
    let buf = alloc_buffer_from_slice(bytes);
    if buf.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let s = perry_runtime::buffer::js_buffer_to_string(buf as *const _, tag.0);
    nanbox_str(s)
}

/// If `arg` is a JS string, return its encoding tag (throwing on unknown
/// names). Returns `None` when the arg is absent or not a string (the
/// Buffer-input / Buffer-output overload). `Some(None)` is impossible —
/// unknown names throw, matching Node.
pub(super) unsafe fn encoding_tag_from_arg(arg: Option<f64>) -> Option<EncodingTag> {
    let v = arg?;
    let s = string_from_jsvalue(v.to_bits())?;
    let lower = s.to_ascii_lowercase();
    match encoding_tag_from_name(&lower) {
        Some(tag) => Some(tag),
        None => throw_unknown_encoding(&s),
    }
}

/// Extract the UTF-8 bytes of a string argument (NaN-boxed STRING_TAG or SSO
/// short string), falling back to `bytes_from_ptr` for a Buffer/StringHeader
/// pointer. Used by `cipher.update(str, inputEncoding, ...)` (#3381).
pub(super) unsafe fn string_bytes_from_arg(arg: f64) -> Vec<u8> {
    if let Some(s) = string_from_jsvalue(arg.to_bits()) {
        return s.into_bytes();
    }
    let ptr = (arg.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
    bytes_from_ptr(ptr)
}

pub(super) fn js_bool(b: bool) -> f64 {
    f64::from_bits(JSValue::bool(b).bits())
}

pub(super) fn js_truthy(v: f64) -> bool {
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

pub(super) fn nanbox_str(ptr: *mut StringHeader) -> f64 {
    f64::from_bits(0x7FFF_0000_0000_0000u64 | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
}

pub(super) unsafe fn mark_keyobject_string(ptr: *mut StringHeader, kind: KeyKind, asym_type: u8) {
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
pub(super) enum KeyKind {
    Private,
    Public,
}

pub(super) fn classify_private_key_surrogate(pem: &str) -> Option<u8> {
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

pub(super) fn classify_public_key_surrogate(pem: &str) -> Option<u8> {
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

pub(super) unsafe fn object_field_bits(obj_bits: u64, name: &[u8]) -> Option<u64> {
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

pub(super) unsafe fn object_field_string(obj_bits: u64, name: &[u8]) -> Option<String> {
    string_from_jsvalue(object_field_bits(obj_bits, name)?)
}

pub(super) unsafe fn bytes_from_value_bits(bits: u64) -> Option<Vec<u8>> {
    if let Some(s) = string_from_jsvalue(bits) {
        return Some(s.into_bytes());
    }
    let ptr = bits & 0x0000_FFFF_FFFF_FFFF;
    if ptr < 0x1000 {
        return None;
    }
    Some(bytes_from_ptr(ptr as i64))
}

pub(super) unsafe fn object_field_bytes(obj_bits: u64, name: &[u8]) -> Option<Vec<u8>> {
    bytes_from_value_bits(object_field_bits(obj_bits, name)?)
}

pub(super) fn b64u_decode_uint(s: &str) -> Option<RsaBigUint> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .ok()?;
    Some(RsaBigUint::from_bytes_be(&bytes))
}

pub(super) fn b64u_uint(value: &RsaBigUint) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_bytes_be())
}

pub(super) unsafe fn jwk_uint_field(obj_bits: u64, name: &[u8]) -> Option<RsaBigUint> {
    b64u_decode_uint(&object_field_string(obj_bits, name)?)
}

pub(super) unsafe fn set_object_string_field(obj: *mut ObjectHeader, name: &[u8], value: &str) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_object_set_field_by_name(obj, key, nanbox_str(val));
}

pub(super) unsafe fn set_object_value_field(obj: *mut ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

pub(super) fn nanbox_pointer(ptr: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

pub(super) unsafe fn rsa_public_jwk_object(public_key: &RsaPublicKey) -> Option<*mut ObjectHeader> {
    let obj = js_object_alloc(0, 3);
    if obj.is_null() {
        return None;
    }
    set_object_string_field(obj, b"kty", "RSA");
    set_object_string_field(obj, b"n", &b64u_uint(public_key.n()));
    set_object_string_field(obj, b"e", &b64u_uint(public_key.e()));
    Some(obj)
}

pub(super) unsafe fn rsa_private_jwk_object(
    private_key: &RsaPrivateKey,
) -> Option<*mut ObjectHeader> {
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

pub(super) unsafe fn ec_p256_public_jwk_object(
    public_key: &P256PublicKey,
) -> Option<*mut ObjectHeader> {
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

pub(super) unsafe fn ec_p256_private_jwk_object(
    private_key: &P256SecretKey,
) -> Option<*mut ObjectHeader> {
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

pub(super) unsafe fn jwk_rsa_private_to_pem(jwk_bits: u64) -> Option<String> {
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

pub(super) unsafe fn jwk_rsa_public_to_pem(jwk_bits: u64) -> Option<String> {
    let kty = object_field_string(jwk_bits, b"kty")?;
    if kty != "RSA" {
        return None;
    }
    let n = jwk_uint_field(jwk_bits, b"n")?;
    let e = jwk_uint_field(jwk_bits, b"e")?;
    let key = RsaPublicKey::new(n, e).ok()?;
    rsa_public_key_to_pem(&key)
}

pub(super) unsafe fn jwk_ec_private_to_pem(jwk_bits: u64) -> Option<String> {
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

pub(super) unsafe fn jwk_ec_public_to_pem(jwk_bits: u64) -> Option<String> {
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

pub(super) unsafe fn crypto_key_input_to_private_pem(value_bits: u64) -> Option<String> {
    let format = object_field_string(value_bits, b"format");
    if let Some(key_bits) = object_field_bits(value_bits, b"key") {
        if matches!(format.as_deref(), Some(f) if f.eq_ignore_ascii_case("jwk")) {
            return jwk_rsa_private_to_pem(key_bits).or_else(|| jwk_ec_private_to_pem(key_bits));
        }
        let format_is_pem = format
            .as_deref()
            .map_or(true, |f| f.eq_ignore_ascii_case("pem"));
        if format_is_pem {
            if let Some(passphrase) = object_field_bytes(value_bits, b"passphrase") {
                let key_bytes = bytes_from_value_bits(key_bits)?;
                let pem = String::from_utf8(key_bytes).ok()?;
                if pem.contains("ENCRYPTED PRIVATE KEY") {
                    return parse_rsa_encrypted_private_key_pem(&pem, &passphrase)
                        .and_then(|key| rsa_private_key_to_pem(&key));
                }
            }
        }
        return crypto_key_input_to_private_pem(key_bits);
    }
    if matches!(format.as_deref(), Some(f) if f.eq_ignore_ascii_case("jwk")) {
        return jwk_rsa_private_to_pem(value_bits).or_else(|| jwk_ec_private_to_pem(value_bits));
    }
    let ptr = (value_bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    String::from_utf8(bytes_from_ptr(ptr)).ok()
}

pub(super) unsafe fn crypto_key_input_to_public_pem(value_bits: u64) -> Option<String> {
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

pub(super) unsafe fn key_input_uses_ieee_p1363(value_bits: u64) -> bool {
    matches!(
        object_field_string(value_bits, b"dsaEncoding").as_deref(),
        Some(enc) if enc.eq_ignore_ascii_case("ieee-p1363")
    )
}

pub(super) unsafe fn key_input_uses_rsa_pss(value_bits: u64) -> bool {
    matches!(object_field_bits(value_bits, b"padding"), Some(v) if f64::from_bits(v) as i32 == 6)
}

pub(super) unsafe fn key_input_pss_salt_len(value_bits: u64, alg: RsaDigestKind) -> usize {
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

pub(super) unsafe fn keygen_encoding_wants_jwk(options_bits: u64, field: &[u8]) -> bool {
    let Some(encoding_bits) = object_field_bits(options_bits, field) else {
        return false;
    };
    matches!(
        object_field_string(encoding_bits, b"format").as_deref(),
        Some(format) if format.eq_ignore_ascii_case("jwk")
    )
}

pub(super) unsafe fn keygen_rsa_private_encryption_passphrase(
    options_bits: u64,
) -> Option<Vec<u8>> {
    let encoding_bits = object_field_bits(options_bits, b"privateKeyEncoding")?;
    let format = object_field_string(encoding_bits, b"format")?;
    if !format.eq_ignore_ascii_case("pem") {
        return None;
    }
    let typ = object_field_string(encoding_bits, b"type")?;
    if !typ.eq_ignore_ascii_case("pkcs8") {
        return None;
    }
    let cipher = object_field_string(encoding_bits, b"cipher")?;
    if !cipher.eq_ignore_ascii_case("aes-256-cbc") {
        return None;
    }
    object_field_bytes(encoding_bits, b"passphrase")
}

pub(super) fn sign_rsa_data(
    alg: RsaDigestKind,
    private_key: RsaPrivateKey,
    data: &[u8],
) -> Vec<u8> {
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

pub(super) fn sign_rsa_pss_data(
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

pub(super) fn verify_rsa_data(
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

pub(super) fn verify_rsa_pss_data(
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

pub(super) fn rsa_public_unpad_pkcs1_type1(
    public_key: &RsaPublicKey,
    encrypted: &[u8],
) -> Option<Vec<u8>> {
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

pub(super) unsafe fn string_array(items: &[&str]) -> *mut perry_runtime::array::ArrayHeader {
    let mut arr = perry_runtime::js_array_alloc(items.len() as u32);
    for item in items {
        let s = js_string_from_bytes(item.as_ptr(), item.len() as u32);
        arr = perry_runtime::js_array_push(arr, JSValue::string_ptr(s));
    }
    arr
}
