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
//!    entries name live, u8-marked, owning inline-storage `BufferHeader`s
//!    only. Foreign-backed buffers and registered views are excluded. The
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
//! aliases (#1205). Views stay excluded from owning-storage admission. A
//! separate miss arm accepts their pointer-storage layout and loads from the
//! authoritative backing. Sibling writes are immediately visible; the view
//! does not contain a byte snapshot (#9360/#7219).

use anyhow::Result;
use perry_hir::Expr;

use super::index_get::numeric_index_has_integer_array_index_proof;
use super::{lower_expr, lower_expr_as_i32, FnCtx};
use crate::nanbox::{double_literal, i64_literal, TAG_UNDEFINED};
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8, PTR};

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
pub(crate) fn u8_buffer_receiver_eligible(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    if ctx.receiver_descriptors.contains_buffer_view(id) {
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
    let param_access = byte_view_param_for(ctx, object);
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

    let bits = ctx.block().bitcast_double_to_i64(&obj_box);
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    // Entry-resolved owning and view storage share the bounds/load path. A
    // failed proof retains the existing guarded receiver recovery below.
    if let Some(access) = &param_access {
        let cache_idx = ctx.new_block("u8b.get.cache");
        let cache_label = ctx.block_label(cache_idx);
        ctx.block()
            .cond_br(&access.valid_i1, &slow_label, &cache_label);
        ctx.current_block = cache_idx;
    }
    {
        let blk = ctx.block();
        let tag_mask = i64_literal(crate::nanbox::TAG_MASK);
        let tagged = blk.and(I64, &bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, crate::nanbox::POINTER_TAG_I64);
        let hit = emit_u8_cache_holds(blk, &raw);
        let g = blk.and(I1, &is_ptr, &hit);
        blk.cond_br(&g, &chk_label, &slow_label);
    }

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

    // A cache miss may be a pointer-backed byte view. Its cell carries the
    // resolved data pointer; it must never enter the owning-storage cache.
    ctx.current_block = slow_idx;
    let fallback_idx = ctx.new_block("u8b.get.fallback");
    let fallback_label = ctx.block_label(fallback_idx);
    let (view_val, view_end) = emit_u8_view_get_value(
        ctx,
        &obj_box,
        &raw,
        &idx_i32,
        &fallback_label,
        &merge_label,
        param_access.as_ref(),
    );
    ctx.current_block = fallback_idx;
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
            (view_val.as_str(), view_end.as_str()),
            (oob_val.as_str(), oob_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    ))
}

// ---------------------------------------------------------------------------
// #10515: the same admission cache, for the i32-ABI reads and for WRITES.
//
// The cache contract (`perry-runtime/src/buffer/header.rs`) is that an entry
// names a live registered byte view — `Uint8Array` or `Buffer` — that OWNS its
// bytes inline at `+8` (no foreign span, not a registered view). A write to
// such a buffer is exactly `js_buffer_set`'s store: views over it resolve
// their bytes through this backing rather than holding a copy (see
// `buffer/view.rs`), so there is nothing to propagate. Every guard miss —
// a foreign span, an out-of-range index, a non-pointer, an unadmitted buffer —
// takes the runtime accessor. JS-value reads also try the cell-local view
// pointer on an owning-cache miss; writes retain their runtime fallback.
// ---------------------------------------------------------------------------

/// Pointer tag + full-address admission hit for `obj_box`. Returns
/// `(hit, raw_address)`, both in the current block.
fn emit_u8_cache_admission(ctx: &mut FnCtx<'_>, obj_box: &str) -> (String, String) {
    let tag_mask = i64_literal(crate::nanbox::TAG_MASK);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(obj_box);
    let raw = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
    let tagged = blk.and(I64, &obj_bits, &tag_mask);
    let is_ptr = blk.icmp_eq(I64, &tagged, crate::nanbox::POINTER_TAG_I64);
    let admitted = emit_u8_cache_holds(blk, &raw);
    (blk.and(I1, &is_ptr, &admitted), raw)
}

