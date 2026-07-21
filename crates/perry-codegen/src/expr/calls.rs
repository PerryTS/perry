//! NativeMethodCall + Call (all dispatch arms).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! Further split (chore: 2000-line cap) into topical sibling modules under
//! `calls/`. The `lower()` dispatcher keeps every arm guard verbatim and
//! delegates each large arm body to a sibling helper.

use anyhow::Result;
use perry_hir::Expr;

use crate::lower_call::{lower_call, lower_native_method_call};
use crate::nanbox::double_literal;
use crate::types::DOUBLE;

use super::{
    emit_string_literal_global, lower_expr, nanbox_pointer_inline, nanbox_string_inline,
    unbox_str_handle, unbox_to_i64, FnCtx,
};

mod crypto_hash;
mod crypto_kdf;
mod crypto_keys;
mod crypto_misc;
mod fs;
mod helpers;

pub(crate) use crypto_hash::{arm_crypto_create_hash, arm_crypto_hash_chain};
pub(crate) use crypto_kdf::{
    arm_crypto_argon2, arm_crypto_argon2_sync, arm_crypto_hkdf_async_alg, arm_crypto_hkdf_sync,
    arm_crypto_hkdf_sync_alg, arm_crypto_pbkdf2_async, arm_crypto_pbkdf2_sync, arm_crypto_scrypt,
    arm_crypto_scrypt_sync,
};
pub(crate) use crypto_keys::{
    arm_crypto_create_ecdh, arm_crypto_create_key, arm_crypto_create_sign_verify_legacy,
    arm_crypto_decapsulate, arm_crypto_diffie_hellman_ctor, arm_crypto_diffie_hellman_stateless,
    arm_crypto_encapsulate, arm_crypto_generate_key_pair_async,
    arm_crypto_generate_key_pair_sync_alg,
};
pub(crate) use crypto_misc::{
    arm_crypto_create_cipheriv, arm_crypto_create_hmac, arm_crypto_create_secret_key,
    arm_crypto_create_sign_verify, arm_crypto_generate_key_async,
    arm_crypto_generate_key_pair_sync, arm_crypto_generate_key_sync, arm_crypto_get_cipher_info,
    arm_crypto_get_fips, arm_crypto_get_inventory, arm_crypto_prime,
    arm_crypto_public_private_crypt, arm_crypto_random_bytes, arm_crypto_random_bytes_async,
    arm_crypto_random_fill, arm_crypto_random_int, arm_crypto_random_uuid,
    arm_crypto_random_uuidv7, arm_crypto_secure_heap_used, arm_crypto_set_fips, arm_crypto_sign,
    arm_crypto_timing_safe_equal, arm_crypto_verify,
};
pub(crate) use fs::{arm_fs, arm_fs_promises};
pub(crate) use helpers::{
    emit_call_location_at, emit_validate_crypto_key_arg, emit_validate_integer_arg,
    emit_validate_string_arg, hash_input_is_buffer,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::NativeMethodCall {
            module,
            class_name,
            method,
            object,
            args,
            ..
        } => lower_native_method_call(
            ctx,
            module,
            class_name.as_deref(),
            method,
            object.as_deref(),
            args,
        ),

        // #1645: `ReadableStream.from(iterable)` (Node 20+). The HIR lowers
        // `(ReadableStream as any).from(x)` to a Call whose callee is
        // `PropertyGet { ExternFuncRef("ReadableStream"), "from" }`; route it to
        // the runtime factory. The result is a numeric stream handle, so
        // downstream `rs.getReader()` dispatches through the #1545 runtime
        // stream-handle probe.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if property == "from"
                        && matches!(
                            object.as_ref(),
                            Expr::ExternFuncRef { name, .. } if name == "ReadableStream"
                        )
            ) =>
        {
            let arg_box = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_readable_stream_from_iterable",
                &[(DOUBLE, &arg_box)],
            ))
        }

        // Phase H crypto: collapse `crypto.createHash(alg).update(data).digest(enc)`
        // into a single runtime call. The HIR shape is a triple-nested
        // Call whose innermost callee is `NativeModuleRef("crypto")`.
        // Only "sha256" and "md5" algorithms have direct runtime
        // helpers (`js_crypto_sha256` / `js_crypto_md5`); other
        // algorithms fall through to the generic dispatch path.
        Expr::Call {
            callee: outer_callee,
            args: outer_args,
            ..
        } if matches!(
            outer_callee.as_ref(),
            Expr::PropertyGet { property: p, object, .. } if p == "digest" && matches!(
                object.as_ref(),
                Expr::Call { callee: c2, .. } if matches!(
                    c2.as_ref(),
                    Expr::PropertyGet { property: p2, object: obj2, .. } if p2 == "update" && matches!(
                        obj2.as_ref(),
                        Expr::Call { callee: c3, .. } if matches!(
                            c3.as_ref(),
                            Expr::PropertyGet { property: p3, object: obj3, .. } if (p3 == "createHash" || p3 == "Hash" || p3 == "createHmac" || p3 == "Hmac") && matches!(
                                obj3.as_ref(),
                                Expr::NativeModuleRef(n) if n == "crypto"
                            )
                        )
                    )
                )
            )
        ) =>
        {
            arm_crypto_hash_chain(ctx, outer_callee.as_ref(), outer_args)
        }

        // Standalone `crypto.createHash(alg)` / legacy callable
        // `crypto.Hash(alg)` — when the user binds the
        // result to a local before calling `.update(...)` / `.digest()`,
        // the three-level chain-collapse above no longer matches and this
        // arm runs instead. It registers a HashHandle in perry-stdlib and
        // returns a small-integer handle NaN-boxed as POINTER_TAG.
        // `js_native_call_method` routes subsequent method calls on that
        // handle through `HANDLE_METHOD_DISPATCH` → `dispatch_hash`. See
        // `perry-stdlib/src/crypto.rs::js_crypto_create_hash`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if (property == "createHash" || property == "Hash") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_create_hash(ctx, callee.as_ref(), args)
        }

        // `crypto.createSign(alg)` / legacy `crypto.Sign(alg)` and
        // `crypto.createVerify(alg)` / legacy `crypto.Verify(alg)` streaming
        // RSA signature handles.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if (property == "createSign" || property == "Sign" || property == "createVerify" || property == "Verify") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_create_sign_verify_legacy(ctx, callee.as_ref(), args)
        }

        // `crypto.createECDH(curve)` — Node-compatible ECDH handle. The
        // runtime currently covers the high-value P-256 aliases used by
        // Node/Bun/Deno parity tests: prime256v1, secp256r1, P-256.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "createECDH" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_create_ecdh(ctx, callee.as_ref(), args)
        }

        // `crypto.createDiffieHellman(...)` / legacy constructor alias
        // `crypto.DiffieHellman(...)` / `crypto.getDiffieHellman(name)` /
        // `crypto.createDiffieHellmanGroup(name)` / constructor alias
        // `crypto.DiffieHellmanGroup(name)` classic DH handles.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if (property == "createDiffieHellman" || property == "DiffieHellman" || property == "getDiffieHellman" || property == "createDiffieHellmanGroup" || property == "DiffieHellmanGroup") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_diffie_hellman_ctor(ctx, callee.as_ref(), args)
        }

        // Minimal KeyObject-compatible input path:
        // `createPrivateKey(pem)` returns the PEM surrogate directly, while
        // `createPublicKey(privateOrPublicPem)` derives a public PEM string.
        // The asymmetric native helpers accept these PEM surrogates as keys.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if (property == "createPrivateKey" || property == "createPublicKey") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_create_key(ctx, callee.as_ref(), args)
        }

        // `crypto.generateKeyPair("rsa"|"ec"|"ed25519"|"x25519", options,
        // callback)` — callback form. Native shim invokes `(err, publicKey,
        // privateKey)`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "generateKeyPair" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) && args.len() >= 3 =>
        {
            arm_crypto_generate_key_pair_async(ctx, callee.as_ref(), args)
        }

        // `crypto.generateKeyPairSync("rsa", { ...pem encodings... })` —
        // returns a plain object with `publicKey`/`privateKey` PEM strings.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "generateKeyPairSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_generate_key_pair_sync_alg(ctx, callee.as_ref(), args)
        }

        // `crypto.diffieHellman({ privateKey, publicKey })` — currently
        // covers the high-value X25519 stateless DH path from Node/Bun.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "diffieHellman" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_diffie_hellman_stateless(ctx, callee.as_ref(), args)
        }

        // `crypto.encapsulate(publicKey[, callback])` — currently covers the
        // high-value X25519 KEM path using Perry's KeyObject surrogate.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "encapsulate" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_encapsulate(ctx, callee.as_ref(), args)
        }

        // `crypto.decapsulate(privateKey, ciphertext[, callback])` — X25519
        // ciphertexts return the recovered shared key Buffer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "decapsulate" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_decapsulate(ctx, callee.as_ref(), args)
        }

        // Standalone `crypto.createHmac(alg, key)` / legacy
        // callable `crypto.Hmac(alg, key)` — same shape as
        // `createHash` above. Closes #1076 for the `const h = createHmac(...)`
        // / for-of patterns where the chain-collapse can't match because
        // `.update()` / `.digest()` happen on subsequent statements (or
        // because the alg isn't a literal the fast path recognizes).
        // `js_crypto_create_hmac` returns a NaN-boxed handle; dispatch_hmac
        // (registered in `perry-stdlib/src/common/dispatch.rs`) handles the
        // method routing.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if (property == "createHmac" || property == "Hmac") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_create_hmac(ctx, callee.as_ref(), args)
        }

        // `crypto.createCipheriv(alg, key, iv)` / `crypto.createDecipheriv(...)`
        // (issue #1075) — registers a CipherHandle in perry-stdlib and
        // returns a small-integer handle NaN-boxed as POINTER_TAG. The
        // runtime's HANDLE_METHOD_DISPATCH then routes subsequent
        // `.update(buf)` / `.final()` / `.getAuthTag()` / `.setAuthTag(tag)`
        // through `dispatch_cipher`. Supports aes-128-cbc, aes-256-cbc,
        // aes-128-gcm, aes-256-gcm. See
        // `perry-stdlib/src/crypto.rs::js_crypto_create_cipheriv`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if (property == "createCipheriv" || property == "createDecipheriv")
                        && matches!(
                            object.as_ref(),
                            Expr::NativeModuleRef(n) if n == "crypto"
                        )
            ) =>
        {
            arm_crypto_create_cipheriv(ctx, callee.as_ref(), args)
        }

        // `crypto.randomBytes(size, callback)` — callback form. Perry
        // invokes the callback synchronously in the native shim, but keeps
        // Node's `(err, buffer)` shape.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "randomBytes" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) && args.len() >= 2 =>
        {
            arm_crypto_random_bytes_async(ctx, callee.as_ref(), args)
        }

        // `crypto.randomFill(buffer[, offset][, size], callback)`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "randomFill" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) && args.len() >= 2 =>
        {
            arm_crypto_random_fill(ctx, callee.as_ref(), args)
        }

        // `crypto.createSign(alg)` / `crypto.createVerify(alg)` (#1364) —
        // registers a SignHandle and returns a small-integer handle NaN-boxed
        // as POINTER_TAG. HANDLE_METHOD_DISPATCH then routes `.update(d)` /
        // `.sign(key, enc?)` / `.verify(key, sig, enc?)` through
        // `dispatch_sign`. RSA PKCS#1 v1.5 over sha1/224/256/384/512.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if (property == "createSign" || property == "createVerify")
                        && matches!(
                            object.as_ref(),
                            Expr::NativeModuleRef(n) if n == "crypto"
                        )
            ) =>
        {
            arm_crypto_create_sign_verify(ctx, callee.as_ref(), args)
        }

        // Phase H crypto: `crypto.randomBytes(n)` as a Buffer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "randomBytes" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_random_bytes(ctx, callee.as_ref(), args)
        }

        // Phase H crypto: `crypto.randomUUID()`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "randomUUID" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_random_uuid(ctx, callee.as_ref(), args)
        }

        // `crypto.randomUUIDv7([options])` — RFC 9562 v7 (#2550).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "randomUUIDv7" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_random_uuidv7(ctx, callee.as_ref(), args)
        }

        // Phase H crypto: `crypto.randomInt([min,] max[, callback])` —
        // uniform integer in `[min, max)`. The single-arg form defaults
        // `min` to 0. The runtime returns the value as a plain double (a
        // JS number), so no NaN-box is needed at the call site. The
        // 3-arg callback form preserves Node's `(err, n)` shape and
        // returns `undefined`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "randomInt" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_random_int(ctx, callee.as_ref(), args)
        }

        // Phase H crypto: `crypto.timingSafeEqual(a, b)` — constant-time
        // compare of two byte sequences. Returns a NaN-boxed boolean.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "timingSafeEqual" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_timing_safe_equal(ctx, callee.as_ref(), args)
        }

        // Prime generation/checking APIs used by Node's crypto prime suite.
        // Perry covers practical Buffer-returning shapes plus callback forms:
        //   generatePrimeSync(size, options?)
        //   generatePrime(size, options, callback)
        //   checkPrimeSync(candidate, options?)
        //   checkPrime(candidate, options, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if matches!(property.as_str(), "generatePrimeSync" | "generatePrime" | "checkPrimeSync" | "checkPrime") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_prime(ctx, callee.as_ref(), args)
        }

        // `crypto.getHashes()` / `getCiphers()` / `getCurves()` — stable
        // deterministic inventories used for feature detection. The runtime
        // helper returns an ArrayHeader pointer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if matches!(property.as_str(), "getHashes" | "getCiphers" | "getCurves") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_get_inventory(ctx, callee.as_ref(), args)
        }

        // `crypto.getCipherInfo(algorithm, options?)` — feature detection
        // for supported symmetric ciphers.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "getCipherInfo" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_get_cipher_info(ctx, callee.as_ref(), args)
        }

        // `crypto.getFips()` — Perry does not expose OpenSSL FIPS mode.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "getFips" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_get_fips(ctx, callee.as_ref(), args)
        }

        // `crypto.setFips(false|0)` — Perry has no OpenSSL FIPS mode, so
        // accepting the disabling no-op matches Node's default environment.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "setFips" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_set_fips(ctx, callee.as_ref(), args)
        }

        // `crypto.secureHeapUsed()` — default Node shape when secure heap
        // is not enabled: { total: 0, used: 0, utilization: 0, min: 0 }.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "secureHeapUsed" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_secure_heap_used(ctx, callee.as_ref(), args)
        }

        // One-shot asymmetric signing/verification. Initial native parity
        // coverage supports Node's common RSA-SHA256/RSASSA-PKCS1-v1_5 PEM
        // path and returns a Buffer / boolean respectively.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "sign" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_sign(ctx, callee.as_ref(), args)
        }

        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "verify" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_verify(ctx, callee.as_ref(), args)
        }

        // RSA encryption/decryption one-shot APIs. Covers the common
        // Node/Bun `publicEncrypt(key, data)` → `privateDecrypt(key, data)`
        // default OAEP roundtrip and `privateEncrypt` → `publicDecrypt`
        // PKCS#1 v1.5 transform for PEM keys.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if (property == "publicEncrypt" || property == "privateDecrypt" || property == "privateEncrypt" || property == "publicDecrypt") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_public_private_crypt(ctx, callee.as_ref(), args)
        }

        // `crypto.createSecretKey(key, encoding?)` — JWT signing key for
        // HS* algorithms. Native-side this returns a Uint8Array-marked
        // BufferHeader; the bridge then materializes a real v8::Uint8Array
        // when the value crosses into a V8-fallback module (jose). See
        // `js_crypto_create_secret_key` for the encoding handling.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "createSecretKey" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_create_secret_key(ctx, callee.as_ref(), args)
        }

        // `crypto.generateKeySync("aes"|"hmac", { length })` — returns a
        // secret KeyObject-shaped BufferHeader, matching createSecretKey's
        // property/export/equality surface.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "generateKeySync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_generate_key_sync(ctx, callee.as_ref(), args)
        }

        // `crypto.generateKey("aes"|"hmac", { length }, cb)` — async Node
        // shape. Perry computes synchronously and invokes the callback with
        // `(null, key)`, matching the observable parity tests.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "generateKey" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_generate_key_async(ctx, callee.as_ref(), args)
        }

        // crypto.argon2Sync(algorithm, parameters) -> Buffer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "argon2Sync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_argon2_sync(ctx, callee.as_ref(), args)
        }

        // crypto.argon2(algorithm, parameters, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "argon2" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_argon2(ctx, callee.as_ref(), args)
        }

        // crypto.hkdfSync(algorithm, ikm, salt, info, keylen) -> Buffer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "hkdfSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_hkdf_sync_alg(ctx, callee.as_ref(), args)
        }

        // crypto.hkdf(algorithm, ikm, salt, info, keylen, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "hkdf" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_hkdf_async_alg(ctx, callee.as_ref(), args)
        }

        // crypto.scrypt(password, salt, keylen[, options], callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "scrypt" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_scrypt(ctx, callee.as_ref(), args)
        }

        // crypto.pbkdf2Sync(password, salt, iterations, keylen, digest) -> Buffer.
        // The digest algorithm (sha256/sha512/sha224/sha384/sha1) is passed
        // through to the runtime so non-SHA256 keys derive correctly (#1355).
        // An absent digest arg passes a null pointer; the runtime defaults to
        // SHA-256 (what SCRAM relies on).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "pbkdf2Sync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_pbkdf2_sync(ctx, callee.as_ref(), args)
        }

        // crypto.pbkdf2(password, salt, iterations, keylen, algorithm, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "pbkdf2" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_pbkdf2_async(ctx, callee.as_ref(), args)
        }

        // crypto.scryptSync(password, salt, keylen, options?) -> Buffer.
        // The runtime returns a Buffer (HIR types scryptSync as Uint8Array)
        // and reads optional `{ N, r, p }` cost params from the options
        // object pointer; an absent options arg passes a null pointer and the
        // runtime uses Node's defaults (N=16384, r=8, p=1).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "scryptSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_scrypt_sync(ctx, callee.as_ref(), args)
        }

        // crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
        // The runtime returns an array-buffer-marked Buffer; callers wrap it
        // with `Buffer.from(...)` / `new Uint8Array(...)`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "hkdfSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_hkdf_sync(ctx, callee.as_ref(), args)
        }

        // crypto.generateKeyPairSync(type, options) -> { publicKey, privateKey }.
        // The runtime builds the object (PEM strings) and returns it already
        // NaN-boxed; `.publicKey` / `.privateKey` reads go through the generic
        // object property dispatch (the object carries a keys array).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. } if property == "generateKeyPairSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            arm_crypto_generate_key_pair_sync(ctx, callee.as_ref(), args)
        }

        // Phase H fs: `fs.promises.METHOD(args...)` — HIR shape is a
        // nested PropertyGet { PropertyGet { NativeModuleRef("fs"),
        // "promises" }, method }. Route supported methods through the
        // runtime fs/promises wrappers so validation failures reject
        // instead of throwing before a Promise is returned.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, .. } if matches!(
                    object.as_ref(),
                    Expr::PropertyGet { object: inner, property: p, .. }
                        if p == "promises" && matches!(
                            inner.as_ref(),
                            Expr::NativeModuleRef(name) if name == "fs"
                        )
                )
            ) =>
        {
            arm_fs_promises(ctx, callee.as_ref(), args)
        }

        // Phase H fs: `fs.METHOD(args...)` — catch all Call expressions
        // where the callee is a PropertyGet on a `NativeModuleRef("fs")`
        // and dispatch to the matching runtime function. HIR already
        // routes the common cases (`readFileSync`, `writeFileSync`,
        // etc.) into dedicated `Expr::Fs*` variants, but several sync
        // APIs (`statSync`, `readdirSync`, `renameSync`, ...) fall
        // through to this generic shape. Handling them here avoids
        // touching HIR or the lower_call dispatch tower.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, .. } if matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(name) if name == "fs"
                )
            ) =>
        {
            arm_fs(ctx, callee.as_ref(), args)
        }

        // -------- Calls --------
        Expr::Call {
            callee,
            args,
            byte_offset,
            ..
        } => {
            super::downgrade_buffer_aliases_in_expr(
                ctx,
                callee,
                crate::native_value::MaterializationReason::UnknownCallEscape,
            );
            for arg in args {
                super::downgrade_buffer_aliases_in_expr(
                    ctx,
                    arg,
                    crate::native_value::MaterializationReason::UnknownCallEscape,
                );
            }
            // #5247: under `--debug-symbols`, record this call's source byte
            // offset so the dynamic method-dispatch emission site can emit a
            // `js_set_call_location` immediately before the throwing dispatch
            // (after the call's args — which may be nested calls that overwrite
            // this — have been lowered). The dynamic dispatch path renders it as
            // `at <file>:<line>` in the "X is not a function" TypeError's
            // `.stack`. No-op in the default build.
            if ctx.strings.debug_locations_enabled() {
                ctx.strings.set_pending_call_offset(*byte_offset);
            }
            lower_call(ctx, callee, args)
        }

        // -------- Proxy / Reflect (metaprogramming) --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
