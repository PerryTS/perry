//! `crypto.Hash` / `crypto.Hmac`: ordinary objects that own their digest
//! state (#11919 P0, `perry_runtime::native_payload`).
//!
//! `createHash` / `createHmac` return a `GC_TYPE_OBJECT` of the family's class
//! id whose prototype carries `update` / `digest` / `copy` and the small
//! stream surface (`write` / `end` / `on` / `pipe` / `setEncoding`). The
//! hasher lives in the object's native payload; `digest()` / `end()` close it,
//! which drops it at once, and a Hash that is never digested is dropped by the
//! collection that finds it dead. Stream listeners and pipe destinations are
//! JS values, so they live in the object's hidden JS-state object, never in
//! the payload, and the stream's events are delivered by a `process.nextTick`
//! closure that holds the object.
//!
//! The handle-free chain path (`hash_chain.rs`, #11516) shares the digest
//! helpers below and allocates no object at all.

use super::*;
use perry_runtime::closure::{js_closure_call0, js_closure_call1, ClosureHeader, JsThis};
use perry_runtime::native_class_ids::{CRYPTO_HASH, CRYPTO_HMAC};
use perry_runtime::native_payload::{self, NativePayloadFamily, PayloadMiss, PayloadPrototype};

/// node validates the `data` arg of `hash.update`/`hmac.update` is a string
/// or `Buffer`/`TypedArray`/`DataView` before touching it. Without this a
/// number/null has its bit pattern masked into a bogus pointer and
/// dereferenced (segfault on `createHash('sha256').update(123)`). Returns the
/// validated value so the caller can decode it.
fn validate_update_data(args: &[f64]) -> f64 {
    let data = args
        .first()
        .copied()
        .unwrap_or_else(|| f64::from_bits(0x7FFC_0000_0000_0001));
    perry_runtime::validators::validate_string_or_buffer_view(
        data,
        "data",
        "of type string or an instance of Buffer, TypedArray, or DataView",
    );
    data
}

const UNDEFINED: f64 = f64::from_bits(0x7FFC_0000_0000_0001);

fn is_undefined(value: f64) -> bool {
    value.to_bits() == UNDEFINED.to_bits()
}

/// The arguments a fixed-arity prototype method received, without the
/// trailing `undefined`s the call padded it with, so the shared decoders see
/// the same slice the old variadic dispatch saw.
pub(super) fn passed_args(args: &[f64]) -> &[f64] {
    let mut len = args.len();
    while len > 0 && is_undefined(args[len - 1]) {
        len -= 1;
    }
    &args[..len]
}

fn js_true() -> f64 {
    f64::from_bits(JSValue::bool(true).bits())
}

pub(super) fn update_hash_state(state: &mut HashState, bytes: &[u8]) {
    match state {
        HashState::Sha1(x) => Sha256Digest::update(x, bytes),
        HashState::Sha224(x) => Sha256Digest::update(x, bytes),
        HashState::Sha256(x) => Sha256Digest::update(x, bytes),
        HashState::Sha384(x) => Sha256Digest::update(x, bytes),
        HashState::Sha512(x) => Sha256Digest::update(x, bytes),
        HashState::Sha512_256(x) => Sha256Digest::update(x, bytes),
        HashState::Shake128(x) => shake::Update::update(x, bytes),
        HashState::Shake256(x) => shake::Update::update(x, bytes),
        HashState::Md5(x) => Md5Digest::update(x, bytes),
    }
}

fn finalize_hash_state(
    state: Option<HashState>,
    output_len: Option<usize>,
    option_len: Option<usize>,
) -> Option<Vec<u8>> {
    Some(match state? {
        HashState::Sha1(x) => x.finalize().to_vec(),
        HashState::Sha224(x) => x.finalize().to_vec(),
        HashState::Sha256(x) => x.finalize().to_vec(),
        HashState::Sha384(x) => x.finalize().to_vec(),
        HashState::Sha512(x) => x.finalize().to_vec(),
        HashState::Sha512_256(x) => x.finalize().to_vec(),
        HashState::Shake128(x) => {
            let mut out = vec![0u8; option_len.or(output_len).unwrap_or(16)];
            let mut reader = x.finalize_xof();
            reader.read(&mut out);
            out
        }
        HashState::Shake256(x) => {
            let mut out = vec![0u8; option_len.or(output_len).unwrap_or(32)];
            let mut reader = x.finalize_xof();
            reader.read(&mut out);
            out
        }
        HashState::Md5(x) => x.finalize().to_vec(),
    })
}

