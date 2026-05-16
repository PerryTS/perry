//! Destructuring lowering.
//!
//! Contains functions for lowering destructuring assignments and variable
//! declarations with destructuring patterns.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

/// Recognize the ink-shape `useState(initial)` pattern when the
/// callee is a perry/tui-imported `useState`. Returns a rewritten
/// HIR expression that calls `useStateTuple` instead — returning a
/// real `[value, setter]` array — when the pattern matches. Otherwise
/// returns None and the caller falls back to standard lowering.
///
/// Only handles the direct-call shape `useState(x)`. Member-call
/// shapes `tui.useState(x)` are also recognized when the namespace
/// resolves to perry/tui.
fn rewrite_use_state_tuple(ctx: &mut LoweringContext, init: &ast::Expr) -> Option<Expr> {
    let call = match init {
        ast::Expr::Call(c) => c,
        _ => return None,
    };
    let (is_use_state, method) = match &call.callee {
        ast::Callee::Expr(e) => match e.as_ref() {
            ast::Expr::Ident(id) => {
                let name = id.sym.as_ref();
                let m = ctx.lookup_native_module(name);
                match m {
                    Some(("perry/tui", Some("useState"))) | Some(("perry/tui", None))
                        if name == "useState" =>
                    {
                        (true, "useStateTuple")
                    }
                    _ => (false, ""),
                }
            }
            ast::Expr::Member(m) => {
                if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                    (m.obj.as_ref(), &m.prop)
                {
                    if prop.sym.as_ref() == "useState" {
                        match ctx.lookup_native_module(obj.sym.as_ref()) {
                            Some(("perry/tui", _)) => (true, "useStateTuple"),
                            _ => (false, ""),
                        }
                    } else {
                        (false, "")
                    }
                } else {
                    (false, "")
                }
            }
            _ => (false, ""),
        },
        _ => return None,
    };
    if !is_use_state {
        return None;
    }
    let mut arg_exprs: Vec<Expr> = Vec::new();
    for a in &call.args {
        if a.spread.is_some() {
            // Don't rewrite if user code spreads — let the standard path error/handle.
            return None;
        }
        arg_exprs.push(lower_expr(ctx, &a.expr).ok()?);
    }
    Some(Expr::NativeMethodCall {
        module: "perry/tui".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args: arg_exprs,
    })
}

/// True iff `e` contains an `ast::Expr::Arrow` or `ast::Expr::Fn` at
/// any depth. Used by the let-decl pre-registration path (#593) to
/// extend the issue-#461 self-recursion fix to indirect shapes —
/// `const f = wrap(() => f())` (closure inside a Call), `const sub =
/// subject.subscribe({ next: () => sub.unsubscribe() })` (closure
/// inside an Object), `const h = handlers.map(b => () => h())[0]`
/// (closure inside an Array+Member chain). The cost of a recursive
/// scan over the init AST is negligible — every let-decl runs through
/// it once at HIR lowering time.
fn ast_expr_contains_function_expr(e: &ast::Expr) -> bool {
    use ast::Expr;
    match e {
        Expr::Arrow(_) | Expr::Fn(_) => true,
        Expr::Call(c) => {
            (match &c.callee {
                ast::Callee::Expr(e) => ast_expr_contains_function_expr(e),
                _ => false,
            }) || c
                .args
                .iter()
                .any(|a| ast_expr_contains_function_expr(&a.expr))
        }
        Expr::New(n) => {
            ast_expr_contains_function_expr(&n.callee)
                || n.args.as_ref().map_or(false, |args| {
                    args.iter()
                        .any(|a| ast_expr_contains_function_expr(&a.expr))
                })
        }
        Expr::Member(m) => ast_expr_contains_function_expr(&m.obj),
        Expr::Object(o) => o.props.iter().any(|p| match p {
            ast::PropOrSpread::Spread(s) => ast_expr_contains_function_expr(&s.expr),
            ast::PropOrSpread::Prop(p) => match &**p {
                ast::Prop::KeyValue(kv) => ast_expr_contains_function_expr(&kv.value),
                ast::Prop::Method(_) | ast::Prop::Getter(_) | ast::Prop::Setter(_) => true,
                _ => false,
            },
        }),
        Expr::Array(a) => a
            .elems
            .iter()
            .filter_map(|e| e.as_ref())
            .any(|e| ast_expr_contains_function_expr(&e.expr)),
        Expr::Bin(b) => {
            ast_expr_contains_function_expr(&b.left) || ast_expr_contains_function_expr(&b.right)
        }
        Expr::Unary(u) => ast_expr_contains_function_expr(&u.arg),
        Expr::Cond(c) => {
            ast_expr_contains_function_expr(&c.test)
                || ast_expr_contains_function_expr(&c.cons)
                || ast_expr_contains_function_expr(&c.alt)
        }
        Expr::Paren(p) => ast_expr_contains_function_expr(&p.expr),
        Expr::TsAs(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::TsNonNull(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::TsTypeAssertion(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::TsSatisfies(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::Assign(a) => ast_expr_contains_function_expr(&a.right),
        Expr::Seq(s) => s.exprs.iter().any(|e| ast_expr_contains_function_expr(e)),
        Expr::Tpl(t) => t.exprs.iter().any(|e| ast_expr_contains_function_expr(e)),
        Expr::TaggedTpl(t) => {
            ast_expr_contains_function_expr(&t.tag)
                || t.tpl
                    .exprs
                    .iter()
                    .any(|e| ast_expr_contains_function_expr(e))
        }
        Expr::OptChain(o) => match &*o.base {
            ast::OptChainBase::Member(m) => ast_expr_contains_function_expr(&m.obj),
            ast::OptChainBase::Call(c) => {
                ast_expr_contains_function_expr(&c.callee)
                    || c.args
                        .iter()
                        .any(|a| ast_expr_contains_function_expr(&a.expr))
            }
        },
        _ => false,
    }
}

pub(crate) fn lower_destructuring_assignment_stmt(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    rhs: &ast::Expr,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    // First, evaluate and store the RHS in a temporary variable
    let rhs_expr = lower_expr(ctx, rhs)?;
    let tmp_id = ctx.fresh_local();
    let tmp_name = format!("__destruct_{}", tmp_id);
    let tmp_ty = Type::Any; // Could infer from rhs, but Any is safe
    ctx.locals.push((tmp_name.clone(), tmp_id, tmp_ty.clone()));

    result.push(Stmt::Let {
        id: tmp_id,
        name: tmp_name,
        ty: tmp_ty,
        mutable: false,
        init: Some(rhs_expr),
    });

    // Now generate assignments from the temp
    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                if let Some(elem_pat) = elem {
                    let index_expr = Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(tmp_id)),
                        index: Box::new(Expr::Number(idx as f64)),
                    };

                    match elem_pat {
                        ast::Pat::Ident(ident) => {
                            let name = ident.id.sym.to_string();
                            if let Some(id) = ctx.lookup_local(&name) {
                                result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(index_expr))));
                            } else {
                                return Err(anyhow!(
                                    "Assignment to undeclared variable in destructuring: {}",
                                    name
                                ));
                            }
                        }
                        ast::Pat::Array(nested_arr) => {
                            // Nested array destructuring
                            // First create a temp for this element
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            // Then recursively assign from it
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Array(nested_arr.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Object(nested_obj) => {
                            // Nested object destructuring
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Object(nested_obj.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Expr(inner_expr) => {
                            // Expression pattern like [obj.prop, obj2.prop2] = arr
                            match inner_expr.as_ref() {
                                ast::Expr::Member(member) => {
                                    let object = Box::new(lower_expr(ctx, &member.obj)?);
                                    match &member.prop {
                                        ast::MemberProp::Ident(prop_ident) => {
                                            let property = prop_ident.sym.to_string();
                                            result.push(Stmt::Expr(Expr::PropertySet {
                                                object,
                                                property,
                                                value: Box::new(index_expr),
                                            }));
                                        }
                                        ast::MemberProp::Computed(computed) => {
                                            let index = Box::new(lower_expr(ctx, &computed.expr)?);
                                            result.push(Stmt::Expr(Expr::IndexSet {
                                                object,
                                                index,
                                                value: Box::new(index_expr),
                                            }));
                                        }
                                        _ => {
                                            return Err(anyhow!(
                                                "Unsupported member expression in destructuring assignment"
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    return Err(anyhow!(
                                        "Unsupported expression pattern in destructuring assignment"
                                    ));
                                }
                            }
                        }
                        _ => {
                            // Other patterns (Rest, etc.) - skip for now
                        }
                    }
                }
            }
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            ast::PropName::Num(n) => n.value.to_string(),
                            _ => continue,
                        };

                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: key,
                        };

                        match &*kv.value {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    result
                                        .push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring: {}",
                                        name
                                    ));
                                }
                            }
                            ast::Pat::Array(nested_arr) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Array(nested_arr.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            ast::Pat::Object(nested_obj) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Object(nested_obj.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            _ => {}
                        }
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: name.clone(),
                        };

                        if let Some(id) = ctx.lookup_local(&name) {
                            result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                        } else {
                            return Err(anyhow!(
                                "Assignment to undeclared variable in destructuring: {}",
                                name
                            ));
                        }
                    }
                    ast::ObjectPatProp::Rest(_) => {
                        // Rest pattern - skip for now
                    }
                }
            }
        }
        ast::AssignTargetPat::Invalid(_) => {
            return Err(anyhow!("Invalid assignment target pattern"));
        }
    }

    Ok(result)
}

