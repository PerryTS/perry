//! Inline `charCodeAt` on an ASCII heap string (#7592).
//!
//! Split out of `lower_string_method.rs` to stay under the repo's
//! 2000-line-per-file lint cap. Pure code move plus the `pub(super)`
//! visibility the split requires.

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::type_analysis::is_string_expr;
use crate::types::{DOUBLE, I1, I32, I64};

use super::str_operand_handle_tag_dispatched;

// ── Inline `charCodeAt` on an ASCII heap string (#7592) ──────────────────
//
// `StringHeader` layout the fast path reads, from
// `crates/perry-runtime/src/string/mod.rs`. That struct is `#[repr(C)]` with
// five `u32` fields; the runtime carries a `const` assertion
// (`STRING_HEADER_ABI_MATCHES_CODEGEN`) pinning exactly these three numbers,
// so a layout change fails the runtime build rather than miscompiling here.
//
// Offset 0 is already an established codegen/runtime contract: the inline
// `.length` fast path in `expr/property_get.rs` loads it, and the struct's
// own doc comment says "`utf16_len` is at offset 0 so codegen can inline
// `.length` as a single i32 load."
const STRING_HEADER_UTF16_LEN_OFFSET: &str = "0";
const STRING_HEADER_BYTE_LEN_OFFSET: &str = "4";
const STRING_HEADER_SIZE: &str = "20";

/// Inline `s.charCodeAt(i)` for a heap-tagged, all-ASCII receiver.
///
/// #7592 — the FNV-1a phase of `json_pipeline` spent 85% of its leaf profile
/// in four opaque runtime calls per character over a 68 MB string:
/// `js_string_char_code_at` (31.5%), `js_dynamic_bitxor` (31.0%),
/// `js_string_index_to_i32` (13.1%) and `js_get_string_pointer_unified`
/// (9.0%). Only 15% was the JS loop. The individual helpers are each a
/// handful of instructions; the cost is the FFI boundary itself, and — worse
/// — an opaque call on the loop's critical path blocks LICM, so the
/// loop-invariant receiver unbox and header loads could never be hoisted.
///
/// The guard chain reproduces exactly what `js_string_char_code_at` +
/// `js_string_index_to_i32` would compute, and anything it cannot prove
/// branches to those same two calls:
/// * `STRING_TAG` receiver — an SSO short string, a lying `string`
///   annotation, or `undefined` all take the slow arm, which still routes
///   through `str_operand_handle_tag_dispatched` (SSO materialization
///   included);
/// * handle ≥ 4096 — the runtime's `is_valid_string_ptr` magnitude check;
/// * `0.0 <= index < 2^31-1` — ORDERED comparisons, so a NaN-boxed index
///   (a string, a bool, `undefined`, or a genuine NaN) fails both and takes
///   the slow arm, where `js_string_index_to_i32` performs the full
///   `ToIntegerOrInfinity` including user `valueOf`. Also makes the
///   subsequent `fptosi` in-range, so it can never be poison;
/// * `utf16_len == byte_len` — the runtime's own `is_ascii_string`
///   predicate. Equality implies every byte is one UTF-16 code unit, i.e.
///   every byte is < 0x80, so no WTF-8 / lone-surrogate / astral input can
///   reach the byte load (#6085's bounded walk still owns those);
/// * `index < utf16_len` — out of range returns `NaN` from the slow arm.
///
/// No allocation and no call occur between the receiver re-read and the byte
/// load, so no collection can move the header underneath the fast path.
///
/// Gated on `static_string_lowering_enabled()` (`PERRY_STATIC_STRING_LOWERING`)
/// — the same knob the sibling inline `.length` fast path uses, so this adds
/// no new mode.
pub(super) fn lower_char_code_at_inline(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv_box: &str,
    idx_d: &str,
) -> Option<String> {
    use crate::nanbox::POINTER_MASK_I64;
    if !crate::expr::static_string_lowering_enabled() || !is_string_expr(ctx, object) {
        return None;
    }

    let bits = ctx.block().bitcast_double_to_i64(recv_box);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let handle_ok = ctx.block().icmp_uge(I64, &handle, "4096");
    let idx_ge0 = ctx.block().fcmp("oge", idx_d, "0.0");
    let idx_lt_max = ctx
        .block()
        .fcmp("olt", idx_d, &crate::nanbox::double_literal(2147483647.0));
    let recv_ok = ctx.block().and(I1, &is_heap, &handle_ok);
    let idx_ok = ctx.block().and(I1, &idx_ge0, &idx_lt_max);
    let entry_ok = ctx.block().and(I1, &recv_ok, &idx_ok);

    let hdr_idx = ctx.new_block("cca.hdr");
    let fast_idx = ctx.new_block("cca.fast");
    let slow_idx = ctx.new_block("cca.slow");
    let merge_idx = ctx.new_block("cca.merge");
    let hdr_label = ctx.block_label(hdr_idx);
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&entry_ok, &hdr_label, &slow_label);

    // Header block: ASCII + in-range test. Dominated by `handle >= 4096`, so
    // the two loads are safe; dominated by the index range test, so `fptosi`
    // is in range.
    ctx.current_block = hdr_idx;
    let hdr_ptr = ctx.block().inttoptr(I64, &handle);
    let u16_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &hdr_ptr,
        &[(I64, STRING_HEADER_UTF16_LEN_OFFSET)],
    );
    let utf16_len = ctx.block().load(I32, &u16_ptr);
    let blen_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &hdr_ptr,
        &[(I64, STRING_HEADER_BYTE_LEN_OFFSET)],
    );
    let byte_len = ctx.block().load(I32, &blen_ptr);
    let is_ascii = ctx.block().icmp_eq(I32, &utf16_len, &byte_len);
    let idx_i32 = ctx.block().fptosi(DOUBLE, idx_d, I32);
    let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &utf16_len);
    let fast_ok = ctx.block().and(I1, &is_ascii, &in_bounds);
    ctx.block().cond_br(&fast_ok, &fast_label, &slow_label);

    // Fast block: one byte load, zero calls.
    ctx.current_block = fast_idx;
    let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
    let data_ptr =
        ctx.block()
            .gep_inbounds(crate::types::I8, &hdr_ptr, &[(I64, STRING_HEADER_SIZE)]);
    let char_ptr = ctx
        .block()
        .gep_inbounds(crate::types::I8, &data_ptr, &[(I64, &idx_i64)]);
    let byte = ctx.block().load(crate::types::I8, &char_ptr);
    let fast_val = ctx.block().uitofp(crate::types::I8, &byte, DOUBLE);
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Slow block: bit-for-bit the pre-#7592 lowering.
    ctx.current_block = slow_idx;
    let recv_handle = str_operand_handle_tag_dispatched(ctx, object, recv_box);
    let slow_idx_i32 = ctx
        .block()
        .call(I32, "js_string_index_to_i32", &[(DOUBLE, idx_d)]);
    let slow_val = ctx.block().call(
        DOUBLE,
        "js_string_char_code_at",
        &[(I64, &recv_handle), (I32, &slow_idx_i32)],
    );
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Some(
        ctx.block()
            .phi(DOUBLE, &[(&fast_val, &fast_pred), (&slow_val, &slow_pred)]),
    )
}
