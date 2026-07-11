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
    // A conditional in callee position — `(c ? a : JSON.parse)(x)` — is itself
    // the callee; its branches are values, not the immediate callee member, so
    // `lowering_call_callee` must not leak into them. Left set, the member-tail
    // reroute-undo collapses a builtin-namespace member in a branch
    // (`JSON.parse`) to the value-less intrinsic form, which lowers to
    // `undefined` → the result throws "value is not a function" when called.
    // See `arm_bin.rs` for the full rationale; a nested *call* branch re-sets
    // the flag for its own callee, so direct intrinsic calls are unaffected.
    let prev_call_callee = ctx.lowering_call_callee;
    ctx.lowering_call_callee = false;
    let condition = lower_expr(ctx, &cond.test);
    let then_expr = lower_expr(ctx, &cond.cons);
    let else_expr = lower_expr(ctx, &cond.alt);
    ctx.lowering_call_callee = prev_call_callee;
    Ok(Expr::Conditional {
        condition: Box::new(condition?),
        then_expr: Box::new(then_expr?),
        else_expr: Box::new(else_expr?),
    })
}

pub(super) fn lower_await(ctx: &mut LoweringContext, await_expr: &ast::AwaitExpr) -> Result<Expr> {
    let inner = Box::new(lower_expr(ctx, &await_expr.arg)?);
    Ok(Expr::Await(inner))
}

pub(super) fn lower_super_prop(
    ctx: &mut LoweringContext,
    super_prop: &ast::SuperPropExpr,
) -> Result<Expr> {
    // `super.<prop>` as a value (NOT followed by a call). Call-form
    // `super.method(...)` is detected in lower_call.rs and routed through
    // SuperMethodCall before this function ever runs, so we only land
    // here for value-form reads like `super._next` (rxjs's
    // OperatorSubscriber), `super.value` (NestJS adapter chains), etc.
    //
    // Ident form (`super.foo`) routes through Expr::SuperPropertyGet so
    // codegen can do an explicit parent-class vtable lookup (issue
    // #774). The previous `this.<prop>` approximation silently
    // returned the child override when the child shadowed the
    // property; strict JS resolves through the parent prototype.
    //
    // Computed form (`super[expr]`) is kept on the `this[expr]`
    // fallback for now — computed super needs a runtime dispatch
    // that's out of scope for #774 (the dominant PR #754 rxjs /
    // NestJS patterns are all ident-form method calls, which go
    // through SuperMethodCall anyway).
    match &super_prop.prop {
        ast::SuperProp::Ident(ident) => {
            if let Some(home_id) = ctx.object_super_home_stack.last().copied() {
                Ok(Expr::ObjectSuperPropertyGet {
                    home: Box::new(Expr::LocalGet(home_id)),
                    key: Box::new(Expr::String(ident.sym.to_string())),
                    receiver: Box::new(Expr::This),
                })
            } else {
                Ok(Expr::SuperPropertyGet {
                    property: ident.sym.to_string(),
                })
            }
        }
        ast::SuperProp::Computed(computed) => {
            if let Some(home_id) = ctx.object_super_home_stack.last().copied() {
                let index = Box::new(lower_expr(ctx, &computed.expr)?);
                Ok(Expr::ObjectSuperPropertyGet {
                    home: Box::new(Expr::LocalGet(home_id)),
                    key: index,
                    receiver: Box::new(Expr::This),
                })
            } else if let Some(key) = match computed.expr.as_ref() {
                ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().map(|s| s.to_string()),
                _ => None,
            } {
                // `super['fromA']` in a CLASS method with a string-literal key:
                // route through the same parent-prototype-chain lookup as the
                // ident form `super.fromA` (Expr::SuperPropertyGet). The previous
                // `this[index]` fallback read the property off the CHILD instance,
                // shadowing the parent value (test262
                // super/prop-expr-cls-val{,-from-arrow}). A truly dynamic computed
                // key (not a literal) still falls back below.
                Ok(Expr::SuperPropertyGet { property: key })
            } else {
                let index = Box::new(lower_expr(ctx, &computed.expr)?);
                Ok(Expr::IndexGet {
                    object: Box::new(Expr::This),
                    index,
                })
            }
        }
    }
}

