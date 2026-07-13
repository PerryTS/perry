pub(super) use std::collections::HashMap;
pub(super) use std::sync::Mutex;

pub(super) use aes::cipher::{
    generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit as AesBlockKeyInit,
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
pub(super) use p256::ecdsa::signature::{
    RandomizedSigner as SignatureRandomizedSigner, Signer as EcdsaSigner, Verifier as EcdsaVerifier,
};
pub(super) use p256::ecdsa::{
    Signature as P256EcdsaSignature, SigningKey as P256EcdsaSigningKey,
    VerifyingKey as P256EcdsaVerifyingKey,
};
pub(super) use p256::elliptic_curve::sec1::ToEncodedPoint;
pub(super) use p256::{PublicKey as P256PublicKey, SecretKey as P256SecretKey};
pub(super) use p384::ecdh::diffie_hellman as p384_diffie_hellman;
pub(super) use p384::ecdsa::{
    Signature as P384EcdsaSignature, SigningKey as P384EcdsaSigningKey,
    VerifyingKey as P384EcdsaVerifyingKey,
};
pub(super) use p384::{PublicKey as P384PublicKey, SecretKey as P384SecretKey};
pub(super) use p521::ecdh::diffie_hellman as p521_diffie_hellman;
pub(super) use p521::ecdsa::{
    Signature as P521EcdsaSignature, SigningKey as P521EcdsaSigningKey,
    VerifyingKey as P521EcdsaVerifyingKey,
};
pub(super) use p521::{PublicKey as P521PublicKey, SecretKey as P521SecretKey};
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
pub(super) use rsa::signature::SignatureEncoding as RsaSignatureEncoding;
pub(super) use rsa::traits::{PrivateKeyParts, PublicKeyParts};
pub(super) use rsa::{BigUint as RsaBigUint, Oaep, RsaPrivateKey, RsaPublicKey};
pub(super) use sha1::Sha1;
pub(super) use sha2::{Digest as Sha2Digest, Sha256, Sha384, Sha512};

pub(super) use ml_kem::kem::KeyExport as MlKemKeyExport;
pub(super) use ml_kem::pkcs8::{
    DecodePrivateKey as MlKemDecodePrivateKey, DecodePublicKey as MlKemDecodePublicKey,
    EncodePrivateKey as MlKemEncodePrivateKey, EncodePublicKey as MlKemEncodePublicKey,
};

pub(super) use perry_runtime::{
    buffer::{buffer_alloc, buffer_data_mut, is_registered_buffer, BufferHeader},
    js_object_alloc, js_object_set_field_by_name, js_promise_resolved, JSValue, Promise,
    StringHeader,
};

extern "C" {
    fn js_buffer_register_external(addr: usize);
    fn js_buffer_mark_as_uint8array_external(addr: usize);
    fn js_buffer_mark_as_crypto_key_external(
        addr: usize,
        algo: u8,
        hash: u8,
        kind: u8,
        extractable: u8,
        usages: u32,
    );
}

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
    Argon2d,
    Argon2i,
    Argon2id,
    AesGcm,
    AesKw,
    AesCbc,
    AesCtr,
    ChaCha20Poly1305,
    AesOcb,
    EcdsaP256,
    EcdhP256,
    EcdsaP384,
    EcdhP384,
    EcdsaP521,
    EcdhP521,
    Ed25519,
    Ed448,
    X25519,
    X448,
    RsaOaep,
    RsassaPkcs1,
    RsaPss,
    Kmac128,
    Kmac256,
    MlKem512,
    MlKem768,
    MlKem1024,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum EcNamedCurve {
    P256,
    P384,
    P521,
}

pub(super) fn parse_ec_named_curve(name: &str) -> Option<EcNamedCurve> {
    match name.to_ascii_uppercase().as_str() {
        "P-256" | "PRIME256V1" | "SECP256R1" => Some(EcNamedCurve::P256),
        "P-384" | "SECP384R1" => Some(EcNamedCurve::P384),
        "P-521" | "SECP521R1" => Some(EcNamedCurve::P521),
        _ => None,
    }
}

pub(super) fn ecdsa_key_algo_for_curve(curve: EcNamedCurve) -> KeyAlgo {
    match curve {
        EcNamedCurve::P256 => KeyAlgo::EcdsaP256,
        EcNamedCurve::P384 => KeyAlgo::EcdsaP384,
        EcNamedCurve::P521 => KeyAlgo::EcdsaP521,
    }
}

pub(super) fn ecdh_key_algo_for_curve(curve: EcNamedCurve) -> KeyAlgo {
    match curve {
        EcNamedCurve::P256 => KeyAlgo::EcdhP256,
        EcNamedCurve::P384 => KeyAlgo::EcdhP384,
        EcNamedCurve::P521 => KeyAlgo::EcdhP521,
    }
}

pub(super) fn ec_curve_for_key_algo(algo: KeyAlgo) -> Option<EcNamedCurve> {
    match algo {
        KeyAlgo::EcdsaP256 | KeyAlgo::EcdhP256 => Some(EcNamedCurve::P256),
        KeyAlgo::EcdsaP384 | KeyAlgo::EcdhP384 => Some(EcNamedCurve::P384),
        KeyAlgo::EcdsaP521 | KeyAlgo::EcdhP521 => Some(EcNamedCurve::P521),
        _ => None,
    }
}

pub(super) fn ec_curve_name(curve: EcNamedCurve) -> &'static str {
    match curve {
        EcNamedCurve::P256 => "P-256",
        EcNamedCurve::P384 => "P-384",
        EcNamedCurve::P521 => "P-521",
    }
}

