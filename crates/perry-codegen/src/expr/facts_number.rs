//! CLS D2: Numbers proven by the whole-program facts, not by annotations.
//!
//! Two proofs, both about values, both fail-closed:
//!
//! * **A field read's kind.** A definite data birth field of prefix-stable
//!   classes (`wpfacts_access::prove`) whose F2 kind mask holds only
//!   primitives that `ToNumber` converts without running user code (Number,
//!   Boolean, null, undefined). Reading it and converting it is a load, a
//!   plain-number test and a cold leaf `js_number_coerce`; `+` of such a read
//!   with a Number is a Number (no string, BigInt or object can reach it).
//! * **A closed call's result.** A method call whose receiver the facts prove
//!   one of a closed class set, every class of which resolves the method to a
//!   cased prototype constant (the D1 site), and every resolved body returns a
//!   Number on every path that returns: literals, `Math.*`, arithmetic whose
//!   operand kinds exclude BigInt, `+` over Numbers and primitive-kind fields,
//!   and `this.m()` calls closed the same way. The body is judged under the
//!   receiver class set, so a subclass that stores a string into the field, or
//!   returns a non-Number from an override, fails the proof for its group.
//!
//! The value representation of a Number is its raw double: a consumer of a
//! proven Number takes the call's result as an `fadd` operand, with no
//! `js_dynamic_string_or_number_add`, no residual `js_number_coerce`.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, Stmt, UnaryOp};

use super::wpfacts_access::{self, Access};
use super::{lower_expr, FnCtx};
use crate::types::DOUBLE;

const K_NUM: u16 = 1;
const K_BOOL: u16 = 4;
const K_NULL: u16 = 8;
const K_UNDEF: u16 = 16;
const K_BIGINT: u16 = 32;
const K_OBJ: u16 = 128;
/// Kinds `ToNumber` converts with no user code and no allocation.
const K_PRIMS: u16 = K_NUM | K_BOOL | K_NULL | K_UNDEF;

/// How far a method-return proof follows `this.m()` calls.
const MAX_CALL_DEPTH: u8 = 4;

/// What the facts prove about an operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Operand {
    /// A canonical raw double (a Number).
    Number,
    /// A field read holding Number, Boolean, null or undefined only.
    Prim,
}