pub(super) fn update_hmac_state(state: &mut HmacState, bytes: &[u8]) {
    use hmac::Mac;
    match state {
        HmacState::Sha1(x) => Mac::update(x, bytes),
        HmacState::Sha224(x) => Mac::update(x, bytes),
        HmacState::Sha256(x) => Mac::update(x, bytes),
        HmacState::Sha384(x) => Mac::update(x, bytes),
        HmacState::Sha512(x) => Mac::update(x, bytes),
        HmacState::Sha512_256(x) => Mac::update(x, bytes),
        HmacState::Md5(x) => Mac::update(x, bytes),
    }
}

fn finalize_hmac_state(state: Option<HmacState>) -> Vec<u8> {
    use hmac::Mac;
    match state {
        Some(HmacState::Sha1(x)) => x.finalize().into_bytes().to_vec(),
        Some(HmacState::Sha224(x)) => x.finalize().into_bytes().to_vec(),
        Some(HmacState::Sha256(x)) => x.finalize().into_bytes().to_vec(),
        Some(HmacState::Sha384(x)) => x.finalize().into_bytes().to_vec(),
        Some(HmacState::Sha512(x)) => x.finalize().into_bytes().to_vec(),
        Some(HmacState::Sha512_256(x)) => x.finalize().into_bytes().to_vec(),
        Some(HmacState::Md5(x)) => x.finalize().into_bytes().to_vec(),
        None => Vec::new(),
    }
}

pub(super) fn latin1_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

fn encoded_digest(bytes: &[u8], encoding: &str) -> String {
    match encoding {
        "hex" => perry_hex::encode(bytes),
        "base64" => perry_base64::engine::general_purpose::STANDARD.encode(bytes),
        "base64url" => perry_base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
        "binary" | "latin1" => latin1_string(bytes),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

unsafe fn digest_value(bytes: &[u8], encoding: Option<&str>) -> f64 {
    if let Some(enc) = encoding {
        let encoded = encoded_digest(bytes, &enc.to_ascii_lowercase());
        let s = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
        return nanbox_str(s);
    }
    nanbox_pointer_f64(alloc_buffer_from_slice(bytes) as usize)
}

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

/// The digest state `createHash(alg, options)` starts from, shared by the
/// handle path above and the handle-free chain path (`hash_chain.rs`, #11516)
/// so both accept exactly the same algorithms and options. An unsupported
/// algorithm throws node's `Error: Digest method not supported`.
pub(super) unsafe fn new_hash_state_or_throw(
    alg_ptr: i64,
    options_bits: f64,
) -> (HashState, Option<usize>) {
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
        _ => throw_plain_error("Digest method not supported"),
    };
    let output_len = object_field_bits(options_bits.to_bits(), b"outputLength")
        .and_then(|bits| nanboxed_to_usize(f64::from_bits(bits)));
    (state, output_len)
}

/// Throw a plain `Error` with no `code`, as node's native crypto binding does
/// for `createHash` with an unknown algorithm.
fn throw_plain_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(msg);
    perry_runtime::exception::js_throw(f64::from_bits(
        0x7FFD_0000_0000_0000u64 | ((err as u64) & 0x0000_FFFF_FFFF_FFFF),
    ))
}

/// Validate and decode the `(data, inputEncoding?)` arguments of
/// `hash.update` / `hmac.update`. Shared with the chain path (#11516).
pub(super) unsafe fn hash_update_bytes(args: &[f64]) -> Vec<u8> {
    let data = validate_update_data(args);
    let encoding = arg_string(args, 1);
    decode_hash_update_value(data, &encoding)
}

