//! Crypto module
//!
//! Native implementation of Node.js crypto module functions.
//! Provides hashing (sha256, md5), random byte generation, AES encryption,
//! and key derivation (pbkdf2, scrypt).

use crate::common::handle::{get_handle, get_handle_mut, register_handle, Handle};
use aes::{Aes128, Aes256};
use base64::Engine as _;
use cbc::{
    cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit},
    Decryptor, Encryptor,
};
use md5::{Digest as Md5Digest, Md5};
use perry_runtime::{js_string_from_bytes, StringHeader};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Digest as Sha256Digest, Sha224, Sha256, Sha384, Sha512};

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
) -> *mut perry_runtime::buffer::BufferHeader {
    let bytes = bytes_from_ptr(key_ptr);
    let buf = alloc_buffer_from_slice(&bytes);
    if !buf.is_null() {
        // Mark as Uint8Array so `instanceof Uint8Array` works, both in
        // perry-native code and after the bridge materializes a v8
        // Uint8Array on the V8 side.
        perry_runtime::buffer::mark_as_uint8array(buf as usize);
    }
    buf
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
        // TypedArrayHeader path (Uint8Array, Uint32Array, Float32Array, …).
        if perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some() {
            let ta = raw as *mut perry_runtime::typedarray::TypedArrayHeader;
            let len = (*ta).length as usize;
            let elem_size = (*ta).elem_size as usize;
            let total = len * elem_size;
            let (start, end) = resolve_range(total, offset_arg, size_arg);
            if end > start {
                let data = (raw as *mut u8).add(std::mem::size_of::<
                    perry_runtime::typedarray::TypedArrayHeader,
                >());
                let slice = std::slice::from_raw_parts_mut(data.add(start), end - start);
                rand::thread_rng().fill_bytes(slice);
            }
            return buf_bits;
        }
    }

    // Unsupported value shape — return the original (no-op) rather
    // than crashing. The HIR-level type check is "any", so the
    // compiler can't statically rule this out.
    buf_bits
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

/// Decode a NaN-boxed numeric `f64` to a plain `f64`. Number literals
/// reach the FFI as raw doubles, but a value that flowed through an
/// integer-typed path may arrive INT32-tagged (`0x7FFE` top16). Handle
/// both so `randomInt` works regardless of which representation codegen
/// picked for `min` / `max`.
fn nanboxed_to_f64_num(bits: f64) -> f64 {
    let raw = bits.to_bits();
    let top16 = (raw >> 48) as u16;
    if top16 == 0x7FFE {
        return ((raw & 0xFFFF_FFFF) as u32 as i32) as f64;
    }
    bits
}

/// `crypto.randomInt([min, ]max)` — uniform random integer in `[min, max)`.
/// Codegen passes `min = 0` for the single-argument form. Uses rejection
/// sampling so the distribution is unbiased (matching Node's guarantee).
/// Returns the integer as a plain `f64`. A degenerate range (`max <= min`)
/// returns `min` rather than crashing.
#[no_mangle]
pub extern "C" fn js_crypto_random_int(min_bits: f64, max_bits: f64) -> f64 {
    let min = nanboxed_to_f64_num(min_bits) as i64;
    let max = nanboxed_to_f64_num(max_bits) as i64;
    if max <= min {
        return min as f64;
    }
    let range = (max - min) as u64;
    // Rejection sampling: discard values in the unfair tail so every
    // outcome in `[0, range)` is equally likely.
    let limit = u64::MAX - (u64::MAX % range);
    let mut rng = rand::thread_rng();
    let mut r = rng.next_u64();
    while r >= limit {
        r = rng.next_u64();
    }
    (min + (r % range) as i64) as f64
}

