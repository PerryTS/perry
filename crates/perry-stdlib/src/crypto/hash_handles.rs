use super::*;
use perry_runtime::{js_closure_call0, js_closure_call1, ClosureHeader};
use std::collections::HashMap;
use std::sync::{Mutex, Once};

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

#[derive(Default)]
struct CryptoDigestStream {
    listeners: HashMap<String, Vec<i64>>,
    pipes: Vec<u64>,
    encoding: Option<String>,
    ended: bool,
}

impl CryptoDigestStream {
    fn scan_roots(&mut self, visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
        for callbacks in self.listeners.values_mut() {
            for cb in callbacks {
                visitor.visit_i64_slot(cb);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum CryptoStreamKind {
    Hash,
    Hmac,
}

enum CryptoStreamEvent {
    Data {
        kind: CryptoStreamKind,
        handle: i64,
        bytes: Vec<u8>,
        encoding: Option<String>,
    },
    End {
        kind: CryptoStreamKind,
        handle: i64,
    },
}

lazy_static::lazy_static! {
    static ref CRYPTO_STREAM_PENDING_EVENTS: Mutex<Vec<CryptoStreamEvent>> = Mutex::new(Vec::new());
}

static CRYPTO_STREAM_GC_REGISTERED: Once = Once::new();

fn ensure_crypto_stream_gc_scanner() {
    CRYPTO_STREAM_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:crypto-streams",
            scan_crypto_stream_roots,
        );
    });
}

fn scan_crypto_stream_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    crate::common::handle::for_each_handle_mut_of::<HashHandle, _>(|h| {
        h.stream.lock().unwrap().scan_roots(visitor);
    });
    crate::common::handle::for_each_handle_mut_of::<HmacHandle, _>(|h| {
        h.stream.lock().unwrap().scan_roots(visitor);
    });
}

extern "C" {
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
}

