//! `(e1, ..., ns.m)(args)`: the indirect call TS, SWC and Babel emit for
//! every call of an imported CommonJS member (`import { join } from "path";
//! join(a, b)` becomes `(0, _path.join)(a, b)`).
//!
//! A comma expression yields the function value, so the call is the member's
//! function called with an undefined receiver; e1..ek are evaluated first,
//! for their effects. When `ns` is a node-core module namespace, the call is
//! lowered exactly as the direct member call `ns.m(args)`: the native-module
//! arms bind it to the same native implementation the value read would hand
//! back, and none of them reads a receiver, so the result is the same and the
//! member read plus the by-name dispatch disappear.
//!
//! When no native arm takes the direct call, its lowering is an ordinary call
//! of the member value. The callee is then put back into the comma form
//! (`Sequence(e1, ..., ek, <member read>)`), which is exactly the lowering of
//! the original callee: a value call with an undefined receiver.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::{lower_expr, LoweringContext};
use super::{native_module, static_call_prop_name, unwrap_call_callee_ts_wrappers};

pub(super) fn try_sequence_member_callee(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Seq(seq) = unwrap_call_callee_ts_wrappers(callee) else {
        return Ok(None);
    };
    let Some((last, prefix)) = seq.exprs.split_last() else {
        return Ok(None);
    };
    let member_expr = unwrap_call_callee_ts_wrappers(last);
    let ast::Expr::Member(member) = member_expr else {
        return Ok(None);
    };
    if static_call_prop_name(&member.prop).is_none()
        || !native_module::is_node_builtin_module_call(ctx, member_expr)
    {
        return Ok(None);
    }
    // e1..ek, in order, before the callee's own evaluation and the arguments.
    let mut effects = Vec::with_capacity(prefix.len());
    let mut prefix_lowered = Vec::with_capacity(prefix.len());
    for e in prefix {
        let lowered = lower_expr(ctx, e)?;
        if !matches!(unwrap_call_callee_ts_wrappers(e), ast::Expr::Lit(_)) {
            effects.push(lowered.clone());
        }
        prefix_lowered.push(lowered);
    }
    let direct = ast::CallExpr {
        callee: ast::Callee::Expr(Box::new(member_expr.clone())),
        ..call.clone()
    };
    let lowered = match super::lower_call(ctx, &direct)? {
        Expr::Call {
            callee,
            args,
            type_args,
            byte_offset,
            byte_end,
        } => {
            return Ok(Some(Expr::Call {
                callee: Box::new(comma_callee(prefix_lowered, *callee)),
                args,
                type_args,
                byte_offset,
                byte_end,
            }))
        }
        Expr::CallSpread {
            callee,
            args,
            type_args,
            byte_offset,
            byte_end,
        } => {
            return Ok(Some(Expr::CallSpread {
                callee: Box::new(comma_callee(prefix_lowered, *callee)),
                args,
                type_args,
                byte_offset,
                byte_end,
            }))
        }
        native => native,
    };
    if effects.is_empty() {
        return Ok(Some(lowered));
    }
    effects.push(lowered);
    Ok(Some(Expr::Sequence(effects)))
}

/// The original callee `(e1, ..., ek, <member read>)`.
fn comma_callee(mut prefix: Vec<Expr>, member_read: Expr) -> Expr {
    prefix.push(member_read);
    Expr::Sequence(prefix)
}
