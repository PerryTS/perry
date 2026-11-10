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
//! **In the floor (DESIGN-p4l1 §2.8, §4.2; PR-C3).** The body's read joins
//! every caller's key, so its own facts keep the bound, the hole test and
//! the `??`. A call whose own index the facts prove inside the floor of
//! every Array the field can hold (`FactsFieldReadCall::in_floor`) reads a
//! present own element: when the field read is a value of representation
//! `Arr` (`proven_array::in_floor_rep`), the call is the field load and the
//! slot load of the rep-keyed tier, and with Number elements the value is
//! never nullish, so `?? L` is the value.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the real call runs beside the read,
//! and the two values must be the same bits. A miss reports the false
//! proof and exits 86. An in-floor read is also tested where it loads
//! (`array_rep::witness_read`: the key is below the call's floor, the slot
//! is no hole and holds an element of the proven kind).

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
    // Condition 5a proved the index an integer Number; inside int32 it is
    // exactly `index | 0`, which the element read lowers as an i32 (no
    // runtime normalization of a double key).
    let int32 = ctx
        .wpfacts
        .call_arg_ranges
        .get(byte_offset)
        .and_then(|r| r.get(fact.index_arg as usize).copied().flatten())
        .is_some_and(|(lo, hi)| lo >= i64::from(i32::MIN) && hi <= i64::from(i32::MAX));
    let index = if int32 {
        Expr::Binary {
            op: perry_hir::BinaryOp::BitOr,
            left: Box::new(index),
            right: Box::new(Expr::Integer(0)),
        }
    } else {
        index
    };
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
    let value = match in_floor(ctx, site, &read) {
        Some((object, index, rep, floor)) => {
            // The index is inert and reaches no ender (the accessor), so no
            // collection runs between it and the field load.
            let idx = crate::expr::lower_expr_as_i32(ctx, index)?;
            let arr = crate::expr::lower_expr(ctx, object)?;
            crate::expr::index_get::proven_array::lower_in_floor(ctx, &arr, &idx, rep, floor)
        }
        None => crate::expr::lower_expr(ctx, &read)?,
    };
    if crate::verify_facts::enabled() {
        witness(ctx, callee, args, &value, site)?;
    }
    Ok(Some(value))
}

/// The in-floor form of the proven read `read` of the call at `site`
/// (module docs): its field read, its index, the field read's
/// representation and the floor the call's index is below. `None` keeps the
/// read as written: the call is not in the floor, the field read is not
/// `Arr`, or a `?? L` is over elements that are not all Numbers (a stored
/// `null` or `undefined` is a present element).
fn in_floor<'e>(
    ctx: &FnCtx<'_>,
    site: u32,
    read: &'e Expr,
) -> Option<(&'e Expr, &'e Expr, crate::codegen::FactsArrRep, u32)> {
    let floor = ctx.wpfacts.field_read_calls.get(&site)?.in_floor?;
    let (element, coalesced) = match read {
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            ..
        } => (left.as_ref(), true),
        e => (e, false),
    };
    let Expr::IndexGet { object, index, .. } = element else {
        return None;
    };
    let rep = crate::expr::index_get::proven_array::in_floor_rep(ctx, object)?;
    if coalesced && rep.elem == crate::codegen::FactsElemRep::Boxed {
        return None;
    }
    Some((object, index, rep, floor))
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