/// Helper for nested destructuring - assigns from an already-computed local
pub(crate) fn lower_destructuring_assignment_stmt_from_local(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    source_id: LocalId,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                if let Some(elem_pat) = elem {
                    let index_expr = Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(source_id)),
                        index: Box::new(Expr::Number(idx as f64)),
                    };

                    match elem_pat {
                        ast::Pat::Ident(ident) => {
                            let name = ident.id.sym.to_string();
                            if let Some(id) = ctx.lookup_local(&name) {
                                result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(index_expr))));
                            } else {
                                return Err(anyhow!(
                                    "Assignment to undeclared variable in destructuring: {}",
                                    name
                                ));
                            }
                        }
                        ast::Pat::Array(nested_arr) => {
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Array(nested_arr.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Object(nested_obj) => {
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Object(nested_obj.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Expr(inner_expr) => match inner_expr.as_ref() {
                            ast::Expr::Member(member) => {
                                let object = Box::new(lower_expr(ctx, &member.obj)?);
                                match &member.prop {
                                    ast::MemberProp::Ident(prop_ident) => {
                                        let property = prop_ident.sym.to_string();
                                        result.push(Stmt::Expr(Expr::PropertySet {
                                            object,
                                            property,
                                            value: Box::new(index_expr),
                                        }));
                                    }
                                    ast::MemberProp::Computed(computed) => {
                                        let index = Box::new(lower_expr(ctx, &computed.expr)?);
                                        result.push(Stmt::Expr(Expr::IndexSet {
                                            object,
                                            index,
                                            value: Box::new(index_expr),
                                        }));
                                    }
                                    _ => {
                                        return Err(anyhow!(
                                                "Unsupported member expression in destructuring assignment"
                                            ));
                                    }
                                }
                            }
                            _ => {
                                return Err(anyhow!(
                                    "Unsupported expression pattern in destructuring assignment"
                                ));
                            }
                        },
                        _ => {}
                    }
                }
            }
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            ast::PropName::Num(n) => n.value.to_string(),
                            _ => continue,
                        };

                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(source_id)),
                            property: key,
                        };

                        match &*kv.value {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    result
                                        .push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring: {}",
                                        name
                                    ));
                                }
                            }
                            ast::Pat::Array(nested_arr) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Array(nested_arr.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            ast::Pat::Object(nested_obj) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Object(nested_obj.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            _ => {}
                        }
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(source_id)),
                            property: name.clone(),
                        };

                        if let Some(id) = ctx.lookup_local(&name) {
                            result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                        } else {
                            return Err(anyhow!(
                                "Assignment to undeclared variable in destructuring: {}",
                                name
                            ));
                        }
                    }
                    ast::ObjectPatProp::Rest(_) => {}
                }
            }
        }
        ast::AssignTargetPat::Invalid(_) => {
            return Err(anyhow!("Invalid assignment target pattern"));
        }
    }

    Ok(result)
}

/// Recursively lower a binding pattern against a source expression, producing
/// `Let` statements that declare each bound variable.
///
/// This is the single source of truth for destructuring binding patterns. It
/// handles:
/// - `Pat::Ident(x)`     → `let x = <source>`
/// - `Pat::Assign(p = d)`→ `let tmp = <source>; <recurse on p with tmp !== undefined ? tmp : d>`
/// - `Pat::Array([...])`→ materialize source in a temp, then recurse on each
///   element with `tmp[i]` as the source. Handles `Pat::Rest` (last element)
///   via `ArraySlice` and skips holes (`None`) like `[a, , c]`.
/// - `Pat::Object({...})`→ materialize source in a temp, then for each prop
///   recurse on the value pattern with `tmp.key` (or `tmp[expr]` for computed
///   keys) as the source. `Assign` shorthand props apply defaults inline.
///   `Rest` props use `ObjectRest` with the list of explicitly-destructured keys.
pub(crate) fn lower_pattern_binding(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    source: Expr,
    mutable: bool,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();
    lower_pattern_binding_into(ctx, pat, source, mutable, &mut result)?;
    Ok(result)
}

