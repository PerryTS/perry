//! The static-key store `o.k = v` (object model step 5, phase 2,
//! `OBJECT_MODEL_2026-10-13.md` §3.7): the 2c colour-slot arm, then the
//! store probe and the complete `[[Set]]`. The site holds no state.
//!
//! Every static-key, same-receiver store that no earlier specialisation took
//! (class field, POD record, scalar-replaced local, array index or length)
//! reaches [`emit_static_store_ic`], whatever its right-hand side:
//!
//! ```text
//!   [colour slot arm, `expr/layout_read.rs`: the inline store]
//!   perry_store_or_put(dir, obj, key, hash, v, strict)
//!        (the store probe: an own plain writable data slot or overflow
//!        entry, one barriered store; else the complete `[[Set]]`: the chain
//!        walk for setters and non-writables, the key add)
//! ```
//!
//! Only the arm's store and its value test sit at the site. The GC
//! obligations of a pointer-carrying value (string alias demotion, the
//! write barrier and its parent gate) are one out-of-line call
//! (`js_slot_store_obligations`), and the probe and `[[Set]]` one more.
//!
//! # Evaluation order: the RHS may collect, so the receiver is read after it
//!
//! The caller evaluates the target FIRST (the reference is evaluated before
//! the value, ECMA-262 §13.15.2), keeps it in an operand root across the RHS
//! (`with_operands_rooted_across`), and hands this emitter the value RE-READ
//! from that root after the RHS. Nothing between that re-read and the probe's
//! store can collect, so the receiver the probe reads is the CURRENT one.

use crate::types::{DOUBLE, I32, I64, PTR};

use super::write_barrier::emit_may_carry_heap_pointer_check;
use super::FnCtx;

/// Emit the static-key store. `obj_box` is the receiver RE-READ after the
/// RHS; `value_double` is the stored value and `value_bits` the same value as
/// the caller lowered it to i64 (used only to recognise an SSA constant).
/// Returns the assignment's value. A nullish receiver fails the probe, and
/// `js_put_value_set` throws its TypeError.
pub(crate) fn emit_static_store_ic(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    property: &str,
    value_double: &str,
    value_bits: &str,
    strict: bool,
    value_is_scalar: bool,
    this_receiver: bool,
    run: Option<&crate::expr::layout_read::RunArm>,
    receiver: Option<crate::expr::layout_read::RunReceiver>,
) -> String {
    // Object model step 5 (case 2c): a coloured key's store through any
    // receiver writes the colour slot when the receiver's layout admits it
    // (`expr/layout_read.rs`); every miss is the site below.
    let bits = ctx.block().bitcast_double_to_i64(obj_box);
    // Step 2c (A): a constructor key's initializing store through `this`.
    let init = if this_receiver {
        crate::expr::layout_read::begin_family_init_store(
            ctx,
            &bits,
            value_double,
            value_bits,
            value_is_scalar,
            run,
        )
    } else {
        None
    };
    let arm = crate::expr::layout_read::begin_store(
        ctx,
        &bits,
        property,
        value_double,
        value_bits,
        value_is_scalar,
        receiver,
    );
    // A 2c store is a run member (an overwrite on its hit, the
    // site's call on its miss); a family init store may add a key.
    let member = arm.is_some() && init.is_none();
    let value = emit_static_store_ic_site(ctx, obj_box, property, value_double, strict);
    let value = match arm {
        Some(arm) => crate::expr::layout_read::finish_store(ctx, arm, value_double, &value),
        None => value,
    };
    let value = match init {
        Some(init) => crate::expr::layout_read::finish_store(ctx, init, value_double, &value),
        None => value,
    };
    ctx.recv_run_store_member = member;
    value
}

