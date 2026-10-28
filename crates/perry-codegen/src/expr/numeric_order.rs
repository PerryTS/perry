//! Bridge the structural sign proof to the solved whole-program callee facts.
use super::FnCtx;
use perry_hir::{
    analysis::{numeric_order_with, NumericCallable, NumericOrder},
    Expr,
};

pub(super) fn numeric_order(ctx: &FnCtx<'_>, comparator: &Expr) -> Option<NumericOrder> {
    numeric_order_with(comparator, &|callee| {
        let (id, closure) = match callee {
            Expr::FuncRef(id) => (*id, false),
            Expr::LocalGet(local) => {
                if ctx.wpfacts_off || ctx.wpfacts.interp_guard {
                    return None;
                }
                let [body] = ctx.wpfacts.local_bodies.get(local)?.as_slice() else {
                    return None;
                };
                // FVA closes the values, not the temporal dead zone. A direct
                // enclosing statement must also have initialized this binding.
                if !ctx.numeric_order_ready.is_ready(*local) {
                    return None;
                }
                (body.func_id, body.closure)
            }
            _ => return None,
        };
        if closure {
            let (_, expr) = ctx.numeric_order_closures.iter().find(|(i, _)| *i == id)?;
            NumericCallable::closure(expr)
        } else {
            NumericCallable::function(ctx.numeric_order_functions.iter().find(|f| f.id == id)?)
        }
    })
}

/// Simple parameter lists initialize all bindings before body entry. Defaults
/// desugar into body statements; do not claim readiness ahead of that work.
pub(crate) fn parameters_ready(ctx: &mut FnCtx<'_>, params: &[perry_hir::Param]) {
    if params
        .iter()
        .any(|p| p.default.is_some() || p.is_rest || p.arguments_object.is_some())
    {
        return;
    }
    ctx.numeric_order_ready.push_list();
    for param in params {
        ctx.numeric_order_ready.note_stored(param.id);
        ctx.numeric_order_ready.settle([param.id]);
    }
}