/// `hash.digest(outputEncoding?)` on a taken state. Shared with the chain
/// path (#11516). `None` state means "already finalized"; callers throw
/// `ERR_CRYPTO_HASH_FINALIZED` before reaching here.
pub(super) unsafe fn hash_digest_value(
    state: Option<HashState>,
    output_len: Option<usize>,
    arg: Option<f64>,
) -> f64 {
    let enc = super::hash_chain::DigestEncoding::parse(arg);
    let Some(digest) = finalize_hash_state(state, output_len, enc.output_len) else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    enc.output(&digest)
}

/// `hmac.digest(outputEncoding?)` on a taken state. A finalized HMAC digests
/// to an empty value in the requested shape, as node does. Shared with the
/// chain path (#11516).
pub(super) unsafe fn hmac_digest_value(state: Option<HmacState>, arg: Option<f64>) -> f64 {
    let digest = finalize_hmac_state(state);
    super::hash_chain::DigestEncoding::parse(arg).output(&digest)
}

pub enum HmacState {
    Sha1(hmac::Hmac<Sha1>),
    Sha224(hmac::Hmac<Sha224>),
    Sha256(hmac::Hmac<Sha256>),
    Sha384(hmac::Hmac<Sha384>),
    Sha512(hmac::Hmac<Sha512>),
    Sha512_256(hmac::Hmac<Sha512_256>),
    Md5(hmac::Hmac<Md5>),
}

/// The MAC state `createHmac(alg, key)` starts from, shared by the handle
/// path above and the handle-free chain path (`hash_chain.rs`, #11516). An
/// unsupported algorithm throws node's
/// `TypeError [ERR_CRYPTO_INVALID_DIGEST]: Invalid digest: <alg>`.
pub(super) unsafe fn new_hmac_state_or_throw(alg_ptr: i64, key_ptr: i64) -> HmacState {
    match new_hmac_state(alg_ptr, key_ptr) {
        Some(state) => state,
        None => {
            let alg_bytes = bytes_from_ptr(alg_ptr);
            let alg = String::from_utf8_lossy(&alg_bytes);
            perry_runtime::fs::validate::throw_type_error_with_code(
                &format!("Invalid digest: {alg}"),
                "ERR_CRYPTO_INVALID_DIGEST",
            )
        }
    }
}

unsafe fn new_hmac_state(alg_ptr: i64, key_ptr: i64) -> Option<HmacState> {
    use hmac::KeyInit;
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes)
        .unwrap_or("")
        .to_ascii_lowercase();
    let key = bytes_from_ptr(key_ptr);
    let state = match alg.as_str() {
        "sha1" | "sha-1" => match hmac::Hmac::<Sha1>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha1(m),
            Err(_) => return None,
        },
        "sha224" | "sha-224" => match hmac::Hmac::<Sha224>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha224(m),
            Err(_) => return None,
        },
        "sha256" | "sha-256" => match hmac::Hmac::<Sha256>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha256(m),
            Err(_) => return None,
        },
        "sha384" | "sha-384" => match hmac::Hmac::<Sha384>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha384(m),
            Err(_) => return None,
        },
        "sha512" | "sha-512" => match hmac::Hmac::<Sha512>::new_from_slice(&key) {
            Ok(m) => HmacState::Sha512(m),
            Err(_) => return None,
        },
        "sha512-256" | "sha512_256" | "sha-512-256" => {
            match hmac::Hmac::<Sha512_256>::new_from_slice(&key) {
                Ok(m) => HmacState::Sha512_256(m),
                Err(_) => return None,
            }
        }
        "md5" => match hmac::Hmac::<Md5>::new_from_slice(&key) {
            Ok(m) => HmacState::Md5(m),
            Err(_) => return None,
        },
        _ => return None,
    };
    Some(state)
}

// ---------------------------------------------------------------------------
// The two families.
// ---------------------------------------------------------------------------

/// The payload of a `Hash`. Closed by `digest()` / `end()`.
pub struct HashPayload {
    /// `Option` only so `digest()` can move the hasher out (sha1/sha2
    /// `finalize()` consumes `self`); it is `Some` while the payload is open.
    state: Option<HashState>,
    output_len: Option<usize>,
}

/// The payload of an `Hmac`. Closed by `digest()` / `end()`.
pub struct HmacPayload {
    state: Option<HmacState>,
}

pub(super) static HASH_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: CRYPTO_HASH,
    name: "Hash",
    constructor_export: None,
    constructor_length: 2,
    links_owner: false,
    install_prototype: install_hash_prototype,
};

