//! `Atomics.<op>(...)` namespace static calls.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::double_literal;

fn is_global_this_atomics_expr(e: &Expr) -> bool {
    matches!(
        e,
        Expr::PropertyGet { object, property, .. }
            if property == "Atomics" && matches!(object.as_ref(), Expr::GlobalGet(_))
    )
}

pub fn try_lower_atomics_static_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return Ok(None);
    };
    if !is_global_this_atomics_expr(object) {
        return Ok(None);
    }

    let (runtime_fn, arity) = match property.as_str() {
        "load" => ("js_atomics_load", 2),
        "isLockFree" => ("js_atomics_is_lock_free", 1),
        "store" => ("js_atomics_store", 3),
        "add" => ("js_atomics_add", 3),
        "sub" => ("js_atomics_sub", 3),
        "and" => ("js_atomics_and", 3),
        "or" => ("js_atomics_or", 3),
        "xor" => ("js_atomics_xor", 3),
        "exchange" => ("js_atomics_exchange", 3),
        "compareExchange" => ("js_atomics_compare_exchange", 4),
        "notify" => ("js_atomics_notify", 3),
        "wait" => ("js_atomics_wait", 4),
        "waitAsync" => ("js_atomics_wait_async", 4),
        _ => return Ok(None),
    };

    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let mut lowered: Vec<String> = Vec::with_capacity(arity);
    for i in 0..arity {
        if let Some(arg) = args.get(i) {
            lowered.push(lower_expr(ctx, arg)?);
        } else {
            lowered.push(undefined.clone());
        }
    }
    for arg in args.iter().skip(arity) {
        let _ = lower_expr(ctx, arg)?;
    }

    // The `js_atomics_*` natives are JS bodies (the function objects
    // `Atomics.load` &c. run): no environment, and they never read their
    // receiver.
    let this_bits = crate::expr::body_call::JS_THIS_UNDEFINED;
    Ok(Some(crate::expr::body_call::emit_js_body_call(
        ctx.block(),
        crate::expr::body_call::JsBody::Symbol(runtime_fn),
        "0",
        this_bits,
        &lowered,
    )))
}
