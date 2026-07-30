//! Inline **checked f64** typed-array element read for a typed-array *parameter*
//! consumed in numeric (non-`| 0`) context.
//!
//! Motivating shape — `bcryptjs`'s `_encipher` (its own profiled bottleneck,
//! ~90% of a cost-N hash): the Blowfish S-box lookups
//! `n = S[l >>> 24]; n += S[0x100 | ((l >> 16) & 0xff)]; …` read `Int32Array`
//! *parameters* and feed the element into `+`/`+=`, i.e. an f64 numeric context.
//! The i32 fast path (`i32_fast_path.rs`) only fires when the read is in a
//! `ToInt32` (`| 0`) context, so these reads fell back to a per-element
//! `call double @js_typed_array_get` runtime call — measured ~26× slower than V8,
//! which compiles the same read to a single bounds-checked load.
//!
//! The runtime getter already returns `double` (the numeric element in-bounds,
//! the `TAG_UNDEFINED` double OOB / negative), so an inline load that reproduces
//! **exactly** those two return values is a bit-exact drop-in — no consumer
//! analysis, no string-vs-number disambiguation, no OOB-semantics divergence.
//! That is what this module emits: the guard/bounds machinery of the checked i32
//! load, but widening the element to f64 and merging in the `TAG_UNDEFINED`
//! double (not `0`) on OOB. Guard misses defer to the memory-safe
//! `js_typed_array_read_f64` cold helper.

use anyhow::Result;
use perry_hir::Expr;

use super::index_get::numeric_index_has_integer_array_index_proof;
use super::{lower_expr, lower_expr_as_i32, FnCtx};
use crate::nanbox::{double_literal, i64_literal, TAG_UNDEFINED};
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue};
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};

/// How a loaded element widens into the f64 result.
#[derive(Clone, Copy)]
enum F64Conv {
    /// Signed integer element (I8/I16/I32) → `sitofp`.
    SInt,
    /// Unsigned integer element (U8/U8Clamped/U16/U32) → `uitofp`.
    UInt,
    /// `Float32` element → `fpext`.
    F32,
    /// `Float64` element → direct load, no conversion.
    F64,
}

/// Numeric element kind of a statically-typed typed-array receiver eligible for
/// the inline **checked f64** element read: `(kind_tag, elem_llvm_ty,
/// elem_size_bytes, conv)`. Covers *every* numeric kind — unlike the i32 sibling
/// (`checked_typed_array_i32_kind`), which stops at the i32-representable integer
/// kinds — because an f64 result represents `Uint32Array` and the float kinds
/// exactly. `None` for the BigInt kinds (BigInt64/BigUint64 are BigInt, not
/// Number) and any non-typed-array / non-local receiver.
///
/// `kind_tag` values MUST match `perry-runtime` `KIND_*`
/// (`typedarray/mod.rs`): the runtime `PERRY_TA_KIND_CACHE` stores `kind as u64`,
/// and the entry guard compares against this tag — a mismatch would merely miss
/// the cache and route every read to the slow helper (correct, but no speedup).
fn checked_typed_array_f64_kind(
    ctx: &FnCtx<'_>,
    object: &Expr,
) -> Option<(u8, crate::types::LlvmType, u32, F64Conv)> {
    if ctx.disable_buffer_fast_path {
        return None;
    }
    // Plain local/param read so the receiver is re-fetched at every access
    // (reassignment / capture stay correct — the emission caches nothing).
    let Expr::LocalGet(id) = object else {
        return None;
    };
    // A tracked buffer view owns this receiver via its own (stronger-bounds)
    // native path; don't shadow it.
    if ctx.buffer_view_slots.contains_key(id) {
        return None;
    }
    match crate::type_analysis::receiver_class_name(ctx, object).as_deref()? {
        "Int8Array" => Some((0, I8, 1, F64Conv::SInt)),
        "Uint8Array" => Some((1, I8, 1, F64Conv::UInt)),
        "Uint8ClampedArray" => Some((8, I8, 1, F64Conv::UInt)),
        "Int16Array" => Some((2, I16, 2, F64Conv::SInt)),
        "Uint16Array" => Some((3, I16, 2, F64Conv::UInt)),
        "Int32Array" => Some((4, I32, 4, F64Conv::SInt)),
        "Uint32Array" => Some((5, I32, 4, F64Conv::UInt)),
        "Float32Array" => Some((6, F32, 4, F64Conv::F32)),
        "Float64Array" => Some((7, DOUBLE, 8, F64Conv::F64)),
        _ => None,
    }
}

