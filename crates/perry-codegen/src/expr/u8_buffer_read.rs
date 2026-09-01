//! Inline checked byte read for an **untracked** `Uint8Array` receiver (#9342).
//!
//! Motivating shape — `s += buf[i]` inside a function over a module-global
//! `const buf = new Uint8Array(N)` (the bench_buffer_readwrite in-function
//! cliff: 560ms vs node's 38ms, 12×). The tracked fresh-view path
//! (`buffer_access.rs::lower_buffer_load`) only serves `let` bindings whose
//! construction the same function saw; a module-global (or any
//! class-proven-but-untracked) receiver fell back to a per-element
//! `js_uint8array_index_get_value` call feeding a dynamic add.
//!
//! The typed-array sibling (`ta_param_f64_read.rs`) cannot serve this shape:
//! perry's `Uint8Array` is a `BufferHeader` in the **buffer** registries —
//! bytes inline at `header + 8`, `length: u32` at offset 0 — invisible to
//! `lookup_typed_array_kind` and laid out differently from a
//! `TypedArrayHeader` (data at +16). Hence a buffer-lane twin:
//!
//!  * **guard**: NaN-box pointer tag + full-address hit in
//!    `PERRY_U8_INLINE_CACHE` (`perry-runtime/src/buffer/header.rs`), whose
//!    entries name live, u8-marked, inline-storage `BufferHeader`s only. The
//!    cache is primed by the slow arm and invalidated on buffer death and
//!    address reuse, so a hit is proof of the layout contract;
//!  * **bounds**: `idx ult length` (`ult` also rejects negative indices);
//!    out-of-bounds merges the `TAG_UNDEFINED` double, matching
//!    `js_buffer_index_get_value`;
//!  * **load**: `zext(load i8 (addr + 8 + idx))` widened via `uitofp` — the
//!    numeric element, bit-exact with the runtime helper's in-range answer;
//!  * **slow arm**: `js_u8_buffer_read_f64`, which primes the cache and
//!    delegates to `js_uint8array_index_get_value` — bug-exact semantics for
//!    every receiver the guard rejects, including #8111 stale-hint recovery.
//!
//! READS ONLY. An inline **write** twin would bypass the `buffer/view.rs`
//! write-propagation protocol and desynchronize slice/`new Uint8Array(ab)`
//! aliases (#1205); view copies are admissible here precisely because writes
//! all still propagate.

use anyhow::Result;
use perry_hir::Expr;

use super::index_get::numeric_index_has_integer_array_index_proof;
use super::{lower_expr, lower_expr_as_i32, FnCtx};
use crate::nanbox::{double_literal, i64_literal, TAG_UNDEFINED};
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `PERRY_U8_INLINE_READ=0` kill switch (default on).
fn u8_inline_read_enabled() -> bool {
    match std::env::var("PERRY_U8_INLINE_READ") {
        Ok(v) => !matches!(v.as_str(), "0" | "off" | "false" | "OFF" | "FALSE"),
        Err(_) => true,
    }
}

/// Static receiver eligibility: a plain local/module-global read whose class
/// proves `Uint8Array`, not owned by the (stronger) tracked-view path. The
/// runtime guard is the safety net — a stale proof merely misses the cache —
/// but reassigned bindings are excluded anyway, mirroring
/// `ta_param_f64_read::checked_typed_array_f64_kind`'s reasoning.
fn u8_buffer_receiver_eligible(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    if ctx.buffer_view_slots.contains_key(id) {
        return false;
    }
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
        .or_else(|| {
            // #9363: a declared `Uint8Array` parameter, on the same
            // guard-validated-hint terms as the typed-array lanes.
            if ctx.reassigned_locals.contains(id) {
                return None;
            }
            match ctx.local_type_hint(id) {
                Some(perry_hir::types::Type::Named(name)) => Some(name.clone()),
                _ => None,
            }
        });
    class.as_deref() == Some("Uint8Array")
}