pub(super) static HMAC_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: CRYPTO_HMAC,
    name: "Hmac",
    constructor_export: None,
    constructor_length: 3,
    links_owner: false,
    install_prototype: install_hmac_prototype,
};

macro_rules! builtin {
    ($body:path, $n:tt) => {
        perry_runtime::fn_info!($body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN))
    };
}

fn install_stream_methods(proto: &mut PayloadPrototype, kind: DigestKind) {
    match kind {
        DigestKind::Hash => {
            proto.method("write", builtin!(hash_write_thunk, 2), 2);
            proto.method("end", builtin!(hash_end_thunk, 2), 2);
        }
        DigestKind::Hmac => {
            proto.method("write", builtin!(hmac_write_thunk, 2), 2);
            proto.method("end", builtin!(hmac_end_thunk, 2), 2);
        }
    }
    proto.method("on", builtin!(digest_on_thunk, 2), 2);
    proto.method("once", builtin!(digest_on_thunk, 2), 2);
    proto.method("addListener", builtin!(digest_on_thunk, 2), 2);
    proto.method("pipe", builtin!(digest_pipe_thunk, 1), 1);
    proto.method("setEncoding", builtin!(digest_set_encoding_thunk, 1), 1);
    proto.method("destroy", builtin!(digest_destroy_thunk, 0), 0);
    proto.method("close", builtin!(digest_destroy_thunk, 0), 0);
}

fn install_hash_prototype(proto: &mut PayloadPrototype) {
    // node's own names on Hash.prototype: copy, update, digest (+ the
    // stream internals); the stream methods node inherits from Transform
    // are installed here directly.
    proto.method("copy", builtin!(hash_copy_thunk, 1), 1);
    proto.method("update", builtin!(hash_update_thunk, 2), 2);
    proto.method("digest", builtin!(hash_digest_thunk, 1), 1);
    install_stream_methods(proto, DigestKind::Hash);
}

fn install_hmac_prototype(proto: &mut PayloadPrototype) {
    proto.method("update", builtin!(hmac_update_thunk, 2), 2);
    proto.method("digest", builtin!(hmac_digest_thunk, 1), 1);
    install_stream_methods(proto, DigestKind::Hmac);
}

#[derive(Clone, Copy)]
enum DigestKind {
    Hash,
    Hmac,
}

impl DigestKind {
    fn family(self) -> &'static NativePayloadFamily {
        match self {
            DigestKind::Hash => &HASH_FAMILY,
            DigestKind::Hmac => &HMAC_FAMILY,
        }
    }

    fn of(value: f64) -> Option<DigestKind> {
        if native_payload::is_instance(value, &HASH_FAMILY) {
            Some(DigestKind::Hash)
        } else if native_payload::is_instance(value, &HMAC_FAMILY) {
            Some(DigestKind::Hmac)
        } else {
            None
        }
    }
}

/// node's own enumerable properties of a fresh Hash / Hmac: `_options` (the
/// options argument, `undefined` when none was passed).
fn digest_object<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: T,
    options: f64,
) -> f64 {
    let bytes = std::mem::size_of::<T>();
    native_payload::alloc(family, payload, bytes, &[(b"_options", options)])
}

fn throw_digest_already_called() -> ! {
    perry_runtime::fs::validate::throw_error_with_code(
        "Digest already called",
        "ERR_CRYPTO_HASH_FINALIZED",
    )
}

/// `crypto.createHash(alg)` — a new `Hash`. An unsupported algorithm throws
/// node's `Error: Digest method not supported`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hash(alg_ptr: i64) -> f64 {
    js_crypto_create_hash_options(alg_ptr, UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hash_options(alg_ptr: i64, options_bits: f64) -> f64 {
    let (state, output_len) = new_hash_state_or_throw(alg_ptr, options_bits);
    digest_object(
        &HASH_FAMILY,
        HashPayload {
            state: Some(state),
            output_len,
        },
        options_bits,
    )
}

/// `crypto.createHmac(alg, key)` — a new `Hmac`. An unsupported algorithm
/// throws node's `ERR_CRYPTO_INVALID_DIGEST`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_hmac(alg_ptr: i64, key_ptr: i64) -> f64 {
    let state = new_hmac_state_or_throw(alg_ptr, key_ptr);
    digest_object(&HMAC_FAMILY, HmacPayload { state: Some(state) }, UNDEFINED)
}