/// Compile-time gate (bisection): unset / `1` / `on` / `true` enable; `0` /
/// `off` / `false` disable. Object-cache keys every codegen env var, so a
/// flipped value re-codegens rather than serving a stale cache.
fn ta_param_f64_read_enabled() -> bool {
    match std::env::var("PERRY_TA_PARAM_F64_READ") {
        Ok(v) => !matches!(v.as_str(), "0" | "off" | "false" | "OFF" | "FALSE"),
        Err(_) => true,
    }
}

/// If `object[index]` is a numeric-context read of a typed-array parameter with
/// a proven non-negative integer index, emit the inline checked f64 load and
/// return its DOUBLE SSA value; otherwise `Ok(None)` so the caller keeps its
/// existing `js_typed_array_get` fallback. Records CheckedNative access-mode
/// evidence for the buffer-facts artifact, mirroring the slow-path sibling.
pub(crate) fn try_lower_ta_param_f64_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    if !ta_param_f64_read_enabled() {
        return Ok(None);
    }
    // Fractional / unproven indices must stay on the runtime getter: the inline
    // path lowers `index` via ToInt32 (`fptosi`), so `S[3.9]` would read element
    // 3, but JS reads a fractional typed-array index as `undefined`. Require the
    // same proven-integer index the i32 read paths use.
    if !numeric_index_has_integer_array_index_proof(ctx, index) {
        return Ok(None);
    }
    let Some((kind, elem_ty, elem_size, conv)) = checked_typed_array_f64_kind(ctx, object) else {
        return Ok(None);
    };
    let value = lower_checked_typed_array_f64_load(
        ctx, object, index, kind, elem_ty, elem_size, conv, false,
    )?;
    let lowered = LoweredValue::js_value(value.clone());
    ctx.record_lowered_value_with_access_mode(
        "TypedArrayGet",
        None,
        "TypedArrayGet.checked_f64_param",
        &lowered,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::CheckedNative),
        Some(super::buffer_views::buffer_access_materialization_reason(
            ctx, object,
        )),
        false,
        false,
        vec!["typed_array_param_f64=checked_inline".to_string()],
    );
    Ok(Some(value))
}

/// Number-context sibling of [`try_lower_ta_param_f64_read`].
///
/// The in-bounds hot path is the same guard + native load. Only the OOB and
/// cold fallback arms apply `ToNumber`, keeping arithmetic call-free for the
/// common case while making `1000 + ta[99]` produce canonical `NaN` (#6884).
pub(crate) fn try_lower_ta_f64_read_for_number_context(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    if !ta_param_f64_read_enabled() || !numeric_index_has_integer_array_index_proof(ctx, index) {
        return Ok(None);
    }
    let Some((kind, elem_ty, elem_size, conv)) = checked_typed_array_f64_kind(ctx, object) else {
        return Ok(None);
    };
    lower_checked_typed_array_f64_load(ctx, object, index, kind, elem_ty, elem_size, conv, true)
        .map(Some)
}

