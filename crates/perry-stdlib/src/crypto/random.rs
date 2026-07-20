use super::*;

/// Issue #2013 — Node throws on `crypto.randomBytes` for the same
/// bad-input shapes the fs/process surface already covers: a
/// non-number (`{}`, `'abc'`, `true`) raises `TypeError
/// [ERR_INVALID_ARG_TYPE]`, and any number outside `[0, 2^31 - 1]`
/// (negative, fractional, NaN/Infinity) raises `RangeError
/// [ERR_OUT_OF_RANGE]`. The codegen passes the full NaN-boxed bits
/// as f64, so a JS pointer-tagged `{}` arrives here as a NaN that
/// the legacy `size as usize` silently truncated to 0 — making the
/// call return an empty buffer instead of throwing.
fn validate_random_bytes_size(size: f64) -> usize {
    let jv = JSValue::from_bits(size.to_bits());
    if !jv.is_number() && !jv.is_int32() {
        let message = format!(
            "The \"size\" argument must be of type number. Received {}",
            perry_runtime::fs::validate::describe_received(size)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        size
    };
    // Node truncates a fractional `randomBytes(1.5)` to 1 (no throw)
    // but rejects NaN/Infinity/negative/out-of-range with
    // ERR_OUT_OF_RANGE — fractional integers are NOT in the reject set.
    if !n.is_finite() || n < 0.0 || n > i32::MAX as f64 {
        let received = if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n.is_sign_negative() {
                "-Infinity".to_string()
            } else {
                "Infinity".to_string()
            }
        } else {
            format!("{}", n)
        };
        let message = format!(
            "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
            received
        );
        perry_runtime::fs::validate::throw_range_error_with_code(&message);
    }
    n as usize
}