/// Feed `bytes` to an open payload. False when the payload is closed (or the
/// receiver is foreign).
unsafe fn feed(this: f64, kind: DigestKind, bytes: &[u8]) -> bool {
    match kind {
        DigestKind::Hash => match native_payload::payload_mut::<HashPayload>(this, &HASH_FAMILY) {
            Ok(p) => {
                if let Some(state) = p.state.as_mut() {
                    update_hash_state(state, bytes);
                }
                true
            }
            Err(_) => false,
        },
        DigestKind::Hmac => match native_payload::payload_mut::<HmacPayload>(this, &HMAC_FAMILY) {
            Ok(p) => {
                if let Some(state) = p.state.as_mut() {
                    update_hmac_state(state, bytes);
                }
                true
            }
            Err(_) => false,
        },
    }
}

/// Close the payload and return its digest bytes (`None` when it was already
/// closed). The payload is dropped before the result is allocated.
unsafe fn finish(this: f64, kind: DigestKind, option_len: Option<usize>) -> Option<Vec<u8>> {
    let bytes = match kind {
        DigestKind::Hash => {
            let p = native_payload::payload_mut::<HashPayload>(this, &HASH_FAMILY).ok()?;
            let (state, output_len) = (p.state.take(), p.output_len);
            finalize_hash_state(state, output_len, option_len)
        }
        DigestKind::Hmac => {
            let p = native_payload::payload_mut::<HmacPayload>(this, &HMAC_FAMILY).ok()?;
            Some(finalize_hmac_state(p.state.take()))
        }
    };
    native_payload::close(this, kind.family());
    bytes
}

fn payload_state(this: f64, kind: DigestKind) -> Result<(), PayloadMiss> {
    unsafe {
        match kind {
            DigestKind::Hash => {
                native_payload::payload_mut::<HashPayload>(this, &HASH_FAMILY).map(|_| ())
            }
            DigestKind::Hmac => {
                native_payload::payload_mut::<HmacPayload>(this, &HMAC_FAMILY).map(|_| ())
            }
        }
    }
}

extern "C" fn hash_update_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    data: f64,
    enc: f64,
) -> f64 {
    digest_update(this.as_f64(), DigestKind::Hash, data, enc)
}

extern "C" fn hmac_update_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    data: f64,
    enc: f64,
) -> f64 {
    digest_update(this.as_f64(), DigestKind::Hmac, data, enc)
}

/// `update(data, inputEncoding?)` returns `this`. After `digest()` both
/// families throw `ERR_CRYPTO_HASH_FINALIZED` (#2944 / #2945).
fn digest_update(this: f64, kind: DigestKind, data: f64, enc: f64) -> f64 {
    match payload_state(this, kind) {
        Err(PayloadMiss::Foreign) => return UNDEFINED,
        Err(PayloadMiss::Closed) => throw_digest_already_called(),
        Ok(()) => {}
    }
    let args = [data, enc];
    // Decoding validates and may allocate, so it runs before the payload is
    // borrowed.
    let bytes = unsafe { hash_update_bytes(passed_args(&args)) };
    unsafe { feed(this, kind, &bytes) };
    this
}

extern "C" fn hash_digest_thunk(_c: *const ClosureHeader, this: JsThis, enc: f64) -> f64 {
    let this = this.as_f64();
    match payload_state(this, DigestKind::Hash) {
        Err(PayloadMiss::Foreign) => return UNDEFINED,
        Err(PayloadMiss::Closed) => throw_digest_already_called(),
        Ok(()) => {}
    }
    let arg = (!is_undefined(enc)).then_some(enc);
    let encoding = unsafe { super::hash_chain::DigestEncoding::parse(arg) };
    let Some(digest) = (unsafe { finish(this, DigestKind::Hash, encoding.output_len) }) else {
        return UNDEFINED;
    };
    unsafe { encoding.output(&digest) }
}

