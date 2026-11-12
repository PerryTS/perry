//! `a[k] ?? <number literal>` over a receiver the facts give
//! `FactsRep::Arr(r)` with Number elements (`r.elem` F64 or I32, not at
//! rest; an at-rest read is `rest_elems::try_lower_default_i32`'s).
//!
//! Every present slot of such an Array holds a plain double (`proven_array`,
//! the rep-keyed tier's claim), which is never `undefined` or `null`. So the
//! nullish test belongs to the miss only: the hit (not forwarded, the key
//! below the capacity, the slot no hole) is the slot's double, and a miss
//! takes the exact generic read with the original key, whose `undefined` or
//! `null` is replaced by the literal. The generic lowering tested the merged
//! value of both arms on every read.
//!
//! The key is lowered as the canonical key split lowers it: an int32 the
//! body's range proofs bound (`int32_key`), else a key the facts prove an
//! int32 or a wide key (`index_key::split_key_fact`); any other key keeps
//! the generic lowering. A negative `i32` (a key at or past 2^31, or below
//! 0) fails the unsigned bound and reads through the miss arm.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`) tests the receiver's claim and the
//! slot before the load (`array_rep::witness_read`, "array rep", exit 86).
//! `PERRY_REP_DEFAULT=0` keeps the generic lowering.

use anyhow::Result;
use perry_hir::Expr;

use super::proven_array::{rep_read, RepRead};
use super::FnCtx;
use crate::codegen::FactsElemRep;
use crate::nanbox::{POINTER_MASK_I64, TAG_HOLE_I64, TAG_NULL_I64, TAG_UNDEFINED_I64};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `perry_abi::GC_FLAG_FORWARDED` in the header word's second byte.
const FORWARDED_I32: &str = "32768";

/// `PERRY_REP_DEFAULT=0` (`off`, `false`) keeps the generic lowering.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_REP_DEFAULT").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// How the key of a taken read is lowered.
#[derive(Clone, Copy)]
enum Key {
    /// An int32 by the body's range proofs.
    Int32,
    /// By the facts (`index_key::split_key_fact`).
    Fact(crate::expr::index_key::KeyFact),
}

/// The read `left` (of `left ?? right`) when the lowering takes it: its
/// receiver's rep and how its key is lowered.
fn admit(ctx: &FnCtx<'_>, left: &Expr, right: &Expr) -> Option<(RepRead, Key, f64)> {
    if !enabled() || ctx.wpfacts_off || ctx.wpfacts.interp_guard {
        return None;
    }
    let default = match right {
        Expr::Integer(n) => *n as f64,
        Expr::Number(x) if !x.is_nan() => *x,
        _ => return None,
    };
    let Expr::IndexGet {
        object,
        index,
        element_offset,
        ..
    } = left
    else {
        return None;
    };
    let read = rep_read(ctx, object, *element_offset)?;
    if read.rep.at_rest || read.rep.elem == FactsElemRep::Boxed {
        return None;
    }
    // Below a dense floor the slot is present: `left ?? right` is `left`
    // with no test at all (`proven_array::number_in_floor`).
    if read.rep.dense && read.in_floor {
        return None;
    }
    let key = if crate::expr::int32_key::proven(ctx, index) {
        Key::Int32
    } else {
        Key::Fact(crate::expr::index_key::split_key_fact(
            ctx,
            *element_offset,
        )?)
    };
    Some((read, key, default))
}

