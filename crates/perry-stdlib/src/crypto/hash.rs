use super::*;

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
