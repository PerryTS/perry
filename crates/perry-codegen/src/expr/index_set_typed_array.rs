//! Guarded **inline** typed-array element store for a type-erased receiver.
//!
//! Split out of `index_set.rs` when that file crossed the 2000-line cap
//! (#7342 pushed it to 2035). Pure mechanical move — the two functions below
//! are verbatim, and `index_set.rs` calls them through a `use super::` path
//! exactly as it called them locally before.
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Listed in `crate::rooting`'s `MIGRATED_MODULES`, and the listing is
//! **vacuous on the committed source**: this module has never named an
//! `expr::temp_root` symbol, so only the sabotage arm makes the line an
//! assertion. The audit that earned it: both functions take the receiver, index
//! and value already lowered by the caller, and emit only pure IR plus the
//! out-of-line `js_dyn_index_set` fallback — no user expression is lowered
//! here, so no window opens. The receiver's exposure across the VALUE's
//! lowering is the caller's to close, and `index_set.rs`'s `#5525` arm is one
//! of the sites #7640 records as still open.

use anyhow::Result;

use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};

use super::FnCtx;

/// #5525 follow-up: guarded **inline** typed-array element STORE for an
/// `obj[i] = v` whose receiver static type is erased (`any`/unknown) but is, at
/// runtime, commonly an owning numeric typed array (bcryptjs's `P[i]=`/`S[i]=`
/// Int32Array boxes). Mirrors [`index_get::lower_inline_dyn_typed_array_get`]:
/// the same pointer / `PERRY_TA_KIND_CACHE` (tag = kind + storage) / index
/// guards, then a direct per-kind store into `header + 16 + idx*elem_size`,
/// falling back to `js_dyn_index_set` on any guard miss. The store result is the
/// assigned value (`val_double`), matching `js_dyn_index_set`'s return.
///
/// Only the kinds with a simple ToInt32/ToUint32 truncating store (Int8/Uint8/
/// Int16/Uint16/Int32/Uint32) or a direct float store (Float32/Float64) are
/// inlined — i.e. `kind <= KIND_FLOAT64` (7). Uint8ClampedArray (round-half-to-
/// even clamp), the BigInt kinds (ToBigInt / throw) and Float16 (f16 encode) are
/// excluded by the guard and defer to the runtime, which already owns them. The
/// integer truncation here (`toint32(value)` then narrow) is bit-identical to
/// the runtime `store_at`'s `to_uint32_bits(value) as <width>`; the float store
/// is identical to `store_at`'s direct slot write — so behavior matches the
/// existing runtime fast path exactly.
pub(super) fn lower_inline_dyn_typed_array_set(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
    strict: bool,
    array_arm: Option<DynArrayStoreFacts>,
) -> Result<String> {
    // As with the ordinary array store, an operand can throw before this
    // helper runs. Do not open fresh blocks using values dropped after the
    // terminator (#11450). No assignment value is consumed on this path.
    if ctx.block().is_terminated() {
        return Ok(crate::nanbox::double_literal(f64::from_bits(
            crate::nanbox::TAG_UNDEFINED,
        )));
    }
    let Some(facts) = array_arm else {
        return Ok(emit_inline_ta_set_then_runtime(
            ctx, obj_box, idx_d, val_double, strict,
        ));
    };
    // #10513: the ordinary-Array arm. An untyped `d[j] = v` onto a live plain
    // Array used to reach `js_dyn_index_set_strict` on EVERY store, which
    // re-classifies the receiver through the proxy / symbol / typed-array /
    // buffer-registry / collection / prototype / arguments ladder before the
    // array setter runs (~450 instructions per element on node-forge's jsbn
    // `am1`). The guarded in-bounds store the statically-typed receivers
    // already use (`index_set_guarded.rs`) proves everything that ladder
    // would conclude for this case from the receiver's own header: a
    // non-forwarded (or once-forwarded, healed inline) `GC_TYPE_ARRAY`, no
    // frozen/sealed/non-extensible/descriptor bits, the default prototype
    // chain, and a canonical index strictly below `length`. Every other
    // receiver and key — a Buffer, a string or Symbol key, an append, a
    // hole-creating sparse write — declines onto `js_dyn_index_set_strict`.
    //
    // The typed-array tier keeps its place in front: a receiver that hits the
    // #5525 kind cache (the only way that tier's fast arm is reachable) goes
    // there on one load and compare, so typed-array stores pay nothing for the
    // Array arm, and an Array pays only that compare for the typed-array one.
    let ta_idx = ctx.new_block("dynarr.ta");
    let array_idx = ctx.new_block("dynarr.array");
    let done_idx = ctx.new_block("dynarr.done");
    let ta_label = ctx.block_label(ta_idx);
    let array_label = ctx.block_label(array_idx);
    let done_label = ctx.block_label(done_idx);
    {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let entry_addr = blk.lshr(I64, &entry_val, "8");
        // A cache entry names a heap address, so a non-pointer box whose low
        // 48 bits collide with one still fails the full guard in `dynarr.ta`.
        let cached_typed_array = blk.icmp_eq(I64, &entry_addr, &raw);
        blk.cond_br(&cached_typed_array, &ta_label, &array_label);
    }
    ctx.current_block = ta_idx;
    let _ = emit_inline_ta_set_then_runtime(ctx, obj_box, idx_d, val_double, strict);
    ctx.block().br(&done_label);

    ctx.current_block = array_idx;
    super::index_set_guarded::emit_guarded_inbounds_array_store_keyed(
        ctx,
        obj_box,
        super::index_set_guarded::StoreIndex::CanonicalOfDouble(idx_d),
        val_double,
        "dynarr.set",
        facts.layout_note_needed,
        facts.write_barrier_needed,
        facts.value_is_numeric,
        |ctx| {
            emit_dyn_index_set_runtime(ctx, obj_box, idx_d, val_double, strict);
            Ok(())
        },
    )?;
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
    Ok(val_double.to_string())
}

