//! `a[i]` with a literal index, of a local whose every Array has the same
//! constant contents (wpfacts fact IM, `ElisionFacts::const_arrays`): each
//! is an immutable program Array born from Number literals, so index `i`
//! below its length holds the same Number for the whole program. The read
//! is that Number: no receiver, no load.
//!
//! An index outside `[0, length)` is no element and keeps the read it had.
//! A local the codegen keeps in scalar slots is not asked. Check mode
//! (`PERRY_VERIFY_FACTS=1`) loads the slot and compares its bits with the
//! constant (`Fact::ConstArray`, exit 86).

use anyhow::Result;
use perry_hir::Expr;

use super::guarded_array::{
    emit_array_guard_word, HEAP_POINTER_BAND_BASE_I64, HEAP_POINTER_BAND_SPAN_I64,
};
use super::{typed_feedback_emission_enabled, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, I8};

/// The constant the read `expr` is, when the facts give one (module docs).
pub(super) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    if ctx.wpfacts_off || ctx.wpfacts.const_arrays.is_empty() || typed_feedback_emission_enabled()
    {
        return Ok(None);
    }
    let Expr::IndexGet { object, index, .. } = expr else {
        return Ok(None);
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return Ok(None);
    };
    let i = match index.as_ref() {
        Expr::Integer(i) => *i,
        Expr::Number(n) if n.fract() == 0.0 && n.abs() < 4294967296.0 => *n as i64,
        _ => return Ok(None),
    };
    if ctx.scalar_replaced_arrays.contains_key(id)
        || ctx.array_row_aliases.contains_key(id)
        || ctx.scalar_replaced.contains_key(id)
    {
        return Ok(None);
    }
    let Some(bits) = ctx
        .wpfacts
        .const_arrays
        .get(id)
        .and_then(|c| usize::try_from(i).ok().and_then(|i| c.get(i)))
        .copied()
    else {
        return Ok(None);
    };
    crate::verify_facts::covered(crate::verify_facts::Fact::ConstArray, || {
        serde_json::json!({ "local": id, "index": i })
    });
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        let recv = super::lower_expr(ctx, object)?;
        witness(ctx, &recv, *id, i, bits);
    }
    Ok(Some(crate::nanbox::double_literal(f64::from_bits(bits))))
}

/// Check mode: `recv` is an Array whose slot `i` holds `bits`.
fn witness(ctx: &mut FnCtx<'_>, recv: &str, id: u32, i: i64, bits: u64) {
    let miss = ctx.new_block("const_arr.verify_miss");
    let next = ctx.new_block("const_arr.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    let word = ctx.block().bitcast_double_to_i64(recv);
    let offset = ctx.block().sub(I64, &word, HEAP_POINTER_BAND_BASE_I64);
    let heap = ctx
        .block()
        .icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    branch(ctx, &heap, "const_arr.verify_header", &miss_l);
    let handle = ctx.block().and(I64, &word, POINTER_MASK_I64);
    let header = emit_array_guard_word(ctx.block(), &handle);
    let ty = ctx.block().and(I32, &header, "255");
    let array = ctx.block().icmp_eq(I32, &ty, "1");
    branch(ctx, &array, "const_arr.verify_length", &miss_l);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &ptr);
    let inside = ctx.block().icmp_ugt(I32, &length, &i.to_string());
    branch(ctx, &inside, "const_arr.verify_slot", &miss_l);
    let cap_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
    let capacity = ctx.block().load(I32, &cap_ptr);
    let addr = ctx
        .block()
        .array_element_addr_with_capacity(&handle, &capacity, &i.to_string());
    let slot = ctx.block().inttoptr(I64, &addr);
    let raw = ctx.block().load(I64, &slot);
    let equal = ctx.block().icmp_eq(I64, &raw, &(bits as i64).to_string());
    ctx.block().cond_br(&equal, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!(
            "constant array of local {id}: index {i} is {}",
            f64::from_bits(bits)
        ),
        crate::verify_facts::Fact::ConstArray,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, recv)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

/// `cond` goes on to a new block `name`, else to `other`; the new block is
/// current.
fn branch(ctx: &mut FnCtx<'_>, cond: &str, name: &str, other: &str) {
    let on = ctx.new_block(name);
    let on_l = ctx.block_label(on);
    ctx.block().cond_br(cond, &on_l, other);
    ctx.current_block = on;
}