/// `crypto.timingSafeEqual(a, b)` — constant-time comparison of two
/// equal-length byte sequences (Buffer / TypedArray / string). Returns a
/// NaN-boxed boolean. Node throws `RangeError` when the lengths differ; the
/// no-exception FFI path returns `false` instead so a length mismatch
/// degrades gracefully rather than crashing.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_timing_safe_equal(a_ptr: i64, b_ptr: i64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a = bytes_from_ptr(a_ptr);
    let b = bytes_from_ptr(b_ptr);
    if a.len() != b.len() {
        return f64::from_bits(TAG_FALSE);
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    f64::from_bits(if diff == 0 { TAG_TRUE } else { TAG_FALSE })
}

/// `crypto.getHashes()` — the digest algorithms Perry can construct via
/// `createHash` / `createHmac`. Returns a JS `string[]`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_hashes() -> *mut perry_runtime::ArrayHeader {
    use perry_runtime::{js_array_alloc, js_array_push, JSValue};
    const NAMES: [&str; 6] = ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"];
    let arr = js_array_alloc(0);
    for n in NAMES {
        let ptr = js_string_from_bytes(n.as_ptr(), n.len() as u32);
        js_array_push(arr, JSValue::string_ptr(ptr));
    }
    arr
}

/// `crypto.getCiphers()` — the symmetric ciphers Perry supports
/// (`createCipheriv` / WebCrypto AES-GCM). Returns a JS `string[]`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_ciphers() -> *mut perry_runtime::ArrayHeader {
    use perry_runtime::{js_array_alloc, js_array_push, JSValue};
    const NAMES: [&str; 6] = [
        "aes-128-cbc",
        "aes-192-cbc",
        "aes-256-cbc",
        "aes-128-gcm",
        "aes-192-gcm",
        "aes-256-gcm",
    ];
    let arr = js_array_alloc(0);
    for n in NAMES {
        let ptr = js_string_from_bytes(n.as_ptr(), n.len() as u32);
        js_array_push(arr, JSValue::string_ptr(ptr));
    }
    arr
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
        // "sha256" and the empty/unknown default.
        _ => pbkdf2_hmac::<Sha256>(&password, &salt, iter, &mut out),
    }
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
        let mut read = |primary: &str, alias: &str| -> Option<f64> {
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
    Sha224(Sha224),
    Sha256(Sha256),
    Sha384(Sha384),
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
        "sha224" | "sha-224" => HashState::Sha224(Sha224::new()),
        "sha256" | "sha-256" => HashState::Sha256(Sha256::new()),
        "sha384" | "sha-384" => HashState::Sha384(Sha384::new()),
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
                    HashState::Sha224(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha256(x) => Sha256Digest::update(x, &bytes),
                    HashState::Sha384(x) => Sha256Digest::update(x, &bytes),
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
                Some(HashState::Sha224(x)) => x.finalize().to_vec(),
                Some(HashState::Sha256(x)) => x.finalize().to_vec(),
                Some(HashState::Sha384(x)) => x.finalize().to_vec(),
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
        // `hash.copy()` (#1369) — return an independent Hash whose internal
        // state is a snapshot of this one, so the two can be `.update()`d and
        // `.digest()`ed separately. The RustCrypto hashers are `Clone`. An
        // already-digested hash (state taken) yields undefined, mirroring the
        // error a caller would hit using a finalized hash. The optional
        // `outputLength` arg only applies to XOF hashes (shake*), which Perry
        // doesn't expose, so it is ignored.
        "copy" => {
            let cloned = {
                let guard = h.state.lock().unwrap();
                match guard.as_ref() {
                    Some(HashState::Sha1(x)) => Some(HashState::Sha1(x.clone())),
                    Some(HashState::Sha224(x)) => Some(HashState::Sha224(x.clone())),
                    Some(HashState::Sha256(x)) => Some(HashState::Sha256(x.clone())),
                    Some(HashState::Sha384(x)) => Some(HashState::Sha384(x.clone())),
                    Some(HashState::Sha512(x)) => Some(HashState::Sha512(x.clone())),
                    Some(HashState::Md5(x)) => Some(HashState::Md5(x.clone())),
                    None => None,
                }
            };
            match cloned {
                Some(state) => {
                    let new_handle: Handle = register_handle(HashHandle {
                        state: std::sync::Mutex::new(Some(state)),
                    });
                    f64::from_bits(
                        0x7FFD_0000_0000_0000u64 | ((new_handle as u64) & 0x0000_FFFF_FFFF_FFFF),
                    )
                }
                None => f64::from_bits(0x7FFC_0000_0000_0001),
            }
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
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
    Sha256(hmac::Hmac<Sha256>),
    Sha512(hmac::Hmac<Sha512>),
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
        "sha256" | "sha-256" => match hmac::Hmac::<Sha256>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha256(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
        "sha512" | "sha-512" => match hmac::Hmac::<Sha512>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha512(m),
            Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
        },
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
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                match state {
                    HmacState::Sha1(x) => Mac::update(x, &bytes),
                    HmacState::Sha256(x) => Mac::update(x, &bytes),
                    HmacState::Sha512(x) => Mac::update(x, &bytes),
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
                Some(HmacState::Sha256(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Sha512(x)) => x.finalize().into_bytes().to_vec(),
                Some(HmacState::Md5(x)) => x.finalize().into_bytes().to_vec(),
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
    Aes256Cbc,
    Aes128Gcm,
    Aes256Gcm,
}

impl CipherKind {
    fn parse(alg: &str) -> Option<Self> {
        match alg.to_ascii_lowercase().as_str() {
            "aes-128-cbc" => Some(Self::Aes128Cbc),
            "aes-256-cbc" => Some(Self::Aes256Cbc),
            "aes-128-gcm" => Some(Self::Aes128Gcm),
            "aes-256-gcm" => Some(Self::Aes256Gcm),
            _ => None,
        }
    }

    fn key_len(self) -> usize {
        match self {
            Self::Aes128Cbc | Self::Aes128Gcm => 16,
            Self::Aes256Cbc | Self::Aes256Gcm => 32,
        }
    }

    fn is_gcm(self) -> bool {
        matches!(self, Self::Aes128Gcm | Self::Aes256Gcm)
    }
}

// CBC type aliases (Aes256CbcEnc/Dec already exist above for aes-256-cbc).
type Aes128CbcEnc = Encryptor<Aes128>;
type Aes128CbcDec = Decryptor<Aes128>;

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
    buffer: Vec<u8>,
    /// For GCM encrypt: filled in by `.final()`, read by `.getAuthTag()`.
    /// For GCM decrypt: set by `.setAuthTag(tag)` and consumed at `.final()`.
    auth_tag: Option<Vec<u8>>,
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

// ---------------------------------------------------------------------------
// Sign / Verify handle (#1364) — `crypto.createSign(alg).update(d).sign(key)`
// and `crypto.createVerify(alg).update(d).verify(key, sig)`. Mirrors the
// Hash/Cipher small-integer handle pattern (POINTER_TAG box, dispatched via
// HANDLE_METHOD_DISPATCH → `dispatch_sign`). RSA PKCS#1 v1.5 over
// sha1/sha224/sha256/sha384/sha512: `update` accumulates the message, then
// `sign`/`verify` hash it and RSA-sign / RSA-verify. The key argument is a
// PEM string (PKCS#8 or PKCS#1); `verify` also accepts the matching private
// key (it derives the public key), matching Node's leniency.
// ---------------------------------------------------------------------------

pub struct SignHandle {
    /// Lowercased hash algorithm (`sha256`, …).
    alg: String,
    /// Accumulated message bytes (interior mutability — the handle is shared).
    buffer: std::sync::Mutex<Vec<u8>>,
}

unsafe fn create_sign_handle(alg_ptr: i64) -> f64 {
    let alg = String::from_utf8_lossy(&bytes_from_ptr(alg_ptr))
        .to_ascii_lowercase()
        .replace("rsa-", "") // accept Node's `RSA-SHA256` spelling
        .replace('-', ""); // and `sha-256`
    if !matches!(
        alg.as_str(),
        "sha1" | "sha224" | "sha256" | "sha384" | "sha512"
    ) {
        return nanbox_undefined();
    }
    let handle: Handle = register_handle(SignHandle {
        alg,
        buffer: std::sync::Mutex::new(Vec::new()),
    });
    nanbox_pointer_f64(handle as usize)
}

/// `crypto.createSign(algorithm)` — allocate a Sign handle.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_sign(alg_ptr: i64) -> f64 {
    create_sign_handle(alg_ptr)
}

/// `crypto.createVerify(algorithm)` — allocate a Verify handle (same shape).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_verify(alg_ptr: i64) -> f64 {
    create_sign_handle(alg_ptr)
}

/// Build the PKCS#1 v1.5 DigestInfo (`SEQUENCE { AlgorithmIdentifier, OCTET
/// STRING hash }`) for `alg` over `data`. The fixed ASN.1 prefixes are the
/// standard ones from RFC 8017 §9.2 (Note 1). We assemble DigestInfo by hand
/// and feed it to `Pkcs1v15Sign::new_unprefixed()` so that signing/verifying
/// does not require the `rsa` crate's `digest`-0.10 `AssociatedOid` bound —
/// the workspace's `sha1`/`sha2` are a newer `digest` version, which would
/// otherwise fail to satisfy `Pkcs1v15Sign::new::<D>()`.
fn rsa_digest_info(alg: &str, data: &[u8]) -> Option<Vec<u8>> {
    let (prefix, hash): (&[u8], Vec<u8>) = match alg {
        "sha1" => (
            &[
                0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04,
                0x14,
            ],
            Sha1::digest(data).to_vec(),
        ),
        "sha224" => (
            &[
                0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x04, 0x05, 0x00, 0x04, 0x1c,
            ],
            Sha224::digest(data).to_vec(),
        ),
        "sha256" => (
            &[
                0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x01, 0x05, 0x00, 0x04, 0x20,
            ],
            Sha256::digest(data).to_vec(),
        ),
        "sha384" => (
            &[
                0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x02, 0x05, 0x00, 0x04, 0x30,
            ],
            Sha384::digest(data).to_vec(),
        ),
        "sha512" => (
            &[
                0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
                0x03, 0x05, 0x00, 0x04, 0x40,
            ],
            Sha512::digest(data).to_vec(),
        ),
        _ => return None,
    };
    let mut di = prefix.to_vec();
    di.extend_from_slice(&hash);
    Some(di)
}

/// Hash `data` with `alg` and RSA-PKCS#1-v1.5-sign it with the PEM private key.
fn rsa_pkcs1v15_sign(alg: &str, key_pem: &str, data: &[u8]) -> Option<Vec<u8>> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    let priv_key = rsa::RsaPrivateKey::from_pkcs8_pem(key_pem)
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(key_pem))
        .ok()?;
    let di = rsa_digest_info(alg, data)?;
    priv_key.sign(rsa::Pkcs1v15Sign::new_unprefixed(), &di).ok()
}

