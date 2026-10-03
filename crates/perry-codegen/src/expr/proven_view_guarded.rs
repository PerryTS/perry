//! **Guarded** element access for storage-proven typed-array views: any key
//! that is a Number, a BigInt or `undefined`, and any value, with one run-time
//! test in front of the bare element load or store.
//!
//! `expr/proven_view_access.rs` serves a proven view only when the index is
//! statically an exact i32 (`index_is_exact_i32_shape`) and, for an integer
//! kind, the stored value is statically ToInt32-exact. A key read back from a
//! typed array (`let k = perm[0]; perm[k]`), a decremented copy of one, or a
//! stored element (`perm[i] = perm[j]`) is none of those, because an element
//! read is `undefined` when out of bounds. Those accesses used to leave the
//! proven tier for the kind-cache path or a runtime call. Here the receiver
//! keeps everything the proven tier knows (kind, data pointer, immutable
//! length) and the index and value are tested at run time:
//!
//! ```text
//!   in  = d >= 0 && d < len              ; ordered: NaN and every NaN-box fail
//!   i   = fptosi(in ? d : 0)             ; never poison
//!   ok  = in && sitofp(i) == d           ; exact integer; -0 is element 0
//!   (store) ok &&= value is a plain double or `undefined`
//!   ok:   bare element load / store      (ToInt32 or float store, exact)
//!   else: js_dyn_index_get / js_dyn_index_set_strict, the complete [[Get]] /
//!         [[Set]]: out of bounds, fractional, NaN, negative, `undefined`
//!         keys, and values needing a full ToNumber
//! ```
//!
//! ## Why the exit cannot invalidate the cached pointer
//!
//! The view caches its data pointer, and observing `.buffer` rebinds a plain
//! typed array onto a fresh `ArrayBuffer`, after which that pointer is stale.
//! A runtime-key access whose key could be the string `"buffer"` (or an
//! object converting to it) therefore invalidates the view in `index_get.rs`.
//! This tier admits only keys proven to be a Number, a BigInt or `undefined`
//! (`collectors/numeric_key_locals.rs`). A numeric key is an integer index the
//! typed array answers itself, never via its prototype; `undefined` is the
//! ordinary key `"undefined"`, the same class as `ta.foo`. None of them names
//! the `buffer` getter, so the exit leaves the view valid.
//!
//! ## Value semantics
//!
//! TypedArraySetElement converts the value with ToNumber before the bounds
//! test. For a plain double and for `undefined` (NaN) that conversion has no
//! side effects, so doing it inline and handing the ORIGINAL value to the
//! runtime on a miss converts it exactly once on every path. Any other value
//! (a string, boolean, object with `valueOf`, BigInt, an int32 box) takes the
//! runtime path before anything is converted. Integer kinds store ToInt32 of
//! the Number (`undefined` and NaN store 0), float kinds store the Number
//! (`undefined` stores NaN). Uint8ClampedArray keeps its runtime clamp.

use anyhow::Result;
use perry_hir::Expr;

use super::proven_view_access::{
    emit_elem_load_f64, emit_proven_view_store, load_data_and_len, proven_view_receiver,
};
use super::{attach_buffer_view_facts, is_numeric_expr, FnCtx};
use crate::collectors::numeric_key_locals::{expr_is_numeric_key, KeyLeaves};
use crate::nanbox::TAG_UNDEFINED;
use crate::native_value::{BoundsState, BufferAccessMode, BufferElem, LoweredValue};
use crate::rooting;
use crate::types::{DOUBLE, I1, I32, I64};

/// The lowest NaN-box tag (`0x7FF9 << 48`). A signed compare below it admits
/// every plain double, negative values and the canonical NaN included.
const LOWEST_NANBOX_TAG: &str = "9221401712017801216";
/// The canonical quiet NaN, `ToNumber(undefined)`.
const CANONICAL_NAN_BITS: u64 = 0x7FF8_0000_0000_0000;

fn undefined_literal() -> String {
    crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
}

/// `index` always evaluates to a Number, a BigInt or `undefined`.
pub(crate) fn index_is_numeric_key(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    let number = |e: &Expr| is_numeric_expr(ctx, e);
    let key_local = |id: u32| ctx.native_facts.numeric_key_locals().contains(&id);
    let owned_view = |id: u32| ctx.known_noalias_buffer_locals.contains(&id);
    expr_is_numeric_key(
        index,
        &KeyLeaves {
            number: &number,
            key_local: &key_local,
            owned_view: &owned_view,
        },
    )
}

