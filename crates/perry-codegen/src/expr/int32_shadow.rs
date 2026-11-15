//! The ToInt32 of an immutable local that holds only Numbers and
//! `undefined`, computed once.
//!
//! `make(move)` uses its parameter as a bitwise operand seven times
//! (`move & 127`, `(move >> 7) & 127`, `move & EP`, ...). The facts give
//! `move` Number or `undefined` (a for-of over an Array whose index stores
//! they cannot bound may yield a hole), so the parameter is Boxed, and each
//! use tested `|v| < 2^63` and truncated the double again
//! (`binary::lower_guarded_numeric_arith`).
//!
//! For a value that is a Number or `undefined`, ToInt32 runs no user code
//! and depends on nothing but the value: `ToInt32(undefined)` is `0`, and
//! `toint32_wrap` of the NaN-boxed word gives exactly that (the `undefined`
//! word is a NaN). So for a local that is never written after its
//! initialization (a parameter no statement assigns, a `const`), the
//! conversion is the same at every use, and is computed once, where the
//! local is initialized, into an i32 **shadow** slot ([`materialize`]).
//! A bitwise operand that reads the local reads the shadow
//! (`int32_operand`).
//!
//! Admission ([`plan`], per body): the local is an operand of at least two
//! bitwise operators, and no `LocalSet` / update names it. At
//! [`materialize`]: the facts prove it Number-or-`undefined`
//! (`ElisionFacts::undef_number_inputs`), it is no boxed capture, module
//! global or TDZ-checked binding, and it has no i32 slot of its own.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the value is tested "a plain double
//! or `undefined`" before it is converted (`Fact::CanonicalNumber`, exit
//! 86). Negative control: `PERRY_FACTS_SABOTAGE=undefnum` (the driver claims
//! every local that may hold a Number).
//!
//! `PERRY_INT32_SHADOWS=0` (`off`, `false`): no shadow.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, Stmt};

use super::FnCtx;
use crate::types::I32;

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_INT32_SHADOWS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn bitwise(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr
    )
}

/// The locals of `body` worth a shadow: an operand of two or more bitwise
/// operators (closure bodies excluded: they are other functions), never
/// assigned.
pub(crate) fn plan(body: &[Stmt]) -> HashSet<u32> {
    if !enabled() {
        return HashSet::new();
    }
    fn in_expr(e: &Expr, uses: &mut HashMap<u32, u32>) {
        match e {
            Expr::Closure { .. } => return,
            Expr::Binary { op, left, right } if bitwise(*op) => {
                for side in [left, right] {
                    if let Expr::LocalGet(id) = side.as_ref() {
                        *uses.entry(*id).or_default() += 1;
                    }
                }
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| in_expr(c, uses));
    }
    let mut uses = HashMap::new();
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            in_expr(e, &mut uses);
            false
        });
    }
    let mut written = HashSet::new();
    crate::collectors::collect_written_locals(body, &mut written);
    uses.into_iter()
        .filter(|(id, n)| *n >= 2 && !written.contains(id))
        .map(|(id, _)| id)
        .collect()
}

/// Local `id` has just been initialized (a parameter at entry, a `const`
/// after its `let`): give it its shadow when it is admitted (module docs).
pub(crate) fn materialize(ctx: &mut FnCtx<'_>, id: u32) -> Result<()> {
    if !ctx.int32_shadow_plan.contains(&id)
        || ctx.wpfacts_off
        || ctx.wpfacts.interp_guard
        || !ctx.wpfacts.undef_number_inputs.contains(&id)
        || ctx.boxed_vars.contains(&id)
        || ctx.module_globals.contains_key(&id)
        || ctx.tdz_boxes.contains(&id)
        || ctx.i32_counter_slots.contains_key(&id)
        || ctx.closure_captures.contains_key(&id)
    {
        return Ok(());
    }
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::CanonicalNumber,
        || serde_json::json!({ "shadow": id }),
    );
    crate::facts_consumed::used();
    let value = super::lower_expr(ctx, &Expr::LocalGet(id))?;
    super::facts_canonical::witness(
        ctx,
        &value,
        super::facts_canonical::Num::NumberOrUndefined,
        "int32 shadow",
    )?;
    let word = ctx.toint32_wrap(&value);
    let slot = match ctx.int32_shadows.get(&id) {
        Some(slot) => slot.clone(),
        None => {
            let slot = ctx.func.alloca_entry(I32);
            ctx.int32_shadows.insert(id, slot.clone());
            slot
        }
    };
    ctx.block().store(I32, &word, &slot);
    Ok(())
}

/// `e` reads a local with a shadow: its ToInt32 is the shadow's word.
pub(crate) fn is_shadowed(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    matches!(e, Expr::LocalGet(id) if ctx.int32_shadows.contains_key(id))
}

/// The shadow's word for `e` ([`is_shadowed`]).
pub(crate) fn load(ctx: &mut FnCtx<'_>, e: &Expr) -> Option<String> {
    let Expr::LocalGet(id) = e else {
        return None;
    };
    let slot = ctx.int32_shadows.get(id)?.clone();
    Some(ctx.block().load(I32, &slot))
}
