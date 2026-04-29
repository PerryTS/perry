//! Small AST→HIR lowering arms extracted from `lower::lower_expr`.
//!
//! Tier 2.3 of the compiler-improvement plan, v0.5.337 (pilot scope).
//! `lower_expr` was a 6,687-line single-`match` function that handled
//! 32 AST variant categories. This module extracts the smallest,
//! self-contained variants — `Cond`, `Await`, `SuperProp`, `Update`,
//! `Tpl`, `Seq`, `MetaProp`, `Yield` — into focused free functions.
//! Each helper takes `&mut LoweringContext` and the SWC AST node, and
//! returns the same `Result<Expr>` that `lower_expr`'s arm did.
//!
//! The match arms in `lower_expr` collapse to one-line delegations.
//! Pattern is the same as Tier 2.1 (compile.rs split) and Tier 2.2
//! (ui_styling extracted from lower_call.rs): `pub(super)` helpers,
//! recursion goes through `super::lower_expr`.
//!
//! **Why these eight specifically**: each arm is well-bounded (10-65
//! LOC), uses only public methods on `LoweringContext`, and doesn't
//! introduce nested helper fns of its own. They're also low-traffic in
//! recent CLAUDE.md history — touching them rarely produces merge
//! conflicts. The bigger arms (`Call` 3986, `Object` 479, `New` 393,
//! `Member` 405, `Assign` 312) are followups: they'd benefit more from
//! extraction in absolute LOC, but each carries its own helper fns and
//! cross-references that need careful coordination.

use anyhow::{anyhow, Result};
use swc_ecma_ast as ast;

use crate::ir::{BinaryOp, Expr, UpdateOp};
use crate::lower_patterns::unescape_template;

use super::{lower_expr, LoweringContext};

pub(super) fn lower_cond(ctx: &mut LoweringContext, cond: &ast::CondExpr) -> Result<Expr> {
    let condition = Box::new(lower_expr(ctx, &cond.test)?);
    let then_expr = Box::new(lower_expr(ctx, &cond.cons)?);
    let else_expr = Box::new(lower_expr(ctx, &cond.alt)?);
    Ok(Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    })
}

pub(super) fn lower_await(ctx: &mut LoweringContext, await_expr: &ast::AwaitExpr) -> Result<Expr> {
    let inner = Box::new(lower_expr(ctx, &await_expr.arg)?);
    Ok(Expr::Await(inner))
}

pub(super) fn lower_super_prop(
    _ctx: &mut LoweringContext,
    super_prop: &ast::SuperPropExpr,
) -> Result<Expr> {
    // super.property access — used in super.method() calls. When used
    // as a call target, the Call expression detects this and routes
    // through SuperMethodCall. Direct super property access (without
    // the trailing call) is a syntax form Perry hasn't implemented yet.
    match &super_prop.prop {
        ast::SuperProp::Ident(_ident) => crate::lower_bail!(
            super_prop.span,
            "Direct super property access not yet supported, use super.method()"
        ),
        ast::SuperProp::Computed(_) => crate::lower_bail!(
            super_prop.span,
            "Computed super property access not supported"
        ),
    }
}

pub(super) fn lower_update(ctx: &mut LoweringContext, update: &ast::UpdateExpr) -> Result<Expr> {
    // Handle ++x, x++, --x, x--
    let binary_op = match update.op {
        ast::UpdateOp::PlusPlus => BinaryOp::Add,
        ast::UpdateOp::MinusMinus => BinaryOp::Sub,
    };

    match update.arg.as_ref() {
        // Simple identifier: x++ or ++x
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            let id = ctx
                .lookup_local(&name)
                .ok_or_else(|| anyhow!("Undefined variable in update expression: {}", name))?;
            let op = match update.op {
                ast::UpdateOp::PlusPlus => UpdateOp::Increment,
                ast::UpdateOp::MinusMinus => UpdateOp::Decrement,
            };
            Ok(Expr::Update {
                id,
                op,
                prefix: update.prefix,
            })
        }
        // Member expression: this.count++ or obj.prop++ or obj[key]++
        ast::Expr::Member(member) => {
            let object = lower_expr(ctx, &member.obj)?;
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    // Desugar: this.count++ becomes (tmp = this.count, this.count = tmp + 1, tmp)
                    // For prefix ++this.count becomes (this.count = this.count + 1, this.count)
                    // We simplify to just: this.count = this.count + 1
                    // The return value semantics are handled at codegen
                    Ok(Expr::PropertyUpdate {
                        object: Box::new(object),
                        property,
                        op: binary_op,
                        prefix: update.prefix,
                    })
                }
                ast::MemberProp::PrivateName(priv_name) => {
                    let property = format!("#{}", priv_name.name);
                    Ok(Expr::PropertyUpdate {
                        object: Box::new(object),
                        property,
                        op: binary_op,
                        prefix: update.prefix,
                    })
                }
                ast::MemberProp::Computed(comp) => {
                    // Computed property: obj[key]++
                    let index = lower_expr(ctx, &comp.expr)?;
                    Ok(Expr::IndexUpdate {
                        object: Box::new(object),
                        index: Box::new(index),
                        op: binary_op,
                        prefix: update.prefix,
                    })
                }
            }
        }
        _ => Err(anyhow!(
            "Update expression only supports identifiers and member expressions"
        )),
    }
}

