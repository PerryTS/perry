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
    matches!(method, "randomFillSync" | "randomUUID" | "randomBytes")
}

/// Lower one of the shared `crypto.<method>(...)` shapes. Returns
/// `Some(expr)` when `method` is in the set this helper covers,
/// `None` otherwise.
///
/// Today's set:
/// - `randomFillSync(buffer, offset?, size?)` → `Expr::CryptoRandomFillSync`.
/// - `randomUUID()` → `Expr::CryptoRandomUUID`.
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
        "randomUUID" => Some(Expr::CryptoRandomUUID),
        "randomBytes" => {
            if args.is_empty() {
                return None;
            }
            Some(Expr::CryptoRandomBytes(Box::new(
                args.into_iter().next().unwrap(),
            )))
        }
        _ => None,
    }
}
