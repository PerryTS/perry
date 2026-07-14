//! `lower_expr_assignment` — lowering of assignment-target expressions.
//! Extracted from the trunk `lower_expr.rs`. Pure code move.

use super::*;
use crate::lower::*;
use anyhow::{anyhow, Result};
use perry_types::LocalId;
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

pub(crate) fn lower_expr_assignment(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
    value: Box<Expr>,
) -> Result<Expr> {
    match expr {
        // #6300: identifier stores — including the ones reached by unwrapping a
        // parenthesized / TS-cast assignment target below — go through the one
        // shared helper, so the `const`-immutability check can't be bypassed by
        // writing `(c as any) = 9` instead of `c = 9`.
        ast::Expr::Ident(ident) => {
            crate::lower::expr_assign::lower_ident_assignment(ctx, ident.sym.to_string(), value)
        }
        ast::Expr::Member(member) => {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                // #5938 follow-up: resolve scope-local class renames so a
                // colliding body-local `class X`'s static write targets the
                // renamed registrant, not the first same-named one.
                let obj_name = ctx.resolve_class_name(obj_ident.sym.as_ref());
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldSet {
                                class_name: obj_name,
                                field_name,
                                value,
                            });
                        }
                    }
                }
            }
            let object_expr = lower_expr(ctx, &member.obj)?;
            // #5437: `PutValueSet` (and the private-name `PropertySet`) carry
            // the object expression in BOTH `target` and `receiver`, and codegen
            // evaluates both. When the object is itself an assignment to a local
            // — Next.js' React renderer does
            // `(r = n2(t = new nX(...), ...)).parentFlushed = !0` — duplicating
            // it re-runs the assignment (and the nested `new nX`), constructing
            // the Request twice with the now-reassigned `t` so its
            // resumableState became another Request → dynamic-SSR 500. Evaluate
            // the assignment ONCE as a prelude and read the just-assigned local
            // back from both slots (reusing the assignment's own already-slotted
            // local — a fresh temp gets no codegen stack slot in expression
            // position). Pure / non-assignment objects keep the long-standing
            // duplicate-in-place shape so codegen fast paths and IR are
            // unchanged.
            let reuse_id = if let Expr::LocalSet(set_id, _) = &object_expr {
                Some(*set_id)
            } else {
                None
            };
            let (prelude, object): (Option<Expr>, Box<Expr>) = match reuse_id {
                Some(id) => (Some(object_expr), Box::new(Expr::LocalGet(id))),
                None => (None, Box::new(object_expr.clone())),
            };
            let result = match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    // Issue #711 part 2: `<expr>.prototype = <value>`
                    // pattern (Effect's effectable.ts uses this to
                    // declare prototype-based classes — `function
                    // Base() {}; Base.prototype = CommitPrototype`).
                    // Route through the SetFunctionPrototype HIR node
                    // so codegen calls
                    // `js_set_function_prototype(func, proto)`, which
                    // allocates a synthetic class id keyed by the
                    // function value. The runtime helper is a no-op
                    // when `object` doesn't evaluate to a function
                    // (preserves baseline for legitimate
                    // `someClass.prototype = X` writes on non-function
                    // values).
                    if property == "prototype" {
                        Expr::SetFunctionPrototype {
                            func: object,
                            proto: value,
                        }
                    } else {
                        Expr::PutValueSet {
                            target: object.clone(),
                            key: Box::new(Expr::String(property)),
                            value,
                            receiver: object,
                            strict: ctx.current_strict,
                        }
                    }
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    Expr::PutValueSet {
                        target: object.clone(),
                        key: index,
                        value,
                        receiver: object,
                        strict: ctx.current_strict,
                    }
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
                    let object = expr_member::wrap_private_guard(
                        ctx,
                        object,
                        &property,
                        expr_member::PRIV_OP_WRITE,
                    );
                    Expr::PropertySet {
                        object,
                        property,
                        value,
                    }
                }
            };
            Ok(match prelude {
                Some(p) => Expr::Sequence(vec![p, result]),
                None => result,
            })
        }
        // Recursively unwrap parens and type annotations
        ast::Expr::Paren(paren) => lower_expr_assignment(ctx, &paren.expr, value),
        ast::Expr::TsAs(ts_as) => lower_expr_assignment(ctx, &ts_as.expr, value),
        ast::Expr::TsNonNull(ts_nn) => lower_expr_assignment(ctx, &ts_nn.expr, value),
        ast::Expr::TsTypeAssertion(ts_ta) => lower_expr_assignment(ctx, &ts_ta.expr, value),
        ast::Expr::TsSatisfies(ts_sat) => lower_expr_assignment(ctx, &ts_sat.expr, value),
        _ => Err(anyhow!(
            "Unsupported expression as assignment target: {:?}",
            expr
        )),
    }
}