/// `hmac.digest(enc?)`. node keeps a second `digest()` idempotent in shape:
/// an empty string for an encoded digest, an empty Buffer otherwise.
extern "C" fn hmac_digest_thunk(_c: *const ClosureHeader, this: JsThis, enc: f64) -> f64 {
    let this = this.as_f64();
    let arg = (!is_undefined(enc)).then_some(enc);
    match payload_state(this, DigestKind::Hmac) {
        Err(PayloadMiss::Foreign) => UNDEFINED,
        Err(PayloadMiss::Closed) => unsafe { hmac_digest_value(None, arg) },
        Ok(()) => {
            let digest = unsafe { finish(this, DigestKind::Hmac, None) }.unwrap_or_default();
            unsafe { super::hash_chain::DigestEncoding::parse(arg).output(&digest) }
        }
    }
}

/// `hash.copy()` (#1369): an independent Hash whose state is a snapshot of
/// this one. A digested hash throws `ERR_CRYPTO_HASH_FINALIZED`.
extern "C" fn hash_copy_thunk(_c: *const ClosureHeader, this: JsThis, _options: f64) -> f64 {
    let this = this.as_f64();
    let snapshot = unsafe { native_payload::payload_mut::<HashPayload>(this, &HASH_FAMILY) }
        .map(|p| (p.state.clone(), p.output_len));
    match snapshot {
        Err(PayloadMiss::Foreign) => UNDEFINED,
        Err(PayloadMiss::Closed) | Ok((None, _)) => throw_digest_already_called(),
        Ok((Some(state), output_len)) => digest_object(
            &HASH_FAMILY,
            HashPayload {
                state: Some(state),
                output_len,
            },
            UNDEFINED,
        ),
    }
}

// ---------------------------------------------------------------------------
// The stream surface. Every JS value it keeps (listeners, pipe destinations,
// the output encoding, the ended flag) is a field of the object's hidden
// JS-state object, so the collector traces and moves it with the object.
// ---------------------------------------------------------------------------

/// `state[key]` for a runtime-chosen ASCII key. `state` is rooted across the
/// key's allocation.
unsafe fn state_get(state: f64, key: &str) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(state);
    let key = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let obj = (state.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF)
        as *mut perry_runtime::object::ObjectHeader;
    perry_runtime::object::js_object_get_field_by_name_f64(obj, key)
}

/// `state[key] = value`, with `state` and `value` rooted across the key's
/// allocation and the store.
unsafe fn state_set(state: f64, key: &str, value: f64) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(state);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(js_string_from_bytes(key.as_ptr(), key.len() as u32));
    key.with_const_ptr::<StringHeader, _>(|key| {
        let obj = (state.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF)
            as *mut perry_runtime::object::ObjectHeader;
        perry_runtime::object::js_object_set_field_by_name(obj, key, value.get_nanbox_f64())
    });
}

/// Append `value` to the array at `state[key]`, creating it.
unsafe fn state_push(state: f64, key: &str, value: f64) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(state);
    let value = scope.root_nanbox_f64(value);
    let existing = state_get(state.get_nanbox_f64(), key);
    let array = if JSValue::from_bits(existing.to_bits()).is_pointer() {
        (existing.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut perry_runtime::array::ArrayHeader
    } else {
        perry_runtime::array::js_array_alloc(2)
    };
    let array = perry_runtime::array::js_array_push_f64(array, value.get_nanbox_f64());
    state_set(
        state.get_nanbox_f64(),
        key,
        perry_runtime::value::js_nanbox_pointer(array as i64),
    );
}