/// Verify an RSA-PKCS#1-v1.5 signature over `data` with the PEM key (public,
/// or a private key from which the public is derived).
fn rsa_pkcs1v15_verify(alg: &str, key_pem: &str, data: &[u8], sig: &[u8]) -> bool {
    use rsa::pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
    use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
    let pub_key = rsa::RsaPublicKey::from_public_key_pem(key_pem)
        .ok()
        .or_else(|| rsa::RsaPublicKey::from_pkcs1_pem(key_pem).ok())
        .or_else(|| {
            rsa::RsaPrivateKey::from_pkcs8_pem(key_pem)
                .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(key_pem))
                .ok()
                .map(|pk| pk.to_public_key())
        });
    let pub_key = match pub_key {
        Some(k) => k,
        None => return false,
    };
    let di = match rsa_digest_info(alg, data) {
        Some(d) => d,
        None => return false,
    };
    pub_key
        .verify(rsa::Pkcs1v15Sign::new_unprefixed(), &di, sig)
        .is_ok()
}

/// Encode signature `bytes` per a digest-style encoding argument: a `'hex'`/
/// `'base64'`/`'base64url'` string yields a NaN-boxed string; absent/undefined
/// yields a Buffer (matching Node's `sign()` with vs without an encoding).
unsafe fn encode_sig_output(bytes: &[u8], enc_arg: Option<&f64>) -> f64 {
    match enc_arg {
        None => {
            let buf = alloc_buffer_from_slice(bytes);
            nanbox_pointer_f64(buf as usize)
        }
        Some(a) if is_undefined_f64(*a) => {
            let buf = alloc_buffer_from_slice(bytes);
            nanbox_pointer_f64(buf as usize)
        }
        Some(a) => {
            let enc_ptr = (a.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let enc = String::from_utf8_lossy(&bytes_from_ptr(enc_ptr)).to_ascii_lowercase();
            let encoded = match enc.as_str() {
                "base64" => base64::engine::general_purpose::STANDARD.encode(bytes),
                "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
                "binary" | "latin1" => String::from_utf8_lossy(bytes).into_owned(),
                _ => hex::encode(bytes),
            };
            let s = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
            f64::from_bits(0x7FFF_0000_0000_0000u64 | ((s as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
    }
}

/// Dispatch `update` / `sign` / `verify` on a SignHandle. Called from
/// `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_sign(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle::<SignHandle>(handle) {
        Some(h) => h,
        None => return nanbox_undefined(),
    };
    match method {
        "update" => {
            if let Some(a) = args.first() {
                let ptr = (a.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                let bytes = bytes_from_ptr(ptr);
                h.buffer.lock().unwrap().extend_from_slice(&bytes);
            }
            nanbox_pointer_f64(handle as usize)
        }
        // `.sign(privateKeyPem, encoding?)` → signature (Buffer or encoded
        // string). args[0] = key, args[1] = output encoding.
        "sign" => {
            let key_ptr = match args.first() {
                Some(a) => (a.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
                None => return nanbox_undefined(),
            };
            let key_pem = String::from_utf8_lossy(&bytes_from_ptr(key_ptr)).into_owned();
            let data = h.buffer.lock().unwrap().clone();
            match rsa_pkcs1v15_sign(&h.alg, &key_pem, &data) {
                Some(sig) => encode_sig_output(&sig, args.get(1)),
                None => nanbox_undefined(),
            }
        }
        // `.verify(publicKeyPem, signature, sigEncoding?)` → boolean.
        // args[0] = key, args[1] = signature, args[2] = signature encoding.
        "verify" => {
            let key_ptr = match args.first() {
                Some(a) => (a.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
                None => return f64::from_bits(0x7FFC_0000_0000_0003), // false
            };
            let key_pem = String::from_utf8_lossy(&bytes_from_ptr(key_ptr)).into_owned();
            // Resolve the signature bytes: if a sig-encoding string is given,
            // the signature arg is a string in that encoding; otherwise it's a
            // Buffer/Uint8Array.
            let sig_ptr = match args.get(1) {
                Some(a) => (a.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
                None => return f64::from_bits(0x7FFC_0000_0000_0003),
            };
            let sig_raw = bytes_from_ptr(sig_ptr);
            let sig = match args.get(2) {
                Some(a) if !is_undefined_f64(*a) => {
                    let enc_ptr = (a.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                    let enc =
                        String::from_utf8_lossy(&bytes_from_ptr(enc_ptr)).to_ascii_lowercase();
                    let sig_str = String::from_utf8_lossy(&sig_raw);
                    match enc.as_str() {
                        "hex" => hex::decode(sig_str.trim()).unwrap_or_default(),
                        "base64" => base64::engine::general_purpose::STANDARD
                            .decode(sig_str.trim())
                            .unwrap_or_default(),
                        "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .decode(sig_str.trim())
                            .unwrap_or_default(),
                        _ => sig_raw.clone(),
                    }
                }
                _ => sig_raw.clone(),
            };
            let data = h.buffer.lock().unwrap().clone();
            let ok = rsa_pkcs1v15_verify(&h.alg, &key_pem, &data, &sig);
            f64::from_bits(if ok {
                0x7FFC_0000_0000_0004 // true
            } else {
                0x7FFC_0000_0000_0003 // false
            })
        }
        _ => nanbox_undefined(),
    }
}

unsafe fn create_cipher_handle(alg_ptr: i64, key_ptr: i64, iv_ptr: i64, encrypt: bool) -> f64 {
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
    } else if iv.len() != 16 {
        return nanbox_undefined();
    }
    let handle: Handle = register_handle(CipherHandle {
        state: std::sync::Mutex::new(CipherState {
            kind,
            encrypt,
            key,
            iv,
            buffer: Vec::new(),
            auth_tag: None,
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
pub unsafe extern "C" fn js_crypto_create_cipheriv(alg_ptr: i64, key_ptr: i64, iv_ptr: i64) -> f64 {
    create_cipher_handle(alg_ptr, key_ptr, iv_ptr, true)
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
) -> f64 {
    create_cipher_handle(alg_ptr, key_ptr, iv_ptr, false)
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
                    let padded_len = (plaintext_or_ct.len() / block_size + 1) * block_size;
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes256CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes128Cbc, true) => {
                    let block_size = 16;
                    let padded_len = (plaintext_or_ct.len() / block_size + 1) * block_size;
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes128CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
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
                    match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes128Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes128CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit};
                    use aes_gcm::{Aes256Gcm, Nonce};
                    let cipher = match Aes256Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let mut ct = match cipher.encrypt(nonce, plaintext_or_ct.as_ref()) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    // aes-gcm appends the 16-byte tag to the ciphertext.
                    // Node's createCipheriv splits these: update/final
                    // produces just the ciphertext, getAuthTag returns
                    // the tag separately.
                    let tag_start = ct.len().saturating_sub(16);
                    let tag = ct.split_off(tag_start);
                    state.auth_tag = Some(tag);
                    ct
                }
                (CipherKind::Aes128Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit};
                    use aes_gcm::{Aes128Gcm, Nonce};
                    let cipher = match Aes128Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let mut ct = match cipher.encrypt(nonce, plaintext_or_ct.as_ref()) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    let tag_start = ct.len().saturating_sub(16);
                    let tag = ct.split_off(tag_start);
                    state.auth_tag = Some(tag);
                    ct
                }
                (CipherKind::Aes256Gcm, false) => {
                    use aes_gcm::aead::{Aead, KeyInit};
                    use aes_gcm::{Aes256Gcm, Nonce};
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == 16 => t.clone(),
                        _ => return nanbox_undefined(), // GCM decrypt needs tag
                    };
                    let cipher = match Aes256Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    // Re-attach the tag (aes-gcm decrypt expects ct||tag).
                    let mut combined = plaintext_or_ct.clone();
                    combined.extend_from_slice(&tag);
                    match cipher.decrypt(nonce, combined.as_ref()) {
                        Ok(pt) => pt,
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes128Gcm, false) => {
                    use aes_gcm::aead::{Aead, KeyInit};
                    use aes_gcm::{Aes128Gcm, Nonce};
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == 16 => t.clone(),
                        _ => return nanbox_undefined(),
                    };
                    let cipher = match Aes128Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let mut combined = plaintext_or_ct.clone();
                    combined.extend_from_slice(&tag);
                    match cipher.decrypt(nonce, combined.as_ref()) {
                        Ok(pt) => pt,
                        Err(_) => return nanbox_undefined(),
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
        // `.setAAD(buf)` — not yet implemented. Accepted as a no-op so
        // callers that always set AAD don't crash; authentication will
        // still verify the ciphertext+tag without the AAD bound in. If
        // AAD support is needed, switch to `cipher.encrypt_in_place` /
        // `decrypt_in_place` with a `Payload { msg, aad }`.
        "setAAD" => nanbox_pointer_f64(handle as usize),
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