/// `i1`: `PERRY_U8_INLINE_CACHE` holds exactly `raw`. The cache is two-way
/// set-associative (#10515): `raw` may sit in either slot of the pair
/// `(raw >> 3) & 62`, which duplicates
/// `perry-runtime/src/buffer/header.rs::u8_inline_cache_pair` — keep in sync.
/// Full-address compares, so an empty slot (0) never matches a real pointer.
pub(crate) fn emit_u8_cache_holds(blk: &mut crate::block::LlBlock, raw: &str) -> String {
    let shifted = blk.lshr(I64, raw, "3");
    let pair = blk.and(I64, &shifted, "62");
    let second = blk.or(I64, &pair, "1");
    let first_ptr = blk.gep(
        "[64 x i64]",
        "@PERRY_U8_INLINE_CACHE",
        &[(I64, "0"), (I64, &pair)],
    );
    let second_ptr = blk.gep(
        "[64 x i64]",
        "@PERRY_U8_INLINE_CACHE",
        &[(I64, "0"), (I64, &second)],
    );
    let first = blk.load(I64, &first_ptr);
    let second = blk.load(I64, &second_ptr);
    let in_first = blk.icmp_eq(I64, &first, raw);
    let in_second = blk.icmp_eq(I64, &second, raw);
    blk.or(I1, &in_first, &in_second)
}

/// `idx ult length` against an admitted buffer's `u32` length at offset 0.
/// `ult` also rejects a negative (or `-1`-sentinel) index.
fn emit_u8_in_bounds(ctx: &mut FnCtx<'_>, raw: &str, idx_i32: &str) -> String {
    let blk = ctx.block();
    let hdr_ptr = blk.inttoptr(I64, raw);
    let len = blk.load(I32, &hdr_ptr);
    blk.icmp_ult(I32, idx_i32, &len)
}

fn emit_u8_byte_ptr(ctx: &mut FnCtx<'_>, raw: &str, idx_i32: &str) -> String {
    let blk = ctx.block();
    let data_base = blk.add(I64, raw, "8");
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let addr = blk.add(I64, &data_base, &idx_i64);
    blk.inttoptr(I64, &addr)
}