pub(super) fn ec_curve_private_len(curve: EcNamedCurve) -> usize {
    match curve {
        EcNamedCurve::P256 => 32,
        EcNamedCurve::P384 => 48,
        EcNamedCurve::P521 => 66,
    }
}

pub(super) fn ec_curve_public_len(curve: EcNamedCurve) -> usize {
    1 + 2 * ec_curve_private_len(curve)
}

pub(super) fn ec_curve_hash(curve: EcNamedCurve) -> HashAlgo {
    match curve {
        EcNamedCurve::P256 => HashAlgo::Sha256,
        EcNamedCurve::P384 => HashAlgo::Sha384,
        EcNamedCurve::P521 => HashAlgo::Sha512,
    }
}

pub(super) fn is_ecdsa_key_algo(algo: KeyAlgo) -> bool {
    matches!(
        algo,
        KeyAlgo::EcdsaP256 | KeyAlgo::EcdsaP384 | KeyAlgo::EcdsaP521
    )
}

pub(super) fn is_ecdh_key_algo(algo: KeyAlgo) -> bool {
    matches!(
        algo,
        KeyAlgo::EcdhP256 | KeyAlgo::EcdhP384 | KeyAlgo::EcdhP521
    )
}

pub(super) fn is_ec_key_algo(algo: KeyAlgo) -> bool {
    is_ecdsa_key_algo(algo) || is_ecdh_key_algo(algo)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum KeyKind {
    Secret,
    Private,
    Public,
}

pub(super) const USAGE_ENCRYPT: u32 = 1 << 0;
pub(super) const USAGE_DECRYPT: u32 = 1 << 1;
pub(super) const USAGE_SIGN: u32 = 1 << 2;
pub(super) const USAGE_VERIFY: u32 = 1 << 3;
pub(super) const USAGE_DERIVE_KEY: u32 = 1 << 4;
pub(super) const USAGE_DERIVE_BITS: u32 = 1 << 5;
pub(super) const USAGE_WRAP_KEY: u32 = 1 << 6;
pub(super) const USAGE_UNWRAP_KEY: u32 = 1 << 7;
pub(super) const USAGE_ENCAPSULATE_BITS: u32 = 1 << 8;
pub(super) const USAGE_DECAPSULATE_BITS: u32 = 1 << 9;
pub(super) const USAGE_ENCAPSULATE_KEY: u32 = 1 << 10;
pub(super) const USAGE_DECAPSULATE_KEY: u32 = 1 << 11;

#[derive(Copy, Clone, Debug)]
pub(super) struct CryptoKeyMaterial {
    pub(super) algo: KeyAlgo,
    /// For HMAC: the underlying hash. For AES-GCM the hash slot is
    /// unused (we keep `HashAlgo::Sha256` as a harmless placeholder so
    /// the struct stays `Copy`).
    pub(super) hash: HashAlgo,
    pub(super) kind: KeyKind,
    pub(super) extractable: bool,
    pub(super) usages: u32,
}

impl CryptoKeyMaterial {
    pub(super) fn new(
        algo: KeyAlgo,
        hash: HashAlgo,
        kind: KeyKind,
        extractable: bool,
        usages: u32,
    ) -> Self {
        Self {
            algo,
            hash,
            kind,
            extractable,
            usages,
        }
    }
}

pub(super) static CRYPTO_KEY_REGISTRY: Lazy<Mutex<HashMap<usize, CryptoKeyMaterial>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) fn register_crypto_key(buf_addr: usize, mat: CryptoKeyMaterial) {
    CRYPTO_KEY_REGISTRY.lock().unwrap().insert(buf_addr, mat);
    unsafe {
        js_buffer_mark_as_crypto_key_external(
            buf_addr,
            runtime_algo_id(mat.algo),
            runtime_hash_id(mat.hash),
            runtime_key_kind_id(mat.kind),
            u8::from(mat.extractable),
            mat.usages,
        );
    }
}