fn lower_pattern_binding_into(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    source: Expr,
    mutable: bool,
    result: &mut Vec<Stmt>,
) -> Result<()> {
    match pat {
        ast::Pat::Ident(ident) => {
            let name = ident.id.sym.to_string();
            let ty = ident
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Any);
            let id = ctx.define_local(name.clone(), ty.clone());
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init: Some(source),
            });
            Ok(())
        }
        ast::Pat::Assign(assign_pat) => {
            // `p = default` — apply default when source is undefined.
            // We also need to treat bare IEEE NaN (e.g., from OOB array reads)
            // as undefined, because Perry's number arrays return NaN rather
            // than TAG_UNDEFINED for out-of-bounds indices.
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            let default_val = lower_expr(ctx, &assign_pat.right)?;
            // If `IsUndefinedOrBareNan(tmp)` then use default, else use tmp.
            let with_default = Expr::Conditional {
                condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(Expr::LocalGet(tmp_id)))),
                then_expr: Box::new(default_val),
                else_expr: Box::new(Expr::LocalGet(tmp_id)),
            };
            lower_pattern_binding_into(ctx, &assign_pat.left, with_default, mutable, result)
        }
        ast::Pat::Array(arr_pat) => {
            // Materialize source into a temp
            let arr_ty = arr_pat
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Array(Box::new(Type::Any)));
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, arr_ty.clone()));
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: arr_ty,
                mutable: false,
                init: Some(source),
            });

            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                let Some(elem_pat) = elem else { continue }; // hole — skip

                if let ast::Pat::Rest(rest_pat) = elem_pat {
                    // Rest element `...rest` — take remaining elements as an array
                    let slice_expr = Expr::ArraySlice {
                        array: Box::new(Expr::LocalGet(tmp_id)),
                        start: Box::new(Expr::Number(idx as f64)),
                        end: None,
                    };
                    lower_pattern_binding_into(ctx, &rest_pat.arg, slice_expr, mutable, result)?;
                    break; // Rest must be last
                }

                let element_source = Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(tmp_id)),
                    index: Box::new(Expr::Number(idx as f64)),
                };
                lower_pattern_binding_into(ctx, elem_pat, element_source, mutable, result)?;
            }
            Ok(())
        }
        ast::Pat::Object(obj_pat) => {
            // Materialize source into a temp
            let obj_ty = obj_pat
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Any);
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, obj_ty.clone()));
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: obj_ty,
                mutable: false,
                init: Some(source),
            });

            // Collect statically-known keys for rest exclusion tracking.
            let mut static_keys: Vec<String> = Vec::new();

            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key_source = match &kv.key {
                            ast::PropName::Ident(ident) => {
                                let key = ident.sym.to_string();
                                static_keys.push(key.clone());
                                Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: key,
                                }
                            }
                            ast::PropName::Str(s) => {
                                let key = s.value.as_str().unwrap_or("").to_string();
                                static_keys.push(key.clone());
                                Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: key,
                                }
                            }
                            ast::PropName::Num(n) => {
                                let key = n.value.to_string();
                                static_keys.push(key.clone());
                                Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: key,
                                }
                            }
                            ast::PropName::Computed(computed) => {
                                // Computed key: const { [prop]: target } = obj
                                // Lower to IndexGet with the computed expression
                                let index_expr = lower_expr(ctx, &computed.expr)?;
                                Expr::IndexGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    index: Box::new(index_expr),
                                }
                            }
                            ast::PropName::BigInt(_) => continue,
                        };
                        lower_pattern_binding_into(ctx, &kv.value, key_source, mutable, result)?;
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        // Shorthand { key } or { key = default }
                        let name = assign.key.sym.to_string();
                        static_keys.push(name.clone());
                        let ty = assign
                            .key
                            .type_ann
                            .as_ref()
                            .map(|ann| extract_ts_type(&ann.type_ann))
                            .unwrap_or(Type::Any);
                        let id = ctx.define_local(name.clone(), ty.clone());

                        let init_value = if let Some(default_expr) = &assign.value {
                            // Materialize the property read into a temp so we
                            // only evaluate it once (important if the property
                            // getter is side-effecting, but also required for
                            // correct NaN detection).
                            let val_tmp_id = ctx.fresh_local();
                            let val_tmp_name = format!("__destruct_{}", val_tmp_id);
                            ctx.locals
                                .push((val_tmp_name.clone(), val_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: val_tmp_id,
                                name: val_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: name.clone(),
                                }),
                            });
                            let default_val = lower_expr(ctx, default_expr)?;
                            Expr::Conditional {
                                condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(
                                    Expr::LocalGet(val_tmp_id),
                                ))),
                                then_expr: Box::new(default_val),
                                else_expr: Box::new(Expr::LocalGet(val_tmp_id)),
                            }
                        } else {
                            Expr::PropertyGet {
                                object: Box::new(Expr::LocalGet(tmp_id)),
                                property: name.clone(),
                            }
                        };
                        result.push(Stmt::Let {
                            id,
                            name,
                            ty,
                            mutable,
                            init: Some(init_value),
                        });
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        // { ...rest } — collect remaining statically-known keys
                        // and use ObjectRest to clone the object without them.
                        let rest_source = Expr::ObjectRest {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            exclude_keys: static_keys.clone(),
                        };
                        lower_pattern_binding_into(ctx, &rest.arg, rest_source, mutable, result)?;
                        break; // Rest must be last
                    }
                }
            }
            Ok(())
        }
        ast::Pat::Rest(_) => {
            // Rest patterns should be handled by their enclosing Array/Object
            Err(anyhow!(
                "Rest pattern outside of array/object destructuring"
            ))
        }
        ast::Pat::Expr(_) => Err(anyhow!(
            "Expression patterns are not supported in binding destructuring"
        )),
        ast::Pat::Invalid(_) => Err(anyhow!("Invalid binding pattern")),
    }
}

/// Lower a destructuring assignment expression.
/// For [a, b] = expr or { a, b } = expr, we generate a Sequence expression:
///   1. Assign each element/property to the corresponding target
///   2. Return the RHS value (assignment expressions evaluate to RHS)
///
/// Note: We reference the RHS value directly multiple times rather than
/// creating a temporary variable, since temps created in expression context
/// aren't visible to codegen. This is safe when the RHS is a simple expression
/// (which is the common case for destructuring).
pub(crate) fn lower_destructuring_assignment(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    value: Box<Expr>,
) -> Result<Expr> {
    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            // Array destructuring assignment: [a, b] = expr
            // Desugar to:
            //   a = expr[0];
            //   b = expr[1];
            //   expr (result)
            //
            // We reference the RHS value directly. This works because:
            // 1. The RHS is typically a local variable or simple expression
            // 2. Creating a temp in expression context is problematic for codegen

            let mut exprs = Vec::new();

            // Now assign each element
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                if let Some(elem_pat) = elem {
                    let index_expr = Expr::IndexGet {
                        object: value.clone(),
                        index: Box::new(Expr::Number(idx as f64)),
                    };

                    match elem_pat {
                        ast::Pat::Ident(ident) => {
                            let name = ident.id.sym.to_string();
                            if let Some(id) = ctx.lookup_local(&name) {
                                exprs.push(Expr::LocalSet(id, Box::new(index_expr)));
                            } else {
                                return Err(anyhow!(
                                    "Assignment to undeclared variable in destructuring: {}",
                                    name
                                ));
                            }
                        }
                        ast::Pat::Expr(inner_expr) => {
                            // Expression pattern like [obj.prop] = arr
                            match inner_expr.as_ref() {
                                ast::Expr::Member(member) => {
                                    let object = Box::new(lower_expr(ctx, &member.obj)?);
                                    match &member.prop {
                                        ast::MemberProp::Ident(prop_ident) => {
                                            let property = prop_ident.sym.to_string();
                                            exprs.push(Expr::PropertySet {
                                                object,
                                                property,
                                                value: Box::new(index_expr),
                                            });
                                        }
                                        ast::MemberProp::Computed(computed) => {
                                            let index = Box::new(lower_expr(ctx, &computed.expr)?);
                                            exprs.push(Expr::IndexSet {
                                                object,
                                                index,
                                                value: Box::new(index_expr),
                                            });
                                        }
                                        _ => {
                                            return Err(anyhow!(
                                                "Unsupported member expression in destructuring"
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    return Err(anyhow!(
                                        "Unsupported expression pattern in destructuring"
                                    ));
                                }
                            }
                        }
                        ast::Pat::Rest(_) => {
                            // Rest pattern in assignment: [...rest] = arr
                            // For now, skip (would need slice operation)
                        }
                        ast::Pat::Array(nested_arr) => {
                            // Nested array destructuring: [[a, b], c] = expr
                            // Recursively lower with the indexed element as the value
                            let nested_target = ast::AssignTargetPat::Array(nested_arr.clone());
                            let nested_expr = lower_destructuring_assignment(
                                ctx,
                                &nested_target,
                                Box::new(index_expr),
                            )?;
                            exprs.push(nested_expr);
                        }
                        ast::Pat::Object(nested_obj) => {
                            // Nested object destructuring: [{ a, b }, c] = expr
                            let nested_target = ast::AssignTargetPat::Object(nested_obj.clone());
                            let nested_expr = lower_destructuring_assignment(
                                ctx,
                                &nested_target,
                                Box::new(index_expr),
                            )?;
                            exprs.push(nested_expr);
                        }
                        _ => {
                            // Other patterns (Assign with default, etc.) - skip for now
                        }
                    }
                }
                // If elem is None, it's a hole like [a, , c] - skip it
            }

            // The result of the assignment is the original RHS value
            exprs.push(*value);

            Ok(Expr::Sequence(exprs))
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            // Object destructuring assignment: { a, b } = expr
            // Desugar to:
            //   a = expr.a;
            //   b = expr.b;
            //   expr (result)

            let mut exprs = Vec::new();

            // Now assign each property
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        // { key: target } - extract obj.key into target
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            ast::PropName::Num(n) => n.value.to_string(),
                            _ => continue, // Skip computed keys
                        };

                        let prop_expr = Expr::PropertyGet {
                            object: value.clone(),
                            property: key,
                        };

                        match &*kv.value {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    exprs.push(Expr::LocalSet(id, Box::new(prop_expr)));
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring: {}",
                                        name
                                    ));
                                }
                            }
                            ast::Pat::Array(nested_arr) => {
                                let nested_target = ast::AssignTargetPat::Array(nested_arr.clone());
                                let nested_expr = lower_destructuring_assignment(
                                    ctx,
                                    &nested_target,
                                    Box::new(prop_expr),
                                )?;
                                exprs.push(nested_expr);
                            }
                            ast::Pat::Object(nested_obj) => {
                                let nested_target =
                                    ast::AssignTargetPat::Object(nested_obj.clone());
                                let nested_expr = lower_destructuring_assignment(
                                    ctx,
                                    &nested_target,
                                    Box::new(prop_expr),
                                )?;
                                exprs.push(nested_expr);
                            }
                            _ => {
                                // Other patterns - skip for now
                            }
                        }
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        // Shorthand: { a } means { a: a }
                        let name = assign.key.sym.to_string();
                        let prop_expr = Expr::PropertyGet {
                            object: value.clone(),
                            property: name.clone(),
                        };

                        if let Some(id) = ctx.lookup_local(&name) {
                            exprs.push(Expr::LocalSet(id, Box::new(prop_expr)));
                        } else {
                            return Err(anyhow!(
                                "Assignment to undeclared variable in destructuring: {}",
                                name
                            ));
                        }
                    }
                    ast::ObjectPatProp::Rest(_) => {
                        // Rest pattern: { ...rest } - skip for now
                    }
                }
            }

            // The result of the assignment is the original RHS value
            exprs.push(*value);

            Ok(Expr::Sequence(exprs))
        }
        ast::AssignTargetPat::Invalid(_) => Err(anyhow!("Invalid assignment target pattern")),
    }
}

