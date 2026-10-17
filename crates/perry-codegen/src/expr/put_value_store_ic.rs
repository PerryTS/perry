//! The static-key store `o.k = v` (object model step 5, phase 2,
//! `OBJECT_MODEL_2026-10-13.md` §3.7): the 2c colour-slot arm, then the
//! store probe and the complete `[[Set]]`. The site holds no state.
//!
//! Every static-key, same-receiver store that no earlier specialisation took
//! (class field, POD record, scalar-replaced local, array index or length)
//! reaches [`emit_static_store_ic`], whatever its right-hand side:
//!
//! ```text
//!   [colour slot arm, `expr/layout_read.rs`]
//!   ok = perry_probe_set(dir, obj, key, hash, v)   (an own plain writable
//!        data slot or overflow entry: one barriered store; a GC leaf)
//!   ok ? v : js_put_value_set(obj, key, v, obj, strict)
//!        (the chain walk for setters and non-writables, the key add)
//! ```
//!
//! # Evaluation order: the RHS may collect, so the receiver is read after it
//!
//! The caller evaluates the target FIRST (the reference is evaluated before
//! the value, ECMA-262 §13.15.2), keeps it in an operand root across the RHS
//! (`with_operands_rooted_across`), and hands this emitter the value RE-READ
//! from that root after the RHS. Nothing between that re-read and the probe's
//! store can collect, so the receiver the probe reads is the CURRENT one.

use crate::types::{DOUBLE, I1, I32, I64, PTR};

use super::write_barrier::{
    emit_may_carry_heap_pointer_check, emit_parent_may_need_remembering_check,
};
use super::FnCtx;

/// The barrier-census stem. Shared with the census registry
/// (`barrier_stem_census_tests::VERIFIED_BARRIER_STEMS`).
pub(crate) const STORE_IC_STEM: &str = "put.pic";

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
    );
    let value = emit_static_store_ic_site(ctx, obj_box, property, value_double, strict);
    let value = match arm {
        Some(arm) => crate::expr::layout_read::finish_store(ctx, arm, value_double, &value),
        None => value,
    };
    match init {
        Some(init) => crate::expr::layout_read::finish_store(ctx, init, value_double, &value),
        None => value,
    }
}

/// [`emit_static_store_ic`]'s ordinary site: the store probe
/// (`perry_probe_set`: an own plain writable data slot or overflow entry,
/// one barriered store), else the complete strict-aware `[[Set]]`
/// (`js_put_value_set`: the chain walk for setters and non-writables, the
/// key add). The site holds no state.
fn emit_static_store_ic_site(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    property: &str,
    value_double: &str,
    strict: bool,
) -> String {
    let (dir, key_bits, hash) = crate::expr::method_site::emit_site_key(ctx, property);
    let obj_bits = ctx.block().bitcast_double_to_i64(obj_box);
    let stored = ctx.block().call(
        I32,
        "perry_probe_set",
        &[
            (PTR, &dir),
            (I64, &obj_bits),
            (I64, &key_bits),
            (I64, &hash),
            (DOUBLE, value_double),
        ],
    );
    let hit = ctx.block().icmp_ne(I32, &stored, "0");
    let hit_end = ctx.block().label.clone();
    let slow_idx = ctx.new_block(&format!("{STORE_IC_STEM}.set"));
    let merge_idx = ctx.new_block(&format!("{STORE_IC_STEM}.merge"));
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&hit, &merge_label, &slow_label);
    ctx.current_block = slow_idx;
    let key_box = ctx.block().bitcast_i64_to_double(&key_bits);
    let strict_i32 = if strict { "1" } else { "0" };
    let set = ctx.block().call(
        DOUBLE,
        "js_put_value_set",
        &[
            (DOUBLE, obj_box),
            (DOUBLE, &key_box),
            (DOUBLE, value_double),
            (DOUBLE, obj_box),
            (I32, strict_i32),
        ],
    );
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[(value_double, &hit_end), (&set, &slow_end)],
    )
}