/// The receiver and key gates shared by the load and the store.
fn guarded_view_for(
    ctx: &FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Option<(u32, crate::native_value::BufferViewSlot)> {
    let (id, view) = proven_view_receiver(ctx, object)?;
    index_is_numeric_key(ctx, index).then_some((id, view))
}

/// Emit the exact in-bounds index test on `idx_d` (any JS value as a double)
/// and return `(ok, idx_i32)`. `idx_i32` is meaningful only where `ok` holds.
fn emit_exact_index_test(ctx: &mut FnCtx<'_>, idx_d: &str, len: &str) -> (String, String) {
    let blk = ctx.block();
    let len_d = blk.uitofp(I32, len, DOUBLE);
    let ge0 = blk.fcmp("oge", idx_d, "0.0");
    let lt_len = blk.fcmp("olt", idx_d, &len_d);
    let in_range = blk.and(I1, &ge0, &lt_len);
    // `fptosi` of an out-of-range double is poison; convert only a value the
    // range test admitted.
    let safe = blk.select(I1, &in_range, DOUBLE, idx_d, "0.0");
    let idx_i32 = blk.fptosi(DOUBLE, &safe, I32);
    let back = blk.sitofp(I32, &idx_i32, DOUBLE);
    let exact = blk.fcmp("oeq", &back, idx_d);
    let ok = blk.and(I1, &in_range, &exact);
    (ok, idx_i32)
}

/// `object[index]` on a proven view with a numeric-key index. Returns the JS
/// value double.
pub(crate) fn try_lower_proven_view_guarded_load(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    let Some((id, view)) = guarded_view_for(ctx, object, index) else {
        return Ok(None);
    };
    rooting::with_operands_rooted(ctx, &[object, index], |ctx, vals| {
        if ctx.block().is_terminated() {
            return Ok(Some(undefined_literal()));
        }
        let (obj_box, idx_d) = (&vals[0], &vals[1]);
        let (data_ptr, len) = load_data_and_len(ctx, &view);
        let (ok, idx_i32) = emit_exact_index_test(ctx, idx_d, &len);

        let load_idx = ctx.new_block("pview.gget.load");
        let slow_idx = ctx.new_block("pview.gget.slow");
        let merge_idx = ctx.new_block("pview.gget.merge");
        let load_label = ctx.block_label(load_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&ok, &load_label, &slow_label);

        ctx.current_block = load_idx;
        let load_val = emit_elem_load_f64(ctx, &view, &data_ptr, &idx_i32);
        let load_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = slow_idx;
        let slow_val = ctx.block().call(
            DOUBLE,
            "js_dyn_index_get",
            &[(DOUBLE, obj_box), (DOUBLE, idx_d)],
        );
        let slow_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        let result = ctx.block().phi(
            DOUBLE,
            &[
                (load_val.as_str(), load_end.as_str()),
                (slow_val.as_str(), slow_end.as_str()),
            ],
        );
        let lowered = LoweredValue::js_value(result.clone());
        ctx.record_lowered_value_with_access_mode(
            "TypedArrayGet",
            Some(id),
            "TypedArrayGet.proven_view_guarded",
            &lowered,
            Some(BoundsState::Guarded {
                guard_id: "proven_view_guarded_index".to_string(),
            }),
            Some(view.alias.clone()),
            Some(BufferAccessMode::CheckedNative),
            None,
            false,
            false,
            vec!["proven_view=guarded_inline; exit=js_dyn_index_get".to_string()],
        );
        attach_buffer_view_facts(ctx, &view);
        Ok(Some(result))
    })
}

/// `object[index] = value` on a proven view with a numeric-key index and any
/// value. Returns the assignment's value: the original `value`.
pub(crate) fn try_lower_proven_view_guarded_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    let Some((id, view)) = guarded_view_for(ctx, object, index) else {
        return Ok(None);
    };
    if matches!(view.elem, BufferElem::U8Clamped) {
        return Ok(None);
    }
    let value_is_number = is_numeric_expr(ctx, value);
    rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, vals| {
        // An operand that throws leaves the block terminated; open no blocks
        // on values dropped after the terminator (#11450).
        if ctx.block().is_terminated() {
            return Ok(Some(undefined_literal()));
        }
        let (obj_box, idx_d, val_d) = (&vals[0], &vals[1], &vals[2]);
        let (data_ptr, len) = load_data_and_len(ctx, &view);
        let (idx_ok, idx_i32) = emit_exact_index_test(ctx, idx_d, &len);
        let (ok, number) = if value_is_number {
            (idx_ok, val_d.clone())
        } else {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(val_d);
            let plain = blk.icmp_slt(I64, &bits, LOWEST_NANBOX_TAG);
            let undef = blk.icmp_eq(I64, &bits, &TAG_UNDEFINED.to_string());
            let convertible = blk.or(I1, &plain, &undef);
            let nan = crate::nanbox::double_literal(f64::from_bits(CANONICAL_NAN_BITS));
            let number = blk.select(I1, &undef, DOUBLE, &nan, val_d);
            (blk.and(I1, &idx_ok, &convertible), number)
        };

        let store_idx = ctx.new_block("pview.gset.store");
        let slow_idx = ctx.new_block("pview.gset.slow");
        let merge_idx = ctx.new_block("pview.gset.merge");
        let store_label = ctx.block_label(store_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&ok, &store_label, &slow_label);

        ctx.current_block = store_idx;
        let native = match view.elem {
            BufferElem::F32 | BufferElem::F64 => LoweredValue::f64(number),
            // ToInt32, then the store narrows: bit-identical to the runtime
            // `store_at`'s `to_uint32_bits(value) as <width>`.
            _ => LoweredValue::i32(ctx.toint32_wrap(&number)),
        };
        emit_proven_view_store(ctx, &view, &data_ptr, &idx_i32, &native);
        ctx.block().br(&merge_label);

        ctx.current_block = slow_idx;
        let strict_flag = if strict { "1" } else { "0" };
        ctx.block().call(
            DOUBLE,
            "js_dyn_index_set_strict",
            &[
                (DOUBLE, obj_box),
                (DOUBLE, idx_d),
                (DOUBLE, val_d),
                (I32, strict_flag),
            ],
        );
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        let lowered = LoweredValue::js_value(val_d.clone());
        ctx.record_lowered_value_with_access_mode(
            "TypedArraySet",
            Some(id),
            "TypedArraySet.proven_view_guarded",
            &lowered,
            Some(BoundsState::Guarded {
                guard_id: "proven_view_guarded_index".to_string(),
            }),
            Some(view.alias.clone()),
            Some(BufferAccessMode::CheckedNative),
            None,
            false,
            false,
            vec!["proven_view=guarded_inline; exit=js_dyn_index_set_strict".to_string()],
        );
        attach_buffer_view_facts(ctx, &view);
        Ok(Some(val_d.clone()))
    })
}