/// [`emit_static_store_ic`]'s ordinary site: one call,
/// `perry_store_or_put` (the store probe, an own plain writable data slot or
/// overflow entry with one barriered store, else the complete strict-aware
/// `[[Set]]`: the chain walk for setters and non-writables, the key add).
/// It may collect, so it is the site's one statepoint. The site holds no
/// state.
fn emit_static_store_ic_site(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    property: &str,
    value_double: &str,
    strict: bool,
) -> String {
    let (dir, key_bits, hash) = crate::expr::method_site::emit_site_key(ctx, property);
    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
    let _probe_decision = crate::fallback_diag::decision(
        "static store has no consumed fixed-slot proof; runtime layout/presence check required",
        "receiver layout/slot presence/store admission",
    );
    let strict_i32 = if strict { "1" } else { "0" };
    ctx.block().call(
        DOUBLE,
        "perry_store_or_put",
        &[
            (PTR, &dir),
            (I64, &obj_bits),
            (I64, &key_bits),
            (I64, &hash),
            (DOUBLE, value_double),
            (I32, strict_i32),
        ],
    )
}

/// The GC obligations after an object slot store. Plain doubles and scalar
/// tags need no bookkeeping; a value that may carry a heap pointer (the
/// `emit_may_carry_heap_pointer_check` superset of the runtime's own test)
/// takes ONE out-of-line call, `js_slot_store_obligations` (string alias
/// demotion, then the write barrier behind its parent gate, a GC leaf).
/// Object layout is a ShapeId fact, so no address-keyed layout note is
/// emitted.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_static_store_ic_bookkeeping(
    ctx: &mut FnCtx<'_>,
    handle: &str,
    slot_ptr: &str,
    value_double: &str,
    value_bits_hint: &str,
    stem: &str,
) {
    // Callers may already skip proven scalar expressions. Here the stored
    // value is an LLVM constant, so its bits are known exactly in the
    // emitted SSA: a plain double or a constant tag (undefined / true /
    // null) can never carry a pointer. Every other value is classified at
    // run time, from its bits.
    if constant_double_bits(value_double)
        .or_else(|| constant_i64_bits(value_bits_hint))
        .is_some()
    {
        return;
    }
    let book_idx = ctx.new_block(&format!("{stem}.gc_bookkeeping"));
    let done_idx = ctx.new_block(&format!("{stem}.gc_bookkeeping.done"));
    let book_label = ctx.block_label(book_idx);
    let done_label = ctx.block_label(done_idx);
    let value_bits = ctx.block().bitcast_double_to_i64(value_double);
    {
        let blk = ctx.block();
        let may_carry_pointer = emit_may_carry_heap_pointer_check(blk, &value_bits);
        blk.cond_br(&may_carry_pointer, &book_label, &done_label);
    }
    ctx.current_block = book_idx;
    {
        let blk = ctx.block();
        if crate::codegen::write_barriers_enabled() {
            let slot_addr = blk.ptrtoint(slot_ptr, I64);
            // The caller's layout test proved `handle` a live, non-forwarded
            // GC_TYPE_OBJECT, the helper's validated-parent contract.
            blk.call_void(
                "js_slot_store_obligations",
                &[(I64, handle), (I64, &slot_addr), (I64, &value_bits)],
            );
        } else {
            // `PERRY_WRITE_BARRIERS=0` exists to A/B the barrier's cost: the
            // alias demotion stays (tag-checked, a no-op for non-strings).
            blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
        }
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
}

/// Bits of `value` when it is an LLVM double constant as the IR builder spells
/// one (`nanbox::double_literal`: a decimal, or `0x…` for a non-finite /
/// NaN-boxed pattern).
fn constant_double_bits(value: &str) -> Option<u64> {
    if let Some(hex) = value.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok();
    }
    let first = value.as_bytes().first()?;
    if !(first.is_ascii_digit() || *first == b'-') {
        return None;
    }
    value.parse::<f64>().ok().map(f64::to_bits)
}

/// Bits of `value` when it is an i64 literal (the native lowering's form of a
/// constant JSValue).
fn constant_i64_bits(value: &str) -> Option<u64> {
    let first = value.as_bytes().first()?;
    if !(first.is_ascii_digit() || *first == b'-') {
        return None;
    }
    value.parse::<i64>().ok().map(|v| v as u64)
}
