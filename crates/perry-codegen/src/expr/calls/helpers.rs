use super::*;

use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::nanbox::double_literal;
use crate::type_analysis::static_type_of;
use crate::types::{DOUBLE, I32, I64, PTR};

/// #5247: under `--debug-symbols`, emit a `js_set_call_location(file, line)`
/// runtime call right before a dynamic method dispatch so the
/// "X is not a function" throw path can render `at <file>:<line>` in the thrown
/// TypeError's `.stack`. Resolves the *pending* call byte offset (recorded by
/// the `Expr::Call` dispatcher) → `(file, line)` via the module's installed
/// debug-location context. No-op (no IR emitted) when the context is absent
/// (default build) or the pending offset is 0 (synthesized call).
///
/// Called at the dispatch emission site (after the call's arguments are
/// lowered) with the offset the dispatcher captured at entry — before any
/// nested-call argument overwrote the shared pending offset — so the location
/// reflects the OUTER call, not its last-lowered argument.
pub(crate) fn emit_call_location_at(ctx: &mut FnCtx<'_>, byte_offset: u32) {
    let Some((file, line)) = ctx
        .strings
        .call_location_for(byte_offset)
        .map(|(f, l)| (f.to_string(), l))
    else {
        return;
    };
    let file_label = emit_string_literal_global(ctx, &file);
    let file_len = file.len();
    let blk = ctx.block();
    blk.call_void(
        "js_set_call_location",
        &[
            (PTR, &file_label),
            (I64, &file_len.to_string()),
            (I32, &line.to_string()),
        ],
    );
}

/// #2013/#3146: emit a setup-time `validateString` call. `value_box` is the
/// original NaN-boxed value; `name` is the static argument name node uses in
/// the error (`"algorithm"` for `createHash`, `"hmac"` for `createHmac`'s
/// algorithm, `"digest"` for `pbkdf2`). The runtime throws `TypeError
/// [ERR_INVALID_ARG_TYPE]` on a non-string value, so this is emitted BEFORE the
/// value is unboxed to a raw pointer (a number would otherwise mask into a
/// bogus pointer and segfault `bytes_from_ptr`).
pub(crate) fn emit_validate_string_arg(ctx: &mut FnCtx<'_>, value_box: &str, name: &str) {
    let name_label = emit_string_literal_global(ctx, name);
    let name_len = name.len();
    let blk = ctx.block();
    blk.call_void(
        "js_runtime_validate_string_arg",
        &[
            (DOUBLE, value_box),
            (PTR, &name_label),
            (I32, &name_len.to_string()),
        ],
    );
}

/// #2013/#3146: emit a setup-time validation for a `node:crypto` key-material
/// argument (`createHmac` key). Accepts a string or `Buffer`/`TypedArray`/
/// `DataView`/`ArrayBuffer`; throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// otherwise. Emitted before the value is unboxed.
pub(crate) fn emit_validate_crypto_key_arg(ctx: &mut FnCtx<'_>, value_box: &str, name: &str) {
    let name_label = emit_string_literal_global(ctx, name);
    let name_len = name.len();
    let blk = ctx.block();
    blk.call_void(
        "js_runtime_validate_crypto_key_arg",
        &[
            (DOUBLE, value_box),
            (PTR, &name_label),
            (I32, &name_len.to_string()),
        ],
    );
}

/// #2013/#3146: emit a setup-time `validateInteger(value, name, min, max)`
/// call. Used for `pbkdf2*` iterations/keylen and `scryptSync` keylen, which
/// node validates as integers in a fixed range before deriving. Emitted in
/// node's argument order so the first bad argument reports the matching error.
pub(crate) fn emit_validate_integer_arg(
    ctx: &mut FnCtx<'_>,
    value_box: &str,
    name: &str,
    min: f64,
    max: f64,
) {
    let name_label = emit_string_literal_global(ctx, name);
    let name_len = name.len();
    let blk = ctx.block();
    blk.call_void(
        "js_runtime_validate_integer_arg",
        &[
            (DOUBLE, value_box),
            (PTR, &name_label),
            (I32, &name_len.to_string()),
            (DOUBLE, &double_literal(min)),
            (DOUBLE, &double_literal(max)),
        ],
    );
}

/// Whether a `createHash(...).update(e)` / `createHmac(alg, e)` argument is a
/// Buffer / Uint8Array — either a direct buffer-producing expression or a
/// local/field whose static type is `Buffer` / `Uint8Array`. Such inputs must
/// not take the inline `*StringHeader` hash fast path, whose UTF-8 string
/// unboxing reads the wrong bytes for a Buffer (#1354).
pub(crate) fn hash_input_is_buffer(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    if matches!(
        e,
        Expr::BufferFrom { .. }
            | Expr::BufferFromArrayBuffer { .. }
            | Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe(_)
            | Expr::BufferConcat(_)
            | Expr::BufferConcatWithLength { .. }
            | Expr::CryptoRandomBytes(_)
    ) {
        return true;
    }
    // `crypto.createSecretKey(...)` / `crypto.generateKeySync(...)` /
    // `crypto.pbkdf2Sync(...)` / `crypto.scryptSync(...)` / `crypto.hkdfSync(...)`
    // all return a BufferHeader (Uint8Array-marked) — the HIR cannot infer
    // that statically without this hint, so without it `createHmac(secretKey, ...)`
    // would route to the string fast-path that misreads buffer bytes as UTF-8.
    if let Expr::Call { callee, .. } = e {
        if let Expr::PropertyGet {
            object, property, ..
        } = callee.as_ref()
        {
            if matches!(object.as_ref(), Expr::NativeModuleRef(n) if n == "crypto")
                && matches!(
                    property.as_str(),
                    "createSecretKey"
                        | "generateKeySync"
                        | "pbkdf2Sync"
                        | "scryptSync"
                        | "hkdfSync"
                        | "randomBytes"
                        | "randomFillSync"
                )
            {
                return true;
            }
        }
    }
    matches!(
        static_type_of(ctx, e),
        Some(HirType::Named(ref n)) if n == "Buffer" || n == "Uint8Array"
    )
}