/// `left ?? right` as a double (module docs), or `None` for any other
/// expression. `number`: a number context, whose miss value is coerced.
pub(crate) fn try_lower(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
    number: bool,
) -> Result<Option<String>> {
    let Some((read, key, default)) = admit(ctx, left, right) else {
        return Ok(None);
    };
    let Expr::IndexGet { object, index, .. } = left else {
        return Ok(None);
    };
    let (object, index) = (object.as_ref(), index.as_ref());
    let r = read.rep;
    static DIAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *DIAG.get_or_init(|| std::env::var("PERRY_STORE_ARM_DIAG").as_deref() == Ok("1")) {
        eprintln!(
            "[perry rep default] {} {} elem={:?} floor={} moved={}",
            ctx.func.name,
            read.receiver.label(),
            r.elem,
            r.floor,
            !r.unmoved() as u8
        );
    }
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "receiver": read.receiver.label(), "floor": r.floor, "default": true }),
    );
    crate::facts_consumed::used();
    let default = crate::nanbox::double_literal(default);
    let value = crate::rooting::with_operands_rooted_across(
        ctx,
        &[object],
        &[index],
        |ctx| match key {
            Key::Int32 => {
                let idx = crate::expr::lower_expr_as_i32(ctx, index)?;
                let boxed = ctx.block().sitofp(I32, &idx, DOUBLE);
                Ok((idx, boxed))
            }
            Key::Fact(fact) => {
                let boxed = crate::expr::lower_expr(ctx, index)?;
                let idx = crate::expr::index_key::exact_i32(ctx, &boxed, fact);
                Ok((idx, boxed))
            }
        },
        |ctx, v, (idx, boxed)| {
            let arr = v[0].clone();
            if crate::verify_facts::enabled() {
                let tested = RepRead {
                    in_floor: false,
                    ..read
                };
                super::array_rep::witness_read(ctx, &arr, &idx, tested, false, read.number);
            }
            let cold_idx = ctx.new_block("arep.default.cold");
            let merge_idx = ctx.new_block("arep.default.merge");
            let cold_label = ctx.block_label(cold_idx);
            let merge_label = ctx.block_label(merge_idx);
            let step = |ctx: &mut FnCtx<'_>, cond: &str, name: &str| {
                let on = ctx.new_block(name);
                let on_l = ctx.block_label(on);
                ctx.block().cond_br(cond, &on_l, &cold_label);
                ctx.current_block = on;
            };
            let bits = ctx.block().bitcast_double_to_i64(&arr);
            let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
            if !r.unmoved() {
                // Growth moves the storage and forwards this header.
                let word = super::guarded_array::emit_array_guard_word(ctx.block(), &handle);
                let moved = ctx.block().and(I32, &word, FORWARDED_I32);
                let live = ctx.block().icmp_eq(I32, &moved, "0");
                step(ctx, &live, "arep.default.live");
            }
            let capacity = {
                let blk = ctx.block();
                let ptr = blk.inttoptr(I64, &handle);
                let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
                blk.load(I32, &cap_ptr)
            };
            let inside = ctx.block().icmp_ult(I32, &idx, &capacity);
            step(ctx, &inside, "arep.default.inside");
            let addr = {
                let blk = ctx.block();
                if r.floor > 0 {
                    // A defined length: the elements start behind the header.
                    let i = blk.zext(I32, &idx, I64);
                    let offset = blk.shl(I64, &i, "3");
                    let base = blk.add(
                        I64,
                        &handle,
                        &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
                    );
                    blk.add(I64, &base, &offset)
                } else {
                    blk.array_element_addr_with_capacity(&handle, &capacity, &idx)
                }
            };
            let slot = ctx.block().inttoptr(I64, &addr);
            let raw = ctx.block().load(I64, &slot);
            let own = ctx.block().icmp_ne(I64, &raw, TAG_HOLE_I64);
            step(ctx, &own, "arep.default.own");
            let hit = ctx.block().bitcast_i64_to_double(&raw);
            let hit_end = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            ctx.current_block = cold_idx;
            // Mode 0: the original key, the exact property-key read.
            let got = super::guarded_array::emit_read_cold(
                ctx, &arr, "0", &boxed, "0", false, false, None,
            );
            let miss = {
                let blk = ctx.block();
                let got_bits = blk.bitcast_double_to_i64(&got);
                let is_null = blk.icmp_eq(I64, &got_bits, TAG_NULL_I64);
                let is_undef = blk.icmp_eq(I64, &got_bits, TAG_UNDEFINED_I64);
                let nullish = blk.or(I1, &is_null, &is_undef);
                if number {
                    let keep_idx = ctx.new_block("arep.default.keep");
                    let done_idx = ctx.new_block("arep.default.done");
                    let (keep_l, done_l) = (ctx.block_label(keep_idx), ctx.block_label(done_idx));
                    let from = ctx.block().label.clone();
                    ctx.block().cond_br(&nullish, &done_l, &keep_l);
                    ctx.current_block = keep_idx;
                    let coerced = ctx
                        .block()
                        .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &got)]);
                    let keep_end = ctx.block().label.clone();
                    ctx.block().br(&done_l);
                    ctx.current_block = done_idx;
                    ctx.block()
                        .phi(DOUBLE, &[(&default, &from), (&coerced, &keep_end)])
                } else {
                    blk.select(I1, &nullish, DOUBLE, &default, &got)
                }
            };
            let miss_end = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
            Ok(ctx
                .block()
                .phi(DOUBLE, &[(&hit, &hit_end), (&miss, &miss_end)]))
        },
    )?;
    Ok(Some(value))
}