/// Guarded inline byte READ in the runtime helper's native i32 ABI:
/// `slow_fn(handle: i64, idx: i32) -> i32` (`js_uint8array_get` /
/// `js_buffer_get`, which answer the `0` byte sentinel out of range).
pub(crate) fn emit_u8_cached_get_i32(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_i32: &str,
    slow_fn: &str,
) -> String {
    let chk_idx = ctx.new_block("u8c.get.chk");
    let load_idx = ctx.new_block("u8c.get.load");
    let slow_idx = ctx.new_block("u8c.get.slow");
    let merge_idx = ctx.new_block("u8c.get.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let (hit, raw) = emit_u8_cache_admission(ctx, obj_box);
    ctx.block().cond_br(&hit, &chk_label, &slow_label);

    ctx.current_block = chk_idx;
    let in_bounds = emit_u8_in_bounds(ctx, &raw, idx_i32);
    ctx.block().cond_br(&in_bounds, &load_label, &slow_label);

    ctx.current_block = load_idx;
    let ptr = emit_u8_byte_ptr(ctx, &raw, idx_i32);
    let (fast_val, fast_end) = {
        let blk = ctx.block();
        let byte = blk.load(I8, &ptr);
        let val = blk.zext(I8, &byte, I32);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = slow_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let val = blk.call(I32, slow_fn, &[(I64, &raw), (I32, idx_i32)]);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_val.as_str(), fast_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    )
}

/// Guarded inline byte read yielding a JS value: the byte as a Number in
/// bounds, else `slow_fn(handle, idx) -> double` (`js_uint8array_index_get_value`
/// / `js_buffer_index_get_value`, which answer `undefined` out of range).
pub(crate) fn emit_u8_cached_get_value(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_i32: &str,
    slow_fn: &str,
) -> String {
    let chk_idx = ctx.new_block("u8c.getv.chk");
    let load_idx = ctx.new_block("u8c.getv.load");
    let slow_idx = ctx.new_block("u8c.getv.slow");
    let merge_idx = ctx.new_block("u8c.getv.merge");
    let chk_label = ctx.block_label(chk_idx);
    let load_label = ctx.block_label(load_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let (hit, raw) = emit_u8_cache_admission(ctx, obj_box);
    ctx.block().cond_br(&hit, &chk_label, &slow_label);

    ctx.current_block = chk_idx;
    let in_bounds = emit_u8_in_bounds(ctx, &raw, idx_i32);
    ctx.block().cond_br(&in_bounds, &load_label, &slow_label);

    ctx.current_block = load_idx;
    let ptr = emit_u8_byte_ptr(ctx, &raw, idx_i32);
    let (fast_val, fast_end) = {
        let blk = ctx.block();
        let byte = blk.load(I8, &ptr);
        let val = blk.uitofp(I8, &byte, DOUBLE);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = slow_idx;
    let fallback_idx = ctx.new_block("u8c.getv.fallback");
    let fallback_label = ctx.block_label(fallback_idx);
    let (view_val, view_end) = emit_u8_view_get_value(
        ctx,
        obj_box,
        &raw,
        idx_i32,
        &fallback_label,
        &merge_label,
        None,
    );
    ctx.current_block = fallback_idx;
    let (slow_val, slow_end) = {
        let blk = ctx.block();
        let val = blk.call(DOUBLE, slow_fn, &[(I64, &raw), (I32, idx_i32)]);
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (fast_val.as_str(), fast_end.as_str()),
            (view_val.as_str(), view_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    )
}

/// #10515: the byte store of an untyped `obj[i] = v` whose receiver is an
/// admitted `Uint8Array` (a `PERRY_U8_INLINE_CACHE` hit: a live, owning,
/// inline-storage byte view). The same facts the runtime's
/// `cached_u8_index_set` checks after its receiver-classification ladder, read
/// here first, from the receiver's address and the operands:
///
/// * the index is a number in `[0, 2^31)` that is an exact integer and below
///   the buffer's `u32` length at offset 0 (an integer-valued double is its
///   own canonical index; `-0` stores at 0, as `ToPropertyKey(-0)` is `"0"`);
/// * the value is a plain number (no NaN-box tag, so `ToNumber` runs no user
///   code) in `(-2^31, 2^31)`, whose truncation's low byte is `ToUint8`.
///
/// Anything else, including NaN, an infinity or a larger magnitude (whose
/// `ToUint8` needs the modulo), runs `slow` — the complete `[[Set]]`.
pub(crate) fn emit_u8_cached_dyn_set(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
    slow: impl FnOnce(&mut FnCtx<'_>),
) {
    let chk_idx = ctx.new_block("u8d.set.chk");
    let store_idx = ctx.new_block("u8d.set.store");
    let slow_idx = ctx.new_block("u8d.set.slow");
    let merge_idx = ctx.new_block("u8d.set.merge");
    let chk_label = ctx.block_label(chk_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let (hit, raw) = emit_u8_cache_admission(ctx, obj_box);
    {
        let blk = ctx.block();
        // Range tests before any `fptosi`, whose out-of-range result is poison.
        let idx_ge0 = blk.fcmp("oge", idx_d, "0.0");
        let idx_lt = blk.fcmp("olt", idx_d, "2147483648.0");
        let val_bits = blk.bitcast_double_to_i64(val_double);
        // 0x7FF9 << 48: the lowest NaN-box tag.
        let val_is_number = blk.icmp_slt(I64, &val_bits, "9221401712017801216");
        let val_lt = blk.fcmp("olt", val_double, "2147483648.0");
        let val_gt = blk.fcmp("ogt", val_double, "-2147483648.0");
        let g = blk.and(I1, &hit, &idx_ge0);
        let g = blk.and(I1, &g, &idx_lt);
        let g = blk.and(I1, &g, &val_is_number);
        let g = blk.and(I1, &g, &val_lt);
        let g = blk.and(I1, &g, &val_gt);
        blk.cond_br(&g, &chk_label, &slow_label);
    }

    ctx.current_block = chk_idx;
    let (idx_i32, val_i32) = {
        let blk = ctx.block();
        let idx_i32 = blk.fptosi(DOUBLE, idx_d, I32);
        let val_i32 = blk.fptosi(DOUBLE, val_double, I32);
        (idx_i32, val_i32)
    };
    let ok = {
        let in_bounds = emit_u8_in_bounds(ctx, &raw, &idx_i32);
        let blk = ctx.block();
        let idx_back = blk.sitofp(I32, &idx_i32, DOUBLE);
        let is_int = blk.fcmp("oeq", &idx_back, idx_d);
        blk.and(I1, &is_int, &in_bounds)
    };
    ctx.block().cond_br(&ok, &store_label, &slow_label);

    ctx.current_block = store_idx;
    let ptr = emit_u8_byte_ptr(ctx, &raw, &idx_i32);
    {
        let blk = ctx.block();
        let byte = blk.trunc(I32, &val_i32, I8);
        blk.store(I8, &byte, &ptr);
        blk.br(&merge_label);
    }

    ctx.current_block = slow_idx;
    slow(ctx);
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
}

/// Guarded inline byte WRITE in the runtime helper's i32 ABI: `val_i32` is
/// already ToInt32'd by the caller, and the store keeps its low byte exactly
/// as `js_buffer_set` does (`value & 0xFF`). Misses call
/// `slow_fn(handle, idx, value)` (`js_uint8array_set` / `js_buffer_set`).
pub(crate) fn emit_u8_cached_set_i32(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_i32: &str,
    val_i32: &str,
    slow_fn: &str,
) {
    let chk_idx = ctx.new_block("u8c.set.chk");
    let store_idx = ctx.new_block("u8c.set.store");
    let slow_idx = ctx.new_block("u8c.set.slow");
    let merge_idx = ctx.new_block("u8c.set.merge");
    let chk_label = ctx.block_label(chk_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let (hit, raw) = emit_u8_cache_admission(ctx, obj_box);
    ctx.block().cond_br(&hit, &chk_label, &slow_label);

    ctx.current_block = chk_idx;
    let in_bounds = emit_u8_in_bounds(ctx, &raw, idx_i32);
    ctx.block().cond_br(&in_bounds, &store_label, &slow_label);

    ctx.current_block = store_idx;
    let ptr = emit_u8_byte_ptr(ctx, &raw, idx_i32);
    {
        let blk = ctx.block();
        let byte = blk.trunc(I32, val_i32, I8);
        blk.store(I8, &byte, &ptr);
        blk.br(&merge_label);
    }

    ctx.current_block = slow_idx;
    {
        let blk = ctx.block();
        blk.call_void(slow_fn, &[(I64, &raw), (I32, idx_i32), (I32, val_i32)]);
        blk.br(&merge_label);
    }
    ctx.current_block = merge_idx;
}

/// The view miss arm. A POINTER-tagged, aligned heap receiver must carry a
/// byte-view brand and the pointer-storage flag before its private payload is
/// read. Symbols lack GC headers, so screen their payload magic as well.
/// No calls, allocations or user expressions occur in this window.
pub(crate) fn emit_u8_view_data_guard(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    raw: &str,
    miss_label: &str,
) -> String {
    let header_idx = ctx.new_block("u8v.header");
    let data_idx = ctx.new_block("u8v.data");
    let header_label = ctx.block_label(header_idx);
    let data_label = ctx.block_label(data_idx);
    let floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(obj_box);
        let tag = blk.and(I64, &bits, &i64_literal(crate::nanbox::TAG_MASK));
        let is_ptr = blk.icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_I64);
        let above = blk.icmp_uge(I64, raw, &floor);
        let below = blk.icmp_ult(I64, raw, &ceiling);
        let low = blk.and(I64, raw, "7");
        let aligned = blk.icmp_eq(I64, &low, "0");
        let ok = blk.and(I1, &is_ptr, &above);
        let ok = blk.and(I1, &ok, &below);
        let ok = blk.and(I1, &ok, &aligned);
        blk.cond_br(&ok, &header_label, miss_label);
    }
    ctx.current_block = header_idx;
    {
        let blk = ctx.block();
        let type_addr = blk.sub(I64, raw, &crate::runtime_abi::GC_HEADER_SIZE.to_string());
        let type_ptr = blk.inttoptr(I64, &type_addr);
        let kind = blk.load(I8, &type_ptr);
        let node = blk.icmp_eq(I8, &kind, &crate::runtime_abi::GC_TYPE_BUFFER.to_string());
        let u8 = blk.icmp_eq(
            I8,
            &kind,
            &crate::runtime_abi::GC_TYPE_BUFFER_UINT8ARRAY.to_string(),
        );
        let byte_brand = blk.or(I1, &node, &u8);
        let reserved_addr = blk.add(I64, &type_addr, "2");
        let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
        let reserved = blk.load(I16, &reserved_ptr);
        let flag = blk.and(
            I16,
            &reserved,
            &crate::runtime_abi::GC_BUFFER_VIEW_DATA.to_string(),
        );
        let pointer_layout = blk.icmp_ne(I16, &flag, "0");
        let payload = blk.inttoptr(I64, raw);
        let first = blk.load(I32, &payload);
        let not_symbol = blk.icmp_ne(
            I32,
            &first,
            &crate::runtime_abi::SYMBOL_HEADER_MAGIC.to_string(),
        );
        let ok = blk.and(I1, &byte_brand, &pointer_layout);
        let ok = blk.and(I1, &ok, &not_symbol);
        blk.cond_br(&ok, &data_label, miss_label);
    }
    ctx.current_block = data_idx;
    emit_u8_view_data_load(ctx, raw)
}

/// Only called after the view-layout guard. Loading PTR keeps this native
/// pointer slot correct on ILP32 targets too.
pub(crate) fn emit_u8_view_data_load(ctx: &mut FnCtx<'_>, raw: &str) -> String {
    let blk = ctx.block();
    let slot = blk.add(
        I64,
        raw,
        &crate::runtime_abi::BUFFER_VIEW_DATA_OFFSET.to_string(),
    );
    let slot = blk.inttoptr(I64, &slot);
    let data = blk.load(PTR, &slot);
    blk.ptrtoint(&data, I64)
}

fn emit_u8_view_get_value(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    raw: &str,
    idx: &str,
    miss_label: &str,
    merge_label: &str,
    param_access: Option<&crate::collectors::ByteViewParamAccess>,
) -> (String, String) {
    let data = emit_u8_view_param_or_guard(ctx, obj_box, raw, miss_label, param_access);
    let load_idx = ctx.new_block("u8v.load");
    let oob_idx = ctx.new_block("u8v.oob");
    let done_idx = ctx.new_block("u8v.done");
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let done_label = ctx.block_label(done_idx);
    let bounds = emit_u8_in_bounds(ctx, raw, idx);
    ctx.block().cond_br(&bounds, &load_label, &oob_label);
    ctx.current_block = load_idx;
    let offset = ctx.block().zext(I32, idx, I64);
    let addr = ctx.block().add(I64, &data, &offset);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let target = ctx.target_triple.to_owned();
    let val = emit_u8_atomic_load_f64(ctx.block(), &target, &ptr);
    let load_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = oob_idx;
    let oob_end = ctx.block().label.clone();
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    let undef = double_literal(f64::from_bits(TAG_UNDEFINED));
    let value = ctx
        .block()
        .phi(DOUBLE, &[(&val, &load_end), (&undef, &oob_end)]);
    let end = ctx.block().label.clone();
    ctx.block().br(merge_label);
    (value, end)
}

/// A relaxed byte load is one indivisible memory operation. On x86 LLVM 22
/// lowers `uitofp (load atomic i8)` to a partial-register load followed by
/// zero extension, making the byte load depend on the preceding length load.
/// MOVZX reads exactly one byte and clears the destination in one instruction.
/// Its memory clobber and sideeffect retain concurrent reads; no fence is
/// required for relaxed ordering on x86. Other targets keep LLVM atomics.
pub(crate) fn emit_u8_atomic_load_f64(
    blk: &mut crate::block::LlBlock,
    target: &str,
    ptr: &str,
) -> String {
    let byte = if target.starts_with("x86_64") {
        let byte = blk.next_reg();
        blk.emit_raw(format!(
            "{byte} = call i32 asm sideeffect \"movzbl ($1), $0\", \"=r,r,~{{memory}}\"(ptr {ptr}) \"gc-leaf-function\""
        ));
        byte
    } else {
        let byte = blk.load_atomic_monotonic(I8, ptr, 1);
        blk.zext(I8, &byte, I32)
    };
    blk.uitofp(I32, &byte, DOUBLE)
}

pub(crate) fn byte_view_param_for(
    ctx: &FnCtx<'_>,
    object: &Expr,
) -> Option<crate::collectors::ByteViewParamAccess> {
    match object {
        Expr::LocalGet(id) => ctx.receiver_descriptors.byte_view_param(*id).cloned(),
        _ => None,
    }
}

/// Amortize entry validation only for indexed reads in loops. Single-read
/// helpers retain their existing owning-cache hit and add no entry calls.
/// Nested closures get their own receiver guards when they are lowered.
pub(crate) fn byte_view_param_is_read(body: &[perry_hir::Stmt], id: u32) -> bool {
    u8_inline_read_enabled() && loop_param_is_read(body, id)
}

pub(crate) fn loop_param_is_read(body: &[perry_hir::Stmt], id: u32) -> bool {
    fn reads(expr: &Expr, id: u32) -> bool {
        if matches!(expr, Expr::Closure { .. }) {
            return false;
        }
        if matches!(expr,
            Expr::IndexGet { object, .. } | Expr::Uint8ArrayGet { array: object, .. }
                if matches!(object.as_ref(), Expr::LocalGet(receiver) if *receiver == id))
        {
            return true;
        }
        let mut found = false;
        perry_hir::walker::walk_expr_children(expr, &mut |child| found |= reads(child, id));
        found
    }
    fn loop_reads(stmt: &perry_hir::Stmt, id: u32) -> bool {
        use perry_hir::Stmt;
        let any_read = |body: &[Stmt]| {
            body.iter()
                .any(|stmt| perry_hir::walker::stmt_any_expr(stmt, &mut |expr| reads(expr, id)))
        };
        match stmt {
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                reads(condition, id) || any_read(body)
            }
            Stmt::For {
                condition,
                update,
                body,
                ..
            } => {
                condition.as_ref().is_some_and(|expr| reads(expr, id))
                    || update.as_ref().is_some_and(|expr| reads(expr, id))
                    || any_read(body)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                then_branch.iter().any(|stmt| loop_reads(stmt, id))
                    || else_branch
                        .as_ref()
                        .is_some_and(|branch| branch.iter().any(|stmt| loop_reads(stmt, id)))
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().any(|stmt| loop_reads(stmt, id))
                    || catch
                        .as_ref()
                        .is_some_and(|clause| clause.body.iter().any(|stmt| loop_reads(stmt, id)))
                    || finally
                        .as_ref()
                        .is_some_and(|body| body.iter().any(|stmt| loop_reads(stmt, id)))
            }
            Stmt::Switch { cases, .. } => cases
                .iter()
                .any(|case| case.body.iter().any(|stmt| loop_reads(stmt, id))),
            Stmt::Labeled { body, .. } => loop_reads(body, id),
            _ => false,
        }
    }
    body.iter().any(|stmt| loop_reads(stmt, id))
}

pub(crate) fn materialize_byte_view_param(ctx: &mut FnCtx<'_>, id: u32, boxed: &str) {
    let data_i64 = ctx
        .block()
        .call(I64, "js_u8_resolve_read_data", &[(DOUBLE, boxed)]);
    let valid_i1 = ctx.block().icmp_ne(I64, &data_i64, "0");
    ctx.receiver_descriptors.materialize_byte_view_param(
        id,
        crate::collectors::ByteViewParamAccess { valid_i1, data_i64 },
    );
}

pub(crate) fn emit_u8_view_param_or_guard(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    raw: &str,
    miss_label: &str,
    param_access: Option<&crate::collectors::ByteViewParamAccess>,
) -> String {
    if let Some(access) = param_access {
        let hit_idx = ctx.new_block("u8v.param.hit");
        let hit_label = ctx.block_label(hit_idx);
        ctx.block()
            .cond_br(&access.valid_i1, &hit_label, miss_label);
        ctx.current_block = hit_idx;
        access.data_i64.clone()
    } else {
        emit_u8_view_data_guard(ctx, obj_box, raw, miss_label)
    }
}

#[cfg(test)]
mod byte_loop_proof_tests {
    use super::*;
    use perry_hir::Stmt;

    #[test]
    fn relaxed_byte_reads_are_one_byte_and_zero_extended() {
        use crate::block::{LlBlock, RegCounter};
        use std::rc::Rc;
        for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
            let mut blk = LlBlock::new("entry.0", Rc::new(RegCounter::new()));
            emit_u8_atomic_load_f64(&mut blk, target, "%data");
            let ir = blk.to_ir();
            if target.starts_with("x86_64") {
                assert!(ir.contains("movzbl ($1), $0"));
                assert!(ir.contains("asm sideeffect") && ir.contains("~{memory}"));
                assert!(ir.contains("gc-leaf-function"));
            } else {
                assert!(ir.contains("load atomic i8, ptr %data monotonic, align 1"));
                assert!(!ir.contains("asm"));
            }
            assert!(ir.contains("uitofp i32"));
            assert!(!ir.contains("load i32"));
        }
    }

    #[test]
    fn entry_resolution_requires_a_loop_read_of_this_parameter() {
        let read = Stmt::Expr(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
        });
        assert!(!byte_view_param_is_read(&[read.clone()], 1));
        let loop_read = Stmt::For {
            init: None,
            condition: None,
            update: None,
            body: vec![read.clone()],
        };
        assert!(byte_view_param_is_read(&[loop_read.clone()], 1));
        assert!(!byte_view_param_is_read(&[loop_read], 2));
        let init_only = Stmt::For {
            init: Some(Box::new(read)),
            condition: None,
            update: None,
            body: vec![],
        };
        assert!(!byte_view_param_is_read(&[init_only], 1));
        // The HIR specializes `bytes[i]` to Uint8ArrayGet before codegen.
        let native_read = Stmt::Expr(Expr::Uint8ArrayGet {
            array: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
        });
        assert!(!byte_view_param_is_read(&[native_read.clone()], 1));
        assert!(byte_view_param_is_read(
            &[Stmt::For {
                init: None,
                condition: None,
                update: None,
                body: vec![native_read],
            }],
            1
        ));
    }
}
