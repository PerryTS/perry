//! `for-in` / `for-of` loop-head target resolution.
//!
//! Extracted from `lower/stmt_loops.rs` (2,000-LOC cap). Resolves every
//! legal loop-head shape — fresh decl bindings (ident + destructuring
//! patterns), bare-ident / member-expression assignment targets, and
//! destructuring-assignment heads — into a `ForHeadBinding` consumed by
//! the for-of / for-in desugars in `stmt_loops.rs` and
//! `lower_decl/body_stmt.rs`.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

/// Pre-resolved `for-in` / `for-of` head target. Built BEFORE the loop body
/// is lowered (so pattern leaves are in scope for body references), consumed
/// AFTER (to build the per-iteration binding statements).
pub(crate) enum ForHeadBinding {
    /// `for (var/let/const x …)` — fresh per-loop binding.
    DeclIdent { name: String, id: LocalId },
    /// `for (var/let/const [x, y] …)` / object patterns — leaves pre-defined.
    DeclPattern {
        pat: ast::Pat,
        var_ids: Vec<(String, LocalId)>,
    },
    /// `for (x …)` / `for ((x) …)` where `x` resolves to an existing
    /// binding — plain assignment each iteration (the binding leaks).
    AssignLocal { id: LocalId },
    /// `for (x.y …)` / `for (x[k] …)` — member store each iteration.
    AssignMember { member: ast::MemberExpr },
    /// `for ([a, b] …)` with pre-existing targets — destructuring
    /// assignment each iteration.
    AssignPattern { pat: ast::AssignTargetPat },
}

fn unwrap_parens_expr(mut e: &ast::Expr) -> &ast::Expr {
    while let ast::Expr::Paren(p) = e {
        e = &p.expr;
    }
    e
}

/// Phase A: resolve the head, defining any fresh bindings so the loop body
/// (lowered next) sees them. `elem_ty` types a simple decl-ident binding.
pub(crate) fn predefine_for_head(
    ctx: &mut LoweringContext,
    left: &ast::ForHead,
    elem_ty: Type,
) -> Result<ForHeadBinding> {
    match left {
        ast::ForHead::VarDecl(var_decl) => {
            let decl = var_decl
                .decls
                .first()
                .ok_or_else(|| anyhow!("for head requires a variable declaration"))?;
            match &decl.name {
                ast::Pat::Ident(ident) => {
                    let name = ident.id.sym.to_string();
                    let id = ctx.define_local(name.clone(), elem_ty);
                    if var_decl.kind == ast::VarDeclKind::Const {
                        // `for (const k in/of …) { k = 1; }` → TypeError.
                        ctx.mark_local_immutable(id);
                    }
                    Ok(ForHeadBinding::DeclIdent { name, id })
                }
                pat => {
                    let mut var_ids = Vec::new();
                    collect_for_of_pattern_leaves(ctx, pat, &mut var_ids);
                    Ok(ForHeadBinding::DeclPattern {
                        pat: pat.clone(),
                        var_ids,
                    })
                }
            }
        }
        ast::ForHead::Pat(pat) => match pat.as_ref() {
            ast::Pat::Ident(ident) => {
                let name = ident.id.sym.to_string();
                let id = ctx
                    .lookup_local(&name)
                    .unwrap_or_else(|| ctx.define_sloppy_implicit_global(name));
                Ok(ForHeadBinding::AssignLocal { id })
            }
            ast::Pat::Expr(expr) => match unwrap_parens_expr(expr) {
                ast::Expr::Ident(ident) => {
                    let name = ident.sym.to_string();
                    let id = ctx
                        .lookup_local(&name)
                        .unwrap_or_else(|| ctx.define_sloppy_implicit_global(name));
                    Ok(ForHeadBinding::AssignLocal { id })
                }
                ast::Expr::Member(member) => Ok(ForHeadBinding::AssignMember {
                    member: member.clone(),
                }),
                other => Err(anyhow!(
                    "Unsupported for-in/for-of head expression: {:?}",
                    std::mem::discriminant(other)
                )),
            },
            ast::Pat::Array(arr_pat) => Ok(ForHeadBinding::AssignPattern {
                pat: ast::AssignTargetPat::Array(arr_pat.clone()),
            }),
            ast::Pat::Object(obj_pat) => Ok(ForHeadBinding::AssignPattern {
                pat: ast::AssignTargetPat::Object(obj_pat.clone()),
            }),
            other => Err(anyhow!(
                "Unsupported for-in/for-of head pattern: {:?}",
                std::mem::discriminant(other)
            )),
        },
        _ => Err(anyhow!("Unsupported for-in/for-of left-hand side")),
    }
}

/// Phase B: build the per-iteration statements that bind/assign `source`
/// (the current key/element) into the head target.
pub(crate) fn for_head_binding_stmts(
    ctx: &mut LoweringContext,
    binding: &ForHeadBinding,
    source: Expr,
    elem_ty: Type,
) -> Result<Vec<Stmt>> {
    match binding {
        ForHeadBinding::DeclIdent { name, id } => Ok(vec![Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: elem_ty,
            mutable: false,
            init: Some(source),
        }]),
        ForHeadBinding::DeclPattern { pat, var_ids } => {
            let mut out = Vec::new();
            let mut var_idx = 0usize;
            // An array pattern iterates the bound value — for for-in keys
            // (strings) that means destructuring by code point. ForOfToArray
            // handles strings/arrays/iterables uniformly.
            let source = if matches!(pat, ast::Pat::Array(_)) {
                Expr::ForOfToArray(Box::new(source))
            } else {
                source
            };
            crate::lower::emit_for_of_pattern_binding(
                ctx,
                pat,
                source,
                var_ids,
                &mut var_idx,
                &mut out,
            )?;
            Ok(out)
        }
        ForHeadBinding::AssignLocal { id } => {
            Ok(vec![Stmt::Expr(Expr::LocalSet(*id, Box::new(source)))])
        }
        ForHeadBinding::AssignMember { member } => {
            let object = Box::new(lower_expr(ctx, &member.obj)?);
            let assign = match &member.prop {
                ast::MemberProp::Ident(prop) => Expr::PropertySet {
                    object,
                    property: prop.sym.to_string(),
                    value: Box::new(source),
                },
                ast::MemberProp::Computed(c) => Expr::IndexSet {
                    object,
                    index: Box::new(lower_expr(ctx, &c.expr)?),
                    value: Box::new(source),
                },
                ast::MemberProp::PrivateName(p) => {
                    // `for (o.#f of iter)` — assign each iteration value to the
                    // private field, brand-guarding the receiver (write op) so a
                    // receiver without the field throws TypeError per spec
                    // (test262 elements/privatefieldset-typeerror-6/7).
                    let property = format!("#{}", p.name);
                    let object = crate::lower::expr_member::wrap_private_guard(
                        ctx,
                        object,
                        &property,
                        crate::lower::expr_member::PRIV_OP_WRITE,
                    );
                    Expr::PropertySet {
                        object,
                        property,
                        value: Box::new(source),
                    }
                }
            };
            Ok(vec![Stmt::Expr(assign)])
        }
        ForHeadBinding::AssignPattern { pat } => {
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__forhead_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            let mut out = vec![Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            }];
            out.extend(
                crate::destructuring::lower_destructuring_assignment_stmt_from_local(
                    ctx, pat, tmp_id,
                )?,
            );
            Ok(out)
        }
    }
}