fn runtime_algo_id(algo: KeyAlgo) -> u8 {
    match algo {
        KeyAlgo::Hmac => 1,
        KeyAlgo::AesGcm => 2,
        KeyAlgo::AesKw => 3,
        KeyAlgo::AesCbc => 4,
        KeyAlgo::AesCtr => 5,
        KeyAlgo::Hkdf => 6,
        KeyAlgo::Pbkdf2 => 7,
        KeyAlgo::EcdsaP256 => 8,
        KeyAlgo::EcdhP256 => 9,
        KeyAlgo::Ed25519 => 10,
        KeyAlgo::X25519 => 11,
        KeyAlgo::RsassaPkcs1 => 12,
        KeyAlgo::RsaOaep => 13,
        KeyAlgo::RsaPss => 14,
        KeyAlgo::EcdsaP384 => 15,
        KeyAlgo::EcdhP384 => 16,
        KeyAlgo::EcdsaP521 => 17,
        KeyAlgo::EcdhP521 => 18,
        KeyAlgo::Argon2d => 19,
        KeyAlgo::Argon2i => 20,
        KeyAlgo::Argon2id => 21,
        KeyAlgo::ChaCha20Poly1305 => 22,
        KeyAlgo::Kmac128 => 23,
        KeyAlgo::Kmac256 => 24,
        KeyAlgo::AesOcb => 25,
        KeyAlgo::X448 => 26,
        KeyAlgo::Ed448 => 27,
        KeyAlgo::MlKem512 => 30,
        KeyAlgo::MlKem768 => 31,
        KeyAlgo::MlKem1024 => 32,
    }
}

fn runtime_hash_id(hash: HashAlgo) -> u8 {
    match hash {
        HashAlgo::Sha1 => 1,
        HashAlgo::Sha256 => 2,
        HashAlgo::Sha384 => 3,
        HashAlgo::Sha512 => 4,
    }
}

fn runtime_key_kind_id(kind: KeyKind) -> u8 {
    match kind {
        KeyKind::Secret => 1,
        KeyKind::Private => 2,
        KeyKind::Public => 3,
    }
}

/// GC buffer-sweep callback: the `BufferHeader` backing this CryptoKey died,
/// so drop its material entry. Registered with the runtime by
/// `js_stdlib_init_dispatch` via `js_set_crypto_key_death_hook`.
///
/// Without this the map grew without bound (one entry per CryptoKey ever
/// created) and — worse — a recycled buffer address inherited the dead key's
/// material, so `lookup_crypto_key` would hand an unrelated fresh Buffer to
/// the subtle sign/verify/encrypt paths. Runs inside the sweep: only removes a
/// HashMap entry, never allocates.
pub(crate) extern "C" fn crypto_key_buffer_died(buf_addr: usize) {
    if let Ok(mut r) = CRYPTO_KEY_REGISTRY.lock() {
        r.remove(&buf_addr);
    }
}