/// The complete dynamic `[[Set]]`, preserving the source function's
/// assignment strictness, behind the byte-store arm for an admitted
/// `Uint8Array` (#10515, [`super::u8_buffer_read::emit_u8_cached_dyn_set`]).
fn emit_dyn_index_set_runtime(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
    strict: bool,
) {
    super::u8_buffer_read::emit_u8_cached_dyn_set(ctx, obj_box, idx_d, val_double, |ctx| {
        emit_dyn_index_set_full(ctx, obj_box, idx_d, val_double, strict)
    });
}

/// The runtime's complete dynamic `[[Set]]`.
fn emit_dyn_index_set_full(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
    strict: bool,
) {
    let strict = if strict { "1" } else { "0" };
    ctx.block().call(
        DOUBLE,
        "js_dyn_index_set_strict",
        &[
            (DOUBLE, obj_box),
            (DOUBLE, idx_d),
            (DOUBLE, val_double),
            (I32, strict),
        ],
    );
}

/// What the caller knows statically about an untyped `obj[i] = v` store's
/// VALUE, handed to the ordinary-Array arm of
/// [`lower_inline_dyn_typed_array_set`]. Same three facts, same predicates, as
/// every other caller of `emit_guarded_inbounds_array_store` passes.
#[derive(Clone, Copy, Debug)]
pub(super) struct DynArrayStoreFacts {
    pub layout_note_needed: bool,
    pub write_barrier_needed: bool,
    pub value_is_numeric: bool,
}

