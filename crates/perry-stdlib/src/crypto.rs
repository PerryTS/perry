//! Crypto module
//!
//! Native implementation of Node.js crypto module functions.
//! Provides hashing (sha256, md5), random byte generation, AES encryption,
//! and key derivation (pbkdf2, scrypt).

use crate::common::handle::{get_handle_mut, register_handle, Handle};
use aes::Aes256;
use base64::Engine as _;
use cbc::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit},
    Decryptor, Encryptor,
};
use md5::{Digest as Md5Digest, Md5};
use perry_runtime::{js_string_from_bytes, StringHeader};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Digest as Sha256Digest, Sha256, Sha512};

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

/// Create HMAC-SHA256
/// crypto.createHmac('sha256', key).update(data).digest('hex') -> string
#[no_mangle]
pub unsafe extern "C" fn js_crypto_hmac_sha256(
    key_ptr: *const StringHeader,
    data_ptr: *const StringHeader,
) -> *mut StringHeader {
    use hmac::{Hmac, Mac};
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
    use hmac::{Hmac, Mac};
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

/// PBKDF2-HMAC-SHA-256 returning a Buffer. Counterpart of
/// `crypto.pbkdf2Sync(password, salt, iterations, keylen, 'sha256')`.
/// Accepts string or Buffer for both password and salt.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_pbkdf2_bytes(
    password_ptr: i64,
    salt_ptr: i64,
    iterations: f64,
    keylen: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    use pbkdf2::pbkdf2_hmac;
    let password = bytes_from_ptr(password_ptr);
    let salt = bytes_from_ptr(salt_ptr);
    let iter = iterations as u32;
    let klen = keylen as usize;
    let mut out = vec![0u8; klen];
    pbkdf2_hmac::<Sha256>(&password, &salt, iter, &mut out);
    alloc_buffer_from_slice(&out)
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

pub enum HashState {
    Sha1(Sha1),
    Sha256(Sha256),
    Sha512(Sha512),
    Md5(Md5),
}

pub struct HashHandle {
    /// `Option` so `digest()` can `take()` ownership of the hasher
    /// (sha1/sha2 `finalize()` consumes `self`).
    state: std::sync::Mutex<Option<HashState>>,
}

/// Allocate a new Hash handle for the given algorithm. Returns the handle
/// id NaN-boxed with POINTER_TAG (0x7FFD_…). Small integers survive the
/// 48-bit POINTER_MASK, and the runtime's handle-range check in
/// `js_native_call_method` (`raw_ptr < 0x100000`) routes subsequent
/// `.update(...)` / `.digest(...)` through `HANDLE_METHOD_DISPATCH` which
/// calls `dispatch_hash` below. Unknown algorithms return undefined.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hash(alg_ptr: i64) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes)
        .unwrap_or("")
        .to_ascii_lowercase();
    let state = match alg.as_str() {
        "sha1" | "sha-1" => HashState::Sha1(Sha1::new()),
        "sha256" | "sha-256" => HashState::Sha256(Sha256::new()),
        "sha512" | "sha-512" => HashState::Sha512(Sha512::new()),
        "md5" => HashState::Md5(Md5::new()),
        _ => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let handle: Handle = register_handle(HashHandle {
        state: std::sync::Mutex::new(Some(state)),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Dispatch `update` / `digest` on a HashHandle. Called from
/// `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_hash(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<HashHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    match method {
        "update" if !args.is_empty() => {
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                match state {
                    HashState::Sha1(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha256(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha512(x) => Sha256Digest::update(x, &bytes),
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
            let digest: Vec<u8> = match state {
                Some(HashState::Sha1(x)) => x.finalize().to_vec(),
                Some(HashState::Sha256(x)) => x.finalize().to_vec(),
                Some(HashState::Sha512(x)) => x.finalize().to_vec(),
                Some(HashState::Md5(x)) => x.finalize().to_vec(),
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
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

#[inline]
fn is_undefined_f64(v: f64) -> bool {
    v.to_bits() == 0x7FFC_0000_0000_0001
}