/// Generate random bytes and return as a Buffer
/// crypto.randomBytes(size) -> Buffer
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_buffer(
    size: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let size = validate_random_bytes_size(size);
    if size == 0 {
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
/// Node runs `randomBytes` on the libuv threadpool, so the `(err, buffer)`
/// callback fires on a LATER event-loop iteration (one macrotask hop), never
/// synchronously. Perry generates the bytes synchronously but must preserve
/// that timing: schedule the callback via `setImmediate` instead of calling it
/// inline. Resolving it synchronously let an `await`ing caller continue a
/// macrotask early — e.g. Auth.js generates its CSRF token with async
/// `randomBytes`, so a Next.js Server Component's `await auth()` completed one
/// event-loop iteration ahead of Node, reordering the React Flight (RSC) rows
/// of the streamed response. The bytes are still produced eagerly; only the
/// callback dispatch is deferred.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_bytes_async(size: f64, callback_bits: f64) -> f64 {
    let buf = js_crypto_random_bytes_buffer(size);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    // Only a genuine closure is a schedulable callback. The timer queue
    // reinterprets the i64 as `*const ClosureHeader` and calls it with no
    // validation, so a bare `is_pointer()` check is not enough: a non-function
    // POINTER value (a plain object or array passed as the callback —
    // `randomBytes(n, {})`, which Node rejects with ERR_INVALID_CALLBACK) would
    // pass it and be miscast as a closure, then dereferenced when the timer
    // fires. Gate on `is_closure_ptr` (the CLOSURE_MAGIC + heap-range probe used
    // by every other node-style-callback site) so a non-callable argument is a
    // safe no-op instead. `is_closure_ptr` self-rejects the whole handle band
    // and any non-heap address, so a primitive/undefined callback is covered
    // without a separate magnitude floor.
    let cb_ptr = perry_runtime::value::js_nanbox_get_pointer(callback_bits);
    if perry_runtime::closure::is_closure_ptr(cb_ptr as usize) {
        let err = f64::from_bits(JSValue::null().bits());
        let args = [err, value];
        perry_runtime::timer::js_set_immediate_callback_args(cb_ptr, args.as_ptr(), 2);
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Generate random bytes and return as hex string
/// crypto.randomBytes(size).toString('hex') -> string
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_hex(size: f64) -> *mut StringHeader {
    let size = validate_random_bytes_size(size);

    let mut bytes = vec![0u8; size];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex_str = hex::encode(&bytes);

    js_string_from_bytes(hex_str.as_ptr(), hex_str.len() as u32)
}

/// Generate a random UUID v4 using crypto-secure random
/// crypto.randomUUID([options]) -> string
#[no_mangle]
pub unsafe extern "C" fn js_crypto_random_uuid(options_bits: f64) -> *mut StringHeader {
    validate_random_uuid_options(options_bits);
    let uuid = uuid::Uuid::new_v4();
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// Generate an RFC 9562 version 7 UUID — a 48-bit millisecond Unix
/// timestamp in the most-significant bits followed by cryptographically
/// secure randomness, with the version (`7`) and variant (`10`) bits set
/// per spec. `crypto.randomUUIDv7([options])` -> string (#2550). The
/// optional `options` object (Node's `{ disableEntropyCache }`) is
/// accepted for shape parity but does not change the generated value.
#[no_mangle]
pub extern "C" fn js_crypto_random_uuidv7() -> *mut StringHeader {
    let uuid = uuid::Uuid::now_v7();
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

unsafe fn validate_random_uuid_options(options_bits: f64) {
    let value = JSValue::from_bits(options_bits.to_bits());
    if value.is_undefined() {
        return;
    }
    if !value.is_pointer() {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            perry_runtime::fs::validate::describe_received(options_bits)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let ptr = value.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < perry_runtime::gc::GC_HEADER_SIZE + 0x1000 {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            perry_runtime::fs::validate::describe_received(options_bits)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let header =
        &*(ptr.sub(perry_runtime::gc::GC_HEADER_SIZE) as *const perry_runtime::gc::GcHeader);
    if header.obj_type == perry_runtime::gc::GC_TYPE_ARRAY {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            perry_runtime::fs::validate::describe_received(options_bits)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    if header.obj_type != perry_runtime::gc::GC_TYPE_OBJECT {
        return;
    }
    let key = js_string_from_bytes(b"disableEntropyCache".as_ptr(), 19);
    let field = js_object_get_field_by_name(ptr as *const ObjectHeader, key);
    if field.is_undefined() {
        return;
    }
    if !field.is_bool() {
        let message = format!(
            "The \"options.disableEntropyCache\" property must be of type boolean. Received {}",
            perry_runtime::fs::validate::describe_received(f64::from_bits(field.bits()))
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
}

const RANDOM_INT_MAX_RANGE: i64 = (1i64 << 48) - 1;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Issue #2013 — validate one of `randomInt`'s integer arguments
/// (min/max). Non-number → ERR_INVALID_ARG_TYPE; non-finite /
/// fractional / unsafe integer → ERR_OUT_OF_RANGE. Mirrors
/// `validate_random_bytes_size` but with the wider int bound Node
/// accepts for randomInt's `min`/`max`.
fn validate_random_int_arg(value: f64, arg_name: &str) -> i64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_number() && !jv.is_int32() {
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            arg_name,
            perry_runtime::fs::validate::describe_received(value)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        value
    };
    if !n.is_finite() || n.fract() != 0.0 {
        let message = format!(
            "The \"{}\" argument must be a safe integer. Received {}",
            arg_name,
            perry_runtime::fs::validate::describe_received(value)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&n) {
        let message = format!(
            "The value of \"{}\" is out of range. It must be a safe integer type number. Received {}",
            arg_name, n
        );
        perry_runtime::fs::validate::throw_range_error_with_code(&message);
    }
    n as i64
}

/// `crypto.randomInt(max)` / `crypto.randomInt(min, max)` synchronous form.
/// Returns a uniformly distributed integer in `[min, max)`.
#[no_mangle]
pub extern "C" fn js_crypto_random_int(min_bits: f64, max_bits: f64) -> f64 {
    let min = validate_random_int_arg(min_bits, "min");
    let max = validate_random_int_arg(max_bits, "max");
    if max <= min {
        let message = format!(
            "The value of \"max\" is out of range. It must be greater than the value of \"min\" ({}). Received {}",
            min, max
        );
        perry_runtime::fs::validate::throw_range_error_with_code(&message);
    }
    let range = max as i128 - min as i128;
    if range > RANDOM_INT_MAX_RANGE as i128 {
        let message = format!(
            "The value of \"max - min\" is out of range. It must be <= {}. Received {}",
            RANDOM_INT_MAX_RANGE, range
        );
        perry_runtime::fs::validate::throw_range_error_with_code(&message);
    }
    rand::thread_rng().gen_range(min..max) as f64
}

/// #1577: dispatcher for captured-then-called `crypto.*` methods
/// (`const f = crypto.createHash; f("sha256")`). The runtime's native-module
/// dispatch (`dispatch_native_module_method`) routes `("crypto", method)`
/// here once it's registered in `js_stdlib_init_dispatch` — the runtime can't
/// call these stdlib impls directly (perry-stdlib depends on perry-runtime).
/// Args arrive as NaN-boxed f64s; string args are unboxed SSO-safely the same
/// way the direct-call lowering does. Unhandled methods return undefined.
///
/// # Safety
/// `method_ptr`/`args_ptr` must be valid for their stated lengths (the runtime
/// passes the live method name and call-arg buffer).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(JSValue::undefined().bits());
    let method = if method_ptr.is_null() || method_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("")
    };
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            undefined
        }
    };
    // SSO-safe StringHeader pointer (matches `unbox_to_i64` on the direct path).
    let str_ptr = |n: usize| -> i64 { perry_runtime::js_get_string_pointer_unified(arg(n)) as i64 };
    // A buffer-or-string arg's raw pointer (bytes_from_ptr handles both).
    let bytes_ptr = |n: usize| -> i64 {
        let v = arg(n);
        if JSValue::from_bits(v.to_bits()).is_any_string() {
            perry_runtime::js_get_string_pointer_unified(v) as i64
        } else {
            perry_runtime::js_nanbox_get_pointer(v)
        }
    };
    let pointer_value = |ptr: *mut u8| -> f64 {
        if ptr.is_null() {
            undefined
        } else {
            f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
        }
    };
    match method {
        "createHash" => js_crypto_create_hash(str_ptr(0)),
        "createHmac" => js_crypto_create_hmac(str_ptr(0), bytes_ptr(1)),
        "createDiffieHellman" | "DiffieHellman" => {
            js_crypto_create_diffie_hellman(arg(0), arg(1), arg(2))
        }
        "createDiffieHellmanGroup" | "getDiffieHellman" | "DiffieHellmanGroup" => {
            js_crypto_get_diffie_hellman(arg(0))
        }
        "diffieHellman" => pointer_value(js_crypto_diffie_hellman(arg(0)) as *mut u8),
        "encapsulate" if args_len >= 2 => js_crypto_encapsulate_async(arg(0), arg(1)),
        "encapsulate" => pointer_value(js_crypto_encapsulate(arg(0)) as *mut u8),
        "decapsulate" if args_len >= 3 => js_crypto_decapsulate_async(arg(0), arg(1), arg(2)),
        "decapsulate" => pointer_value(js_crypto_decapsulate(arg(0), arg(1)) as *mut u8),
        "randomUUID" => f64::from_bits(JSValue::string_ptr(js_crypto_random_uuid(arg(0))).bits()),
        "randomUUIDv7" => f64::from_bits(JSValue::string_ptr(js_crypto_random_uuidv7()).bits()),
        // `crypto.randomBytes(size[, callback])`. Reached as a VALUE (named
        // import / `util.promisify(crypto.randomBytes)`). The 2-arg async form
        // must invoke `(err, buf)`; without the callback branch the
        // promisified call returned the buffer synchronously and never
        // settled, so the awaiting Promise hung. The 1-arg sync form returns
        // the Buffer directly, matching Node.
        "randomBytes" if args_len >= 2 => js_crypto_random_bytes_async(arg(0), arg(1)),
        "randomBytes" => {
            let buf = js_crypto_random_bytes_buffer(arg(0));
            f64::from_bits(JSValue::pointer(buf as *const u8).bits())
        }
        "generateKeyPair" => js_crypto_generate_key_pair_async(str_ptr(0), arg(1), arg(2)),
        "generateKeyPairSync" => {
            let alg = bytes_from_ptr(str_ptr(0));
            let pair = match alg.as_slice() {
                b"ec" => js_crypto_generate_key_pair_sync_ec_p256(arg(1)),
                b"ed25519" => js_crypto_generate_key_pair_sync_ed25519(arg(1)),
                b"x25519" => js_crypto_generate_key_pair_sync_x25519(arg(1)),
                _ => js_crypto_generate_key_pair_sync_rsa(arg(1)),
            };
            pointer_value(pair as *mut u8)
        }
        "generateKey" => js_crypto_generate_key_async(str_ptr(0), arg(1), arg(2)),
        "generateKeySync" => {
            pointer_value(js_crypto_generate_key_sync(str_ptr(0), arg(1)) as *mut u8)
        }
        // #6675: the key-material constructors reached as a VALUE — a named
        // import (`import { createPrivateKey } from "crypto"`), a computed
        // property access, or a bundled `require('crypto').createPrivateKey`
        // (turbopack/webpack) — route here instead of through the codegen
        // fast-path. Without these arms they fell to `_ => undefined`, so
        // `createPrivateKey(secret)` returned `undefined` rather than THROWING
        // on non-key material. jsonwebtoken's sign()/verify() depend on the
        // throw: `try { createPrivateKey(secret) } catch { createSecretKey(...) }`.
        // The missing throw left `secretOrPrivateKey` undefined and crashed at
        // `secretOrPrivateKey.type` ("Cannot read properties of undefined").
        // The runtime helpers throw on invalid material, matching Node.
        "createSecretKey" => {
            let encoding = if args_len >= 2 && JSValue::from_bits(arg(1).to_bits()).is_any_string()
            {
                str_ptr(1)
            } else {
                0
            };
            pointer_value(js_crypto_create_secret_key(bytes_ptr(0), encoding) as *mut u8)
        }
        "createPrivateKey" => {
            f64::from_bits(JSValue::string_ptr(js_crypto_create_private_key_value(arg(0))).bits())
        }
        "createPublicKey" => {
            f64::from_bits(JSValue::string_ptr(js_crypto_create_public_key_value(arg(0))).bits())
        }
        "generatePrime" if args_len >= 3 => js_crypto_generate_prime_async(arg(0), arg(1), arg(2)),
        "generatePrime" | "generatePrimeSync" => js_crypto_generate_prime_sync(arg(0), arg(1)),
        "checkPrime" if args_len >= 3 => js_crypto_check_prime_async(arg(0), arg(1), arg(2)),
        "checkPrime" | "checkPrimeSync" => js_crypto_check_prime_sync(arg(0), arg(1)),
        "getFips" => 0.0,
        "setFips" => undefined,
        "secureHeapUsed" => pointer_value(js_crypto_secure_heap_used() as *mut u8),
        "hkdf" => js_crypto_hkdf_async_alg(
            str_ptr(0),
            bytes_ptr(1),
            bytes_ptr(2),
            bytes_ptr(3),
            arg(4),
            arg(5),
        ),
        "hkdfSync" => pointer_value(js_crypto_hkdf_bytes_alg(
            str_ptr(0),
            bytes_ptr(1),
            bytes_ptr(2),
            bytes_ptr(3),
            arg(4),
        ) as *mut u8),
        // `crypto.pbkdf2(password, salt, iterations, keylen, digest, callback)`
        // reached as a VALUE — a named import (`import { pbkdf2 } from
        // "crypto"`) or `util.promisify(crypto.pbkdf2)`. The direct
        // `crypto.pbkdf2(...)` call site is special-cased in codegen
        // (→ `js_crypto_pbkdf2_async_alg`), but the value-form routes here.
        // Without this arm the call fell to `_ => undefined`, so the
        // callback never fired and the awaiting (promisified) Promise hung
        // forever. Node's async pbkdf2 requires the `digest` arg, so the
        // callback is the 6th arg (index 5); we still tolerate a missing
        // digest by treating the last arg as the callback.
        "pbkdf2" => {
            // The callback is always the final argument; the digest is the
            // arg immediately before it when present (>= 6 args).
            let (digest, callback) = if args_len >= 6 {
                (str_ptr(4), arg(5))
            } else {
                // No digest supplied (rare) — pass a null digest pointer
                // (the runtime defaults to SHA-256) and take the last arg
                // as the callback.
                (0i64, arg(args_len.saturating_sub(1)))
            };
            js_crypto_pbkdf2_async_alg(bytes_ptr(0), bytes_ptr(1), arg(2), arg(3), digest, callback)
        }
        "scrypt" => {
            let callback = if args_len >= 5 { arg(4) } else { arg(3) };
            js_crypto_scrypt_async(bytes_ptr(0), bytes_ptr(1), arg(2), callback)
        }
        "scryptSync" => {
            let options_ptr = if args_len >= 4 { bytes_ptr(3) } else { 0 };
            pointer_value(
                js_crypto_scrypt_bytes(bytes_ptr(0), bytes_ptr(1), arg(2), options_ptr) as *mut u8,
            )
        }
        // Node: randomInt(max) → [0,max); randomInt(min,max) → [min,max).
        "randomInt" if args_len >= 2 => js_crypto_random_int(arg(0), arg(1)),
        "randomInt" => js_crypto_random_int(0.0, arg(0)),
        "timingSafeEqual" => js_crypto_timing_safe_equal(arg(0), arg(1)),
        "Certificate.verifySpkac" => js_crypto_certificate_verify_spkac(arg(0)),
        "Certificate.exportPublicKey" => js_crypto_certificate_export_public_key(arg(0)),
        "Certificate.exportChallenge" => js_crypto_certificate_export_challenge(arg(0)),
        _ => undefined,
    }
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
    unsafe {
        let raw = raw_addr_from_value(buf_bits);
        // TypedArrayHeader path (Uint8Array, Uint32Array, Float32Array, …).
        if perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some() {
            let ta = raw as *mut perry_runtime::typedarray::TypedArrayHeader;
            if let Some(data) = perry_runtime::typedarray::typed_array_bytes_mut(ta) {
                let elem_size = (*ta).elem_size as usize;
                let len = if elem_size == 0 {
                    0
                } else {
                    data.len() / elem_size
                };
                let (start_elem, count_elem) =
                    validate_random_fill_range(len, offset_bits, size_bits);
                let start = start_elem.saturating_mul(elem_size);
                let end = start
                    .saturating_add(count_elem.saturating_mul(elem_size))
                    .min(data.len());
                if end > start {
                    rand::thread_rng().fill_bytes(&mut data[start..end]);
                }
                return buf_bits;
            }
            throw_invalid_random_fill_buffer(buf_bits);
        }
        // BufferHeader / Uint8Array path.
        if perry_runtime::buffer::is_registered_buffer(raw) {
            let buf = raw as *mut perry_runtime::buffer::BufferHeader;
            let total = (*buf).length as usize;
            let (start, count) = validate_random_fill_range(total, offset_bits, size_bits);
            if count > 0 {
                let data = perry_runtime::buffer::buffer_data_mut(buf);
                let slice = std::slice::from_raw_parts_mut(data.add(start), count);
                rand::thread_rng().fill_bytes(slice);
            }
            // Hand back the same NaN-boxed value the caller passed.
            return buf_bits;
        }
    }

    throw_invalid_random_fill_buffer(buf_bits);
}

fn raw_addr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

fn throw_invalid_random_fill_buffer(value: f64) -> ! {
    let message = format!(
        "The \"buf\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received {}",
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn random_fill_number_arg(value: f64, name: &str) -> Option<f64> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return None;
    }
    if !js.is_number() && !js.is_int32() {
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            name,
            perry_runtime::fs::validate::describe_received(value)
        );
        perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    Some(if js.is_int32() {
        js.as_int32() as f64
    } else {
        value
    })
}

fn validate_random_fill_range(total: usize, offset_bits: f64, size_bits: f64) -> (usize, usize) {
    let offset = match random_fill_number_arg(offset_bits, "offset") {
        Some(n) if n.is_finite() && n >= 0.0 && n <= total as f64 => n as usize,
        Some(n) => {
            let message = format!(
                "The value of \"offset\" is out of range. It must be >= 0 && <= {}. Received {}",
                total, n
            );
            perry_runtime::fs::validate::throw_range_error_with_code(&message);
        }
        None => 0,
    };
    let size = match random_fill_number_arg(size_bits, "size") {
        Some(n) if n.is_finite() && n >= 0.0 && n <= i32::MAX as f64 => n as usize,
        Some(n) => {
            let message = format!(
                "The value of \"size\" is out of range. It must be >= 0 && <= 2147483647. Received {}",
                n
            );
            perry_runtime::fs::validate::throw_range_error_with_code(&message);
        }
        None => total.saturating_sub(offset),
    };
    let end = offset.saturating_add(size);
    if end > total {
        let message = format!(
            "The value of \"size + offset\" is out of range. It must be <= {}. Received {}",
            total, end
        );
        perry_runtime::fs::validate::throw_range_error_with_code(&message);
    }
    (offset, size)
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
pub(super) fn nanboxed_to_usize(bits: f64) -> Option<usize> {
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

pub(super) unsafe fn crypto_value_bytes(bits: f64) -> Vec<u8> {
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

pub(super) fn bytes_to_u128(bytes: &[u8]) -> Option<u128> {
    if bytes.len() > 16 {
        return None;
    }
    let mut n = 0u128;
    for &b in bytes {
        n = (n << 8) | b as u128;
    }
    Some(n)
}

pub(super) unsafe fn object_field_bool(obj_bits: u64, name: &[u8]) -> Option<bool> {
    if (obj_bits >> 48) as u16 != 0x7FFD {
        return None;
    }
    let obj_ptr = (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    match js_object_get_field_by_name(obj_ptr, key_ptr).bits() {
        0x7FFC_0000_0000_0004 => Some(true),
        0x7FFC_0000_0000_0003 => Some(false),
        _ => None,
    }
}

pub(super) unsafe fn object_field_u128(obj_bits: u64, name: &[u8]) -> Option<u128> {
    let bits = object_field_bits(obj_bits, name)?;
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFE {
        let i = (bits & 0xFFFF_FFFF) as u32 as i32;
        return (i >= 0).then_some(i as u128);
    }
    bytes_to_u128(&crypto_value_bytes(f64::from_bits(bits)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_int;

    fn boxed_ptr(ptr: *const u8) -> f64 {
        f64::from_bits(perry_runtime::JSValue::pointer(ptr).bits())
    }

    fn undefined() -> f64 {
        f64::from_bits(perry_runtime::JSValue::undefined().bits())
    }

    fn catch_runtime_throw(f: impl FnOnce()) -> bool {
        let env = perry_runtime::exception::js_try_push();
        let jumped = unsafe { perry_runtime::ffi::setjmp::setjmp(env as *mut c_int) };
        if jumped == 0 {
            f();
            perry_runtime::exception::js_try_end();
            false
        } else {
            perry_runtime::exception::js_try_end();
            perry_runtime::exception::js_clear_exception();
            true
        }
    }

    #[test]
    fn crypto_native_dispatch_get_fips_matches_default_node_mode() {
        let method = b"getFips";
        let result = unsafe {
            js_crypto_native_dispatch(method.as_ptr(), method.len(), std::ptr::null(), 0)
        };
        assert_eq!(result.to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn random_fill_sync_native_uint8_view_preserves_metadata() {
        let owner = perry_runtime::native_arena::js_native_arena_alloc(96);
        let view = perry_runtime::native_arena::js_native_arena_view(
            owner as u64,
            perry_runtime::typedarray::KIND_UINT8 as i32,
            8,
            64,
        );
        let target = boxed_ptr(view as *const u8);
        let before = unsafe {
            (
                (*view).owner,
                (*view).data,
                (*view).byte_offset,
                (*view).byte_length,
                (*view).generation,
            )
        };

        let returned = js_crypto_random_fill_sync(target, undefined(), undefined());
        assert_eq!(returned.to_bits(), target.to_bits());
        unsafe {
            assert_eq!((*view).owner, before.0);
            assert_eq!((*view).data, before.1);
            assert_eq!((*view).byte_offset, before.2);
            assert_eq!((*view).byte_length, before.3);
            assert_eq!((*view).generation, before.4);
            let bytes = std::slice::from_raw_parts((*view).data, (*view).byte_length as usize);
            assert!(
                bytes.iter().any(|&byte| byte != 0),
                "randomFillSync should mutate native view backing bytes"
            );
        }
        perry_runtime::native_arena::js_native_arena_dispose(owner as u64);
    }

    #[test]
    fn random_fill_sync_disposed_native_uint8_view_throws() {
        let owner = perry_runtime::native_arena::js_native_arena_alloc(16);
        let view = perry_runtime::native_arena::js_native_arena_view(
            owner as u64,
            perry_runtime::typedarray::KIND_UINT8 as i32,
            0,
            16,
        );
        let target = boxed_ptr(view as *const u8);
        perry_runtime::native_arena::js_native_arena_dispose(owner as u64);
        assert!(catch_runtime_throw(|| {
            let _ = js_crypto_random_fill_sync(target, undefined(), undefined());
        }));
    }

    /// #2550 — `crypto.randomUUIDv7()` must emit an RFC 9562 v7 UUID:
    /// canonical 36-char dashed form, version nibble `7`, variant nibble
    /// in `{8,9,a,b}`, a non-zero timestamp prefix, and fresh randomness
    /// on every call.
    fn read_uuid_string(ptr: *mut StringHeader) -> String {
        unsafe {
            let blen = (*ptr).byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, blen);
            std::str::from_utf8(bytes).unwrap().to_string()
        }
    }

    #[test]
    fn random_uuidv7_has_version_and_variant_bits() {
        let s = read_uuid_string(js_crypto_random_uuidv7());
        assert_eq!(s.len(), 36, "v7 UUID is 36 chars, got {:?}", s);
        let b = s.as_bytes();
        assert_eq!(b[8], b'-');
        assert_eq!(b[13], b'-');
        assert_eq!(b[18], b'-');
        assert_eq!(b[23], b'-');
        // version nibble (index 14) must be '7'
        assert_eq!(b[14] as char, '7', "version nibble in {:?}", s);
        // variant nibble (index 19) must encode the 10xx variant
        assert!(
            matches!(b[19], b'8' | b'9' | b'a' | b'b'),
            "variant nibble {:?} not in {{8,9,a,b}} for {:?}",
            b[19] as char,
            s
        );
        // The 48-bit millisecond timestamp prefix is non-zero for a
        // current-epoch value (it would only be all-zero near 1970).
        assert_ne!(&s[0..8], "00000000", "timestamp prefix should be set");
    }

    #[test]
    fn random_uuidv7_is_fresh_each_call() {
        let a = read_uuid_string(js_crypto_random_uuidv7());
        let b = read_uuid_string(js_crypto_random_uuidv7());
        assert_ne!(a, b, "consecutive v7 UUIDs must differ");
    }

    // Regression: the VALUE-form of async crypto (named-import bare call /
    // `util.promisify(crypto.pbkdf2)`) routes through
    // `js_crypto_native_dispatch`, NOT the direct `crypto.pbkdf2(...)`
    // codegen fast-path. Before the fix, `js_crypto_native_dispatch` had no
    // `"pbkdf2"` / 2-arg `"randomBytes"` arm, so the call returned `undefined`
    // and the Node-style `(err, value)` callback never fired — the awaiting
    // (promisified) Promise hung forever. These tests assert the callback IS
    // invoked, with a null error and a non-null Buffer result.
    use std::cell::Cell;
    thread_local! {
        static CB_FIRED: Cell<bool> = const { Cell::new(false) };
        static CB_ERR_NULLISH: Cell<bool> = const { Cell::new(false) };
        static CB_VALUE_PTR: Cell<bool> = const { Cell::new(false) };
    }

    extern "C" fn record_cb_thunk(
        _closure: *const perry_runtime::ClosureHeader,
        err: f64,
        value: f64,
    ) -> f64 {
        CB_FIRED.with(|f| f.set(true));
        let err_bits = err.to_bits();
        CB_ERR_NULLISH.with(|f| {
            f.set(
                err_bits == perry_runtime::JSValue::null().bits()
                    || err_bits == perry_runtime::JSValue::undefined().bits(),
            )
        });
        CB_VALUE_PTR
            .with(|f| f.set(perry_runtime::JSValue::from_bits(value.to_bits()).is_pointer()));
        undefined()
    }

    fn make_record_callback() -> f64 {
        perry_runtime::closure::js_register_closure_arity(record_cb_thunk as *const u8, 2);
        let closure = perry_runtime::closure::js_closure_alloc(record_cb_thunk as *const u8, 0);
        boxed_ptr(closure as *const u8)
    }

    fn reset_record() {
        CB_FIRED.with(|f| f.set(false));
        CB_ERR_NULLISH.with(|f| f.set(false));
        CB_VALUE_PTR.with(|f| f.set(false));
    }

    fn js_str(s: &str) -> f64 {
        let ptr = perry_runtime::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(perry_runtime::JSValue::string_ptr(ptr).bits())
    }

    #[test]
    fn native_dispatch_pbkdf2_value_form_fires_callback() {
        reset_record();
        let cb = make_record_callback();
        // pbkdf2(password, salt, iterations, keylen, digest, callback)
        let args = [
            js_str("password"),
            js_str("salt"),
            1000.0,
            32.0,
            js_str("sha256"),
            cb,
        ];
        let method = b"pbkdf2";
        unsafe {
            js_crypto_native_dispatch(method.as_ptr(), method.len(), args.as_ptr(), args.len());
        }
        assert!(CB_FIRED.with(|f| f.get()), "pbkdf2 callback must fire");
        assert!(
            CB_ERR_NULLISH.with(|f| f.get()),
            "pbkdf2 callback err must be null/undefined"
        );
        assert!(
            CB_VALUE_PTR.with(|f| f.get()),
            "pbkdf2 callback must deliver a Buffer pointer"
        );
    }

    #[test]
    fn native_dispatch_random_bytes_value_form_fires_callback() {
        reset_record();
        let cb = make_record_callback();
        // randomBytes(size, callback)
        let args = [16.0, cb];
        let method = b"randomBytes";
        unsafe {
            js_crypto_native_dispatch(method.as_ptr(), method.len(), args.as_ptr(), args.len());
        }
        assert!(CB_FIRED.with(|f| f.get()), "randomBytes callback must fire");
        assert!(
            CB_ERR_NULLISH.with(|f| f.get()),
            "randomBytes callback err must be null/undefined"
        );
        assert!(
            CB_VALUE_PTR.with(|f| f.get()),
            "randomBytes callback must deliver a Buffer pointer"
        );
    }
}
