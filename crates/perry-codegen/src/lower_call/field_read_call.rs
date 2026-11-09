//! b1 (DESIGN-b1-field-read-call rev 2): a method call that wpfacts proves
//! to be a field element read (`ElisionFacts::field_read_calls`) lowers as
//! that read, `r.f[a_k] ?? L`, with the method body's own offsets, so the
//! read consumes the facts proven at its sites.
//!
//! The fact says the read can neither throw nor run user code (condition
//! 5): no frame is observable, and the call's only effect is its value.
//! This route admits a receiver that is `this` or a local and arguments that
//! are inert and reach no ender, so evaluating the index argument inside the
//! read (and dropping the others) is the call's evaluation exactly.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the real call runs beside the read,
//! and the two values must be the same bits. A miss reports the false
//! proof and exits 86.

use anyhow::Result;
use perry_hir::{Expr, LogicalOp};

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I64};

/// THE accessor (REVIEW-b1 B1): the read a call `callee(args)` is, when the
/// facts prove it and this route admits its operands. `lower_call` and the
/// loop classifier both use it.
pub(crate) fn field_read_call_at(ctx: &FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Option<Expr> {
    let Expr::PropertyGet {
        object,
        byte_offset,
        ..
    } = callee
    else {
        return None;
    };
    if *byte_offset == 0 || ctx.wpfacts_off || ctx.wpfacts.interp_guard {
        return None;
    }
    let fact = ctx.wpfacts.field_read_calls.get(byte_offset)?;
    if !matches!(object.as_ref(), Expr::This | Expr::LocalGet(_)) {
        return None;
    }
    let is_inert = |x: &Expr| crate::rooting::expr_is_inert_primitive(ctx, x);
    if !args
        .iter()
        .all(|a| is_inert(a) && crate::collectors::region_enders_in_expr(a, &is_inert).is_empty())
    {
        return None;
    }
    let index = args.get(fact.index_arg as usize)?.clone();
    let read = Expr::IndexGet {
        element_offset: fact.element_offset,
        byte_offset: fact.element_offset,
        byte_end: 0,
        object: Box::new(Expr::PropertyGet {
            object: object.clone(),
            property: fact.field.clone(),
            byte_offset: fact.field_offset,
            byte_end: 0,
        }),
        index: Box::new(index),
    };
    Some(match fact.default {
        Some(bits) => Expr::Logical {
            op: LogicalOp::Coalesce,
            left: Box::new(read),
            right: Box::new(Expr::Number(f64::from_bits(bits))),
        },
        None => read,
    })
}

/// Lower `callee(args)` as its proven read, when it is one.
pub(crate) fn try_lower(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Some(read) = field_read_call_at(ctx, callee, args) else {
        return Ok(None);
    };
    let Expr::PropertyGet { byte_offset, .. } = callee else {
        unreachable!("the accessor admits a method call")
    };
    let site = *byte_offset;
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::FieldReadCall,
        || serde_json::json!({ "call": site }),
    );
    crate::facts_consumed::used();
    let value = crate::expr::lower_expr(ctx, &read)?;
    if crate::verify_facts::enabled() {
        witness(ctx, callee, args, &value, site)?;
    }
    Ok(Some(value))
}

/// Check mode: the real call's value equals the read's.
fn witness(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
    value: &str,
    site: u32,
) -> Result<()> {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::FieldReadCall,
        || serde_json::json!({ "call": site }),
    );
    let called =
        super::index_split::with_call_site(callee, || super::lower_call_routes(ctx, callee, args))?;
    let blk = ctx.block();
    let a = blk.bitcast_double_to_i64(value);
    let b = blk.bitcast_double_to_i64(&called);
    let same = blk.icmp_eq(I64, &a, &b);
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("field_read_call.verify_miss");
    let next = ctx.new_block("field_read_call.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&same, &next_l, &miss_l);
    ctx.current_block = miss;
    let site_arg = crate::verify_facts::site_arg(
        ctx,
        &format!("field read call @{site}"),
        crate::verify_facts::Fact::FieldReadCall,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, value)],
        &site_arg,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}