fn nanbox_handle(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

fn js_true() -> f64 {
    f64::from_bits(JSValue::bool(true).bits())
}

fn unbox_to_i64(value: f64) -> i64 {
    (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}

fn update_hash_state(state: &mut HashState, bytes: &[u8]) {
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

fn update_hmac_state(state: &mut HmacState, bytes: &[u8]) {
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

fn encoded_digest(bytes: &[u8], encoding: &str) -> String {
    match encoding {
        "hex" => hex::encode(bytes),
        "base64" => base64::engine::general_purpose::STANDARD.encode(bytes),
        "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
        "binary" | "latin1" => String::from_utf8_lossy(bytes).into_owned(),
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

unsafe fn emit_callback0(cb: i64) {
    if cb != 0 {
        js_closure_call0(cb as *const ClosureHeader);
    }
}

unsafe fn emit_callback1(cb: i64, arg: f64) {
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, arg);
    }
}

fn listeners_for(kind: CryptoStreamKind, handle: i64, event: &str) -> Vec<i64> {
    match kind {
        CryptoStreamKind::Hash => get_handle_mut::<HashHandle>(handle)
            .and_then(|h| h.stream.lock().unwrap().listeners.get(event).cloned())
            .unwrap_or_default(),
        CryptoStreamKind::Hmac => get_handle_mut::<HmacHandle>(handle)
            .and_then(|h| h.stream.lock().unwrap().listeners.get(event).cloned())
            .unwrap_or_default(),
    }
}

fn pipes_for(kind: CryptoStreamKind, handle: i64) -> Vec<u64> {
    match kind {
        CryptoStreamKind::Hash => get_handle_mut::<HashHandle>(handle)
            .map(|h| h.stream.lock().unwrap().pipes.clone())
            .unwrap_or_default(),
        CryptoStreamKind::Hmac => get_handle_mut::<HmacHandle>(handle)
            .map(|h| h.stream.lock().unwrap().pipes.clone())
            .unwrap_or_default(),
    }
}

unsafe fn forward_method(dest_bits: u64, name: &[u8], args: &[f64]) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    if key.is_null() {
        return;
    }
    js_native_call_method_str_key(
        f64::from_bits(dest_bits),
        key as i64,
        args.as_ptr(),
        args.len(),
    );
}

unsafe fn forward_write(dest_bits: u64, bytes: &[u8], encoding: Option<&str>) {
    let chunk = digest_value(bytes, encoding);
    forward_method(dest_bits, b"write", &[chunk]);
}

unsafe fn forward_end(dest_bits: u64) {
    forward_method(dest_bits, b"end", &[]);
}

fn queue_crypto_stream_digest(
    kind: CryptoStreamKind,
    handle: i64,
    bytes: Vec<u8>,
    encoding: Option<String>,
) {
    {
        let mut pending = CRYPTO_STREAM_PENDING_EVENTS.lock().unwrap();
        pending.push(CryptoStreamEvent::Data {
            kind,
            handle,
            bytes,
            encoding,
        });
        pending.push(CryptoStreamEvent::End { kind, handle });
    }
    crate::common::async_bridge::ensure_pump_registered();
    perry_runtime::event_pump::js_notify_main_thread();
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_stream_process_pending() -> i32 {
    let events = {
        let mut pending = CRYPTO_STREAM_PENDING_EVENTS.lock().unwrap();
        std::mem::take(&mut *pending)
    };
    let count = events.len() as i32;
    for event in events {
        match event {
            CryptoStreamEvent::Data {
                kind,
                handle,
                bytes,
                encoding,
            } => {
                let callbacks = listeners_for(kind, handle, "data");
                if !callbacks.is_empty() {
                    let value = digest_value(&bytes, encoding.as_deref());
                    for cb in callbacks {
                        emit_callback1(cb, value);
                    }
                }
                for dest in pipes_for(kind, handle) {
                    forward_write(dest, &bytes, encoding.as_deref());
                }
            }
            CryptoStreamEvent::End { kind, handle } => {
                for event_name in ["end", "finish", "close"] {
                    for cb in listeners_for(kind, handle, event_name) {
                        emit_callback0(cb);
                    }
                }
                for dest in pipes_for(kind, handle) {
                    forward_end(dest);
                }
            }
        }
    }
    count
}

pub fn js_crypto_stream_has_active_handles() -> i32 {
    if CRYPTO_STREAM_PENDING_EVENTS.lock().unwrap().is_empty() {
        0
    } else {
        1
    }
}

unsafe fn stream_event_name(value: f64) -> Option<String> {
    string_from_jsvalue(value.to_bits())
}

unsafe fn register_stream_listener(stream: &Mutex<CryptoDigestStream>, args: &[f64]) {
    if args.len() < 2 {
        return;
    }
    ensure_crypto_stream_gc_scanner();
    let Some(event) = stream_event_name(args[0]) else {
        return;
    };
    stream
        .lock()
        .unwrap()
        .listeners
        .entry(event)
        .or_default()
        .push(unbox_to_i64(args[1]));
}

unsafe fn set_stream_encoding(stream: &Mutex<CryptoDigestStream>, args: &[f64]) {
    let encoding = args
        .first()
        .and_then(|value| string_from_jsvalue(value.to_bits()))
        .map(|s| s.to_ascii_lowercase());
    stream.lock().unwrap().encoding = encoding;
}

fn stream_pipe(stream: &Mutex<CryptoDigestStream>, args: &[f64]) -> f64 {
    if let Some(dest) = args.first() {
        stream.lock().unwrap().pipes.push(dest.to_bits());
        *dest
    } else {
        nanbox_undefined()
    }
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
    state: Mutex<Option<HashState>>,
    output_len: Option<usize>,
    stream: Mutex<CryptoDigestStream>,
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
        state: Mutex::new(Some(state)),
        output_len,
        stream: Mutex::new(CryptoDigestStream::default()),
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
    // #2944 — once `digest()` consumed the hasher state, Node throws
    // `Error [ERR_CRYPTO_HASH_FINALIZED]: Digest already called` for any
    // subsequent `update`, `digest`, or `copy`. The `state` Mutex holds
    // `None` after the first `digest()`, so a `None` here means finalized.
    if matches!(method, "update" | "digest" | "copy") && h.state.lock().unwrap().is_none() {
        perry_runtime::fs::validate::throw_error_with_code(
            "Digest already called",
            "ERR_CRYPTO_HASH_FINALIZED",
        );
    }
    match method {
        "update" => {
            let data = validate_update_data(args);
            let encoding = arg_string(args, 1);
            let bytes = decode_hash_update_value(data, &encoding);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                update_hash_state(state, &bytes);
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
            let Some(digest) = finalize_hash_state(state, h.output_len, option_len) else {
                return f64::from_bits(0x7FFC_0000_0000_0001);
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
                state: Mutex::new(Some(state)),
                output_len: h.output_len,
                stream: Mutex::new(CryptoDigestStream::default()),
            });
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "write" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let bytes = decode_hash_update_value(args[0], &encoding);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                update_hash_state(state, &bytes);
            }
            js_true()
        }
        "end" => {
            if let Some(chunk) = args.first().copied() {
                let v = JSValue::from_bits(chunk.to_bits());
                if !v.is_undefined() && !v.is_null() {
                    let encoding = arg_string(args, 1);
                    let bytes = decode_hash_update_value(chunk, &encoding);
                    let mut guard = h.state.lock().unwrap();
                    if let Some(state) = guard.as_mut() {
                        update_hash_state(state, &bytes);
                    }
                }
            }
            let encoding = {
                let mut stream = h.stream.lock().unwrap();
                if stream.ended {
                    return nanbox_handle(handle);
                }
                stream.ended = true;
                stream.encoding.clone()
            };
            let state = {
                let mut guard = h.state.lock().unwrap();
                guard.take()
            };
            if let Some(digest) = finalize_hash_state(state, h.output_len, None) {
                queue_crypto_stream_digest(CryptoStreamKind::Hash, handle, digest, encoding);
            }
            nanbox_handle(handle)
        }
        "on" | "once" | "addListener" if args.len() >= 2 => {
            register_stream_listener(&h.stream, args);
            nanbox_handle(handle)
        }
        "setEncoding" => {
            set_stream_encoding(&h.stream, args);
            nanbox_handle(handle)
        }
        "pipe" => stream_pipe(&h.stream, args),
        "destroy" | "close" => nanbox_undefined(),
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_hash_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "digest" => b"digest",
        "copy" => b"copy",
        "write" => b"write",
        "end" => b"end",
        "on" => b"on",
        "once" => b"once",
        "addListener" => b"addListener",
        "pipe" => b"pipe",
        "setEncoding" => b"setEncoding",
        "destroy" => b"destroy",
        "close" => b"close",
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
    state: Mutex<Option<HmacState>>,
    stream: Mutex<CryptoDigestStream>,
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
        state: Mutex::new(Some(state)),
        stream: Mutex::new(CryptoDigestStream::default()),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Dispatch `update` / `digest` on an HmacHandle. Called from
/// `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_hmac(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<HmacHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    // #2945 — after the MAC is finalized by `digest()`, Node keeps a second
    // `digest()` idempotent (returns `""` / empty Buffer) but throws
    // `Error [ERR_CRYPTO_HASH_FINALIZED]: Digest already called` on `update()`.
    // The `state` Mutex holds `None` once finalized, so only the `update`
    // path needs to throw here (the `digest` arm already returns the empty
    // shape for a taken state).
    if method == "update" && h.state.lock().unwrap().is_none() {
        perry_runtime::fs::validate::throw_error_with_code(
            "Digest already called",
            "ERR_CRYPTO_HASH_FINALIZED",
        );
    }
    match method {
        "update" => {
            let data = validate_update_data(args);
            let encoding = arg_string(args, 1);
            let bytes = decode_hash_update_value(data, &encoding);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                update_hmac_state(state, &bytes);
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
            // Node keeps Hmac.digest() idempotent in shape after the first
            // finalization: encoded digests become an empty string and buffer
            // digests become an empty Buffer instead of `undefined`.
            let digest = finalize_hmac_state(state);
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
        "write" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let bytes = decode_hash_update_value(args[0], &encoding);
            let mut guard = h.state.lock().unwrap();
            if let Some(state) = guard.as_mut() {
                update_hmac_state(state, &bytes);
            }
            js_true()
        }
        "end" => {
            if let Some(chunk) = args.first().copied() {
                let v = JSValue::from_bits(chunk.to_bits());
                if !v.is_undefined() && !v.is_null() {
                    let encoding = arg_string(args, 1);
                    let bytes = decode_hash_update_value(chunk, &encoding);
                    let mut guard = h.state.lock().unwrap();
                    if let Some(state) = guard.as_mut() {
                        update_hmac_state(state, &bytes);
                    }
                }
            }
            let encoding = {
                let mut stream = h.stream.lock().unwrap();
                if stream.ended {
                    return nanbox_handle(handle);
                }
                stream.ended = true;
                stream.encoding.clone()
            };
            let state = {
                let mut guard = h.state.lock().unwrap();
                guard.take()
            };
            let digest = finalize_hmac_state(state);
            queue_crypto_stream_digest(CryptoStreamKind::Hmac, handle, digest, encoding);
            nanbox_handle(handle)
        }
        "on" | "once" | "addListener" if args.len() >= 2 => {
            register_stream_listener(&h.stream, args);
            nanbox_handle(handle)
        }
        "setEncoding" => {
            set_stream_encoding(&h.stream, args);
            nanbox_handle(handle)
        }
        "pipe" => stream_pipe(&h.stream, args),
        "destroy" | "close" => nanbox_undefined(),
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_hmac_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "digest" => b"digest",
        "write" => b"write",
        "end" => b"end",
        "on" => b"on",
        "once" => b"once",
        "addListener" => b"addListener",
        "pipe" => b"pipe",
        "setEncoding" => b"setEncoding",
        "destroy" => b"destroy",
        "close" => b"close",
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