/// Lower a variable declaration, handling array destructuring patterns.
/// Returns a vector of statements (multiple for destructuring, single for simple bindings).
pub(crate) fn lower_var_decl_with_destructuring(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    mutable: bool,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match &decl.name {
        ast::Pat::Ident(ident) => {
            // Simple binding: let x = expr
            let name = ident.id.sym.to_string();

            // #809: tag locals provably bound to a plain object (an object
            // literal or `Object.create(...)`). `static_receiver_class`
            // consults this so `x.toJSON()` / `.toString()` / `.valueOf()`
            // etc. on such a local fall through to generic dynamic dispatch
            // instead of the Date intrinsics (which would interpret the
            // object pointer's bits as a timestamp).
            if let Some(init_expr) = decl.init.as_deref() {
                let is_plain_object = match init_expr {
                    ast::Expr::Object(_) => true,
                    ast::Expr::Call(call) => {
                        if let ast::Callee::Expr(callee) = &call.callee {
                            matches!(
                                callee.as_ref(),
                                ast::Expr::Member(m)
                                    if matches!(m.obj.as_ref(), ast::Expr::Ident(o) if o.sym.as_ref() == "Object")
                                        && matches!(&m.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == "create")
                            )
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if is_plain_object {
                    ctx.plain_object_locals.insert(name.clone());
                }
            }
            let mut ty = ident
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or_else(|| {
                    // No type annotation: try local inference from initializer
                    if let Some(init_expr) = &decl.init {
                        let inferred = infer_type_from_expr(init_expr, ctx);
                        if !matches!(inferred, Type::Any) {
                            return inferred;
                        }
                        // Fall back to tsgo resolved types if available
                        if let Some(resolved) = ctx.resolved_types.as_ref() {
                            if let Some(resolved_ty) = resolved.get(&(ident.id.span.lo.0)) {
                                return resolved_ty.clone();
                            }
                        }
                    }
                    Type::Any
                });

            // If no type annotation, infer from new Set<T>() or new Map<K, V>() or new URLSearchParams() expressions
            if matches!(ty, Type::Any) {
                if let Some(init_expr) = &decl.init {
                    if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                        if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                            let class_name = class_ident.sym.as_ref();
                            if class_name == "Set" || class_name == "Map" {
                                // Extract type arguments from new Set<T>() or new Map<K, V>()
                                let type_args: Vec<Type> = new_expr
                                    .type_args
                                    .as_ref()
                                    .map(|ta| {
                                        ta.params.iter().map(|t| extract_ts_type(t)).collect()
                                    })
                                    .unwrap_or_default();
                                ty = Type::Generic {
                                    base: class_name.to_string(),
                                    type_args,
                                };
                            } else if class_name == "URLSearchParams" {
                                ty = Type::Named("URLSearchParams".to_string());
                            } else if class_name == "TextEncoder" {
                                ty = Type::Named("TextEncoder".to_string());
                            } else if class_name == "TextDecoder" {
                                ty = Type::Named("TextDecoder".to_string());
                            } else if class_name == "Uint8Array" || class_name == "Buffer" {
                                ty = Type::Named("Uint8Array".to_string());
                            } else if matches!(
                                class_name,
                                "Int8Array"
                                    | "Int16Array"
                                    | "Uint16Array"
                                    | "Int32Array"
                                    | "Uint32Array"
                                    | "Float32Array"
                                    | "Float64Array"
                            ) {
                                ty = Type::Named(class_name.to_string());
                            } else if ctx.classes_index.contains_key(class_name) {
                                // User-defined class: infer type from new ClassName(...)
                                let type_args: Vec<Type> = new_expr
                                    .type_args
                                    .as_ref()
                                    .map(|ta| {
                                        ta.params.iter().map(|t| extract_ts_type(t)).collect()
                                    })
                                    .unwrap_or_default();
                                if type_args.is_empty() {
                                    ty = Type::Named(class_name.to_string());
                                } else {
                                    ty = Type::Generic {
                                        base: class_name.to_string(),
                                        type_args,
                                    };
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a native class instantiation and register it
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                        let class_name = class_ident.sym.as_ref();
                        // A user `class Big {...}` in scope shadows the
                        // hardcoded library-name fallback below. Without
                        // this gate `class Big { f0=0; ... } const b = new
                        // Big()` routed through big.js's handle-based
                        // dispatch so every property read returned 0.
                        let user_class_defined = ctx.classes_index.contains_key(class_name)
                            || ctx.pending_classes.iter().any(|c| c.name == class_name);
                        // First try the general native module lookup (covers all imported native classes)
                        let module_name = if let Some((m, _)) = ctx.lookup_native_module(class_name)
                        {
                            Some(m.to_string())
                        } else if user_class_defined {
                            None
                        } else {
                            // Fallback to hardcoded map for known classes.
                            // Pool/Client/MongoClient are intentionally NOT
                            // listed here: those names collide with user
                            // classes and TS-source npm packages (e.g.
                            // `@perryts/mysql` exports its own `Pool`), so
                            // an unconditional mapping misclassified them
                            // as `pg`/`mongodb` and routed `.query()` /
                            // `.end()` to `js_pg_*` runtime symbols that
                            // don't exist in user TS code, failing at link
                            // time. The legitimate `import { Pool } from
                            // "pg"` flow is caught by the general lookup
                            // above. (Issue #536.)
                            match class_name {
                                "EventEmitter" => Some("events".to_string()),
                                "AsyncLocalStorage" => Some("async_hooks".to_string()),
                                "AsyncResource" => Some("async_hooks".to_string()),
                                "WebSocket" | "WebSocketServer" => Some("ws".to_string()),
                                "Redis" => Some("ioredis".to_string()),
                                "LRUCache" => Some("lru-cache".to_string()),
                                "Command" => Some("commander".to_string()),
                                "Big" => Some("big.js".to_string()),
                                "Decimal" => Some("decimal.js".to_string()),
                                "BigNumber" => Some("bignumber.js".to_string()),
                                _ => None,
                            }
                        };
                        // Issue #848: StringDecoder dispatches entirely through
                        // HANDLE_*_DISPATCH; don't register as a typed native
                        // instance (see the mirroring gate in lower.rs).
                        let module_name = match (class_name, module_name.as_deref()) {
                            ("StringDecoder", Some("string_decoder")) => None,
                            _ => module_name,
                        };
                        if let Some(module) = module_name {
                            ctx.register_native_instance(
                                name.clone(),
                                module,
                                class_name.to_string(),
                            );
                        }
                    } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                        if let (
                            ast::Expr::Ident(module_ident),
                            ast::MemberProp::Ident(class_ident),
                        ) = (member.obj.as_ref(), &member.prop)
                        {
                            let module_alias = module_ident.sym.as_ref();
                            if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                                let class_name = class_ident.sym.as_ref();
                                let is_known_native_class = matches!(
                                    (module_name, class_name),
                                    ("async_hooks", "AsyncLocalStorage" | "AsyncResource")
                                );
                                if is_known_native_class {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_name.to_string(),
                                        class_name.to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is an awaited native class instantiation (e.g., await new Redis())
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let ast::Expr::New(new_expr) = await_expr.arg.as_ref() {
                        if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                            let class_name = class_ident.sym.as_ref();
                            // Same user-class shadowing rule as the
                            // non-await new-expr path above.
                            let user_class_defined = ctx.classes_index.contains_key(class_name)
                                || ctx.pending_classes.iter().any(|c| c.name == class_name);
                            // First try the general native module lookup.
                            // Pool/Client/MongoClient are intentionally NOT
                            // in the fallback map — see the sync `new` arm
                            // above for the rationale (issue #536).
                            let module_name =
                                if let Some((m, _)) = ctx.lookup_native_module(class_name) {
                                    Some(m.to_string())
                                } else if user_class_defined {
                                    None
                                } else {
                                    match class_name {
                                        "EventEmitter" => Some("events".to_string()),
                                        "AsyncLocalStorage" => Some("async_hooks".to_string()),
                                        "AsyncResource" => Some("async_hooks".to_string()),
                                        "WebSocket" | "WebSocketServer" => Some("ws".to_string()),
                                        "Redis" => Some("ioredis".to_string()),
                                        "LRUCache" => Some("lru-cache".to_string()),
                                        "Command" => Some("commander".to_string()),
                                        "Big" => Some("big.js".to_string()),
                                        "Decimal" => Some("decimal.js".to_string()),
                                        "BigNumber" => Some("bignumber.js".to_string()),
                                        _ => None,
                                    }
                                };
                            if let Some(module) = module_name {
                                ctx.register_native_instance(
                                    name.clone(),
                                    module,
                                    class_name.to_string(),
                                );
                            }
                        }
                    }
                }
            }

            // Check if this is a native module factory function call (e.g., mysql.createPool())
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                let obj_name = obj_ident.sym.as_ref();
                                // Check if it's a known native module
                                if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.as_ref();
                                        // Map factory functions to their class names
                                        let class_name = match (module_name, method_name) {
                                            ("async_hooks", "createHook") => Some("AsyncHook"),
                                            ("mysql2" | "mysql2/promise", "createPool") => {
                                                Some("Pool")
                                            }
                                            ("mysql2" | "mysql2/promise", "createConnection") => {
                                                Some("Connection")
                                            }
                                            ("pg", "connect") => Some("Client"),
                                            ("http" | "https", "request" | "get") => {
                                                Some("ClientRequest")
                                            }
                                            // node-cron's `cron.schedule(expr, cb)` returns a job
                                            // handle whose `start()`/`stop()`/`isRunning()` methods
                                            // dispatch via the ("node-cron", true, METHOD) entries
                                            // in expr.rs's native_module dispatch table. Without
                                            // registering the result as a "CronJob" native instance,
                                            // `job.stop()` falls through to dynamic dispatch and the
                                            // call never reaches js_cron_job_stop.
                                            ("node-cron", "schedule") => Some("CronJob"),
                                            // readline.createInterface() returns a singleton
                                            // handle whose .question/.on/.close methods
                                            // dispatch via the ("readline", true, METHOD)
                                            // entries in lower_call.rs's native_module dispatch
                                            // table. Without registering the result as a
                                            // "Interface" native instance, those calls fall
                                            // through to dynamic dispatch and never reach
                                            // js_readline_question / js_readline_on / etc.
                                            ("readline", "createInterface") => Some("Interface"),
                                            // perry/tui state(initial) returns a handle whose
                                            // .get()/.set() methods dispatch via the
                                            // ("perry/tui", true, "get"/"set", class_filter:
                                            // Some("State")) entries in lower_call.rs's
                                            // NativeModSig table. Without this registration,
                                            // those calls fall through to dynamic dispatch and
                                            // never reach the runtime FFI. (#358 Phase 2.)
                                            ("perry/tui", "state") => Some("State"),
                                            // perry/tui ink-shape hooks (#679 Phase 1): the
                                            // useApp/useStdout/useRef factories each return
                                            // a singleton handle. .exit()/.write()/.get()
                                            // etc. dispatch through the class_filter rows
                                            // in lower_call.rs.
                                            ("perry/tui", "useApp") => Some("TuiApp"),
                                            ("perry/tui", "useStdout") => Some("TuiStdout"),
                                            ("perry/tui", "useRef") => Some("RefBox"),
                                            ("perry/tui", "useFocusManager") => {
                                                Some("FocusManager")
                                            }
                                            _ => None,
                                        };
                                        if let Some(class_name) = class_name {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name.to_string(),
                                                class_name.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Check if this is a direct call to a default import from a native module
                        // e.g., Fastify() where Fastify is imported from 'fastify'
                        if let ast::Expr::Ident(func_ident) = callee.as_ref() {
                            let func_name = func_ident.sym.as_ref();
                            // Check if this is a default import from a native module
                            if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                                // Register as native instance - the "class" is "App" for default exports
                                ctx.register_native_instance(
                                    name.clone(),
                                    module_name.to_string(),
                                    "App".to_string(),
                                );
                            }
                            // Check if this is a named import that returns a handle (e.g., State from perry/ui)
                            // Clone module_name + method_name to owned String first
                            // so the immutable borrow of ctx ends before we call
                            // register_native_instance (mutable borrow).
                            let mod_method: Option<(String, String)> = ctx
                                .lookup_native_module(func_name)
                                .and_then(|(m, mm)| mm.map(|x| (m.to_string(), x.to_string())));
                            if let Some((module_name, method_name)) = mod_method {
                                if module_name == "perry/ui" {
                                    match method_name.as_str() {
                                        "Canvas" | "State" | "Sheet" | "Toolbar" | "Window"
                                        | "LazyVStack" | "NavigationStack" | "Picker" | "Table"
                                        | "TabBar" => {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name.clone(),
                                                method_name.clone(),
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                                // perry/tui state(initial) — register the receiver as a
                                // "State" native instance so subsequent .get()/.set()
                                // calls dispatch via the perry/tui NativeModSig table
                                // (class_filter: Some("State")). (#358 Phase 2.)
                                if module_name == "perry/tui" && method_name == "state" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_name.clone(),
                                        "State".to_string(),
                                    );
                                }
                                // perry/tui ink-shape hooks (#679 Phase 1).
                                // useApp/useStdout/useRef each return a
                                // singleton handle whose receiver-methods
                                // dispatch through the class_filter rows
                                // ("TuiApp"/"TuiStdout"/"RefBox") added in
                                // lower_call.rs. Without these registrations
                                // a call like `app.exit()` falls back to
                                // dynamic dispatch and the matching FFI
                                // (js_perry_tui_app_exit) is never invoked.
                                if module_name == "perry/tui" {
                                    let class = match method_name.as_str() {
                                        "useApp" => Some("TuiApp"),
                                        "useStdout" => Some("TuiStdout"),
                                        "useRef" => Some("RefBox"),
                                        "useFocusManager" => Some("FocusManager"),
                                        _ => None,
                                    };
                                    if let Some(cn) = class {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module_name.clone(),
                                            cn.to_string(),
                                        );
                                    }
                                }
                                // node:http / node:https / node:http2 — issue #604
                                // followup to #577. The module-level decl path
                                // (lower.rs:5530) already handles `const s =
                                // createServer(...)` at top level; this arm
                                // covers the inside-function case where the
                                // factory call lives in a body. Without this,
                                // `async function main() { const server =
                                // createServer(handler); server.listen(...); }`
                                // had `server` unregistered, so the listen
                                // dispatch fell through the class_filter
                                // gate and never invoked the cb closure.
                                let http_class = match (module_name.as_str(), method_name.as_str())
                                {
                                    ("http", "createServer") => Some("HttpServer"),
                                    ("https", "createServer") => Some("HttpsServer"),
                                    ("http2", "createSecureServer") => Some("Http2SecureServer"),
                                    ("async_hooks", "createHook") => Some("AsyncHook"),
                                    _ => None,
                                };
                                if let Some(cn) = http_class {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_name,
                                        cn.to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is an awaited factory call (e.g., const client = await MongoClient.connect(uri))
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                        if let ast::Callee::Expr(callee) = &call_expr.callee {
                            if let ast::Expr::Member(member) = callee.as_ref() {
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    let obj_name = obj_ident.sym.as_ref();
                                    if let Some((module_name, _)) =
                                        ctx.lookup_native_module(obj_name)
                                    {
                                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                            let class_name =
                                                match (module_name, method_ident.sym.as_ref()) {
                                                    ("mongodb", "connect") => Some("MongoClient"),
                                                    ("mysql2" | "mysql2/promise", "createPool") => {
                                                        Some("Pool")
                                                    }
                                                    (
                                                        "mysql2" | "mysql2/promise",
                                                        "createConnection",
                                                    ) => Some("Connection"),
                                                    ("pg", "connect") => Some("Client"),
                                                    // axios.get/post/put/delete/patch/request — mirror
                                                    // the top-level decl arm in lower.rs:4011 so
                                                    // `await axios.get(...)` registers the result as
                                                    // an axios.Response inside async function bodies.
                                                    // Without this, `r.status` / `r.data` fall through
                                                    // to generic property dispatch and read the
                                                    // raw handle pointer as an ObjectHeader. Issue
                                                    // #604 followup — same pattern as the createServer
                                                    // registration above.
                                                    (
                                                        "axios",
                                                        "get" | "post" | "put" | "delete" | "patch"
                                                        | "request",
                                                    ) => Some("Response"),
                                                    _ => None,
                                                };
                                            if let Some(class_name) = class_name {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    module_name.to_string(),
                                                    class_name.to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a method call on a registered native instance (chaining).
            // e.g., const db = client.db(name) where client is a mongodb native instance.
            if let Some(init_expr) = &decl.init {
                // Unwrap await if present
                let actual_init = if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    await_expr.arg.as_ref()
                } else {
                    init_expr.as_ref()
                };
                if let ast::Expr::Call(call_expr) = actual_init {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                let obj_name = obj_ident.sym.to_string();
                                if let Some((module_name, _class)) = ctx
                                    .lookup_native_instance(&obj_name)
                                    .map(|(m, c)| (m.to_string(), c.to_string()))
                                {
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.as_ref();
                                        // Determine if the method returns a handle (another native instance)
                                        let returns_handle =
                                            match (module_name.as_str(), method_name) {
                                                ("mongodb", "db") => Some("Database"),
                                                ("mongodb", "collection") => Some("Collection"),
                                                ("mysql2" | "mysql2/promise", "getConnection") => {
                                                    Some("PoolConnection")
                                                }
                                                _ => None,
                                            };
                                        if let Some(class_name) = returns_handle {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name,
                                                class_name.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a require() call for a built-in module
            if let Some(init_expr) = &decl.init {
                if let Some(module_name) = is_require_builtin_module(init_expr) {
                    // Register this variable as an alias to the built-in module
                    ctx.register_builtin_module_alias(name.clone(), module_name);
                    // Don't emit a variable declaration - the module is handled specially
                    return Ok(result);
                }
            }

            // Check if this is calling toString() on URLSearchParams - returns String
            if matches!(ty, Type::Any) {
                if let Some(init_expr) = &decl.init {
                    if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                        if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                            if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                                if let ast::MemberProp::Ident(method_ident) = &member_expr.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "toString" || method_name == "get" {
                                        // Check if object is a URLSearchParams
                                        if let ast::Expr::Ident(obj_ident) =
                                            member_expr.obj.as_ref()
                                        {
                                            let obj_name = obj_ident.sym.as_ref();
                                            if let Some(obj_ty) = ctx.lookup_local_type(obj_name) {
                                                if matches!(obj_ty, Type::Named(name) if name == "URLSearchParams")
                                                {
                                                    ty = Type::String;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is assigning the result of a native method call that returns the same type
            // e.g., const sum = d1.plus(d2) where d1 is a Decimal -> sum should also be tracked as Decimal
            // Also handles: const r1 = new Big(...).div(...) patterns
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                        if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                            let mut handled = false;
                            // First try: object is an ident that's a known native instance
                            if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                                let obj_name = obj_ident.sym.as_ref();
                                // Check if object is a native instance
                                if let Some((module, class)) = ctx.lookup_native_instance(obj_name)
                                {
                                    // Check if this method returns the same type (builder pattern)
                                    if let ast::MemberProp::Ident(method_ident) = &member_expr.prop
                                    {
                                        let method_name = method_ident.sym.as_ref();
                                        // Methods that return the same type (Decimal, etc.)
                                        let returns_same_type = match class {
                                            "Decimal" | "Big" | "BigNumber" => matches!(
                                                method_name,
                                                "plus"
                                                    | "minus"
                                                    | "times"
                                                    | "div"
                                                    | "mod"
                                                    | "pow"
                                                    | "sqrt"
                                                    | "abs"
                                                    | "neg"
                                                    | "round"
                                                    | "floor"
                                                    | "ceil"
                                            ),
                                            _ => false,
                                        };
                                        if returns_same_type {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module.to_string(),
                                                class.to_string(),
                                            );
                                            handled = true;
                                        }
                                    }
                                }
                            }
                            // Second try: object is new Big(...) or a chained call like new Big(...).div(...)
                            if !handled {
                                if let Some(module_name) =
                                    detect_native_instance_expr(ctx, &member_expr.obj)
                                {
                                    let class_name = match module_name {
                                        "big.js" => "Big",
                                        "decimal.js" => "Decimal",
                                        "bignumber.js" => "BigNumber",
                                        "lru-cache" => "LRUCache",
                                        "commander" => "Command",
                                        _ => "",
                                    };
                                    if !class_name.is_empty() {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module_name.to_string(),
                                            class_name.to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is assigning from fetch() or await fetch() - register as fetch Response
            if let Some(init_expr) = &decl.init {
                // Helper to check if an expression is a fetch-like call
                // Returns the module name if it matches fetch/fetchWithAuth/fetchPostWithAuth
                fn get_fetch_module(expr: &ast::Expr) -> Option<&'static str> {
                    if let ast::Expr::Call(call_expr) = expr {
                        if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                                // Closes #644: all three return the same
                                // Response handle, so they must register
                                // under module="fetch". The codegen dispatch
                                // in `lower_fetch_native_method` gates on
                                // `module == "fetch"` — pre-fix, registering
                                // under "fetchWithAuth"/"fetchPostWithAuth"
                                // missed the gate so a post-narrowing
                                // `r.status` lowered as a NativeMethodCall
                                // with module="fetchWithAuth" and fell
                                // through to a generic 0.0-returning arm.
                                // (Without narrowing the access went through
                                // generic PropertyGet → handle dispatch →
                                // js_fetch_response_status, so the bug only
                                // surfaced inside an `if r !== null/undefined`
                                // block.)
                                return match ident.sym.as_ref() {
                                    "fetch" | "fetchWithAuth" | "fetchPostWithAuth" => {
                                        Some("fetch")
                                    }
                                    _ => None,
                                };
                            }
                        }
                    }
                    None
                }

                // Check for: const response = fetch(url) / fetchWithAuth(url, auth) / fetchPostWithAuth(url, auth, body)
                if let Some(module) = get_fetch_module(init_expr) {
                    ctx.register_native_instance(
                        name.clone(),
                        module.to_string(),
                        "Response".to_string(),
                    );
                }
                // Check for: const response = await fetch(url) / await fetchWithAuth(...) / await fetchPostWithAuth(...)
                else if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let Some(module) = get_fetch_module(&await_expr.arg) {
                        ctx.register_native_instance(
                            name.clone(),
                            module.to_string(),
                            "Response".to_string(),
                        );
                    }
                }

                // Web Fetch API: new Response(...) / new Headers(...) / new Request(...)
                // Also handle Response.json(...) and Response.redirect(...) static factories.
                if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                        match class_ident.sym.as_ref() {
                            "Response" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "fetch".to_string(),
                                    "Response".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "Headers" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "Headers".to_string(),
                                    "Headers".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "Request" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "Request".to_string(),
                                    "Request".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            // Issue #237: Web Streams API constructors.
                            "ReadableStream" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "readable_stream".to_string(),
                                    "ReadableStream".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "WritableStream" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "writable_stream".to_string(),
                                    "WritableStream".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "TransformStream" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "transform_stream".to_string(),
                                    "TransformStream".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            other => {
                                // Issue #562: `let x = new SubclassOfStream()`
                                // — walk the user class's `native_extends` to
                                // see if it points at a stream module. If so,
                                // register `x` under the same module/class
                                // tag the bare-stream constructor would. The
                                // codegen FFI sites unwrap the
                                // `__perry_stream_handle__` field at dispatch
                                // time, so a subclass instance and a bare
                                // numeric handle are interchangeable.
                                if let Some((module, class)) =
                                    ctx.lookup_class_native_extends(other)
                                {
                                    if matches!(
                                        module,
                                        "readable_stream" | "writable_stream" | "transform_stream"
                                    ) {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module.to_string(),
                                            class.to_string(),
                                        );
                                        ctx.uses_fetch = true;
                                    }
                                }
                            }
                        }
                    }
                }
                // Response.json(...) / Response.redirect(...) static factories
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if obj_ident.sym.as_ref() == "Response" {
                                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                        match prop_ident.sym.as_ref() {
                                            "json" | "redirect" | "error" => {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    "fetch".to_string(),
                                                    "Response".to_string(),
                                                );
                                                ctx.uses_fetch = true;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Response.clone() — for: const r5clone = r5.clone();
                // The result is a new Response. Detect by checking if the receiver is already
                // a fetch::Response native instance.
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    if prop_ident.sym.as_ref() == "clone" {
                                        if let Some((m, c)) =
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                        {
                                            if c == "Response" {
                                                let m = m.to_string();
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    m,
                                                    "Response".to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Issue #234: const blob = await <res>.blob() — register `blob`
                // as a blob::Blob native instance so subsequent `blob.text()` /
                // `.arrayBuffer()` / `.bytes()` / `.slice()` / `.size` /
                // `.type` calls dispatch through the codegen `module=="blob"`
                // arm in `lower_call.rs`.
                if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                        if let ast::Callee::Expr(callee) = &call_expr.callee {
                            if let ast::Expr::Member(member) = callee.as_ref() {
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                        if prop_ident.sym.as_ref() == "blob" {
                                            if let Some((_, c)) =
                                                ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                            {
                                                if c == "Response" {
                                                    ctx.register_native_instance(
                                                        name.clone(),
                                                        "blob".to_string(),
                                                        "Blob".to_string(),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Issue #234: const b2 = blob.slice(...) — chained slicing
                // returns a new Blob. Detect when the receiver is already a
                // blob::Blob native instance.
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    if prop_ident.sym.as_ref() == "slice" {
                                        if let Some((_, c)) =
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                        {
                                            if c == "Blob" {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    "blob".to_string(),
                                                    "Blob".to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Issue #237: Web Streams chained-typed-method bindings.
                // Recognize chained method/property forms that return a new
                // streams native instance so subsequent dispatch routes to
                // the right `module == "..."` arm in lower_call.rs.
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    let m = prop_ident.sym.as_ref().to_string();
                                    let class_owned = ctx
                                        .lookup_native_instance(obj_ident.sym.as_ref())
                                        .map(|(_, c)| c.to_string());
                                    if let Some(c) = class_owned {
                                        if m == "stream" && c == "Blob" {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "readable_stream".to_string(),
                                                "ReadableStream".to_string(),
                                            );
                                        }
                                        if m == "getReader" && c == "ReadableStream" {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "readable_stream_reader".to_string(),
                                                "ReadableStreamDefaultReader".to_string(),
                                            );
                                        }
                                        if m == "getWriter" && c == "WritableStream" {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "writable_stream_writer".to_string(),
                                                "WritableStreamDefaultWriter".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Issue #237: const stream = response.body / const r = ts.readable / .writable
                // Property reads on a native instance — destructured as Member
                // expressions (no Call wrapper).
                if let ast::Expr::Member(member) = init_expr.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            let p = prop_ident.sym.as_ref().to_string();
                            let class_owned = ctx
                                .lookup_native_instance(obj_ident.sym.as_ref())
                                .map(|(_, c)| c.to_string());
                            if let Some(c) = class_owned {
                                if p == "body" && c == "Response" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "readable_stream".to_string(),
                                        "ReadableStream".to_string(),
                                    );
                                }
                                if p == "readable" && c == "TransformStream" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "readable_stream".to_string(),
                                        "ReadableStream".to_string(),
                                    );
                                }
                                if p == "writable" && c == "TransformStream" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "writable_stream".to_string(),
                                        "WritableStream".to_string(),
                                    );
                                }
                            }
                        }
                    }
                }

                // Issue #237: const stream = upstream.pipeThrough(transform)
                // returns a ReadableStream (the transform's readable side).
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    if prop_ident.sym.as_ref() == "pipeThrough" {
                                        let class_owned = ctx
                                            .lookup_native_instance(obj_ident.sym.as_ref())
                                            .map(|(_, c)| c.to_string());
                                        if class_owned.as_deref() == Some("ReadableStream") {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "readable_stream".to_string(),
                                                "ReadableStream".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if calling a function whose return type is a native module type
            // e.g., const dbPool = initializePool() where initializePool(): mysql.Pool
            // Also handles: const dbPool = await initializePool()
            if let Some(init_expr) = &decl.init {
                let call_expr = match init_expr.as_ref() {
                    ast::Expr::Call(c) => Some(c),
                    ast::Expr::Await(await_expr) => {
                        if let ast::Expr::Call(c) = await_expr.arg.as_ref() {
                            Some(c)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                // Variable-to-variable propagation for native instances
                // (`let sock: Socket = plainSock`) is handled by the
                // post-lowering cross-module pass; see
                // `js_transform::scan_for_ident_init_propagation`.
                if let Some(call_expr) = call_expr {
                    if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                        // Check direct function calls: const x = someFunc()
                        if let ast::Expr::Ident(func_ident) = callee_expr.as_ref() {
                            let func_name = func_ident.sym.as_ref();
                            if let Some((module, class)) =
                                ctx.lookup_func_return_native_instance(func_name)
                            {
                                ctx.register_native_instance(
                                    name.clone(),
                                    module.to_string(),
                                    class.to_string(),
                                );
                            }
                        }
                        // Check method calls on native instances: const conn = pool.getConnection()
                        if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                                let obj_name = obj_ident.sym.as_ref();
                                if let Some((module, class)) = ctx.lookup_native_instance(obj_name)
                                {
                                    if let ast::MemberProp::Ident(method_ident) = &member_expr.prop
                                    {
                                        let method_name = method_ident.sym.as_ref();
                                        // Map method calls to their return types
                                        let return_class = match (module, class, method_name) {
                                            (
                                                "mysql2" | "mysql2/promise",
                                                "Pool",
                                                "getConnection",
                                            ) => Some("PoolConnection"),
                                            ("pg", "Pool", "connect") => Some("Client"),
                                            _ => None,
                                        };
                                        if let Some(ret_class) = return_class {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module.to_string(),
                                                ret_class.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Issue #461: when the init is an arrow / function expression
            // (`const f = (x) => …` or `const f = function() {}`), pre-define
            // the local BEFORE lowering the init so self-recursive references
            // inside the closure body resolve to `LocalGet(id)` instead of
            // falling through to `lookup_imported_func` and lowering as
            // `ExternFuncRef { name: "f" }` (which then emits a bare unmangled
            // `_f` symbol at link time). Effect's `internal/stream.ts` hits this:
            // `import * as pull from "./stream/pull.js"` (namespace import) +
            // `const pull = (state) => { … pull(...) … }` (local rebinding) —
            // without pre-registration, the inner closure's `pull` reference
            // resolves to the namespace import. Function declarations
            // (`function f() {}`) already have this pre-registration via
            // `lower_decl.rs`'s `Decl::Fn` arm.
            //
            // Gate on function-expr init only: pre-defining for `const x = x + 1`
            // would silently turn a TDZ violation into a self-reference. For
            // closures, the body doesn't execute until call time, so the slot
            // holds the closure value by then.
            // #593: extend the pre-registration to inits that *contain*
            // an Arrow / Fn anywhere in their tree (e.g.
            // `const off = ev.on(() => off())` — Call wrapping Arrow,
            // `const sub = subject.subscribe({ next: () => sub.unsubscribe() })`
            // — Object wrapping Arrow). The closure body is lowered in
            // its own LoweringContext but reuses the parent's `locals`
            // for outer-scope lookups (see `lower_arrow` /
            // `lower_fn_expr`). Without pre-registration, the inner
            // `off` / `sub` reference resolves to GlobalGet(0) and the
            // self-recursive call no-ops at runtime.
            let is_function_expr_init = matches!(
                decl.init.as_deref(),
                Some(ast::Expr::Arrow(_)) | Some(ast::Expr::Fn(_))
            ) || decl
                .init
                .as_deref()
                .map_or(false, ast_expr_contains_function_expr);
            let pre_id = if is_function_expr_init
                && !ctx.pre_registered_module_vars.contains(&name)
                && ctx.lookup_local(&name).is_none()
            {
                Some(ctx.define_local(name.clone(), ty.clone()))
            } else {
                None
            };

            let init = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            let id = if let Some(pid) = pre_id {
                pid
            } else if ctx.pre_registered_module_vars.remove(&name) {
                // Reuse pre-registered LocalId from module-level forward-declaration pass
                let id = ctx.lookup_local(&name).unwrap();
                // Update the type now that we have full inference
                if let Some((_, _, existing_ty)) =
                    ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name)
                {
                    *existing_ty = ty.clone();
                }
                id
            } else {
                ctx.define_local(name.clone(), ty.clone())
            };
            // Issue #740: track `let/const/var <name> = ClassRef(...)` so
            // `new <name>(...)` can resolve captures via the alias chain.
            // Also follow LocalGet aliases for `const B = A` style chains.
            if let Some(init_expr) = &init {
                match init_expr {
                    Expr::ClassRef(class_name) => {
                        ctx.register_let_class_alias(name.clone(), class_name.clone());
                    }
                    Expr::LocalGet(src_id) => {
                        if let Some((src_name, _, _)) =
                            ctx.locals.iter().rev().find(|(_, lid, _)| lid == src_id)
                        {
                            let src_name = src_name.clone();
                            if let Some(resolved) = ctx.resolve_class_alias(&src_name) {
                                ctx.register_let_class_alias(name.clone(), resolved);
                            } else if ctx.classes_index.contains_key(&src_name) {
                                ctx.register_let_class_alias(name.clone(), src_name);
                            }
                        }
                        // Issue #838: follow prototype-alias chains too,
                        // so `var m = M.prototype; var n = m; n.foo = …`
                        // still recognises the underlying class.
                        if let Some(class_name) = ctx.prototype_aliases.get(src_id).cloned() {
                            ctx.prototype_aliases.insert(id, class_name);
                        }
                    }
                    // Issue #838: `var p = <ClassName>.prototype` records
                    // the alias so a later `p.<method> = <fn>` lowers to
                    // RegisterPrototypeMethod. dayjs's minified shape
                    // (`var m = M.prototype; m.parse = function(){…};
                    //  m.init = function(){…};`) hits this — without
                    // alias-tracking the assignments fell through to a
                    // generic PropertySet on the prototype proxy that
                    // nothing downstream observed.
                    Expr::PropertyGet { object, property } if property == "prototype" => {
                        if let Expr::ClassRef(class_name) = object.as_ref() {
                            ctx.prototype_aliases.insert(id, class_name.clone());
                        }
                    }
                    _ => {}
                }
            }
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            });
        }
        ast::Pat::Array(_) | ast::Pat::Object(_) => {
            // Delegate to the recursive pattern binding helper so that all
            // destructuring features (nested patterns, defaults, rest, computed
            // keys) work consistently across all call sites.

            // ink-shape useState: `const [v, setV] = useState(0)` (#679 Phase 1).
            // Rewrite RHS to call useStateTuple which returns a real
            // [value, setter_closure] 2-element array. Without this, the
            // regular destructure path indexes a scalar return as if it were
            // an array — both elements come out undefined.
            let init_expr =
                if let (ast::Pat::Array(_), Some(init)) = (&decl.name, decl.init.as_ref()) {
                    if let Some(rewritten) = rewrite_use_state_tuple(ctx, init) {
                        rewritten
                    } else {
                        lower_expr(ctx, init)?
                    }
                } else {
                    decl.init
                        .as_ref()
                        .map(|e| lower_expr(ctx, e))
                        .transpose()?
                        .ok_or_else(|| anyhow!("Destructuring requires an initializer"))?
                };
            let stmts = lower_pattern_binding(ctx, &decl.name, init_expr, mutable)?;
            result.extend(stmts);
        }
        _ => {
            // For other patterns, fall back to existing behavior
            let name = get_binding_name(&decl.name)?;
            let ty = extract_binding_type(&decl.name);
            let init = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            let id = if ctx.pre_registered_module_vars.remove(&name) {
                let id = ctx.lookup_local(&name).unwrap();
                if let Some((_, _, existing_ty)) =
                    ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name)
                {
                    *existing_ty = ty.clone();
                }
                id
            } else {
                ctx.define_local(name.clone(), ty.clone())
            };
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            });
        }
    }

    Ok(result)
}
