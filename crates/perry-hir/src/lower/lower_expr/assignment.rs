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
        ast::Expr::Ident(ident) => {
            let name = ident.sym.to_string();
            if let Some(env_id) = ctx.active_with_envs_for_ident(&name).into_iter().next() {
                let fallback = with_set_fallback_for_ident(ctx, &name);
                return Ok(Expr::WithSet {
                    object: Box::new(Expr::LocalGet(env_id)),
                    property: name,
                    value,
                    fallback,
                    strict: ctx.current_strict,
                });
            }
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalSet(id, value))
            } else if ctx.lookup_class(&name).is_some() || ctx.lookup_func(&name).is_some() {
                // v0.5.757: don't shadow a class/function binding with an
                // implicit local for `<Name> = X` patterns. Drizzle's
                // sql.js uses `((sql2) => { ... })(sql || (sql = {}))` —
                // the binding exists (truthy), the OR short-circuits, and
                // the assignment is dead. Pre-fix the implicit local hid
                // the original binding from later reads. Just evaluate
                // the RHS for side effects. Refs #420.
                Ok(*value)
            } else {
                if ctx.current_strict {
                    // #5989: strict-mode assignment to an existing global
                    // builtin is a property write, not a ReferenceError. See
                    // `strict_global_assign_existing_or_throw` for the full
                    // rationale (shared with the sibling arm in expr_assign.rs).
                    return Ok(strict_global_assign_existing_or_throw(name, value));
                }
                eprintln!(
                    "  Warning: Assignment to undeclared variable '{}', creating sloppy global",
                    name
                );
                // Sloppy implicit global: the binding IS a property of
                // globalThis (spec CreateGlobalVarBinding on the global
                // object), so `foo = 1` must be visible as
                // `globalThis.foo`, write through to a pre-existing global
                // property, and observe a later `delete globalThis.foo`.
                // Reads of the name resolve through the
                // `js_global_get_or_throw_unresolved` fallback, so no
                // module-local shadow may be created here (a stale local
                // would keep serving deleted/overwritten values).
                // NOTE: `GlobalGet(0)` alone is a by-name routing SENTINEL in
                // codegen (bare reads lower to 0.0) — the write must target
                // the VALUE globalThis, which the `PropertyGet { GlobalGet(0),
                // "globalThis" }` shape resolves to the real global object.
                Ok(Expr::PropertySet {
                    object: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::GlobalGet(0)),
                        property: "globalThis".to_string(),
                    }),
                    property: name,
                    value,
                })
            }
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