/// The #5525 guarded inline typed-array store, exiting to
/// `js_dyn_index_set_strict` on any guard miss. Returns the assignment's value.
fn emit_inline_ta_set_then_runtime(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
    strict: bool,
) -> String {
    let tag_mask = crate::nanbox::i64_literal(crate::nanbox::TAG_MASK);
    let pointer_tag = crate::nanbox::POINTER_TAG_I64;
    let pointer_mask = crate::nanbox::POINTER_MASK_I64;

    let fast_idx = ctx.new_block("tav.set.fast");
    let store_idx = ctx.new_block("tav.set.store");
    let slow_idx = ctx.new_block("tav.set.slow");
    let merge_idx = ctx.new_block("tav.set.merge");
    let fast_label = ctx.block_label(fast_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    // ---- entry: combined cache/kind/range guard -> fast | slow ----
    let entry_guard = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, pointer_mask);
        let tagged = blk.and(I64, &obj_bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, pointer_tag);
        // #10516: the kind-cache tag carries the receiver's storage: an
        // external-storage typed array (a view) caches `kind | 0x80`, so the
        // kind compare below rejects it. No process-wide view count.
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let entry_addr = blk.lshr(I64, &entry_val, "8");
        let addr_match = blk.icmp_eq(I64, &entry_addr, &raw);
        let kind = blk.and(I64, &entry_val, "255");
        // Stores inline only kinds with a trivial truncating/float store:
        // kind <= KIND_FLOAT64 (7). Uint8Clamped (8), BigInt (9/10), Float16
        // (11), and the 0xFF sentinel all defer to the runtime.
        let kind_ok = blk.icmp_ule(I64, &kind, "7");
        let idx_ge0 = blk.fcmp("oge", idx_d, "0.0");
        let idx_lt = blk.fcmp("olt", idx_d, "4294967296.0");
        // The store arms below apply ToNumber's identity only: a float kind
        // writes the double's bits and the integer kinds ToInt32 them. Any
        // NaN-boxed value — a string, boolean, null/undefined, object with
        // `valueOf`, or an int32-boxed number — needs the runtime's full
        // ToNumber first. Without this a warmed site stored the box's bits
        // raw (`f64[i] = true` read back `true`) and truncated every one of
        // them to 0 in an integer array. One signed compare admits every
        // plain double, negative values and NaN included.
        let val_bits = blk.bitcast_double_to_i64(val_double);
        // 0x7FF9 << 48: the lowest NaN-box tag.
        let val_is_plain_number = blk.icmp_slt(I64, &val_bits, "9221401712017801216");
        let g = blk.and(I1, &is_ptr, &addr_match);
        let g = blk.and(I1, &g, &kind_ok);
        let g = blk.and(I1, &g, &idx_ge0);
        let g = blk.and(I1, &g, &val_is_plain_number);
        blk.and(I1, &g, &idx_lt)
    };
    ctx.block().cond_br(&entry_guard, &fast_label, &slow_label);

    // ---- fast: validate integer index + bounds -> store | slow ----
    ctx.current_block = fast_idx;
    let (raw, idx_i64, kind) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, pointer_mask);
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let kind = blk.and(I64, &entry_val, "255");
        let idx_i64 = blk.fptosi(DOUBLE, idx_d, I64);
        (raw, idx_i64, kind)
    };
    let fast_ok = {
        let blk = ctx.block();
        let idx_back = blk.sitofp(I64, &idx_i64, DOUBLE);
        let is_int = blk.fcmp("oeq", &idx_back, idx_d);
        let hdr_ptr = blk.inttoptr(I64, &raw);
        let len = blk.load(I32, &hdr_ptr);
        let len_i64 = blk.zext(I32, &len, I64);
        let in_bounds = blk.icmp_ult(I64, &idx_i64, &len_i64);
        blk.and(I1, &is_int, &in_bounds)
    };
    ctx.block().cond_br(&fast_ok, &store_label, &slow_label);

    // ---- store: per-kind direct element store (data = header + 16) ----
    ctx.current_block = store_idx;
    let data_base = {
        let blk = ctx.block();
        blk.add(I64, &raw, "16")
    };
    // ToInt32 of the value once (shared by all integer kinds). For float kinds
    // we use the raw double directly. `toint32_wrap` matches the runtime
    // `to_uint32_bits` for EVERY finite value (NaN/±Inf/±0 → 0, else
    // trunc-toward-zero mod 2^32); the unwrapped `toint32` is poison for
    // |v| >= 2^63, so `u32[i] = 1e300` stored garbage instead of 0.
    let val_i32 = ctx.toint32_wrap(val_double);

    let b_i8 = ctx.new_block("tav.s.i8");
    let b_u8 = ctx.new_block("tav.s.u8");
    let b_i16 = ctx.new_block("tav.s.i16");
    let b_u16 = ctx.new_block("tav.s.u16");
    let b_i32 = ctx.new_block("tav.s.i32");
    let b_u32 = ctx.new_block("tav.s.u32");
    let b_f32 = ctx.new_block("tav.s.f32");
    let b_f64 = ctx.new_block("tav.s.f64");
    let l_i8 = ctx.block_label(b_i8);
    let l_u8 = ctx.block_label(b_u8);
    let l_i16 = ctx.block_label(b_i16);
    let l_u16 = ctx.block_label(b_u16);
    let l_i32 = ctx.block_label(b_i32);
    let l_u32 = ctx.block_label(b_u32);
    let l_f32 = ctx.block_label(b_f32);
    let l_f64 = ctx.block_label(b_f64);

    // Dispatch chain on `kind` (in the store block, after data_base/val_i32).
    let chk = |ctx: &mut FnCtx<'_>, k: &str, hit: &str, next_idx: usize| {
        let next_label = ctx.block_label(next_idx);
        let cond = ctx.block().icmp_eq(I64, &kind, k);
        ctx.block().cond_br(&cond, hit, &next_label);
    };
    let c1 = ctx.new_block("tav.sd1");
    let c2 = ctx.new_block("tav.sd2");
    let c3 = ctx.new_block("tav.sd3");
    let c4 = ctx.new_block("tav.sd4");
    let c5 = ctx.new_block("tav.sd5");
    let c6 = ctx.new_block("tav.sd6");
    chk(ctx, "0", &l_i8, c1);
    ctx.current_block = c1;
    chk(ctx, "1", &l_u8, c2);
    ctx.current_block = c2;
    chk(ctx, "2", &l_i16, c3);
    ctx.current_block = c3;
    chk(ctx, "3", &l_u16, c4);
    ctx.current_block = c4;
    chk(ctx, "4", &l_i32, c5);
    ctx.current_block = c5;
    chk(ctx, "5", &l_u32, c6);
    ctx.current_block = c6;
    // remaining: kind 6 → f32, else (7) → f64.
    let is_f32 = ctx.block().icmp_eq(I64, &kind, "6");
    ctx.block().cond_br(&is_f32, &l_f32, &l_f64);

    // Per-kind stores. Each: off = idx << shift; addr = data_base + off;
    // store narrowed value; br merge.
    emit_inline_ta_int_store(
        ctx,
        b_i8,
        &idx_i64,
        &data_base,
        &merge_label,
        "0",
        &val_i32,
        I8,
    );
    emit_inline_ta_int_store(
        ctx,
        b_u8,
        &idx_i64,
        &data_base,
        &merge_label,
        "0",
        &val_i32,
        I8,
    );
    emit_inline_ta_int_store(
        ctx,
        b_i16,
        &idx_i64,
        &data_base,
        &merge_label,
        "1",
        &val_i32,
        I16,
    );
    emit_inline_ta_int_store(
        ctx,
        b_u16,
        &idx_i64,
        &data_base,
        &merge_label,
        "1",
        &val_i32,
        I16,
    );
    emit_inline_ta_int_store(
        ctx,
        b_i32,
        &idx_i64,
        &data_base,
        &merge_label,
        "2",
        &val_i32,
        I32,
    );
    emit_inline_ta_int_store(
        ctx,
        b_u32,
        &idx_i64,
        &data_base,
        &merge_label,
        "2",
        &val_i32,
        I32,
    );
    // F32: fptrunc the double to float, store.
    {
        ctx.current_block = b_f32;
        let blk = ctx.block();
        let off = blk.shl(I64, &idx_i64, "2");
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        let f = blk.fptrunc(DOUBLE, val_double, F32);
        blk.store(F32, &f, &ptr);
        blk.br(&merge_label);
    }
    // F64: store the double raw.
    {
        ctx.current_block = b_f64;
        let blk = ctx.block();
        let off = blk.shl(I64, &idx_i64, "3");
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        blk.store(DOUBLE, val_double, &ptr);
        blk.br(&merge_label);
    }

    // ---- slow: preserve the source function's assignment strictness ----
    ctx.current_block = slow_idx;
    emit_dyn_index_set_runtime(ctx, obj_box, idx_d, val_double, strict);
    ctx.block().br(&merge_label);

    // ---- merge: assignment yields the stored value on every path ----
    ctx.current_block = merge_idx;
    // All paths produce `val_double` as the expression result (matching
    // `js_dyn_index_set`'s `return value`), so no phi is needed.
    val_double.to_string()
}

/// Emit one per-kind integer typed-array element store block for
/// [`lower_inline_dyn_typed_array_set`]: switches to `blk_idx`, computes the
/// element address (`data_base + (idx << shift)`), narrows the shared
/// ToInt32-coerced `val_i32` to `elem_ty`, stores it, and branches to
/// `merge_label`.
#[allow(clippy::too_many_arguments)]
fn emit_inline_ta_int_store(
    ctx: &mut FnCtx<'_>,
    blk_idx: usize,
    idx_i64: &str,
    data_base: &str,
    merge_label: &str,
    shift: &str,
    val_i32: &str,
    elem_ty: crate::types::LlvmType,
) {
    ctx.current_block = blk_idx;
    let blk = ctx.block();
    let off = blk.shl(I64, idx_i64, shift);
    let addr = blk.add(I64, data_base, &off);
    let ptr = blk.inttoptr(I64, &addr);
    let narrowed = if elem_ty == I32 {
        val_i32.to_string()
    } else {
        blk.trunc(I32, val_i32, elem_ty)
    };
    blk.store(elem_ty, &narrowed, &ptr);
    blk.br(merge_label);
}
