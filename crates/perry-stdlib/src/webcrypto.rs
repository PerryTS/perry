//! Web Crypto API: `crypto.subtle.digest` / `importKey` / `sign` / `verify`
//! / `encrypt` / `decrypt`.
//!
//! The implementation is split into real Rust submodules so each algorithm
//! family has its own namespace and compilation unit while preserving the
//! public `webcrypto` module ABI expected by generated runtime bindings.
//!
//! `util` declares shared imports, helpers, and private types that are
//! re-exported only inside this module for sibling shards.
mod aes;
mod digest;
mod encapsulation;
mod hmac;
mod jwk;
mod kdf;
mod key_object;
mod keys;
mod supports;
mod util;
mod wrap;

// Private imports keep sibling modules able to share `pub(super)` helpers.
use self::{aes::*, hmac::*, jwk::*, key_object::*, util::*};

// Public re-exports preserve the parent module surface for FFI entry points.
pub use self::{
    aes::*, digest::*, encapsulation::*, hmac::*, jwk::*, kdf::*, keys::*, supports::*, wrap::*,
};

// GC buffer-sweep callback for dead CryptoKey buffers, installed by
// `js_stdlib_init_dispatch`. Re-exported here so `util` stays a private module.
pub(crate) use self::util::crypto_key_buffer_died;

/// Dispatcher for captured/dynamic `crypto.subtle.*` calls. Static
/// `crypto.subtle.method(...)` call sites still lower directly in codegen;
/// this keeps namespace property reads such as `const subtle = crypto.subtle`
/// coherent with Node.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(perry_runtime::JSValue::undefined().bits());
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
    let promise_to_value = |promise: *mut perry_runtime::Promise| -> f64 {
        f64::from_bits(perry_runtime::JSValue::pointer(promise as *const u8).bits())
    };
    match method {
        "digest" if args_len >= 2 => promise_to_value(js_webcrypto_digest(arg(0), arg(1))),
        "importKey" if args_len >= 5 => promise_to_value(js_webcrypto_import_key(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
        )),
        "exportKey" if args_len >= 2 => promise_to_value(js_webcrypto_export_key(arg(0), arg(1))),
        "sign" if args_len >= 3 => promise_to_value(js_webcrypto_sign(arg(0), arg(1), arg(2))),
        "verify" if args_len >= 4 => {
            promise_to_value(js_webcrypto_verify(arg(0), arg(1), arg(2), arg(3)))
        }
        "deriveBits" if args_len >= 3 => {
            promise_to_value(js_webcrypto_derive_bits(arg(0), arg(1), arg(2)))
        }
        "deriveKey" if args_len >= 5 => promise_to_value(js_webcrypto_derive_key(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
        )),
        "encrypt" if args_len >= 3 => {
            promise_to_value(js_webcrypto_encrypt(arg(0), arg(1), arg(2)))
        }
        "decrypt" if args_len >= 3 => {
            promise_to_value(js_webcrypto_decrypt(arg(0), arg(1), arg(2)))
        }
        "generateKey" if args_len >= 3 => {
            promise_to_value(js_webcrypto_generate_key(arg(0), arg(1), arg(2)))
        }
        "wrapKey" if args_len >= 4 => {
            promise_to_value(js_webcrypto_wrap_key(arg(0), arg(1), arg(2), arg(3)))
        }
        "unwrapKey" if args_len >= 7 => promise_to_value(js_webcrypto_unwrap_key(
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4),
            arg(5),
            arg(6),
        )),
        "keyObjectToCryptoKey" if args_len >= 4 => {
            js_webcrypto_key_object_to_crypto_key(arg(0), arg(1), arg(2), arg(3))
        }
        // Internal bridge for `crypto.KeyObject.from(<asymmetric CryptoKey>)`
        // (#6302) — the runtime owns the secret-key shape but the asymmetric
        // key encoders live here. Not a `crypto.subtle` method name.
        "keyObjectFromCryptoKey" if args_len >= 1 => {
            js_webcrypto_key_object_from_crypto_key(arg(0))
        }
        "supports" if args_len >= 2 => js_webcrypto_supports(arg(0), arg(1), arg(2)),
        "encapsulateBits" => promise_to_value(js_webcrypto_encapsulate_bits(args_ptr, args_len)),
        "decapsulateBits" => promise_to_value(js_webcrypto_decapsulate_bits(args_ptr, args_len)),
        "encapsulateKey" => promise_to_value(js_webcrypto_encapsulate_key(args_ptr, args_len)),
        "decapsulateKey" => promise_to_value(js_webcrypto_decapsulate_key(args_ptr, args_len)),
        _ => undefined,
    }
}
