use super::*;

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
pub(super) fn is_undefined_f64(v: f64) -> bool {
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
