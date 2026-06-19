//! Inline NaN-box / unbox helpers (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use crate::block::LlBlock;
use crate::nanbox::{BIGINT_TAG_I64, INT32_TAG_I64, POINTER_TAG_I64, STRING_TAG_I64};
use crate::types::{I1, I32, I64};

/// Inline NaN-box of a raw heap pointer with `POINTER_TAG`.
pub(crate) fn nanbox_pointer_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, POINTER_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Inline NaN-box of a raw `BigIntHeader*` with `BIGINT_TAG`. Required
/// for `typeof x === "bigint"` (which reads the tag byte), and for the
/// runtime's dynamic-dispatch helpers (`js_dynamic_add` etc.) to
/// recognize the value as a bigint at their check sites. Without this,
/// literals like `5n` get tagged as `POINTER_TAG` and `typeof` reports
/// `"object"` / arithmetic falls back to float and returns `NaN`.
pub(crate) fn nanbox_bigint_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, BIGINT_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Alias kept for backwards compatibility with existing callers
/// in `stmt.rs` and `codegen.rs` that use the `_pub` suffix.
pub(crate) fn nanbox_pointer_inline_pub(blk: &mut LlBlock, ptr_i64: &str) -> String {
    nanbox_pointer_inline(blk, ptr_i64)
}

/// Inline NaN-box of a raw string handle with `STRING_TAG`.
pub(crate) fn nanbox_string_inline(blk: &mut LlBlock, ptr_i64: &str) -> String {
    let tagged = blk.or(I64, ptr_i64, STRING_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}

/// Convert an i32 boolean (0 or 1) returned by a runtime function into a
/// NaN-tagged JSValue boolean (`TAG_TRUE` / `TAG_FALSE`).
pub(crate) fn i32_bool_to_nanbox(blk: &mut LlBlock, i32_val: &str) -> String {
    let bit = blk.icmp_ne(I32, i32_val, "0");
    let tagged = blk.select(
        I1,
        &bit,
        I64,
        crate::nanbox::TAG_TRUE_I64,
        crate::nanbox::TAG_FALSE_I64,
    );
    blk.bitcast_i64_to_double(&tagged)
}

/// Inline NaN-box of a raw signed i32. The low 32 payload bits are interpreted
/// as `u32`, matching `JSValue::int32` in the runtime.
pub(crate) fn i32_to_nanbox(blk: &mut LlBlock, i32_val: &str) -> String {
    let payload = blk.zext(I32, i32_val, I64);
    let tagged = blk.or(I64, &payload, INT32_TAG_I64);
    blk.bitcast_i64_to_double(&tagged)
}