/// The elements of the array at `state[key]` (empty when absent), copied
/// out so a listener that adds listeners cannot disturb the walk.
unsafe fn state_list(state: f64, key: &str) -> Vec<f64> {
    let existing = state_get(state, key);
    if !JSValue::from_bits(existing.to_bits()).is_pointer() {
        return Vec::new();
    }
    let array =
        (existing.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::array::ArrayHeader;
    let len = perry_runtime::array::js_array_length(array);
    (0..len)
        .map(|i| perry_runtime::array::js_array_get_f64(array, i))
        .collect()
}

fn listener_key(event: &str) -> String {
    format!("on:{event}")
}

extern "C" fn digest_on_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    event: f64,
    listener: f64,
) -> f64 {
    let this = this.as_f64();
    let Some(kind) = DigestKind::of(this) else {
        return UNDEFINED;
    };
    let Some(event) = (unsafe { string_from_jsvalue(event.to_bits()) }) else {
        return this;
    };
    if !JSValue::from_bits(listener.to_bits()).is_pointer() {
        return this;
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_root = scope.root_nanbox_f64(this);
    let listener = scope.root_nanbox_f64(listener);
    let state = native_payload::js_state(this_root.get_nanbox_f64(), kind.family(), true);
    unsafe { state_push(state, &listener_key(&event), listener.get_nanbox_f64()) };
    this_root.get_nanbox_f64()
}

extern "C" fn digest_pipe_thunk(_c: *const ClosureHeader, this: JsThis, dest: f64) -> f64 {
    let this = this.as_f64();
    let Some(kind) = DigestKind::of(this) else {
        return UNDEFINED;
    };
    if is_undefined(dest) {
        return UNDEFINED;
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let dest = scope.root_nanbox_f64(dest);
    let state = native_payload::js_state(this, kind.family(), true);
    unsafe { state_push(state, "pipes", dest.get_nanbox_f64()) };
    dest.get_nanbox_f64()
}

extern "C" fn digest_set_encoding_thunk(_c: *const ClosureHeader, this: JsThis, enc: f64) -> f64 {
    let this = this.as_f64();
    let Some(kind) = DigestKind::of(this) else {
        return UNDEFINED;
    };
    let encoding = unsafe { string_from_jsvalue(enc.to_bits()) }.map(|s| s.to_ascii_lowercase());
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_root = scope.root_nanbox_f64(this);
    let value = match encoding {
        Some(enc) => nanbox_str(js_string_from_bytes(enc.as_ptr(), enc.len() as u32)),
        None => UNDEFINED,
    };
    let value = scope.root_nanbox_f64(value);
    let state = native_payload::js_state(this_root.get_nanbox_f64(), kind.family(), true);
    unsafe { state_set(state, "encoding", value.get_nanbox_f64()) };
    this_root.get_nanbox_f64()
}

/// `destroy()` / `close()`: release the hasher now.
extern "C" fn digest_destroy_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    let this = this.as_f64();
    if let Some(kind) = DigestKind::of(this) {
        native_payload::close(this, kind.family());
    }
    UNDEFINED
}

extern "C" fn hash_write_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    chunk: f64,
    enc: f64,
) -> f64 {
    digest_write(this.as_f64(), DigestKind::Hash, chunk, enc)
}

extern "C" fn hmac_write_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    chunk: f64,
    enc: f64,
) -> f64 {
    digest_write(this.as_f64(), DigestKind::Hmac, chunk, enc)
}

fn digest_write(this: f64, kind: DigestKind, chunk: f64, enc: f64) -> f64 {
    if DigestKind::of(this).is_none() || is_undefined(chunk) {
        return UNDEFINED;
    }
    let args = [chunk, enc];
    let args = passed_args(&args);
    let encoding = unsafe { arg_string(args, 1) };
    let bytes = unsafe { decode_hash_update_value(chunk, &encoding) };
    unsafe { feed(this, kind, &bytes) };
    js_true()
}

extern "C" fn hash_end_thunk(_c: *const ClosureHeader, this: JsThis, chunk: f64, enc: f64) -> f64 {
    digest_end(this.as_f64(), DigestKind::Hash, chunk, enc)
}

extern "C" fn hmac_end_thunk(_c: *const ClosureHeader, this: JsThis, chunk: f64, enc: f64) -> f64 {
    digest_end(this.as_f64(), DigestKind::Hmac, chunk, enc)
}