pub(super) fn lower_tpl(ctx: &mut LoweringContext, tpl: &ast::Tpl) -> Result<Expr> {
    // Template literal: `Hello, ${name}!`
    // quasis = ["Hello, ", "!"], exprs = [name]
    // We desugar this to string concatenation.
    if tpl.quasis.is_empty() {
        return Ok(Expr::String(String::new()));
    }

    // Start with the first quasi
    let first_raw = tpl.quasis.first().map(|q| q.raw.as_ref()).unwrap_or("");
    let mut result = Expr::String(unescape_template(first_raw));

    // Interleave expressions and remaining quasis
    for (i, expr) in tpl.exprs.iter().enumerate() {
        let lowered = lower_expr(ctx, expr)?;
        // Concatenate: result + toString(expr)
        result = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(result),
            right: Box::new(lowered),
        };

        // Add the next quasi (if it's non-empty)
        if let Some(quasi) = tpl.quasis.get(i + 1) {
            let quasi_str: &str = quasi.raw.as_ref();
            if !quasi_str.is_empty() {
                result = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(result),
                    right: Box::new(Expr::String(unescape_template(quasi_str))),
                };
            }
        }
    }

    Ok(result)
}

pub(super) fn lower_seq(ctx: &mut LoweringContext, seq: &ast::SeqExpr) -> Result<Expr> {
    // Comma operator: evaluate all expressions left-to-right, return
    // the last value. e.g., `(a++, b++, c)` evaluates a++, then b++,
    // then returns c. All sub-exprs run for side effects (the for-loop
    // update slot uses this when chaining `it3--, i++`).
    let mut exprs = Vec::new();
    for expr in &seq.exprs {
        exprs.push(lower_expr(ctx, expr)?);
    }
    if exprs.len() == 1 {
        Ok(exprs.pop().unwrap())
    } else {
        Ok(Expr::Sequence(exprs))
    }
}

pub(super) fn lower_meta_prop(
    ctx: &mut LoweringContext,
    meta_prop: &ast::MetaPropExpr,
) -> Result<Expr> {
    // import.meta / new.target. Property access on either (e.g.
    // `import.meta.url`) is handled by the Member expression arm.
    match meta_prop.kind {
        ast::MetaPropKind::ImportMeta => {
            // Return the file:// URL directly for `import.meta.url`.
            // Since `import.meta` is typically only used via `.url`,
            // we synthesise a small one-field object so the Member
            // arm's PropertyGet path resolves it.
            let file_url = format!("file://{}", ctx.source_file_path);
            Ok(Expr::Object(vec![(
                "url".to_string(),
                Expr::String(file_url),
            )]))
        }
        ast::MetaPropKind::NewTarget => {
            // Inside a class constructor, `new.target` evaluates to the
            // class itself. We approximate this with a small object
            // literal `{ name: <class_name> }` so:
            //   - `new.target ? a : b` is truthy → takes the `a` branch
            //   - `new.target.name` returns the class name string
            // Outside a constructor (e.g., a regular function called
            // without `new`), `new.target` is `undefined`.
            if let Some(class_name) = ctx.in_constructor_class.clone() {
                Ok(Expr::Object(vec![(
                    "name".to_string(),
                    Expr::String(class_name),
                )]))
            } else {
                Ok(Expr::Undefined)
            }
        }
    }
}

pub(super) fn lower_yield(ctx: &mut LoweringContext, y: &ast::YieldExpr) -> Result<Expr> {
    let value = match &y.arg {
        Some(arg) => Some(Box::new(lower_expr(ctx, arg)?)),
        None => None,
    };
    Ok(Expr::Yield {
        value,
        delegate: y.delegate,
    })
}
