//! End-to-end encryption module for Hone Sync.
//!
//! Provides X25519 key exchange, AES-256-GCM authenticated encryption,
//! and HKDF-SHA256 key derivation — all exposed as synchronous FFI functions.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine as _;
use hkdf::Hkdf;
use perry_runtime::{js_string_from_bytes, StringHeader};
use rand::RngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// Helper to extract bytes from a StringHeader pointer.
unsafe fn str_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(bytes.to_vec())
}

/// Decode a hex-encoded StringHeader into raw bytes.
unsafe fn hex_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    let text = str_from_header(ptr)?;
    hex::decode(&text).ok()
}

/// Return a hex string as a StringHeader pointer.
fn return_hex(bytes: &[u8]) -> *mut StringHeader {
    let hex_str = hex::encode(bytes);
    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

// ---------------------------------------------------------------------------
// X25519 Key Exchange
// ---------------------------------------------------------------------------

/// Generate an X25519 keypair.
///
/// Returns a JSON string: `{"publicKey":"<hex>","secretKey":"<hex>"}`
///
/// The secret key is 32 random bytes; the public key is the X25519
/// scalar multiplication of the secret with the base point.
#[no_mangle]
pub extern "C" fn js_crypto_x25519_keypair() -> *mut StringHeader {
    let mut rng = rand::thread_rng();
    let mut secret_bytes = [0u8; 32];
    rng.fill_bytes(&mut secret_bytes);

    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);

    let pub_hex = hex::encode(public.as_bytes());
    let sec_hex = hex::encode(secret_bytes);

    // Build JSON manually (no serde needed for this simple shape)
    let json = format!(
        "{{\"publicKey\":\"{}\",\"secretKey\":\"{}\"}}",
        pub_hex, sec_hex
    );

    js_string_from_bytes(json.as_ptr(), json.len() as u32)
}

/// Compute X25519 shared secret from my secret key and their public key.
///
/// Both parameters are hex-encoded 32-byte keys.
/// Returns the shared secret as a hex string (32 bytes / 64 hex chars).
///
/// # Safety
/// Pointers must be valid StringHeader pointers containing hex strings.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_x25519_shared_secret(
    secret_ptr: *const StringHeader,
    public_ptr: *const StringHeader,
) -> *mut StringHeader {
    let secret_bytes = match hex_from_header(secret_ptr) {
        Some(b) if b.len() == 32 => b,
        _ => return std::ptr::null_mut(),
    };

    let public_bytes = match hex_from_header(public_ptr) {
        Some(b) if b.len() == 32 => b,
        _ => return std::ptr::null_mut(),
    };

    let mut sec_arr = [0u8; 32];
    sec_arr.copy_from_slice(&secret_bytes);
    let secret = StaticSecret::from(sec_arr);

    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&public_bytes);
    let public = PublicKey::from(pub_arr);

    let shared = secret.diffie_hellman(&public);
    return_hex(shared.as_bytes())
}

// ---------------------------------------------------------------------------
// AES-256-GCM Authenticated Encryption
// ---------------------------------------------------------------------------