/// The kind mask of the guard-free facts read `object.property`.
fn read_kinds(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<u16> {
    let p = wpfacts_access::prove(ctx, object, property, Access::Get)?;
    (!p.guarded() && p.kind_mask != 0).then_some(p.kind_mask)
}

/// Classify the caller-side operand `e`.
pub(crate) fn operand(ctx: &FnCtx<'_>, e: &Expr) -> Option<Operand> {
    if let Expr::PropertyGet {
        object, property, ..
    } = e
    {
        if let Some(mask) = read_kinds(ctx, object, property) {
            if wpfacts_access::proves_raw_f64(ctx, e) {
                return Some(Operand::Number);
            }
            if mask & !K_PRIMS == 0 {
                return Some(Operand::Prim);
            }
        }
        return None;
    }
    crate::type_analysis::expr_produces_canonical_raw_f64(ctx, e).then_some(Operand::Number)
}

/// `left + right` is a Number by the facts: both operands are Numbers or
/// primitive-kind reads. Neither can be a string, a BigInt or an object, so
/// the sum is `ToNumber(left) + ToNumber(right)`.
pub(crate) fn add_is_number(ctx: &FnCtx<'_>, left: &Expr, right: &Expr) -> bool {
    operand(ctx, left).is_some() && operand(ctx, right).is_some()
}

/// `ToNumber` of a primitive-kind value: the plain-number case is the value,
/// every other case (undefined, null, a Boolean, an INT32 box) is the leaf
/// `js_number_coerce`.
pub(crate) fn emit_prim_to_number(ctx: &mut FnCtx<'_>, raw: &str) -> String {
    let is_num = crate::stmt::emit_js_value_is_number(ctx, raw);
    let fast_idx = ctx.new_block("facts_num.plain");
    let slow_idx = ctx.new_block("facts_num.coerce");
    let merge_idx = ctx.new_block("facts_num.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_num, &fast_label, &slow_label);
    ctx.current_block = fast_idx;
    ctx.block().br(&merge_label);
    ctx.current_block = slow_idx;
    let coerced = ctx
        .block()
        .call(DOUBLE, "js_number_coerce", &[(DOUBLE, raw)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(raw, &fast_label), (&coerced, &slow_end)])
}

/// A primitive-kind field read, lowered as a number operand.
pub(crate) fn lower_prim_operand(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    if operand(ctx, e) != Some(Operand::Prim) {
        return Ok(None);
    }
    let raw = lower_expr(ctx, e)?;
    Ok(Some(emit_prim_to_number(ctx, &raw)))
}

/// `left + right` over proven Numbers and primitive-kind reads: an `fadd`
/// of the two `ToNumber` values, no dynamic helper. Both operands are
/// evaluated left to right; neither conversion can run user code.
pub(crate) fn try_lower_add(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> Result<Option<String>> {
    let (Some(lk), Some(rk)) = (operand(ctx, left), operand(ctx, right)) else {
        return Ok(None);
    };
    // Number + Number is the existing numeric path.
    if lk == Operand::Number && rk == Operand::Number {
        return Ok(None);
    }
    let l_raw = lower_expr(ctx, left)?;
    let l = if lk == Operand::Prim {
        emit_prim_to_number(ctx, &l_raw)
    } else {
        l_raw
    };
    let r_raw = lower_expr(ctx, right)?;
    let r = if rk == Operand::Prim {
        emit_prim_to_number(ctx, &r_raw)
    } else {
        r_raw
    };
    Ok(Some(ctx.block().fadd(&l, &r)))
}

// ---------------------------------------------------------------------------
// Closed calls
// ---------------------------------------------------------------------------

/// The call `receiver.m(...)` is a Number by the facts: see the module doc.
pub(crate) fn closed_number_call(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::Call { callee, .. } = e else {
        return false;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return false;
    };
    if ctx.wpfacts_off {
        return false;
    }
    let Some((cids, _nullable)) = wpfacts_access::proven_receiver_classes(ctx, object) else {
        return false;
    };
    targets_return_number(ctx, &cids, property, 0)
}

/// One resolved implementor and the receiver classes that reach it.
struct Group {
    owner: String,
    function: String,
    cids: Vec<u32>,
}

fn targets_return_number(ctx: &FnCtx<'_>, cids: &[u32], property: &str, depth: u8) -> bool {
    if depth > MAX_CALL_DEPTH || cids.is_empty() || ctx.wpfacts.interp_guard {
        return false;
    }
    let mut groups: Vec<Group> = Vec::new();
    for &cid in cids {
        let Some(class) = wpfacts_access::class_name_of(ctx, cid) else {
            return false;
        };
        let mut cur = Some(class.clone());
        let mut found: Option<(String, String)> = None;
        while let Some(c) = cur {
            if let Some(f) = ctx.methods.get(&(c.clone(), property.to_string())) {
                found = Some((c, f.clone()));
                break;
            }
            cur = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
        }
        let Some((owner, function)) = found else {
            return false;
        };
        // The site's own admission: a prototype constant, not shadowed by a
        // field of that name.
        if !crate::lower_call::property_get::method_target_is_cased(
            ctx, &class, property, &function,
        ) {
            return false;
        }
        match groups.iter_mut().find(|g| g.function == function) {
            Some(g) => g.cids.push(cid),
            None => groups.push(Group {
                owner,
                function,
                cids: vec![cid],
            }),
        }
    }
    groups.iter().all(|g| {
        let Some(class) = ctx.classes.get(&g.owner) else {
            return false;
        };
        let Some(method) = class.methods.iter().find(|m| {
            m.name == property
                && ctx
                    .methods
                    .get(&(g.owner.clone(), m.name.clone()))
                    .is_some_and(|f| *f == g.function)
        }) else {
            return false;
        };
        !method.is_async
            && !method.is_generator
            && block_returns_number(ctx, &method.body, &g.cids, depth) == (true, true)
    })
}

/// `(every return is a Number, every normal path exits)` for the
/// straight-line / `if` subset; anything else rejects the proof.
fn block_returns_number(ctx: &FnCtx<'_>, stmts: &[Stmt], cids: &[u32], depth: u8) -> (bool, bool) {
    for stmt in stmts {
        match stmt {
            Stmt::Return(Some(expr)) => return (expr_is_number(ctx, expr, cids, depth), true),
            Stmt::Return(None) => return (false, true),
            Stmt::Throw(_) => return (true, true),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                let (then_ok, then_exits) = block_returns_number(ctx, then_branch, cids, depth);
                let (else_ok, else_exits) = else_branch
                    .as_deref()
                    .map(|b| block_returns_number(ctx, b, cids, depth))
                    .unwrap_or((true, false));
                if !then_ok || !else_ok {
                    return (false, false);
                }
                if then_exits && else_exits {
                    return (true, true);
                }
            }
            Stmt::Let { .. } | Stmt::Expr(_) => {}
            _ => return (false, false),
        }
    }
    (true, false)
}

/// The kind mask of `this.key` over the receiver classes `cids`.
fn this_kinds(ctx: &FnCtx<'_>, cids: &[u32], key: &str) -> Option<u16> {
    let mut mask = 0u16;
    for &cid in cids {
        let f = ctx.wpfacts.field(cid, key)?;
        if f.declared {
            return None;
        }
        mask |= f.kind_mask;
    }
    (mask != 0).then_some(mask)
}

fn is_this_field(e: &Expr) -> Option<&str> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => Some(property.as_str()),
        _ => None,
    }
}