pub(super) fn lookup_crypto_key(buf_addr: usize) -> Option<CryptoKeyMaterial> {
    CRYPTO_KEY_REGISTRY
        .lock()
        .unwrap()
        .get(&buf_addr)
        .copied()
        .or_else(|| {
            let (algo, hash, kind, extractable, usages) =
                perry_runtime::buffer::crypto_key_meta(buf_addr)?;
            let algo = match algo {
                1 => KeyAlgo::Hmac,
                2 => KeyAlgo::AesGcm,
                3 => KeyAlgo::AesKw,
                4 => KeyAlgo::AesCbc,
                5 => KeyAlgo::AesCtr,
                6 => KeyAlgo::Hkdf,
                7 => KeyAlgo::Pbkdf2,
                8 => KeyAlgo::EcdsaP256,
                9 => KeyAlgo::EcdhP256,
                10 => KeyAlgo::Ed25519,
                11 => KeyAlgo::X25519,
                12 => KeyAlgo::RsassaPkcs1,
                13 => KeyAlgo::RsaOaep,
                14 => KeyAlgo::RsaPss,
                15 => KeyAlgo::EcdsaP384,
                16 => KeyAlgo::EcdhP384,
                17 => KeyAlgo::EcdsaP521,
                18 => KeyAlgo::EcdhP521,
                19 => KeyAlgo::Argon2d,
                20 => KeyAlgo::Argon2i,
                21 => KeyAlgo::Argon2id,
                22 => KeyAlgo::ChaCha20Poly1305,
                23 => KeyAlgo::Kmac128,
                24 => KeyAlgo::Kmac256,
                25 => KeyAlgo::AesOcb,
                26 => KeyAlgo::X448,
                27 => KeyAlgo::Ed448,
                30 => KeyAlgo::MlKem512,
                31 => KeyAlgo::MlKem768,
                32 => KeyAlgo::MlKem1024,
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
            Some(CryptoKeyMaterial {
                algo,
                hash,
                kind,
                extractable,
                usages,
            })
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
        // BufferSource can be any TypedArray. Native arena views keep their
        // bytes out-of-line, so route through the typed-array byte helper.
        let ta = raw as *const perry_runtime::typedarray::TypedArrayHeader;
        if let Some(bytes) = perry_runtime::typedarray::typed_array_bytes(ta) {
            return bytes.to_vec();
        }
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

pub(super) fn bool_from_jsvalue(bits: u64) -> bool {
    matches!(bits, TAG_TRUE)
}

pub(super) unsafe fn key_usages_from_jsvalue(bits: u64) -> Option<u32> {
    let is_array =
        JSValue::from_bits(perry_runtime::array::js_array_is_array(f64::from_bits(bits)).to_bits());
    if !is_array.is_bool() || !is_array.as_bool() {
        return Some(0);
    }
    let arr = strip_ptr(bits) as *const perry_runtime::ArrayHeader;
    if (arr as usize) < 0x1000 {
        return Some(0);
    }
    let len = perry_runtime::array::js_array_length(arr);
    let mut usages = 0u32;
    for i in 0..len {
        let item = perry_runtime::array::js_array_get(arr, i);
        let name = string_from_jsvalue(item.bits())?;
        let bit = usage_bit(&name)?;
        usages |= bit;
    }
    Some(usages)
}

pub(super) fn usage_bit(name: &str) -> Option<u32> {
    match name {
        "encrypt" => Some(USAGE_ENCRYPT),
        "decrypt" => Some(USAGE_DECRYPT),
        "sign" => Some(USAGE_SIGN),
        "verify" => Some(USAGE_VERIFY),
        "deriveKey" => Some(USAGE_DERIVE_KEY),
        "deriveBits" => Some(USAGE_DERIVE_BITS),
        "wrapKey" => Some(USAGE_WRAP_KEY),
        "unwrapKey" => Some(USAGE_UNWRAP_KEY),
        "encapsulateBits" => Some(USAGE_ENCAPSULATE_BITS),
        "decapsulateBits" => Some(USAGE_DECAPSULATE_BITS),
        "encapsulateKey" => Some(USAGE_ENCAPSULATE_KEY),
        "decapsulateKey" => Some(USAGE_DECAPSULATE_KEY),
        _ => None,
    }
}

pub(super) fn argon2_key_algo(name: &str) -> Option<KeyAlgo> {
    match name.to_ascii_uppercase().as_str() {
        "ARGON2D" => Some(KeyAlgo::Argon2d),
        "ARGON2I" => Some(KeyAlgo::Argon2i),
        "ARGON2ID" => Some(KeyAlgo::Argon2id),
        _ => None,
    }
}

pub(super) fn supported_usages(algo: KeyAlgo, kind: KeyKind) -> u32 {
    match (algo, kind) {
        (KeyAlgo::Hmac, KeyKind::Secret) => USAGE_SIGN | USAGE_VERIFY,
        (KeyAlgo::Kmac128 | KeyAlgo::Kmac256, KeyKind::Secret) => USAGE_SIGN | USAGE_VERIFY,
        (
            KeyAlgo::AesGcm
            | KeyAlgo::AesCbc
            | KeyAlgo::AesCtr
            | KeyAlgo::AesOcb
            | KeyAlgo::ChaCha20Poly1305,
            KeyKind::Secret,
        ) => USAGE_ENCRYPT | USAGE_DECRYPT | USAGE_WRAP_KEY | USAGE_UNWRAP_KEY,
        (KeyAlgo::AesKw, KeyKind::Secret) => USAGE_WRAP_KEY | USAGE_UNWRAP_KEY,
        (
            KeyAlgo::Hkdf
            | KeyAlgo::Pbkdf2
            | KeyAlgo::Argon2d
            | KeyAlgo::Argon2i
            | KeyAlgo::Argon2id,
            KeyKind::Secret,
        ) => USAGE_DERIVE_KEY | USAGE_DERIVE_BITS,
        (
            KeyAlgo::EcdsaP256
            | KeyAlgo::EcdsaP384
            | KeyAlgo::EcdsaP521
            | KeyAlgo::Ed25519
            | KeyAlgo::Ed448
            | KeyAlgo::RsassaPkcs1
            | KeyAlgo::RsaPss,
            KeyKind::Private,
        ) => USAGE_SIGN,
        (
            KeyAlgo::EcdsaP256
            | KeyAlgo::EcdsaP384
            | KeyAlgo::EcdsaP521
            | KeyAlgo::Ed25519
            | KeyAlgo::Ed448
            | KeyAlgo::RsassaPkcs1
            | KeyAlgo::RsaPss,
            KeyKind::Public,
        ) => USAGE_VERIFY,
        (
            KeyAlgo::EcdhP256
            | KeyAlgo::EcdhP384
            | KeyAlgo::EcdhP521
            | KeyAlgo::X25519
            | KeyAlgo::X448,
            KeyKind::Private,
        ) => USAGE_DERIVE_KEY | USAGE_DERIVE_BITS,
        (
            KeyAlgo::EcdhP256
            | KeyAlgo::EcdhP384
            | KeyAlgo::EcdhP521
            | KeyAlgo::X25519
            | KeyAlgo::X448,
            KeyKind::Public,
        ) => 0,
        (KeyAlgo::RsaOaep, KeyKind::Public) => USAGE_ENCRYPT | USAGE_WRAP_KEY,
        (KeyAlgo::RsaOaep, KeyKind::Private) => USAGE_DECRYPT | USAGE_UNWRAP_KEY,
        (KeyAlgo::MlKem512 | KeyAlgo::MlKem768 | KeyAlgo::MlKem1024, KeyKind::Public) => {
            USAGE_ENCAPSULATE_BITS | USAGE_ENCAPSULATE_KEY
        }
        (KeyAlgo::MlKem512 | KeyAlgo::MlKem768 | KeyAlgo::MlKem1024, KeyKind::Private) => {
            USAGE_DECAPSULATE_BITS | USAGE_DECAPSULATE_KEY
        }
        _ => 0,
    }
}

pub(super) fn ml_kem_key_algo_from_name(name: &str) -> Option<KeyAlgo> {
    match name {
        "ML-KEM-512" => Some(KeyAlgo::MlKem512),
        "ML-KEM-768" => Some(KeyAlgo::MlKem768),
        "ML-KEM-1024" => Some(KeyAlgo::MlKem1024),
        _ => None,
    }
}

pub(super) fn is_ml_kem_key_algo(algo: KeyAlgo) -> bool {
    matches!(
        algo,
        KeyAlgo::MlKem512 | KeyAlgo::MlKem768 | KeyAlgo::MlKem1024
    )
}

pub(super) fn ml_kem_algorithm_name(algo: KeyAlgo) -> Option<&'static str> {
    match algo {
        KeyAlgo::MlKem512 => Some("ML-KEM-512"),
        KeyAlgo::MlKem768 => Some("ML-KEM-768"),
        KeyAlgo::MlKem1024 => Some("ML-KEM-1024"),
        _ => None,
    }
}

pub(super) fn ml_kem_der_pair_from_seed(
    algo: KeyAlgo,
    seed_bytes: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let seed = ml_kem::Seed::try_from(seed_bytes).ok()?;
    match algo {
        KeyAlgo::MlKem512 => {
            let private = ml_kem::DecapsulationKey512::from_seed(seed);
            let private_der = private.to_pkcs8_der().ok()?.as_bytes().to_vec();
            let public_der = private
                .encapsulation_key()
                .to_public_key_der()
                .ok()?
                .as_bytes()
                .to_vec();
            Some((private_der, public_der))
        }
        KeyAlgo::MlKem768 => {
            let private = ml_kem::DecapsulationKey768::from_seed(seed);
            let private_der = private.to_pkcs8_der().ok()?.as_bytes().to_vec();
            let public_der = private
                .encapsulation_key()
                .to_public_key_der()
                .ok()?
                .as_bytes()
                .to_vec();
            Some((private_der, public_der))
        }
        KeyAlgo::MlKem1024 => {
            let private = ml_kem::DecapsulationKey1024::from_seed(seed);
            let private_der = private.to_pkcs8_der().ok()?.as_bytes().to_vec();
            let public_der = private
                .encapsulation_key()
                .to_public_key_der()
                .ok()?
                .as_bytes()
                .to_vec();
            Some((private_der, public_der))
        }
        _ => None,
    }
}

pub(super) fn ml_kem_public_bytes_from_der(algo: KeyAlgo, der: &[u8]) -> Option<Vec<u8>> {
    match algo {
        KeyAlgo::MlKem512 => Some(
            ml_kem::EncapsulationKey512::from_public_key_der(der)
                .ok()?
                .to_bytes()
                .as_slice()
                .to_vec(),
        ),
        KeyAlgo::MlKem768 => Some(
            ml_kem::EncapsulationKey768::from_public_key_der(der)
                .ok()?
                .to_bytes()
                .as_slice()
                .to_vec(),
        ),
        KeyAlgo::MlKem1024 => Some(
            ml_kem::EncapsulationKey1024::from_public_key_der(der)
                .ok()?
                .to_bytes()
                .as_slice()
                .to_vec(),
        ),
        _ => None,
    }
}

pub(super) fn ml_kem_private_seed_and_public_from_der(
    algo: KeyAlgo,
    der: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    match algo {
        KeyAlgo::MlKem512 => {
            let private = ml_kem::DecapsulationKey512::from_pkcs8_der(der).ok()?;
            Some((
                private.to_seed()?.as_slice().to_vec(),
                private.encapsulation_key().to_bytes().as_slice().to_vec(),
            ))
        }
        KeyAlgo::MlKem768 => {
            let private = ml_kem::DecapsulationKey768::from_pkcs8_der(der).ok()?;
            Some((
                private.to_seed()?.as_slice().to_vec(),
                private.encapsulation_key().to_bytes().as_slice().to_vec(),
            ))
        }
        KeyAlgo::MlKem1024 => {
            let private = ml_kem::DecapsulationKey1024::from_pkcs8_der(der).ok()?;
            Some((
                private.to_seed()?.as_slice().to_vec(),
                private.encapsulation_key().to_bytes().as_slice().to_vec(),
            ))
        }
        _ => None,
    }
}

pub(super) fn ml_kem_public_der_from_bytes(algo: KeyAlgo, public_bytes: &[u8]) -> Option<Vec<u8>> {
    match algo {
        KeyAlgo::MlKem512 => {
            let public = ml_kem::Key::<ml_kem::EncapsulationKey512>::try_from(public_bytes).ok()?;
            let key = ml_kem::EncapsulationKey512::new(&public).ok()?;
            Some(key.to_public_key_der().ok()?.as_bytes().to_vec())
        }
        KeyAlgo::MlKem768 => {
            let public = ml_kem::Key::<ml_kem::EncapsulationKey768>::try_from(public_bytes).ok()?;
            let key = ml_kem::EncapsulationKey768::new(&public).ok()?;
            Some(key.to_public_key_der().ok()?.as_bytes().to_vec())
        }
        KeyAlgo::MlKem1024 => {
            let public =
                ml_kem::Key::<ml_kem::EncapsulationKey1024>::try_from(public_bytes).ok()?;
            let key = ml_kem::EncapsulationKey1024::new(&public).ok()?;
            Some(key.to_public_key_der().ok()?.as_bytes().to_vec())
        }
        _ => None,
    }
}

pub(super) unsafe fn validate_key_usages(
    algo: KeyAlgo,
    kind: KeyKind,
    usages_bits: u64,
    empty_allowed: bool,
    empty_message: &'static str,
    bad_message: &'static str,
) -> Result<u32, (&'static str, &'static str)> {
    let usages = match key_usages_from_jsvalue(usages_bits) {
        Some(u) => u,
        None => return Err(("SyntaxError", bad_message)),
    };
    let supported = supported_usages(algo, kind);
    if usages & !supported != 0 {
        return Err(("SyntaxError", bad_message));
    }
    if usages == 0 && !empty_allowed {
        return Err(("SyntaxError", empty_message));
    }
    Ok(usages)
}

pub(super) unsafe fn validate_key_pair_usages(
    algo: KeyAlgo,
    usages_bits: u64,
    empty_message: &'static str,
    bad_message: &'static str,
) -> Result<(u32, u32), (&'static str, &'static str)> {
    let requested = match key_usages_from_jsvalue(usages_bits) {
        Some(u) => u,
        None => return Err(("SyntaxError", bad_message)),
    };
    let private_supported = supported_usages(algo, KeyKind::Private);
    let public_supported = supported_usages(algo, KeyKind::Public);
    if requested & !(private_supported | public_supported) != 0 {
        return Err(("SyntaxError", bad_message));
    }
    let private_usages = requested & private_supported;
    let public_usages = requested & public_supported;
    if private_usages == 0 && public_usages == 0 {
        return Err(("SyntaxError", empty_message));
    }
    Ok((private_usages, public_usages))
}

pub(super) fn require_usage(
    mat: CryptoKeyMaterial,
    usage: u32,
    message: &'static str,
) -> Result<(), (&'static str, &'static str)> {
    if mat.usages & usage == 0 {
        Err(("InvalidAccessError", message))
    } else {
        Ok(())
    }
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
    js_buffer_register_external(buf as usize);
    js_buffer_mark_as_uint8array_external(buf as usize);
    buf
}

/// Wrap a heap value (NaN-boxed bits) in an already-resolved Promise.
pub(super) fn resolve_with_bits(bits: u64) -> *mut Promise {
    js_promise_resolved(f64::from_bits(bits))
}

/// Construct a DOMException and return a rejected Promise carrying it.
pub(super) unsafe fn reject_with_dom_exception(name: &str, message: &str) -> *mut Promise {
    let name_str = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let message_str = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let name_val = f64::from_bits(JSValue::string_ptr(name_str).bits());
    let message_val = f64::from_bits(JSValue::string_ptr(message_str).bits());
    let err = perry_runtime::event_target::js_dom_exception_new(message_val, name_val);
    if err.is_null() {
        return perry_runtime::js_promise_rejected(f64::from_bits(0x7FFC_0000_0000_0001));
    }
    let err_val = f64::from_bits(JSValue::pointer(err as *const u8).bits());
    perry_runtime::js_promise_rejected(err_val)
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

pub(super) fn compute_kmac(
    algo: KeyAlgo,
    key: &[u8],
    customization: &[u8],
    data: &[u8],
    output_bits: u32,
) -> Option<Vec<u8>> {
    if output_bits % 8 != 0 {
        return None;
    }
    let mut out = vec![0u8; (output_bits / 8) as usize];
    match algo {
        KeyAlgo::Kmac128 => {
            use sha3_010::digest::{core_api::CoreProxy, ExtendableOutput, Update, XofReader};
            let core = <sha3_010::CShake128 as CoreProxy>::Core::new_with_function_name(
                b"KMAC",
                customization,
            );
            let mut cshake = sha3_010::CShake128::from_core(core);
            for item in sha3_utils::bytepad::<168, _>([sha3_utils::encode_string(key)]) {
                Update::update(&mut cshake, item.as_bytes());
            }
            Update::update(&mut cshake, data);
            Update::update(
                &mut cshake,
                sha3_utils::right_encode(output_bits as usize).as_bytes(),
            );
            let mut reader = cshake.finalize_xof();
            XofReader::read(&mut reader, &mut out);
        }
        KeyAlgo::Kmac256 => {
            use sha3_010::digest::{core_api::CoreProxy, ExtendableOutput, Update, XofReader};
            let core = <sha3_010::CShake256 as CoreProxy>::Core::new_with_function_name(
                b"KMAC",
                customization,
            );
            let mut cshake = sha3_010::CShake256::from_core(core);
            for item in sha3_utils::bytepad::<136, _>([sha3_utils::encode_string(key)]) {
                Update::update(&mut cshake, item.as_bytes());
            }
            Update::update(&mut cshake, data);
            Update::update(
                &mut cshake,
                sha3_utils::right_encode(output_bits as usize).as_bytes(),
            );
            let mut reader = cshake.finalize_xof();
            XofReader::read(&mut reader, &mut out);
        }
        _ => return None,
    }
    Some(out)
}

pub(super) fn generate_p256_signing_key() -> Option<P256EcdsaSigningKey> {
    let mut rng = rand::rngs::OsRng;
    Some(P256EcdsaSigningKey::random(&mut rng))
}

pub(super) fn generate_p384_signing_key() -> Option<P384EcdsaSigningKey> {
    let mut rng = rand::rngs::OsRng;
    Some(P384EcdsaSigningKey::random(&mut rng))
}

pub(super) fn generate_p521_signing_key() -> Option<P521EcdsaSigningKey> {
    let mut rng = rand::rngs::OsRng;
    Some(P521EcdsaSigningKey::random(&mut rng))
}

pub(super) fn generate_p256_secret_key() -> Option<P256SecretKey> {
    let mut rng = rand::rngs::OsRng;
    Some(P256SecretKey::random(&mut rng))
}

pub(super) fn generate_p384_secret_key() -> Option<P384SecretKey> {
    let mut rng = rand::rngs::OsRng;
    Some(P384SecretKey::random(&mut rng))
}

pub(super) fn generate_p521_secret_key() -> Option<P521SecretKey> {
    let mut rng = rand::rngs::OsRng;
    Some(P521SecretKey::random(&mut rng))
}

pub(super) fn generate_ecdsa_key_pair_bytes(curve: EcNamedCurve) -> Option<(Vec<u8>, Vec<u8>)> {
    match curve {
        EcNamedCurve::P256 => {
            let key = generate_p256_signing_key()?;
            Some((
                key.to_bytes().as_slice().to_vec(),
                key.verifying_key()
                    .to_encoded_point(false)
                    .as_bytes()
                    .to_vec(),
            ))
        }
        EcNamedCurve::P384 => {
            let key = generate_p384_signing_key()?;
            Some((
                key.to_bytes().as_slice().to_vec(),
                key.verifying_key()
                    .to_encoded_point(false)
                    .as_bytes()
                    .to_vec(),
            ))
        }
        EcNamedCurve::P521 => {
            let key = generate_p521_signing_key()?;
            let verifying_key = P521EcdsaVerifyingKey::from(&key);
            Some((
                key.to_bytes().as_slice().to_vec(),
                verifying_key.to_encoded_point(false).as_bytes().to_vec(),
            ))
        }
    }
}

pub(super) fn generate_ecdh_key_pair_bytes(curve: EcNamedCurve) -> Option<(Vec<u8>, Vec<u8>)> {
    match curve {
        EcNamedCurve::P256 => {
            let key = generate_p256_secret_key()?;
            Some((
                key.to_bytes().as_slice().to_vec(),
                key.public_key().to_encoded_point(false).as_bytes().to_vec(),
            ))
        }
        EcNamedCurve::P384 => {
            let key = generate_p384_secret_key()?;
            Some((
                key.to_bytes().as_slice().to_vec(),
                key.public_key().to_encoded_point(false).as_bytes().to_vec(),
            ))
        }
        EcNamedCurve::P521 => {
            let key = generate_p521_secret_key()?;
            Some((
                key.to_bytes().as_slice().to_vec(),
                key.public_key().to_encoded_point(false).as_bytes().to_vec(),
            ))
        }
    }
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