/// Encrypt plaintext with AES-256-GCM.
///
/// - `plain_ptr`: UTF-8 plaintext
/// - `key_ptr`: hex-encoded 32-byte key (64 hex chars)
/// - `nonce_ptr`: hex-encoded 12-byte nonce (24 hex chars)
///
/// Returns base64-encoded ciphertext (includes 16-byte auth tag appended by AES-GCM).
///
/// # Safety
/// All pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_aes256_gcm_encrypt(
    plain_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
    nonce_ptr: *const StringHeader,
) -> *mut StringHeader {
    let plaintext = match str_from_header(plain_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let key_bytes = match hex_from_header(key_ptr) {
        Some(k) if k.len() == 32 => k,
        _ => return std::ptr::null_mut(),
    };

    let nonce_bytes = match hex_from_header(nonce_ptr) {
        Some(n) if n.len() == 12 => n,
        _ => return std::ptr::null_mut(),
    };

    let cipher = match Aes256Gcm::new_from_slice(&key_bytes) {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = match cipher.encrypt(nonce, plaintext.as_ref()) {
        Ok(ct) => ct,
        Err(_) => return std::ptr::null_mut(),
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&ciphertext);
    js_string_from_bytes(b64.as_ptr(), b64.len() as u32)
}

/// Decrypt ciphertext with AES-256-GCM.
///
/// - `cipher_ptr`: base64-encoded ciphertext (with auth tag)
/// - `key_ptr`: hex-encoded 32-byte key (64 hex chars)
/// - `nonce_ptr`: hex-encoded 12-byte nonce (24 hex chars)
///
/// Returns UTF-8 plaintext, or null on authentication failure.
///
/// # Safety
/// All pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_aes256_gcm_decrypt(
    cipher_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
    nonce_ptr: *const StringHeader,
) -> *mut StringHeader {
    let cipher_b64 = match str_from_header(cipher_ptr) {
        Some(c) => c,
        None => return std::ptr::null_mut(),
    };

    let ciphertext = match base64::engine::general_purpose::STANDARD.decode(&cipher_b64) {
        Ok(ct) => ct,
        Err(_) => return std::ptr::null_mut(),
    };

    let key_bytes = match hex_from_header(key_ptr) {
        Some(k) if k.len() == 32 => k,
        _ => return std::ptr::null_mut(),
    };

    let nonce_bytes = match hex_from_header(nonce_ptr) {
        Some(n) if n.len() == 12 => n,
        _ => return std::ptr::null_mut(),
    };

    let cipher = match Aes256Gcm::new_from_slice(&key_bytes) {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = match cipher.decrypt(nonce, ciphertext.as_ref()) {
        Ok(pt) => pt,
        Err(_) => return std::ptr::null_mut(), // Auth tag mismatch
    };

    js_string_from_bytes(plaintext.as_ptr(), plaintext.len() as u32)
}

/// Generate a random 12-byte nonce for AES-256-GCM, returned as hex (24 chars).
#[no_mangle]
pub extern "C" fn js_crypto_random_nonce() -> *mut StringHeader {
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    return_hex(&nonce_bytes)
}

// ---------------------------------------------------------------------------
// HKDF-SHA256 Key Derivation
// ---------------------------------------------------------------------------

/// Derive a key using HKDF-SHA256.
///
/// - `ikm_ptr`: hex-encoded input keying material
/// - `salt_ptr`: hex-encoded salt (can be empty for no salt)
/// - `info_ptr`: UTF-8 context/info string
/// - `length`: desired output length in bytes (f64, max 255*32 = 8160)
///
/// Returns hex-encoded derived key.
///
/// # Safety
/// All pointers must be valid StringHeader pointers.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_hkdf_sha256(
    ikm_ptr: *const StringHeader,
    salt_ptr: *const StringHeader,
    info_ptr: *const StringHeader,
    length: f64,
) -> *mut StringHeader {
    let ikm = match hex_from_header(ikm_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };

    let salt_bytes = hex_from_header(salt_ptr).unwrap_or_default();
    let salt_ref: Option<&[u8]> = if salt_bytes.is_empty() {
        None
    } else {
        Some(&salt_bytes)
    };

    let info = match str_from_header(info_ptr) {
        Some(i) => i,
        None => return std::ptr::null_mut(),
    };

    let out_len = length as usize;
    if out_len == 0 || out_len > 8160 {
        return std::ptr::null_mut();
    }

    let hk = Hkdf::<Sha256>::new(salt_ref, &ikm);
    let mut okm = vec![0u8; out_len];

    if hk.expand(&info, &mut okm).is_err() {
        return std::ptr::null_mut();
    }

    return_hex(&okm)
}
