//! The own-override guard for builtin calls HIR already folded (#10943).
//!
//! `lower_call/property_get/own_override_guard.rs` guards the calls codegen's
//! ordered chain lowers. It is not where most of them are: HIR folds
//! `m.get(k)`, `s.has(v)`, `a.push(x)` on a proven receiver into dedicated
//! nodes (`Expr::MapGet`, `Expr::SetHas`, `Expr::ArrayPush`, …) long before
//! codegen sees a `PropertyGet` call, and those nodes lower straight to
//! `js_map_get`/`js_set_has`/`js_array_push`. Measured on #10943's
//! differential: with only the chain guarded, **3** guard calls were emitted
//! and 16 of 30 rows stayed wrong, because the rows go through the fold.
//!
//! So the same diamond is applied here, at the ONE place every folded node
//! passes through — `lower_expr`'s dispatch — rather than in each of the forty
//! lowerings. The table below is what a node has to answer to be guarded: its
//! receiver, the method name a user could shadow, and its arguments.
//!
//! The rules are the chain guard's, unchanged:
//!
//! * the guard only CHOOSES A BRANCH — the other side is the universal
//!   dispatcher, which finds an own or inherited user method. It never
//!   resolves the property and calls it, because an own slot can hold a
//!   builtin thunk that dispatches by name again;
//! * the receiver is materialised once when re-evaluating it could be
//!   observed, and re-read in both arms;
//! * arguments are lowered inside each arm, because a diamond runs one arm.

use anyhow::Result;
use perry_hir::Expr;
use std::cell::Cell;

use super::{lower_expr, FnCtx};
use crate::rooting;
use crate::types::{DOUBLE, I32, I64};

thread_local! {
    /// Non-zero while the builtin arm is re-entering `lower_expr` for the very
    /// node being guarded, which must reach the ordinary fold rather than
    /// forming a second diamond around itself.
    static SUPPRESS: Cell<u32> = const { Cell::new(0) };
}

struct Suppressed;

impl Suppressed {
    fn enter() -> Self {
        SUPPRESS.with(|s| s.set(s.get() + 1));
        Suppressed
    }
}

impl Drop for Suppressed {
    fn drop(&mut self) {
        SUPPRESS.with(|s| s.set(s.get() - 1));
    }
}

/// Where a folded node's receiver comes from.
enum Receiver<'a> {
    /// A receiver expression, which may be effectful and is therefore
    /// materialised once.
    Expr(&'a Expr),
    /// A local the fold captured. Re-reading a local is free and cannot be
    /// observed, so it needs no materialisation.
    Local(u32),
}

struct FoldedCall<'a> {
    receiver: Receiver<'a>,
    /// The name a user property would have to carry to shadow this call.
    method: &'static str,
    args: Vec<&'a Expr>,
}

/// The folded builtin-method nodes this guard covers, and what each one is a
/// call of. A node absent from this table keeps today's behaviour: its builtin
/// runs even when an own property shadows it, which is #10943 for that
/// spelling and is why the table is meant to grow to every folded method.
fn folded_call(expr: &Expr) -> Option<FoldedCall<'_>> {
    let call = match expr {
        Expr::MapGet { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "get",
            args: vec![key],
        },
        Expr::MapSet { map, key, value } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "set",
            args: vec![key, value],
        },
        Expr::MapHas { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "has",
            args: vec![key],
        },
        Expr::MapDelete { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "delete",
            args: vec![key],
        },
        Expr::SetHas { set, value } => FoldedCall {
            receiver: Receiver::Expr(set),
            method: "has",
            args: vec![value],
        },
        Expr::SetDelete { set, value } => FoldedCall {
            receiver: Receiver::Expr(set),
            method: "delete",
            args: vec![value],
        },
        Expr::SetAdd { set_id, value } => FoldedCall {
            receiver: Receiver::Local(*set_id),
            method: "add",
            args: vec![value],
        },
        Expr::ArrayPush { array_id, value, .. } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "push",
            args: vec![value],
        },
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "indexOf",
            args: match from_index {
                Some(from) => vec![value, from],
                None => vec![value],
            },
        },
        Expr::ArraySlice { array, start, end } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "slice",
            args: match end {
                Some(end) => vec![start, end],
                None => vec![start],
            },
        },
        _ => return None,
    };
    Some(call)
}

/// Branch to `own_label` when `recv` may own a property named `method`, else
/// to `builtin_label`.
fn emit_own_override_branch(
    ctx: &mut FnCtx<'_>,
    method: &str,
    recv: &str,
    own_label: &str,
    builtin_label: &str,
) {
    let key_idx = ctx.strings.intern(method);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let blk = ctx.block();
    let maybe = blk.call(
        I32,
        "js_receiver_may_own_named_method",
        &[(DOUBLE, recv), (I64, &method_id)],
    );
    let may_own = blk.icmp_ne(I32, &maybe, "0");
    blk.cond_br(&may_own, own_label, builtin_label);
}

fn emit_dispatcher(
    ctx: &mut FnCtx<'_>,
    receiver: &Expr,
    method: &str,
    args: &[&Expr],
) -> Result<String> {
    let mut operands: Vec<&Expr> = Vec::with_capacity(args.len() + 1);
    operands.push(receiver);
    operands.extend(args.iter().copied());
    rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv, arg_vals) = values.split_first().expect("the receiver is operand 0");
        Ok(crate::lower_call::emit_native_method_str_dispatch(
            ctx, method, 0, recv, arg_vals,
        ))
    })
}

/// Guard a folded builtin-method node, or return `None` for everything else.
pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    if SUPPRESS.with(|s| s.get()) > 0 {
        return Ok(None);
    }
    let Some(call) = folded_call(expr) else {
        return Ok(None);
    };

    let local_receiver;
    let receiver_expr: &Expr = match call.receiver {
        Receiver::Expr(expr) => expr,
        Receiver::Local(id) => {
            local_receiver = Expr::LocalGet(id);
            &local_receiver
        }
    };
    let materialize = matches!(call.receiver, Receiver::Expr(_));

    let receiver_value = lower_expr(ctx, receiver_expr)?;
    let key = receiver_expr as *const Expr as usize;
    let emit = |ctx: &mut FnCtx<'_>| -> Result<String> {
        let own_idx = ctx.new_block("ownoverride.folded.own");
        let builtin_idx = ctx.new_block("ownoverride.folded.builtin");
        let merge_idx = ctx.new_block("ownoverride.folded.merge");
        let own_label = ctx.block_label(own_idx);
        let builtin_label = ctx.block_label(builtin_idx);
        let merge_label = ctx.block_label(merge_idx);

        let recv = match rooting::materialized_receiver_reread(ctx, key) {
            Some(value) => value,
            None => lower_expr(ctx, receiver_expr)?,
        };
        emit_own_override_branch(ctx, call.method, &recv, &own_label, &builtin_label);

        ctx.current_block = own_idx;
        let own_value = emit_dispatcher(ctx, receiver_expr, call.method, &call.args)?;
        let own_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = builtin_idx;
        let builtin_value = {
            let _suppressed = Suppressed::enter();
            lower_expr(ctx, expr)?
        };
        let builtin_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[
                (own_value.as_str(), own_end.as_str()),
                (builtin_value.as_str(), builtin_end.as_str()),
            ],
        ))
    };

    let value = if materialize {
        rooting::with_materialized_receiver(ctx, key, &receiver_value, emit)?
    } else {
        emit(ctx)?
    };
    Ok(Some(value))
}