/// An expression whose value is a Number whatever the inputs, under the
/// receiver classes `cids`.
fn expr_is_number(ctx: &FnCtx<'_>, e: &Expr, cids: &[u32], depth: u8) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::NumberCoerce(_) | Expr::DateNow => true,
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add => {
                prim_or_number(ctx, left, cids, depth) && prim_or_number(ctx, right, cids, depth)
            }
            BinaryOp::UShr => true,
            // A BigInt result needs two BigInt operands: one non-BigInt
            // operand makes it a Number or a throw.
            _ => non_bigint(ctx, left, cids, depth) || non_bigint(ctx, right, cids, depth),
        },
        Expr::Unary { op, operand } => match op {
            UnaryOp::Pos => true,
            UnaryOp::Neg | UnaryOp::BitNot => non_bigint(ctx, operand, cids, depth),
            UnaryOp::Not => false,
        },
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => expr_is_number(ctx, then_expr, cids, depth) && expr_is_number(ctx, else_expr, cids, depth),
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                targets_return_number(ctx, cids, property, depth + 1)
            }
            _ => false,
        },
        _ => {
            if is_ctx_free_math(e) {
                return true;
            }
            is_this_field(e)
                .and_then(|k| this_kinds(ctx, cids, k))
                .is_some_and(|m| m == K_NUM)
        }
    }
}

/// Number, or a primitive-kind field read.
fn prim_or_number(ctx: &FnCtx<'_>, e: &Expr, cids: &[u32], depth: u8) -> bool {
    expr_is_number(ctx, e, cids, depth)
        || is_this_field(e)
            .and_then(|k| this_kinds(ctx, cids, k))
            .is_some_and(|m| m & !K_PRIMS == 0)
}

/// Provably not a BigInt (and not an object whose conversion could yield one).
fn non_bigint(ctx: &FnCtx<'_>, e: &Expr, cids: &[u32], depth: u8) -> bool {
    expr_is_number(ctx, e, cids, depth)
        || is_this_field(e)
            .and_then(|k| this_kinds(ctx, cids, k))
            .is_some_and(|m| m & (K_BIGINT | K_OBJ | 64) == 0)
}

fn is_ctx_free_math(e: &Expr) -> bool {
    matches!(
        e,
        Expr::MathFloor(..)
            | Expr::MathCeil(..)
            | Expr::MathRound(..)
            | Expr::MathTrunc(..)
            | Expr::MathSign(..)
            | Expr::MathAbs(..)
            | Expr::MathSqrt(..)
            | Expr::MathLog(..)
            | Expr::MathLog2(..)
            | Expr::MathLog10(..)
            | Expr::MathPow(..)
            | Expr::MathMin(..)
            | Expr::MathMax(..)
            | Expr::MathImul(..)
            | Expr::MathSin(..)
            | Expr::MathCos(..)
            | Expr::MathTan(..)
            | Expr::MathAtan2(..)
            | Expr::MathExp(..)
    )
}
