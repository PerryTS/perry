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
use crate::nanbox::{double_literal, TAG_UNDEFINED};
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue};
use crate::types::{DOUBLE, F32, I16, I32, I64, I8};

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

/// Access uses the common cell header and current owner storage. Any derived
/// data address is consumed without collection, or retained with its owner.
fn declared_typed_array_class_f64(ctx: &FnCtx<'_>, id: &u32) -> Option<String> {
    if ctx.reassigned_locals.contains(id) {
        return None;
    }
    match ctx.local_type_hint(id)? {
        perry_hir::types::Type::Named(name) => Some(name.clone()),
        _ => None,
    }
}

/// Byte offset of a typed-array object's inline elements from its header, which
/// holds the `u32` length at offset 0. The checked load reads exactly this
/// layout.

/// Whether a receiver with a tracked buffer view may take the guarded read.
///
/// A view owns its receiver while it can still serve the read itself: every
/// view tier (`lower_typed_array_load`, the proven checked and proven guarded
/// tiers) requires the view's `noalias` proof and a live alias scope, and a
/// native-owned view names memory this load does not read. Such a view is
/// left alone; the guarded read must not shadow its stronger path (in number
/// context, `binary.rs` tries this tier first).
///
/// A view that lost the proof (an alias, an escape, a refresh or a `.buffer`
/// exposure demoted it) is served by no view tier, so the read used to fall
/// to the unconditional `js_typed_array_get` call, slower than having no view
/// at all (#11810). The guarded read takes it instead: it uses nothing from
/// the view. It re-reads the receiver at every access, admits it only through
/// the runtime kind cache keyed by the object's own address, reads the length
/// from the object's header and the element from its inline storage, and sends
/// every miss (a different kind, out-of-line or detached storage) to the
/// memory-safe helper.
///
/// The guard only pays off while the elements are where the load reads them,
/// so the view must still prove that: inline storage, and a cached pointer
/// that no `.buffer` exposure or storage change has invalidated. Exposing
/// `.buffer` moves the elements out of line (#10516), the kind cache then
/// tags the address as external storage, and every guarded read would miss
/// into the helper, which costs more than the plain call. Likewise the view
/// must have the typed-array object layout the load reads; a buffer-layout
/// view (a perry `Uint8Array`, whose length sits 8 bytes before its data)
/// keeps its own lane.
fn view_leaves_receiver_to_guarded_read(view: &crate::native_value::BufferViewSlot) -> bool {
    let serves_itself = view.alias.allows_noalias() && view.scope_idx.is_some();
    !serves_itself
        && view.native_owned.is_none()
        && view.storage_inline_proven
        && view.pointer_state.is_stable()
        && view.length_slot.is_some()
}

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
    if let Some(view) = ctx.receiver_descriptors.buffer_view(id) {
        if !view_leaves_receiver_to_guarded_read(view) {
            return None;
        }
    }
    // Class proof: first the function-local proof, then — for a MODULE-GLOBAL
    // typed array (allocated once at module scope and read inside functions,
    // the common bundled shape) — the module-global runtime-type proof. The
    // load below is guard-protected (a wrong class simply misses the runtime
    // KIND cache and defers to the safe helper), so an optimistic module-global
    // proof can only cost a missed speedup, never correctness. Reassigned
    // bindings are still excluded so a rebind can't make the proof stale for
    // the local-proof case; for the module-global case the runtime guard is
    // the safety net regardless. #8595-followup / typed-array read inlining.
    let class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| {
            if ctx.reassigned_locals.contains(id) {
                return None;
            }
            match ctx.module_global_proven_types.get(id) {
                Some(perry_hir::types::Type::Named(name)) => Some(name.clone()),
                _ => None,
            }
        })
        .or_else(|| declared_typed_array_class_f64(ctx, id))?;
    f64_kind_from_class(&class)
}

/// `(kind_tag, elem_llvm_ty, elem_size_bytes, conv)` for a typed-array class
/// name, or `None` for a non-typed-array class. Shared by the local-proof and
/// module-global-proof arms so both admit exactly the same kinds.
fn f64_kind_from_class(name: &str) -> Option<(u8, crate::types::LlvmType, u32, F64Conv)> {
    match name {
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
pub(crate) fn ta_param_f64_read_enabled() -> bool {
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
/// (pointer + kind-cache addr/kind, whose tag also says inline storage),
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

    let access = super::byte_cell::resolve_read(
        ctx,
        object,
        &obj_box,
        &[super::byte_cell::brand_for_kind(kind)],
        &slow_label,
    );
    let raw = access.raw;
    ctx.block().br(&chk_label);

    // ---- chk: bounds check against header length (u32 at offset 0) ----
    ctx.current_block = chk_idx;
    {
        let blk = ctx.block();
        let len = &access.len;
        // `ult` also rejects a negative index (wraps huge unsigned) — JS `S[-1]`
        // is undefined; the oob arm merges `TAG_UNDEFINED`.
        let in_bounds = blk.icmp_ult(I32, &idx_i32, len);
        blk.cond_br(&in_bounds, &load_label, &oob_label);
    }

    // ---- load: bare per-kind element load (data base = raw + 16) → f64 ----
    ctx.current_block = load_idx;
    let (load_val, load_end) = {
        let blk = ctx.block();
        let data_base = &access.data;
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let shift = elem_size.trailing_zeros().to_string();
        let off = blk.shl(I64, &idx_i64, &shift);
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        let raw_elem = blk.load(elem_ty, &ptr);
        // #10779: the float kinds are the only ones whose lane can be a NaN,
        // and an ArrayBuffer lane is arbitrary user bytes — canonicalise so the
        // value cannot alias a NaN-box tag downstream.
        let val = match conv {
            F64Conv::F64 => crate::expr::nanbox_inline::canonicalize_lane_f64(blk, &raw_elem),
            F64Conv::F32 => {
                let widened = blk.fpext(F32, &raw_elem, DOUBLE);
                crate::expr::nanbox_inline::canonicalize_lane_f64(blk, &widened)
            }
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
    let slow_val = ctx.block().call(
        DOUBLE,
        "js_typed_array_read_f64",
        &[(I64, &raw), (I32, &idx_i32)],
    );
    let slow_val = if number_context {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &slow_val)])
    } else {
        slow_val
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

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