pub(super) fn lower_update(ctx: &mut LoweringContext, update: &ast::UpdateExpr) -> Result<Expr> {
    // Handle ++x, x++, --x, x--
    let binary_op = match update.op {
        ast::UpdateOp::PlusPlus => BinaryOp::Add,
        ast::UpdateOp::MinusMinus => BinaryOp::Sub,
    };

    // Unwrap compile-time-only wrappers: `obj.x!++` (TS non-null assertion) and
    // `(obj.x)++` (parenthesized) are transparent to update-expression lowering.
    let mut arg = update.arg.as_ref();
    loop {
        match arg {
            ast::Expr::TsNonNull(inner) => arg = inner.expr.as_ref(),
            ast::Expr::Paren(inner) => arg = inner.expr.as_ref(),
            _ => break,
        }
    }

    match arg {
        // Simple identifier: x++ or ++x
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            let Some(id) = ctx.lookup_local(&name) else {
                // `++x` / `x--` on a name with no lexical binding is a (sloppy)
                // global property reference: read-modify-write globalThis[x] at
                // runtime (`for (i = 0; i < n; i++)` with an undeclared `i` —
                // #3575). The helper throws the spec ReferenceError only when
                // the property is genuinely absent (`++neverDeclared` —
                // test262 prefix/postfix S11.4.4_A2.1_T2), so this still both
                // compiles and throws at the right time; `if (false) { ++x }`
                // never reaches the helper at runtime.
                let is_increment = matches!(update.op, ast::UpdateOp::PlusPlus);
                return Ok(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "js_global_update".to_string(),
                        param_types: vec![
                            perry_types::Type::Any,
                            perry_types::Type::Any,
                            perry_types::Type::Any,
                        ],
                        return_type: perry_types::Type::Any,
                    }),
                    args: vec![
                        Expr::String(name),
                        Expr::Bool(is_increment),
                        Expr::Bool(update.prefix),
                    ],
                    type_args: vec![],
                    byte_offset: 0,
                });
            };
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
        // #6078: template substitutions use ToString, NOT the `+` operator's
        // ToPrimitive(default) hint. `+` with a string operand coerces the other
        // side valueOf-first, so `` `${obj}` `` (obj with both valueOf+toString)
        // disagreed with `String(obj)`. Wrap each substitution in `StringCoerce`
        // (the same `js_string_coerce`/ToString that `String(x)` uses), so it is
        // toString-first and the concat sees a plain string. No-op for
        // string/number substitutions; fixes the object case.
        result = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(result),
            right: Box::new(Expr::StringCoerce(Box::new(lowered))),
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
    // A comma sequence in callee position — `(a, JSON.parse)(x)` — is itself
    // the callee; its elements are values, not the immediate callee member, so
    // `lowering_call_callee` must not leak into them (else a builtin-namespace
    // member collapses to the value-less intrinsic form → `undefined` → "value
    // is not a function" when called). See `arm_bin.rs` for the full rationale.
    let prev_call_callee = ctx.lowering_call_callee;
    ctx.lowering_call_callee = false;
    let lowered: Result<Vec<Expr>> = seq.exprs.iter().map(|e| lower_expr(ctx, e)).collect();
    ctx.lowering_call_callee = prev_call_callee;
    let mut exprs = lowered?;
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
    // `import.meta.url`) is intercepted at the Member expression arm
    // (`expr_member::lower_member`) and folded directly to a literal —
    // the Object synthesis below is the fallback for the rare bare
    // `import.meta` use (spread, destructure, JSON.stringify, etc.).
    match meta_prop.kind {
        ast::MetaPropKind::ImportMeta => {
            // Bare `import.meta` reference. Property access goes through
            // the Member arm and folds to a literal directly; this Object
            // is the fallback for the rare cases where the value is used
            // as an object (spread / destructure / passed to a function).
            // Carries the same set of properties the Member arm exposes
            // so `Object.keys(import.meta).includes("url")` still works.
            let (url, dirname, filename) = import_meta_paths(ctx);
            Ok(Expr::Object(vec![
                ("url".to_string(), Expr::String(url)),
                ("main".to_string(), Expr::Bool(ctx.is_entry_module)),
                ("dirname".to_string(), Expr::String(dirname)),
                ("filename".to_string(), Expr::String(filename)),
            ]))
        }
        ast::MetaPropKind::NewTarget => {
            // `new.target` always lowers to the runtime meta-property read
            // (#2768). Codegen resolves it to the active constructor's leaf
            // class ref: for an inlined `new C()` via a `new_target_stack`
            // slot holding `C`'s class ref, and for dynamic dispatch
            // (`Reflect.construct`, imported classes) via the `js_new_target_*`
            // cell the construct path sets. The previous in-constructor
            // approximation hardcoded `{ name: <enclosing-class> }`, which made
            // `new.target` the class whose BODY runs (a base class via super())
            // rather than the actual constructed class, and broke
            // `new.target === C` identity (a fresh object never equals the
            // class ref). `ctx.in_constructor_class` is no longer consulted
            // here.
            //
            // ES2025 §15.7.10: class field initializers are invoked with
            // [[Call]] (not [[Construct]]), so [[NewTarget]] is `undefined`
            // throughout the initializer, including inside arrow functions
            // (which inherit it lexically) and direct eval bodies
            // (which share the calling context's new.target binding).
            if ctx.in_class_field_init {
                return Ok(Expr::Undefined);
            }
            Ok(Expr::NewTarget)
        }
    }
}

/// Issue #444: compute the `(url, dirname, filename)` triplet exposed via
/// `import.meta`. Mirrors Node 20+ semantics — `url` is `file://<path>`,
/// `filename` is the absolute file path, `dirname` is its parent directory.
/// Used by both the bare-`import.meta` Object synthesis above and the
/// member-access fast path in `expr_member::lower_member`.
pub(crate) fn import_meta_paths(ctx: &LoweringContext) -> (String, String, String) {
    let path = ctx.source_file_path.replace('\\', "/");
    let url = format!("file://{}", path);
    let dirname = match path.rfind('/') {
        Some(i) if i > 0 => path[..i].to_string(),
        Some(_) => "/".to_string(),
        None => String::new(),
    };
    let filename = path;
    (url, dirname, filename)
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
