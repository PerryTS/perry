//! Static folds for `node:util/types` predicates that ask about function kind.
//!
//! Async/generator metadata exists during HIR lowering but is erased by later
//! transforms and is not encoded in the runtime closure value. Fold only the
//! shapes we can prove from the lowered expression; dynamic values still route
//! to the conservative runtime predicates.

use crate::ir::*;

use super::super::LoweringContext;

#[derive(Clone, Copy)]
struct FunctionKind {
    is_async: bool,
    is_generator: bool,
}

pub(super) fn try_static_util_types_predicate(
    ctx: &LoweringContext,
    method: &str,
    args: &[Expr],
) -> Option<Expr> {
    let first = args.first()?;
    match method {
        "isAsyncFunction" => {
            function_kind(ctx, first).map(|kind| Expr::Bool(kind.is_async && !kind.is_generator))
        }
        "isGeneratorFunction" => {
            function_kind(ctx, first).map(|kind| Expr::Bool(kind.is_generator && !kind.is_async))
        }
        "isGeneratorObject" => match first {
            Expr::Call { callee, .. } => function_kind(ctx, callee.as_ref())
                .map(|kind| Expr::Bool(kind.is_generator && !kind.is_async)),
            value if function_kind(ctx, value).is_some() => Some(Expr::Bool(false)),
            _ => None,
        },
        "isNativeError" if is_static_native_error_expr(first) => Some(Expr::Bool(true)),
        _ => None,
    }
}

fn function_kind(ctx: &LoweringContext, expr: &Expr) -> Option<FunctionKind> {
    match expr {
        Expr::FuncRef(func_id) => function_id_kind(ctx, *func_id),
        Expr::Closure {
            is_async,
            is_generator,
            ..
        } => Some(FunctionKind {
            is_async: *is_async,
            is_generator: *is_generator,
        }),
        _ => None,
    }
}

fn function_id_kind(ctx: &LoweringContext, func_id: perry_types::FuncId) -> Option<FunctionKind> {
    if let Some(func) = ctx.pending_functions.iter().find(|func| func.id == func_id) {
        return Some(FunctionKind {
            is_async: func.is_async,
            is_generator: func.is_generator,
        });
    }

    ctx.functions
        .iter()
        .rev()
        .find(|(_, id)| *id == func_id)
        .map(|(name, _)| FunctionKind {
            is_async: ctx.async_func_names.contains(name),
            is_generator: ctx.generator_func_names.contains(name),
        })
}

fn is_static_native_error_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::ErrorNew(_)
            | Expr::ErrorNewWithCause { .. }
            | Expr::TypeErrorNew(_)
            | Expr::RangeErrorNew(_)
            | Expr::ReferenceErrorNew(_)
            | Expr::SyntaxErrorNew(_)
            | Expr::AggregateErrorNew { .. }
    )
}
