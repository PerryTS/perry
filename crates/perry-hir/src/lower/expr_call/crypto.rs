//! Shared `crypto.<method>` lowering helpers.
//!
//! #1434: both the named-import path (`import { randomBytes } from
//! "node:crypto"; randomBytes(16)`, lowered in `globals.rs`) and the
//! dotted path (`crypto.randomBytes(16)`, lowered in `module_static.rs`)
//! used to inline the same `Expr::CryptoRandom*` constructions side-by-
//! side, requiring parallel edits every time a new method joined the
//! group. Extracting the shared piece into one helper makes the
//! "named-import + dotted form share a codegen path" invariant the
//! type-checker enforces.
//!
//! Only methods whose lowering shape is **identical between the two
//! sites** live here. Methods that diverge — e.g. dotted-only
//! `getRandomValues` (rewrites into an instance method on the buffer)
//! and dotted-only `sha256`/`md5` (dedicated `Expr::CryptoSha256` /
//! `Expr::CryptoMd5` shortcuts) — stay at their call site.

use crate::ir::Expr;

/// Cheap pre-check so the caller can avoid moving `args` into the
/// helper when the method isn't ours. Must stay in sync with the
/// `match` in [`lower_crypto_passthrough`].
pub(super) fn is_passthrough_method(method: &str) -> bool {
    matches!(
        method,
        // Methods whose lowering has a dedicated HIR variant or special
        // arg munging.
        "randomFillSync"
            | "randomUUID"
            | "randomUUIDv7"
            | "randomBytes"
            | "hash"
            // Plain `crypto.<method>(...)` → `Expr::Call { PropertyGet {
            // NativeModuleRef("crypto"), method }, args }` passthrough.
            // These all live in the api-manifest and the runtime
            // dispatch handles them by (module, method) — but the
            // named-import path won't reach the manifest's
            // `NativeMethodCall` route without an explicit lowering arm,
            // so they must be enumerated here to share the route with
            // the dotted form.
            | "randomFill"
            | "randomInt"
            | "generatePrime"
            | "generatePrimeSync"
            | "checkPrime"
            | "checkPrimeSync"
            | "createHash"
            | "Hash"
            | "createSign"
            | "Sign"
            | "createVerify"
            | "Verify"
            | "createECDH"
            | "createDiffieHellman"
            | "DiffieHellman"
            | "createDiffieHellmanGroup"
            | "DiffieHellmanGroup"
            | "getDiffieHellman"
            | "diffieHellman"
            | "encapsulate"
            | "decapsulate"
            | "createPrivateKey"
            | "createPublicKey"
            | "generateKeyPair"
            | "generateKeyPairSync"
            | "generateKey"
            | "createHmac"
            | "Hmac"
            | "pbkdf2Sync"
            | "hkdf"
            | "argon2"
            | "argon2Sync"
            | "hkdfSync"
            | "scrypt"
            | "scryptSync"
            | "timingSafeEqual"
            | "sign"
            | "verify"
            | "publicEncrypt"
            | "privateDecrypt"
            | "privateEncrypt"
            | "publicDecrypt"
            | "getHashes"
            | "getCiphers"
            | "getCurves"
            | "getFips"
            | "setFips"
            | "secureHeapUsed"
            | "createCipheriv"
            | "createDecipheriv"
    )
}

