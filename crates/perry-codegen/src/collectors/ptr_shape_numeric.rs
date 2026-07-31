//! Number-by-construction proof for `collectors/ptr_shape.rs`'s numeric-field
//! rule: does this expression evaluate to a JS Number for every input, per
//! spec, never a string / BigInt / bool / undefined / pointer?
//!
//! Split out of `ptr_shape.rs` to stay under the 2000-line CI gate; declared
//! there with `#[path]` so it remains a child module and can reach the
//! collector's private items through `use super::*`.

use super::*;
/// Number-by-construction: the expression's runtime value is a JS Number for
/// every input, per spec — never a string/BigInt/bool/undefined/pointer.
pub(super) fn expr_numeric_by_construction(
    e: &Expr,
    param_env: &ParamEnv<'_>,
    members: &HashSet<u32>,
    numeric_fields: &HashSet<String>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&Expr>>,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    use perry_hir::BinaryOp;
    let rec = |x: &Expr| {
        expr_numeric_by_construction(
            x,
            param_env,
            members,
            numeric_fields,
            not_bigint_locals,
            const_local_inits,
            depth + 1,
        )
    };
    match e {
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::Unary { op, operand } => match op {
            perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::BitNot => {
                rec(operand)
            }
            _ => false,
        },
        Expr::Binary { op, left, right } => match op {
            // `+` concatenates strings; both sides must be numbers.
            BinaryOp::Add => rec(left) && rec(right),
            // `- * / %` produce BigInt only for BigInt⊗BigInt; a provably
            // non-BigInt operand forces the Number path.
            // `- * / %` produce a BigInt only for BigInt⊗BigInt; mixing a
            // BigInt with anything else THROWS (no value is stored). ONE
            // provably-non-BigInt operand therefore forces the completed
            // result onto the Number path.
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                (rec(left) && rec(right))
                    || expr_provably_not_bigint(left, not_bigint_locals)
                    || expr_provably_not_bigint(right, not_bigint_locals)
            }
            // Same either-side argument for the BigInt-capable bitwise ops.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                (rec(left) && rec(right))
                    || expr_provably_not_bigint(left, not_bigint_locals)
                    || expr_provably_not_bigint(right, not_bigint_locals)
            }
            // `>>>` throws for BigInt operands; result is always a Number.
            BinaryOp::UShr => true,
            _ => false,
        },
        Expr::NumberCoerce(_)
        | Expr::ParseFloat(_)
        | Expr::ParseInt { .. }
        | Expr::MathSqrt(_)
        | Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathSign(_)
        | Expr::MathAbs(_)
        | Expr::MathF16round(_)
        | Expr::MathPow(..)
        | Expr::MathMin(_)
        | Expr::MathMax(_)
        | Expr::MathMinSpread(_)
        | Expr::MathMaxSpread(_)
        | Expr::DateNow
        | Expr::PerformanceNow => true,
        // A proven-numeric field of the SAME object (fixpoint edge): `this`
        // inside the candidate's ctor/method contexts (a non-None param env),
        // or a tracked member local in function scope. A same-named field of
        // a DIFFERENT object proves nothing.
        Expr::PropertyGet {
            object, property, ..
        } if match object.as_ref() {
            Expr::This => !matches!(param_env, ParamEnv::None),
            Expr::LocalGet(id) => members.contains(id),
            _ => false,
        } =>
        {
            numeric_fields.contains(property)
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => rec(then_expr) && rec(else_expr),
        Expr::Sequence(es) => es.last().map(|x| rec(x)).unwrap_or(false),
        // A parameter: numeric iff every recorded call site passes a numeric
        // argument at that position (missing argument = `undefined`, not
        // numeric). No recorded sites = unproven.
        Expr::LocalGet(id) => {
            match param_env {
                ParamEnv::Sites { param_ids, sites } => {
                    if let Some(pos) = param_ids.iter().position(|p| p == id) {
                        return !sites.is_empty()
                            && sites.iter().all(|args| {
                                args.get(pos).map(|a| {
                                    expr_numeric_by_construction(
                                        a,
                                        &ParamEnv::None,
                                        members,
                                        numeric_fields,
                                        not_bigint_locals,
                                        const_local_inits,
                                        depth + 1,
                                    )
                                }) == Some(true)
                            });
                    }
                }
                ParamEnv::Resolved(env) => {
                    if let Some(&ok) = env.get(id) {
                        return ok;
                    }
                }
                ParamEnv::None => {
                    // A single-Let const temp: chase its init (function
                    // scope, so no parameter mapping applies to it).
                    if let Some(Some(init)) = const_local_inits.get(id) {
                        return expr_numeric_by_construction(
                            init,
                            &ParamEnv::None,
                            members,
                            numeric_fields,
                            not_bigint_locals,
                            const_local_inits,
                            depth + 1,
                        );
                    }
                }
            }
            false
        }
        _ => false,
    }
}