/// `end(chunk?)`: feed the last chunk, close the payload, and deliver the
/// digest as one `data` event (and to every pipe), then `end` / `finish` /
/// `close`, on the next tick.
fn digest_end(this: f64, kind: DigestKind, chunk: f64, enc: f64) -> f64 {
    if DigestKind::of(this).is_none() {
        return UNDEFINED;
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_root = scope.root_nanbox_f64(this);
    let v = JSValue::from_bits(chunk.to_bits());
    if !v.is_undefined() && !v.is_null() {
        let args = [chunk, enc];
        let args = passed_args(&args);
        let encoding = unsafe { arg_string(args, 1) };
        let bytes = unsafe { decode_hash_update_value(chunk, &encoding) };
        unsafe { feed(this_root.get_nanbox_f64(), kind, &bytes) };
    }
    let state = scope.root_nanbox_f64(native_payload::js_state(
        this_root.get_nanbox_f64(),
        kind.family(),
        true,
    ));
    let ended = unsafe { state_get(state.get_nanbox_f64(), "ended") };
    if JSValue::from_bits(ended.to_bits()).is_bool() && ended.to_bits() == js_true().to_bits() {
        return this_root.get_nanbox_f64();
    }
    unsafe { state_set(state.get_nanbox_f64(), "ended", js_true()) };
    let encoding_value = unsafe { state_get(state.get_nanbox_f64(), "encoding") };
    let encoding = unsafe { string_from_jsvalue(encoding_value.to_bits()) };
    let Some(digest) = (unsafe { finish(this_root.get_nanbox_f64(), kind, None) }) else {
        return this_root.get_nanbox_f64();
    };
    let chunk = scope.root_nanbox_f64(unsafe { digest_value(&digest, encoding.as_deref()) });
    let tick =
        perry_runtime::closure::js_closure_alloc(perry_runtime::fn_info!(digest_stream_tick, 0), 2);
    perry_runtime::closure::js_closure_set_capture_f64(tick, 0, this_root.get_nanbox_f64());
    perry_runtime::closure::js_closure_set_capture_f64(tick, 1, chunk.get_nanbox_f64());
    perry_runtime::builtins::js_queue_next_tick(tick as i64);
    this_root.get_nanbox_f64()
}

/// The next-tick delivery `end()` scheduled: `data` with the digest, a
/// `write` to every pipe, then `end` / `finish` / `close` and `end()` on
/// every pipe.
extern "C" fn digest_stream_tick(closure: *const ClosureHeader, _this: JsThis) -> f64 {
    unsafe {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 0,
        ));
        let chunk = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 1,
        ));
        let Some(kind) = DigestKind::of(this.get_nanbox_f64()) else {
            return UNDEFINED;
        };
        let state = native_payload::js_state(this.get_nanbox_f64(), kind.family(), false);
        if !JSValue::from_bits(state.to_bits()).is_pointer() {
            return UNDEFINED;
        }
        let state = scope.root_nanbox_f64(state);
        let data_listeners =
            scope.root_nanbox_f64_slice(&state_list(state.get_nanbox_f64(), &listener_key("data")));
        let pipes = scope.root_nanbox_f64_slice(&state_list(state.get_nanbox_f64(), "pipes"));
        for cb in data_listeners.iter() {
            emit_callback1(cb.get_nanbox_f64(), chunk.get_nanbox_f64());
        }
        for dest in pipes.iter() {
            forward_method(dest.get_nanbox_f64(), b"write", &[chunk.get_nanbox_f64()]);
        }
        for event in ["end", "finish", "close"] {
            let listeners = state_list(state.get_nanbox_f64(), &listener_key(event));
            for cb in scope.root_nanbox_f64_slice(&listeners).iter() {
                emit_callback0(cb.get_nanbox_f64());
            }
        }
        for dest in pipes.iter() {
            forward_method(dest.get_nanbox_f64(), b"end", &[]);
        }
    }
    UNDEFINED
}

unsafe fn emit_callback0(cb: f64) {
    if JSValue::from_bits(cb.to_bits()).is_pointer() {
        let ptr = (cb.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ClosureHeader;
        js_closure_call0(ptr, perry_runtime::closure::plain_call_receiver());
    }
}

unsafe fn emit_callback1(cb: f64, arg: f64) {
    if JSValue::from_bits(cb.to_bits()).is_pointer() {
        let ptr = (cb.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ClosureHeader;
        js_closure_call1(ptr, perry_runtime::closure::plain_call_receiver(), arg);
    }
}

extern "C" {
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
}

unsafe fn forward_method(dest: f64, name: &[u8], args: &[f64]) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    if key.is_null() {
        return;
    }
    js_native_call_method_str_key(dest, key as i64, args.as_ptr(), args.len());
}