/// Lower one of the shared `crypto.<method>(...)` shapes. Returns
/// `Some(expr)` when `method` is in the set this helper covers,
/// `None` otherwise.
///
/// Today's set:
/// - `randomFillSync(buffer, offset?, size?)` → `Expr::CryptoRandomFillSync`.
/// - `randomUUID()` → `Expr::CryptoRandomUUID`; option-bearing calls keep
///   the generic call shape so codegen can pass the options value through.
/// - `randomBytes(size)` → `Expr::CryptoRandomBytes`.
pub(super) fn lower_crypto_passthrough(method: &str, args: Vec<Expr>) -> Option<Expr> {
    match method {
        "randomFillSync" => {
            if args.is_empty() {
                return None;
            }
            let mut iter = args.into_iter();
            let buffer = iter.next().unwrap();
            let offset = iter.next().unwrap_or(Expr::Undefined);
            let size = iter.next().unwrap_or(Expr::Undefined);
            Some(Expr::CryptoRandomFillSync {
                buffer: Box::new(buffer),
                offset: Box::new(offset),
                size: Box::new(size),
            })
        }
        "randomUUID" if args.is_empty() => Some(Expr::CryptoRandomUUID),
        "randomUUID" => Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                property: "randomUUID".to_string(),
            }),
            args,
            type_args: vec![],
            byte_offset: 0,
        }),
        // `randomUUIDv7([options])` — options accepted for shape parity but
        // do not affect the generated value (#2550).
        "randomUUIDv7" => Some(Expr::CryptoRandomUUIDv7),

        "randomBytes" => {
            if args.is_empty() {
                return None;
            }
            if args.len() >= 2 {
                // `randomBytes(size, callback)` — async form. Keep the
                // generic call shape so the codegen async dispatch path
                // resolves the callback, instead of the inline-bytes
                // Expr::CryptoRandomBytes that targets only the sync form.
                return Some(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                        property: "randomBytes".to_string(),
                    }),
                    args,
                    type_args: vec![],
                    byte_offset: 0,
                });
            }
            Some(Expr::CryptoRandomBytes(Box::new(
                args.into_iter().next().unwrap(),
            )))
        }
        // Generic passthrough crypto helpers: same Expr::Call shape on
        // both call sites, no special args munging. Keep this list in
        // sync with `is_passthrough_method` above.
        "randomFill"
        | "randomInt"
        | "generatePrime"
        | "generatePrimeSync"
        | "checkPrime"
        | "checkPrimeSync"
        | "createHash"
        | "Hash"
        | "createSign"
        | "Sign"
        | "createVerify"
        | "Verify"
        | "createECDH"
        | "createDiffieHellman"
        | "DiffieHellman"
        | "createDiffieHellmanGroup"
        | "DiffieHellmanGroup"
        | "getDiffieHellman"
        | "diffieHellman"
        | "encapsulate"
        | "decapsulate"
        | "createPrivateKey"
        | "createPublicKey"
        | "generateKeyPair"
        | "generateKeyPairSync"
        | "generateKey"
        | "createHmac"
        | "Hmac"
        | "pbkdf2Sync"
        | "hkdf"
        | "argon2"
        | "argon2Sync"
        | "hkdfSync"
        | "scrypt"
        | "scryptSync"
        | "timingSafeEqual"
        | "sign"
        | "verify"
        | "publicEncrypt"
        | "privateDecrypt"
        | "privateEncrypt"
        | "publicDecrypt"
        | "getHashes"
        | "getCiphers"
        | "getCurves"
        | "getFips"
        | "setFips"
        | "secureHeapUsed"
        | "createCipheriv"
        | "createDecipheriv" => Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                property: method.to_string(),
            }),
            args,
            type_args: vec![],
            byte_offset: 0,
        }),
        // `crypto.hash(alg, data, enc?)` — Node 21+ one-shot helper.
        // Expand into the `createHash(alg).update(data).digest(enc)`
        // chain so it shares the existing codegen fast-path. `enc`
        // defaults to `"hex"` (matching the codegen fallback).
        "hash" => {
            if args.len() < 2 {
                return None;
            }
            let mut iter = args.into_iter();
            let alg = iter.next().unwrap();
            let data = iter.next().unwrap();
            let enc = iter.next().unwrap_or_else(|| Expr::String("hex".into()));
            Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            byte_offset: 0,
                            object: Box::new(Expr::Call {
                                callee: Box::new(Expr::PropertyGet {
                                    byte_offset: 0,
                                    object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                                    property: "createHash".to_string(),
                                }),
                                args: vec![alg],
                                type_args: vec![],
                                byte_offset: 0,
                            }),
                            property: "update".to_string(),
                        }),
                        args: vec![data],
                        type_args: vec![],
                        byte_offset: 0,
                    }),
                    property: "digest".to_string(),
                }),
                args: vec![enc],
                type_args: vec![],
                byte_offset: 0,
            })
        }
        _ => None,
    }
}