/// If `object[index]` is a proven-integer-index read of an untracked
/// `Uint8Array` receiver, emit the guarded inline byte load and return its
/// DOUBLE SSA value; otherwise `Ok(None)` so the caller keeps its existing
/// fallback. Records CheckedNative access-mode evidence, mirroring the
/// typed-array sibling.
pub(crate) fn try_lower_u8_buffer_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    if ctx.disable_buffer_fast_path || !u8_inline_read_enabled() {
        return Ok(None);
    }
    // Fractional / unproven indices stay on the runtime helper: the inline
    // path lowers `index` via ToInt32, but JS reads `buf[3.9]` as `undefined`.
    if !numeric_index_has_integer_array_index_proof(ctx, index) {
        return Ok(None);
    }
    if !u8_buffer_receiver_eligible(ctx, object) {
        return Ok(None);
    }
    let value = lower_u8_buffer_checked_load(ctx, object, index)?;
    let lowered = LoweredValue::js_value(value.clone());
    ctx.record_lowered_value_with_access_mode(
        "Uint8ArrayGet",
        None,
        "Uint8ArrayGet.checked_u8_inline",
        &lowered,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::CheckedNative),
        Some(super::buffer_views::buffer_access_materialization_reason(
            ctx, object,
        )),
        false,
        false,
        vec!["u8_buffer_read=checked_inline".to_string()],
    );
    Ok(Some(value))
}

fn lower_u8_buffer_checked_load(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<String> {
    let obj_box = lower_expr(ctx, object)?;
    let idx_i32 = lower_expr_as_i32(ctx, index)?;

    let chk_idx = ctx.new_block("u8b.get.chk");
    let load_idx = ctx.new_block("u8b.get.load");
    let oob_idx = ctx.new_block("u8b.get.oob");
    let slow_idx = ctx.new_block("u8b.get.slow");
    let merge_idx = ctx.new_block("u8b.get.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    let tag_mask = i64_literal(crate::nanbox::TAG_MASK);

    // ---- entry guard: pointer tag + admission-cache full-address hit ----
    let raw = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&obj_box);
        let raw = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
        let tagged = blk.and(I64, &obj_bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, crate::nanbox::POINTER_TAG_I64);
        // Slot formula duplicates `buffer/header.rs::u8_inline_cache_slot`.
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_U8_INLINE_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        // Full-address compare — an empty slot (0) can never match a real
        // pointer, so no separate emptiness test.
        let hit = blk.icmp_eq(I64, &entry_val, &raw);
        let g = blk.and(I1, &is_ptr, &hit);
        blk.cond_br(&g, &chk_label, &slow_label);
        raw
    };

    // ---- chk: bounds against `BufferHeader.length` (u32 at offset 0) ----
    ctx.current_block = chk_idx;
    {
        let blk = ctx.block();
        let hdr_ptr = blk.inttoptr(I64, &raw);
        let len = blk.load(I32, &hdr_ptr);
        // `ult` also rejects a negative index (wraps huge unsigned) — JS
        // `buf[-1]` is undefined; the oob arm merges `TAG_UNDEFINED`.
        let in_bounds = blk.icmp_ult(I32, &idx_i32, &len);
        blk.cond_br(&in_bounds, &load_label, &oob_label);
    }

    // ---- load: inline byte at `header + 8 + idx`, widened to f64 ----
    ctx.current_block = load_idx;
    let (load_val, load_end) = {
        let blk = ctx.block();
        let data_base = blk.add(I64, &raw, "8");
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let addr = blk.add(I64, &data_base, &idx_i64);
        let ptr = blk.inttoptr(I64, &addr);
        let byte = blk.load(I8, &ptr);
        let val = blk.uitofp(I8, &byte, DOUBLE);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    // ---- oob: `undefined`, matching `js_buffer_index_get_value` ----
    ctx.current_block = oob_idx;
    let (oob_val, oob_end) = {
        let blk = ctx.block();
        let end = blk.label.clone();
        blk.br(&merge_label);
        (double_literal(f64::from_bits(TAG_UNDEFINED)), end)
    };

    // ---- slow: cache miss / non-pointer → priming memory-safe helper ----
    ctx.current_block = slow_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let value = blk.call(
            DOUBLE,
            "js_u8_buffer_read_f64",
            &[(I64, &raw), (I32, &idx_i32)],
        );
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