/// The GC obligations after an object slot store. Plain doubles and scalar
/// tags need no bookkeeping; pointer-bearing values need string alias demotion
/// and the generational/incremental write barrier. Object layout is now a
/// ShapeId fact, so no address-keyed layout note is emitted.
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
    // value is an LLVM constant, so its bits
    // are known exactly in the emitted SSA. A plain double needs nothing, and
    // a constant tag (undefined / true / null) can never carry a pointer.
    // Every other value is classified at run time, from its bits.
    let constant =
        constant_double_bits(value_double).or_else(|| constant_i64_bits(value_bits_hint));
    if constant.is_some_and(is_plain_double_bits) {
        return;
    }
    let tagged_idx = ctx.new_block(&format!("{stem}.scalar.tagged"));
    let done_idx = ctx.new_block(&format!("{stem}.gc_bookkeeping.done"));
    let tagged_label = ctx.block_label(tagged_idx);
    let done_label = ctx.block_label(done_idx);

    let value_bits = ctx.block().bitcast_double_to_i64(value_double);
    let value_bits = value_bits.as_str();

    if constant.is_some() {
        // A constant scalar tag carries no heap pointer.
        ctx.block().br(&done_label);
    } else {
        let classify_idx = ctx.new_block(&format!("{stem}.classify"));
        let classify_label = ctx.block_label(classify_idx);
        {
            // Plain double <=> top 16 bits neither in the NaN-boxed tag range
            // [0x7FF9, 0x7FFF] nor zero. Everything the runtime's pointer test
            // or its raw-f64 test could act on has one of the two: a tagged
            // value, or (top 16 bits zero) a bare heap address. The zero half
            // also sends +0.0 and the subnormals to the exact classification
            // below — a superset, so it costs those values time, never
            // correctness — and keeps this test to the one `shr` both halves
            // share.
            let blk = ctx.block();
            // Sign-closed tag band: includes HANDLE_TAG's mirror 0xFFF9.
            let magnitude = blk.and(I64, value_bits, "9223372036854775807");
            let boxed_tag = blk.icmp_uge(
                I64,
                &magnitude,
                &crate::nanbox::i64_literal(crate::nanbox::SHORT_STRING_TAG),
            );
            let top_zero = blk.icmp_ult(I64, value_bits, "281474976710656");
            let not_plain = blk.or(I1, &boxed_tag, &top_zero);
            blk.cond_br(&not_plain, &classify_label, &done_label);
        }
        ctx.current_block = classify_idx;
        emit_pointer_arm(
            ctx,
            stem,
            handle,
            slot_ptr,
            value_double,
            value_bits,
            &tagged_label,
            &done_label,
        );
    }

    ctx.current_block = tagged_idx;
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

/// The pointer-bearing arm, entered from `<stem>.classify` on
/// `emit_may_carry_heap_pointer_check` (a superset of the runtime's own test,
/// so it can only send a value here that the runtime would ignore).
#[allow(clippy::too_many_arguments)]
fn emit_pointer_arm(
    ctx: &mut FnCtx<'_>,
    stem: &str,
    handle: &str,
    slot_ptr: &str,
    value_double: &str,
    value_bits: &str,
    tagged_label: &str,
    done_label: &str,
) {
    let book_idx = ctx.new_block(&format!("{stem}.gc_bookkeeping"));
    let alias_idx = ctx.new_block(&format!("{stem}.string_alias"));
    let book_label = ctx.block_label(book_idx);
    let alias_label = ctx.block_label(alias_idx);
    let book_done_idx = ctx.new_block(&format!("{stem}.book_done"));
    let book_done_label = ctx.block_label(book_done_idx);
    {
        let blk = ctx.block();
        let may_carry_pointer = emit_may_carry_heap_pointer_check(blk, value_bits);
        blk.cond_br(&may_carry_pointer, &book_label, tagged_label);
    }

    // A uniquely-owned heap string aliased into the slot is demoted to shared
    // (`js_string_addref_if_heap_string` is a no-op for every other tag, so
    // the call is made only for a STRING-tagged value).
    ctx.current_block = book_idx;
    {
        let blk = ctx.block();
        let top16 = blk.lshr(I64, value_bits, "48");
        let is_string = blk.icmp_eq(I64, &top16, crate::nanbox::STRING_TAG_TOP16_I64);
        blk.cond_br(&is_string, &alias_label, &book_done_label);
    }
    ctx.current_block = alias_idx;
    {
        let blk = ctx.block();
        blk.call_void("js_string_addref_if_heap_string", &[(DOUBLE, value_double)]);
        blk.br(&book_done_label);
    }
    ctx.current_block = book_done_idx;
    if crate::codegen::write_barriers_enabled() {
        // Created only when emitted: `PERRY_WRITE_BARRIERS=0` exists to A/B
        // the barrier's cost, and dead IR in one arm makes that comparison lie.
        let barrier_idx = ctx.new_block(&format!("{stem}.barrier"));
        let barrier_label = ctx.block_label(barrier_idx);
        {
            let blk = ctx.block();
            // TENURED parent (remembered set) OR a live incremental cycle
            // (insertion shading) — the runtime barrier's two jobs.
            let needed = emit_parent_may_need_remembering_check(blk, handle);
            blk.cond_br(&needed, &barrier_label, done_label);
        }
        ctx.current_block = barrier_idx;
        {
            let blk = ctx.block();
            let slot_addr = blk.ptrtoint(slot_ptr, I64);
            // The shape compare proved `handle` a live, non-forwarded
            // GC_TYPE_OBJECT, which is the validated-parent entry's contract.
            blk.call_void(
                "js_write_barrier_slot_validated_parent",
                &[(I64, handle), (I64, &slot_addr), (I64, value_bits)],
            );
            blk.br(done_label);
        }
    } else {
        ctx.block().br(done_label);
    }
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

/// Bits that the runtime's layout note treats as a raw f64 and the barrier as
/// pointer-free: not a NaN-boxed tag (`0x7FF9..=0x7FFF`) and not a bare heap
/// address (`[0x1000, 2^48)`).
fn is_plain_double_bits(bits: u64) -> bool {
    let top16 = bits >> 48;
    !(0x7FF9..=0x7FFF).contains(&top16) && !(0x1000..(1u64 << 48)).contains(&bits)
}