/// Emit the checked inline f64 element load. Same runtime-fact guard and header
/// bounds check as [`super::i32_fast_path`]'s `lower_checked_typed_array_i32_load`
/// (pointer + inline-storage `PERRY_TA_VIEW_GUARD == 0` + kind-cache addr/kind),
/// but the load arm widens the element to f64, the OOB arm merges in the
/// `TAG_UNDEFINED` double, and guard misses defer to `js_typed_array_read_f64`.
#[allow(clippy::too_many_arguments)]
fn lower_checked_typed_array_f64_load(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    kind: u8,
    elem_ty: crate::types::LlvmType,
    elem_size: u32,
    conv: F64Conv,
    number_context: bool,
) -> Result<String> {
    let obj_box = lower_expr(ctx, object)?;
    let idx_i32 = lower_expr_as_i32(ctx, index)?;

    let chk_idx = ctx.new_block("ctaf.get.chk");
    let load_idx = ctx.new_block("ctaf.get.load");
    let oob_idx = ctx.new_block("ctaf.get.oob");
    let slow_idx = ctx.new_block("ctaf.get.slow");
    let merge_idx = ctx.new_block("ctaf.get.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let tag_mask = i64_literal(crate::nanbox::TAG_MASK);

    // ---- entry guard: pointer + inline-storage + kind-cache addr/kind ----
    let raw = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&obj_box);
        let raw = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
        let tagged = blk.and(I64, &obj_bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, crate::nanbox::POINTER_TAG_I64);
        let vg = blk.load(I64, "@PERRY_TA_VIEW_GUARD");
        let vg_zero = blk.icmp_eq(I64, &vg, "0");
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let entry_addr = blk.lshr(I64, &entry_val, "8");
        let addr_match = blk.icmp_eq(I64, &entry_addr, &raw); // also rejects empty slot 0
        let kind_bits = blk.and(I64, &entry_val, "255");
        let kind_ok = blk.icmp_eq(I64, &kind_bits, &kind.to_string());
        let g = blk.and(I1, &is_ptr, &vg_zero);
        let g = blk.and(I1, &g, &addr_match);
        let g = blk.and(I1, &g, &kind_ok);
        blk.cond_br(&g, &chk_label, &slow_label);
        raw
    };

    // ---- chk: bounds check against header length (u32 at offset 0) ----
    ctx.current_block = chk_idx;
    {
        let blk = ctx.block();
        let hdr_ptr = blk.inttoptr(I64, &raw);
        let len = blk.load(I32, &hdr_ptr);
        // `ult` also rejects a negative index (wraps huge unsigned) — JS `S[-1]`
        // is undefined; the oob arm merges `TAG_UNDEFINED`.
        let in_bounds = blk.icmp_ult(I32, &idx_i32, &len);
        blk.cond_br(&in_bounds, &load_label, &oob_label);
    }

    // ---- load: bare per-kind element load (data base = raw + 16) → f64 ----
    ctx.current_block = load_idx;
    let (load_val, load_end) = {
        let blk = ctx.block();
        let data_base = blk.add(I64, &raw, "16");
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let shift = elem_size.trailing_zeros().to_string();
        let off = blk.shl(I64, &idx_i64, &shift);
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        let raw_elem = blk.load(elem_ty, &ptr);
        let val = match conv {
            F64Conv::F64 => raw_elem,
            F64Conv::F32 => blk.fpext(F32, &raw_elem, DOUBLE),
            F64Conv::SInt => blk.sitofp(elem_ty, &raw_elem, DOUBLE),
            F64Conv::UInt => blk.uitofp(elem_ty, &raw_elem, DOUBLE),
        };
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    // ---- oob --------------------------------------------------------------
    // Value context preserves the typed-array read (`undefined`). Arithmetic
    // context applies ToNumber at the read boundary, yielding a canonical NaN
    // instead of allowing TAG_UNDEFINED's NaN payload to leak through fadd
    // and remain observably `undefined` (#6884).
    ctx.current_block = oob_idx;
    let (oob_val, oob_end) = {
        let blk = ctx.block();
        let end = blk.label.clone();
        blk.br(&merge_label);
        (
            if number_context {
                double_literal(f64::NAN)
            } else {
                double_literal(f64::from_bits(TAG_UNDEFINED))
            },
            end,
        )
    };

    // ---- slow: view / detached / wrong-kind / non-TA -> memory-safe helper ---
    ctx.current_block = slow_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let value = blk.call(
            DOUBLE,
            "js_typed_array_read_f64",
            &[(I64, &raw), (I32, &idx_i32)],
        );
        let value = if number_context {
            blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &value)])
        } else {
            value
        };
        let end = blk.label.clone();
        blk.br(&merge_label);
        (value, end)
    };

    // ---- merge ----
    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (load_val.as_str(), load_end.as_str()),
            (oob_val.as_str(), oob_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    ))
}
