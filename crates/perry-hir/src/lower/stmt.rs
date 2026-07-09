//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

fn emit_class_expression_value_binding(
    ctx: &mut LoweringContext,
    module: &mut Module,
    bind_name: &str,
    mutable: bool,
    is_var: bool,
) {
    let ty = Type::Any;
    let id = if ctx.scope_depth == 0
        && ctx.inside_block_scope == 0
        && ctx.pre_registered_module_vars.remove(bind_name)
    {
        ctx.pre_registered_module_var_decls.remove(bind_name);
        let id = ctx
            .lookup_local(bind_name)
            .unwrap_or_else(|| ctx.define_local(bind_name.to_string(), ty.clone()));
        if let Some((_, _, existing_ty)) =
            ctx.locals.iter_mut().rev().find(|(_, lid, _)| *lid == id)
        {
            *existing_ty = ty.clone();
        }
        id
    } else if is_var {
        // Bind the lookup result first so the `iter_named` borrow of
        // `ctx.locals` ends before the mutable reuse below (#5267).
        let existing_var = ctx
            .locals
            .iter_named(bind_name)
            .find(|(_, (_, id, _))| ctx.var_hoisted_ids.contains(id))
            .map(|(pos, (_, id, _))| (pos, *id));
        if let Some((reuse_pos, id)) = existing_var {
            *ctx.locals.type_mut_at(reuse_pos) = ty.clone();
            id
        } else {
            ctx.define_local(bind_name.to_string(), ty.clone())
        }
    } else {
        ctx.define_local(bind_name.to_string(), ty.clone())
    };

    if is_var {
        ctx.var_hoisted_ids.insert(id);
    }
    if !mutable {
        ctx.mark_local_immutable(id);
    }
    ctx.register_let_class_alias(bind_name.to_string(), bind_name.to_string());
    module.init.push(Stmt::Let {
        id,
        name: bind_name.to_string(),
        ty,
        mutable,
        init: Some(Expr::ClassRef(bind_name.to_string())),
    });
}

/// For a `let X = T1 = … = class [Y] {…}` declaration, record the chained
/// assignment targets (`T1`, …) as class self-aliases of the class's lowering
/// name in `ctx.class_expr_aliases`, BEFORE the init is lowered. This does NOT
/// `register_class` the names (which would trip `lower_class_expr`'s
/// collision-rename), it only feeds `synthesize_class_captures`'s self-alias
/// exclusion so a method-body reference to `T1` (e.g. tsc's `Logger_1`) is not
/// counted as an outer-scope capture — keeping the class on the shared-template
/// `ClassRef` path instead of the static-method-dropping `ClassExprFresh` path.
/// The class's own bind/inner name is already covered by `name` in that pass.
pub(crate) fn record_chained_class_self_aliases(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
) {
    let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, &decl.init) else {
        return;
    };
    let bind_name = ident.id.sym.to_string();
    let mut chained_targets: Vec<String> = Vec::new();
    let mut e = init.as_ref();
    let inner_class = loop {
        match e {
            ast::Expr::Paren(p) => e = &p.expr,
            ast::Expr::TsAs(a) => e = &a.expr,
            ast::Expr::TsNonNull(n) => e = &n.expr,
            ast::Expr::TsTypeAssertion(a) => e = &a.expr,
            ast::Expr::Assign(assign) if assign.op == ast::AssignOp::Assign => {
                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(target)) =
                    &assign.left
                {
                    chained_targets.push(target.id.sym.to_string());
                    e = &assign.right;
                } else {
                    return;
                }
            }
            ast::Expr::Class(c) => break c,
            _ => return,
        }
    };
    if chained_targets.is_empty() {
        return;
    }
    // The class lowers under its inner expression name if present, else the
    // binding name (matching `lower_class_expr`'s `ident_name`/synthetic-name
    // choice for the common non-colliding case).
    let class_name = inner_class
        .ident
        .as_ref()
        .map(|i| i.sym.to_string())
        .unwrap_or_else(|| bind_name.clone());
    for t in chained_targets {
        ctx.class_expr_aliases
            .entry(t)
            .or_insert(class_name.clone());
    }
}

/// Recursively walk a destructuring pattern collecting every leaf identifier
/// (and pre-defining each as a local). Used by the for-of binding pre-pass so
/// the loop body can reference variables introduced by *nested* patterns like
/// `for (const { foo, bar: [a, b] } of arr)` — the outer per-prop loop only
/// handled `Ident` leaves, so leaves buried in nested array/object patterns
/// were silently skipped and read as zero in the body. Issue #554.
pub(crate) fn collect_for_of_pattern_leaves(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    out: &mut Vec<(String, LocalId)>,
) {
    match pat {
        ast::Pat::Ident(ident) => {
            let name = ident.id.sym.to_string();
            let id = ctx.define_local(name.clone(), Type::Any);
            out.push((name, id));
        }
        ast::Pat::Array(arr_pat) => {
            for ep in arr_pat.elems.iter().flatten() {
                if let ast::Pat::Rest(rest) = ep {
                    collect_for_of_pattern_leaves(ctx, &rest.arg, out);
                } else {
                    collect_for_of_pattern_leaves(ctx, ep, out);
                }
            }
        }
        ast::Pat::Object(obj_pat) => {
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        let id = ctx.define_local(name.clone(), Type::Any);
                        out.push((name, id));
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        collect_for_of_pattern_leaves(ctx, &kv.value, out);
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        collect_for_of_pattern_leaves(ctx, &rest.arg, out);
                    }
                }
            }
        }
        ast::Pat::Assign(assign_pat) => {
            collect_for_of_pattern_leaves(ctx, &assign_pat.left, out);
        }
        ast::Pat::Rest(rest) => {
            collect_for_of_pattern_leaves(ctx, &rest.arg, out);
        }
        _ => {}
    }
}

/// Emit `Stmt::Let` bindings for a destructuring pattern given a `source`
/// expression that produces the value being destructured. Reuses the
/// pre-allocated leaf ids in `var_ids` (in source order) so the loop body — which
/// was already lowered against those ids — sees the correct bindings. Mirrors
/// `destructuring::lower_pattern_binding_into` but takes pre-allocated ids
/// instead of calling `define_local` itself. Issue #554.
pub(crate) fn emit_for_of_pattern_binding(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    source: Expr,
    var_ids: &[(String, LocalId)],
    var_idx: &mut usize,
    out: &mut Vec<Stmt>,
) -> Result<()> {
    match pat {
        ast::Pat::Ident(_) => {
            let (name, id) = var_ids[*var_idx].clone();
            *var_idx += 1;
            out.push(Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            Ok(())
        }
        ast::Pat::Array(arr_pat) => {
            // Default to Array(Any) so OOB reads return undefined/NaN as the
            // existing `destructuring::lower_pattern_binding_into` helper does.
            // (Typing the temp as Any pulls reads through the typed-element
            // fast path which returns 0 for OOB and breaks default-value
            // handling.)
            let arr_ty = arr_pat
                .type_ann
                .as_ref()
                .map(|ann| crate::lower_types::extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Array(Box::new(Type::Any)));
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, arr_ty.clone()));
            out.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: arr_ty,
                mutable: false,
                init: Some(source),
            });
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                let Some(elem_pat) = elem else { continue };
                if let ast::Pat::Rest(rest) = elem_pat {
                    let slice = Expr::ArraySlice {
                        array: Box::new(Expr::LocalGet(tmp_id)),
                        start: Box::new(Expr::Number(idx as f64)),
                        end: None,
                    };
                    emit_for_of_pattern_binding(ctx, &rest.arg, slice, var_ids, var_idx, out)?;
                    break;
                }
                let elem_src = Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(tmp_id)),
                    index: Box::new(Expr::Number(idx as f64)),
                };
                emit_for_of_pattern_binding(ctx, elem_pat, elem_src, var_ids, var_idx, out)?;
            }
            Ok(())
        }
        ast::Pat::Object(obj_pat) => {
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            out.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let key = assign.key.sym.to_string();
                        let prop_access = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: key,
                        };
                        let init = if let Some(default_expr) = &assign.value {
                            let default_val = lower_expr(ctx, default_expr)?;
                            Expr::Conditional {
                                condition: Box::new(Expr::Compare {
                                    op: CompareOp::Ne,
                                    left: Box::new(prop_access.clone()),
                                    right: Box::new(Expr::Undefined),
                                }),
                                then_expr: Box::new(prop_access),
                                else_expr: Box::new(default_val),
                            }
                        } else {
                            prop_access
                        };
                        let (name, id) = var_ids[*var_idx].clone();
                        *var_idx += 1;
                        out.push(Stmt::Let {
                            id,
                            name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(init),
                        });
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            _ => continue,
                        };
                        let elem_src = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: key,
                        };
                        emit_for_of_pattern_binding(
                            ctx, &kv.value, elem_src, var_ids, var_idx, out,
                        )?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        ast::Pat::Assign(assign_pat) => {
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            out.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            let default_val = lower_expr(ctx, &assign_pat.right)?;
            let with_default = Expr::Conditional {
                condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(Expr::LocalGet(tmp_id)))),
                then_expr: Box::new(default_val),
                else_expr: Box::new(Expr::LocalGet(tmp_id)),
            };
            emit_for_of_pattern_binding(ctx, &assign_pat.left, with_default, var_ids, var_idx, out)
        }
        _ => Ok(()),
    }
}

/// Does a labeled statement's body ultimately target a loop? `continue`/`break`
/// semantics for a label depend on the *loop* the label names, but a label
/// chain (`outer: inner: for (...)`) nests one `Labeled` inside another, so the
/// outer label's immediate body is a `Labeled`, not the loop. Unwrap nested
/// labels to find the real target. Used by both lowering paths (this file and
/// `lower_decl/body_stmt.rs`) to decide whether a labeled statement is a loop
/// label (bind the label to the loop, so `continue` works) or a non-loop label
/// (desugar to a run-once do-while as a `break` target).
pub(crate) fn labeled_body_targets_loop(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Labeled(labeled) => labeled_body_targets_loop(&labeled.body),
        ast::Stmt::For(_)
        | ast::Stmt::While(_)
        | ast::Stmt::DoWhile(_)
        | ast::Stmt::ForIn(_)
        | ast::Stmt::ForOf(_) => true,
        _ => false,
    }
}

pub(crate) fn lower_stmt(
    ctx: &mut LoweringContext,
    module: &mut Module,
    stmt: &ast::Stmt,
) -> Result<()> {
    match stmt {
        ast::Stmt::Decl(decl) => {
            match decl {
                ast::Decl::Fn(fn_decl) => {
                    // Skip declare functions (no body) - they are external FFI declarations
                    if fn_decl.function.body.is_none() {
                        return Ok(());
                    }
                    let func = lower_fn_decl(ctx, fn_decl)?;
                    // Register return type for call-site inference
                    if let Some((module, class)) =
                        native_instance_from_return_type(&func.return_type)
                    {
                        ctx.push_func_return_native_instance((
                            func.name.clone(),
                            module.to_string(),
                            class.to_string(),
                        ));
                    }
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func.name.clone(), func.return_type.clone());
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> =
                        func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    let rest_idx = func.params.iter().position(|p| p.is_rest);
                    let has_synth_args = func
                        .params
                        .last()
                        .is_some_and(|p| p.arguments_object.is_some());
                    ctx.func_defaults.push((
                        func.id,
                        defaults,
                        param_ids,
                        rest_idx,
                        has_synth_args,
                    ));
                    // #5579: record bare top-level function declarations for
                    // global-object reflection (Script GlobalDeclarationInst-
                    // antiation). `func` is moved into the dedup helper below,
                    // so capture name+id first. Codegen reflects these onto
                    // `globalThis` only for a non-ESM entry program.
                    let global_decl = (func.name.clone(), func.id);
                    push_function_decl_dedup(module, func);
                    // Keep at most one entry per name (last declaration wins,
                    // matching the dedup helper's overwrite semantics).
                    module
                        .script_global_functions
                        .retain(|(n, _)| n != &global_decl.0);
                    module.script_global_functions.push(global_decl);
                }
                ast::Decl::Var(var_decl) => {
                    let mutable = var_decl.kind != ast::VarDeclKind::Const;
                    let is_var = var_decl.kind == ast::VarDeclKind::Var;
                    for decl in &var_decl.decls {
                        // Check if this is a Widget({...}) call from perry/widget
                        if let Some(init) = &decl.init {
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let Some(widget_decl) = try_lower_widget_decl(ctx, call_expr) {
                                    module.widgets.push(widget_decl);
                                    continue;
                                }
                            }
                        }
                        // For array destructuring from generator calls, wrap init in
                        // IteratorToArray so the destructuring gets a real array.
                        // This converts: const [a, b, ...rest] = gen()
                        // to: const [a, b, ...rest] = IteratorToArray(gen())
                        // by inserting a temp variable.
                        if matches!(&decl.name, ast::Pat::Array(_)) {
                            if let Some(init) = &decl.init {
                                if let ast::Expr::Call(call) = init.as_ref() {
                                    if let ast::Callee::Expr(callee) = &call.callee {
                                        if let ast::Expr::Ident(ident) = callee.as_ref() {
                                            if ctx.generator_func_names.contains(ident.sym.as_ref())
                                            {
                                                // Lower the generator call, wrap in IteratorToArray, assign to temp
                                                let gen_expr = lower_expr(ctx, init)?;
                                                let arr_expr =
                                                    Expr::IteratorToArray(Box::new(gen_expr));
                                                let temp_id = ctx.fresh_local();
                                                ctx.locals.push((
                                                    format!("__gen_arr_{}", temp_id),
                                                    temp_id,
                                                    Type::Array(Box::new(Type::Any)),
                                                ));
                                                module.init.push(Stmt::Let {
                                                    id: temp_id,
                                                    name: format!("__gen_arr_{}", temp_id),
                                                    ty: Type::Array(Box::new(Type::Any)),
                                                    mutable: false,
                                                    init: Some(arr_expr),
                                                });
                                                // Now destructure from the temp array
                                                // Create a synthetic VarDeclarator with init = LocalGet(temp_id)
                                                // For simplicity, manually extract each element
                                                if let ast::Pat::Array(arr_pat) = &decl.name {
                                                    let mut idx = 0;
                                                    for elem in &arr_pat.elems {
                                                        if let Some(elem_pat) = elem {
                                                            match elem_pat {
                                                                ast::Pat::Ident(ident) => {
                                                                    let name =
                                                                        ident.id.sym.to_string();
                                                                    let id = ctx.define_local(
                                                                        name.clone(),
                                                                        Type::Any,
                                                                    );
                                                                    module.init.push(Stmt::Let {
                                                                        id,
                                                                        name,
                                                                        ty: Type::Any,
                                                                        mutable,
                                                                        init: Some(
                                                                            Expr::IndexGet {
                                                                                object: Box::new(
                                                                                    Expr::LocalGet(
                                                                                        temp_id,
                                                                                    ),
                                                                                ),
                                                                                index: Box::new(
                                                                                    Expr::Number(
                                                                                        idx as f64,
                                                                                    ),
                                                                                ),
                                                                            },
                                                                        ),
                                                                    });
                                                                    idx += 1;
                                                                }
                                                                ast::Pat::Rest(rest) => {
                                                                    if let ast::Pat::Ident(
                                                                        rest_ident,
                                                                    ) = &*rest.arg
                                                                    {
                                                                        let name = rest_ident
                                                                            .id
                                                                            .sym
                                                                            .to_string();
                                                                        let id = ctx.define_local(
                                                                            name.clone(),
                                                                            Type::Array(Box::new(
                                                                                Type::Any,
                                                                            )),
                                                                        );
                                                                        module.init.push(Stmt::Let {
                                                                            id,
                                                                            name,
                                                                            ty: Type::Array(Box::new(Type::Any)),
                                                                            mutable,
                                                                            init: Some(Expr::ArraySlice {
                                                                                array: Box::new(Expr::LocalGet(temp_id)),
                                                                                start: Box::new(Expr::Number(idx as f64)),
                                                                                end: None,
                                                                            }),
                                                                        });
                                                                    }
                                                                }
                                                                _ => {
                                                                    idx += 1;
                                                                }
                                                            }
                                                        } else {
                                                            idx += 1; // skip holes
                                                        }
                                                    }
                                                }
                                                continue; // skip the regular destructuring path
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // `const { proxy: revProxy, revoke } = Proxy.revocable(t, h)`
                        // is rewritten to a ProxyNew binding + a dummy revoke binding.
                        if let (ast::Pat::Object(obj_pat), Some(init)) = (&decl.name, &decl.init) {
                            let inner = {
                                let mut e = init.as_ref();
                                loop {
                                    match e {
                                        ast::Expr::TsAs(ts_as) => e = &ts_as.expr,
                                        ast::Expr::TsNonNull(nn) => e = &nn.expr,
                                        ast::Expr::TsConstAssertion(ca) => e = &ca.expr,
                                        ast::Expr::TsTypeAssertion(ta) => e = &ta.expr,
                                        ast::Expr::Paren(p) => e = &p.expr,
                                        _ => break,
                                    }
                                }
                                e
                            };
                            let mut is_proxy_revocable = false;
                            if let ast::Expr::Call(call) = inner {
                                if let ast::Callee::Expr(callee) = &call.callee {
                                    if let ast::Expr::Member(m) = callee.as_ref() {
                                        if let ast::Expr::Ident(o) = m.obj.as_ref() {
                                            if o.sym.as_ref() == "Proxy" {
                                                if let ast::MemberProp::Ident(p) = &m.prop {
                                                    if p.sym.as_ref() == "revocable" {
                                                        is_proxy_revocable = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if is_proxy_revocable {
                                if let ast::Expr::Call(call) = inner {
                                    let target_ast = call.args.first().map(|a| a.expr.clone());
                                    let handler_ast = call.args.get(1).map(|a| a.expr.clone());
                                    let target = if let Some(t) = target_ast {
                                        lower_expr(ctx, &t)?
                                    } else {
                                        Expr::Undefined
                                    };
                                    let handler = if let Some(h) = handler_ast {
                                        lower_expr(ctx, &h)?
                                    } else {
                                        Expr::Object(vec![])
                                    };
                                    let mut proxy_alias: Option<String> = None;
                                    let mut revoke_alias: Option<String> = None;
                                    for prop in &obj_pat.props {
                                        match prop {
                                            ast::ObjectPatProp::KeyValue(kv) => {
                                                let key_name = match &kv.key {
                                                    ast::PropName::Ident(i) => i.sym.to_string(),
                                                    ast::PropName::Str(s) => {
                                                        s.value.as_str().unwrap_or("").to_string()
                                                    }
                                                    _ => continue,
                                                };
                                                if let ast::Pat::Ident(alias) = &*kv.value {
                                                    let alias_name = alias.id.sym.to_string();
                                                    if key_name == "proxy" {
                                                        proxy_alias = Some(alias_name);
                                                    } else if key_name == "revoke" {
                                                        revoke_alias = Some(alias_name);
                                                    }
                                                }
                                            }
                                            ast::ObjectPatProp::Assign(a) => {
                                                let name = a.key.sym.to_string();
                                                if name == "proxy" {
                                                    proxy_alias = Some(name);
                                                } else if name == "revoke" {
                                                    revoke_alias = Some(name);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if let Some(p_name) = proxy_alias {
                                        let proxy_id = ctx.define_local(p_name.clone(), Type::Any);
                                        module.init.push(Stmt::Let {
                                            id: proxy_id,
                                            name: p_name.clone(),
                                            ty: Type::Any,
                                            mutable,
                                            init: Some(Expr::ProxyNew {
                                                target: Box::new(target),
                                                handler: Box::new(handler),
                                            }),
                                        });
                                        ctx.proxy_locals.insert(p_name.clone());
                                        if let Some(r_name) = revoke_alias {
                                            ctx.proxy_revoke_locals.insert(r_name.clone(), p_name);
                                            let rev_id =
                                                ctx.define_local(r_name.clone(), Type::Any);
                                            module.init.push(Stmt::Let {
                                                id: rev_id,
                                                name: r_name,
                                                ty: Type::Any,
                                                mutable,
                                                init: Some(Expr::Undefined),
                                            });
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                        // `const X = class { ... }` — lower the class expression
                        // inline using the binding name as the class name (so
                        // `new X(...)` later resolves without a dynamic dispatch
                        // shim). The let binding still stores a sentinel value
                        // (the new'd object) but the class is fully lowered.
                        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, &decl.init) {
                            let inner_expr = {
                                let mut e = init.as_ref();
                                loop {
                                    match e {
                                        ast::Expr::Paren(p) => e = &p.expr,
                                        ast::Expr::TsAs(a) => e = &a.expr,
                                        ast::Expr::TsNonNull(n) => e = &n.expr,
                                        ast::Expr::TsTypeAssertion(a) => e = &a.expr,
                                        _ => break,
                                    }
                                }
                                e
                            };
                            if let ast::Expr::Class(class_expr) = inner_expr {
                                let bind_name = ident.id.sym.to_string();
                                // Only handle if there's no explicit type annotation
                                // that would conflict, and the binding name isn't
                                // already a class (no shadow).
                                if ctx.lookup_class(&bind_name).is_none() {
                                    // Refs #486: `var X = class _X { ... new _X() ... }` —
                                    // the inner self-binding name `_X` references the same
                                    // class as the outer binding `X`. Pre-register the inner
                                    // name as a class alias BEFORE lowering the class body
                                    // so any `Ident("_X")` inside method bodies (e.g.
                                    // `new _X()`) lowers to `Expr::ClassRef("_X")` instead
                                    // of falling through to ExternFuncRef. The HIR `new`
                                    // ident path keys off `lookup_class`. Hono's
                                    // `var Node = class _Node { ... }` and similar npm dist
                                    // shapes hit this.
                                    let inner_name_for_register = class_expr
                                        .ident
                                        .as_ref()
                                        .map(|i| i.sym.to_string())
                                        .filter(|n| n != &bind_name);
                                    if let Some(ref inner_name) = inner_name_for_register {
                                        // Allocate the class id eagerly so we can register
                                        // it under both names; lower_class_from_ast picks up
                                        // the same id via lookup_class(bind_name).
                                        let class_id = ctx.fresh_class();
                                        ctx.register_class(bind_name.clone(), class_id);
                                        ctx.register_class(inner_name.clone(), class_id);
                                        ctx.class_expr_aliases
                                            .insert(inner_name.clone(), bind_name.clone());
                                    }
                                    // The inner (const) binding visible inside the
                                    // body is the class expression's own source ident
                                    // (`var cls = class C {...}` -> `C`); feed it so the
                                    // const-assignment guard uses the user-visible name,
                                    // not the `bind_name` registration key.
                                    ctx.pending_class_inner_name = class_expr
                                        .ident
                                        .as_ref()
                                        .map(|i| i.sym.to_string());
                                    // Lower the class with the binding name so
                                    // `new BindName(...)` works unchanged.
                                    let mut lowered_class =
                                        crate::lower_decl::lower_class_from_ast(
                                            ctx,
                                            &class_expr.class,
                                            &bind_name,
                                            false,
                                        )?;
                                    if let Some(inner_name) = inner_name_for_register {
                                        lowered_class.aliases.push(inner_name);
                                    }
                                    // Computed member keys (`static get [expr]()`,
                                    // `[expr]() {}`) register at runtime against the
                                    // class id — the general class-expression arm in
                                    // `lower_expr.rs` sequences these in front of the
                                    // `ClassRef`. This `var C = class {…}` fast path
                                    // emits a bare `ClassRef` binding instead, so emit
                                    // the same registrations here or the computed
                                    // accessors/methods never reach the side tables
                                    // (Test262 accessor-name-{static,inst}/computed).
                                    let computed_member_registrations: Vec<Expr> = lowered_class
                                        .computed_members
                                        .iter()
                                        .map(|member| {
                                            class_computed_member_registration_expr(
                                                &bind_name, member,
                                            )
                                        })
                                        .collect();
                                    // Runtime-value parent (`var X = class extends
                                    // <expr> {}` where the parent isn't a known class —
                                    // e.g. @hono/node-server's `var Request = class
                                    // extends GlobalRequest {}`, `GlobalRequest =
                                    // global.Request`). The general class-expression arm
                                    // in `lower_expr.rs` and the `Decl::Class` arms emit
                                    // `RegisterClassParentDynamic` so the parent edge —
                                    // and the fetch-parent kind for Request/Response
                                    // subclasses — is wired at module init (where the
                                    // alias still resolves). This `var C = class {…}`
                                    // fast path emitted a bare `ClassRef` binding and
                                    // skipped it, so the parent never registered and a
                                    // `Request`/`Response` subclass got no native handle
                                    // (inherited body methods threw "text is not a
                                    // function"). Emit it here too, in source order
                                    // before the value binding. Clone the extends
                                    // expression before `push_class_dedup` moves the
                                    // class out.
                                    let parent_register =
                                        lowered_class.extends_expr.clone().map(|p| {
                                            Stmt::Expr(Expr::RegisterClassParentDynamic {
                                                class_name: bind_name.clone(),
                                                parent_expr: p,
                                            })
                                        });
                                    // Inline static field/element initializers and
                                    // static blocks at the class-expression's source
                                    // position, exactly as the `Decl::Class` arm does
                                    // for declarations. Without this the `var C =
                                    // class { static x = 1 }` fast path relied solely
                                    // on the late `init_static_fields_late` codegen
                                    // pass, which runs AFTER the surrounding top-level
                                    // statements — so `C.x` read immediately after the
                                    // binding saw the uninitialized (0.0) slot, and a
                                    // static method's `this.#priv` read undefined.
                                    // Interleaved in source order (see
                                    // `build_interleaved_static_init_stmts`) rather
                                    // than all fields then all blocks.
                                    let static_init_stmts =
                                        crate::lower_decl::build_interleaved_static_init_stmts(
                                            &class_expr.class.body,
                                            &bind_name,
                                            &lowered_class.static_fields,
                                            &lowered_class.static_methods,
                                        );
                                    push_class_dedup(module, lowered_class);
                                    if let Some(reg) = parent_register {
                                        module.init.push(reg);
                                    }
                                    for reg in computed_member_registrations {
                                        module.init.push(Stmt::Expr(reg));
                                    }
                                    for s in static_init_stmts {
                                        module.init.push(s);
                                    }
                                    // Register the alias so `new X()` → `new X()`
                                    // (no-op lookup, but marks the binding as a class).
                                    ctx.class_expr_aliases
                                        .insert(bind_name.clone(), bind_name.clone());
                                    emit_class_expression_value_binding(
                                        ctx, module, &bind_name, mutable, is_var,
                                    );
                                    continue;
                                }
                            }
                            // `const Mixed = MixinFn(BaseClass)` — detect a call
                            // to a known mixin function and synthesize a real
                            // class extending the supplied base. The mixin's
                            // class AST is taken from the pre-scan map and
                            // copied verbatim with the `extends` clause rewritten
                            // to point at the concrete base class.
                            if let ast::Expr::Call(call) = inner_expr {
                                if let ast::Callee::Expr(callee_expr) = &call.callee {
                                    if let ast::Expr::Ident(fn_ident) = callee_expr.as_ref() {
                                        let fn_name = fn_ident.sym.to_string();
                                        if let Some((_param_name, mixin_class_box)) =
                                            ctx.mixin_funcs.get(&fn_name).cloned()
                                        {
                                            if call.args.len() == 1 {
                                                if let ast::Expr::Ident(base_ident) =
                                                    call.args[0].expr.as_ref()
                                                {
                                                    let base_class_name =
                                                        base_ident.sym.to_string();
                                                    if ctx.lookup_class(&base_class_name).is_some()
                                                    {
                                                        let bind_name = ident.id.sym.to_string();
                                                        if ctx.lookup_class(&bind_name).is_none() {
                                                            let mut new_class =
                                                                (*mixin_class_box).clone();
                                                            let base_id = ast::Ident::new(
                                                                base_class_name.clone().into(),
                                                                base_ident.span,
                                                                base_ident.ctxt,
                                                            );
                                                            new_class.super_class = Some(Box::new(
                                                                ast::Expr::Ident(base_id),
                                                            ));
                                                            let lowered_class = crate::lower_decl::lower_class_from_ast(
                                                                ctx,
                                                                &new_class,
                                                                &bind_name,
                                                                false,
                                                            )?;
                                                            push_class_dedup(module, lowered_class);
                                                            ctx.class_expr_aliases.insert(
                                                                bind_name.clone(),
                                                                bind_name.clone(),
                                                            );
                                                            continue;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Record chained-assignment class self-aliases (`let
                        // Logger = Logger_1 = class …`) so the self-reference
                        // isn't captured (see `synthesize_class_captures`). This
                        // top-level path is reached for any `let X = T = class`
                        // not handled by the direct `class` fast path above.
                        record_chained_class_self_aliases(ctx, decl);
                        let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable, is_var)?;
                        // `var` is function-scoped: mark defined locals so
                        // `pop_block_scope` preserves them when leaving an inner block.
                        if is_var {
                            for s in &stmts {
                                if let Stmt::Let { id, .. } = s {
                                    ctx.var_hoisted_ids.insert(*id);
                                }
                            }
                        }
                        // Track awaited native module calls as native instances
                        // so property accesses (response.status, response.data) route
                        // through NativeMethodCall dispatch instead of generic PropertyGet.
                        for s in &stmts {
                            if let Stmt::Let {
                                name,
                                init: Some(Expr::Await(inner)),
                                ..
                            } = s
                            {
                                if let Expr::NativeMethodCall {
                                    module: mod_name,
                                    method,
                                    ..
                                } = inner.as_ref()
                                {
                                    let class_name = match (mod_name.as_str(), method.as_str()) {
                                        (
                                            "axios",
                                            "get" | "post" | "put" | "delete" | "patch" | "request",
                                        ) => Some("Response"),
                                        ("mongodb", "connect") => Some("MongoClient"),
                                        ("pg", "connect") => Some("Client"),
                                        _ => None,
                                    };
                                    if let Some(cn) = class_name {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            mod_name.clone(),
                                            cn.to_string(),
                                        );
                                    }
                                }
                            }
                            // Track synchronous native module factories as native instances.
                            // Added for workstream A1.5 so `const sock = net.createConnection(...)`
                            // registers `sock` as a Socket instance; without this, subsequent
                            // `sock.write/on/end/destroy` miss the NATIVE_MODULE_TABLE dispatch
                            // and never reach the `js_net_socket_*` FFI in perry-stdlib.
                            if let Stmt::Let {
                                name,
                                init:
                                    Some(Expr::NativeMethodCall {
                                        module: mod_name,
                                        method,
                                        object: None,
                                        ..
                                    }),
                                ..
                            } = s
                            {
                                let class_name = match (mod_name.as_str(), method.as_str()) {
                                    ("net", "createConnection" | "connect") => Some("Socket"),
                                    // tls.connect returns the same Socket class — reuses
                                    // all the write/end/destroy/on/upgradeToTLS dispatch.
                                    ("tls", "connect") => Some("Socket"),
                                    // Issue #422: `new net.Socket()` lowers to a
                                    // receiver-less `Expr::NativeMethodCall` whose
                                    // method is the constructor name "Socket"; this
                                    // arm registers the binding so `sock.connect/...`
                                    // dispatches via the class-filtered entries.
                                    ("net", "Socket") => Some("Socket"),
                                    ("net", "Server") => Some("Server"),
                                    ("net", "BlockList") => Some("BlockList"),
                                    ("net", "SocketAddress") => Some("SocketAddress"),
                                    ("vm", "SourceTextModule") => Some("SourceTextModule"),
                                    ("vm", "SyntheticModule") => Some("SyntheticModule"),
                                    _ => None,
                                };
                                if let Some(cn) = class_name {
                                    let instance_module =
                                        if mod_name == "vm" { "vm" } else { "net" };
                                    ctx.register_native_instance(
                                        name.clone(),
                                        instance_module.to_string(),
                                        cn.to_string(),
                                    );
                                    let _ = mod_name; // suppress unused on tls branch
                                }
                                // Issue #577 — node:http / node:https / node:http2 server
                                // factories. `const s = createServer(...)` (named import)
                                // and `const s = http.createServer(...)` (namespace import)
                                // both lower to a receiver-less NativeMethodCall here, so
                                // this arm covers both shapes.
                                let http_class = match (mod_name.as_str(), method.as_str()) {
                                    ("http", "createServer") => Some("HttpServer"),
                                    ("https", "createServer") => Some("HttpsServer"),
                                    ("http2", "createSecureServer") => Some("Http2SecureServer"),
                                    ("tls", "createServer" | "Server") => Some("Server"),
                                    ("async_hooks", "createHook") => Some("AsyncHook"),
                                    _ => None,
                                };
                                if let Some(cn) = http_class {
                                    let module_owned = mod_name.clone();
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_owned.clone(),
                                        cn.to_string(),
                                    );
                                    ctx.push_module_native_instance((
                                        name.clone(),
                                        module_owned,
                                        cn.to_string(),
                                    ));
                                }
                                let dns_class = match (mod_name.as_str(), method.as_str()) {
                                    ("dns" | "dns/promises", "Resolver") => Some("Resolver"),
                                    _ => None,
                                };
                                if let Some(cn) = dns_class {
                                    let module_owned = mod_name.clone();
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_owned.clone(),
                                        cn.to_string(),
                                    );
                                    ctx.push_module_native_instance((
                                        name.clone(),
                                        module_owned,
                                        cn.to_string(),
                                    ));
                                }
                                // Issue #769 — node:http / node:https CLIENT factories.
                                // `const req = http.request(url, cb)` and `http.get` / `https.*`
                                // variants return a ClientRequest handle; register under
                                // module `"http"` so subsequent `req.on/.end/.write/...`
                                // resolve via the class-filtered entries in NATIVE_MODULE_TABLE.
                                let client_class = match (mod_name.as_str(), method.as_str()) {
                                    ("http", "request" | "get") => Some("ClientRequest"),
                                    ("https", "request" | "get") => Some("ClientRequest"),
                                    _ => None,
                                };
                                if let Some(cn) = client_class {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "http".to_string(),
                                        cn.to_string(),
                                    );
                                }
                            }
                            // Issue #1123 followup — `net.createServer(...)` /
                            // bare `createServer(...)` from `node:net` lower
                            // to `Expr::NetCreateServer { … }` (NOT the
                            // generic `NativeMethodCall` shape above), so
                            // they need their own registration arm. Tagging
                            // the binding as `("net", "Server")` makes
                            // subsequent `server.listen/.close/.on/.address`
                            // calls dispatch via the class_filter rows
                            // added in lower_call.rs.
                            if let Stmt::Let {
                                name,
                                init: Some(Expr::NetCreateServer { .. }),
                                ..
                            } = s
                            {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "net".to_string(),
                                    "Server".to_string(),
                                );
                                ctx.push_module_native_instance((
                                    name.clone(),
                                    "net".to_string(),
                                    "Server".to_string(),
                                ));
                            }
                            // User-defined factory wrappers: when the init is a
                            // bare call to `userFunc(...)` and `userFunc` was
                            // registered as a native-instance factory (via
                            // its declared return type), inherit the class so
                            // downstream `local.method(...)` dispatches statically.
                            // Example: `function openSocket(): Socket { ... }`
                            // followed by `const sock = openSocket(...)` registers
                            // sock as ("net", "Socket").
                            if let Stmt::Let {
                                name,
                                init: Some(Expr::Call { callee, .. }),
                                ..
                            } = s
                            {
                                if let Expr::FuncRef(func_id) = callee.as_ref() {
                                    let func_name_owned =
                                        ctx.lookup_func_name(*func_id).map(|s| s.to_string());
                                    if let Some(func_name) = func_name_owned {
                                        let lookup = ctx
                                            .lookup_func_return_native_instance(&func_name)
                                            .map(|(m, c)| (m.to_string(), c.to_string()));
                                        if let Some((m, c)) = lookup {
                                            ctx.register_native_instance(name.clone(), m, c);
                                        }
                                    }
                                }
                            }
                        }
                        module.init.extend(stmts);
                    }
                }
                ast::Decl::Class(class_decl) => {
                    let binding_name = class_decl.ident.sym.to_string();
                    let class = lower_class_decl(ctx, class_decl, false)?;
                    // #5833: give the class NAME a real mutable lexical
                    // binding slot, matching GlobalDeclarationInstantiation's
                    // "a class declaration creates a MUTABLE binding" step
                    // (unlike `const`) — test262 `language/global-code/
                    // decl-lex.js`. Without a local slot, every bare read of
                    // the name fell through to `ctx.lookup_class` and
                    // re-derived `Expr::ClassRef` from the class registry,
                    // and every assignment hit the `lookup_class`/`lookup_func`
                    // "evaluate RHS, don't shadow the binding" special case in
                    // `expr_assign.rs` (added for Drizzle's `sql || (sql =
                    // {})` idiom, #420) — so `Foo = 5; console.log(Foo)`
                    // silently dropped the store and printed the frozen
                    // class-id value instead of `5`.
                    //
                    // Gated on `reassigned_top_level_identifiers`: `lookup_local`
                    // is checked before both of those class-registry fallbacks,
                    // so seeding a local unconditionally would ALSO redirect
                    // every plain read of the name (not just reassignments) to
                    // `Expr::LocalGet` — breaking call sites that pattern-match
                    // `Expr::ClassRef` after lowering a bare class-name
                    // identifier, e.g. `export default Widget;` (#665). Only
                    // classes actually reassigned somewhere in the module get a
                    // local; the overwhelmingly common never-reassigned case is
                    // untouched.
                    if ctx.reassigned_top_level_identifiers.contains(&binding_name) {
                        let class_local_id = match ctx.lookup_local(&binding_name) {
                            Some(id) => id,
                            None => ctx.define_local(binding_name.clone(), Type::Any),
                        };
                        module.init.push(Stmt::Let {
                            id: class_local_id,
                            name: binding_name,
                            ty: Type::Any,
                            mutable: true,
                            init: Some(Expr::ClassRef(class.name.clone())),
                        });
                    }
                    // Issue #711: emit dynamic parent-class registration
                    // at the source-order position of the class declaration
                    // BEFORE the static-field-init stmts. Static field
                    // initializers may reference inherited static methods,
                    // and method dispatch reads the parent chain — wiring
                    // the parent edge first keeps the inherited lookup
                    // path consistent for those inits.
                    if let Some(extends_expr) = &class.extends_expr {
                        module
                            .init
                            .push(Stmt::Expr(Expr::RegisterClassParentDynamic {
                                class_name: class.name.clone(),
                                parent_expr: extends_expr.clone(),
                            }));
                    }
                    for member in &class.computed_members {
                        module
                            .init
                            .push(Stmt::Expr(class_computed_member_registration_expr(
                                &class.name,
                                member,
                            )));
                    }
                    // Inject static-field-init and static-block-call
                    // statements at the source position of the class
                    // declaration, INTERLEAVED in source order (see
                    // `build_interleaved_static_init_stmts`) — per
                    // ClassDefinitionEvaluation, static fields and static
                    // blocks are a single ordered evaluation pass, not all
                    // fields followed by all blocks. This matters both for
                    // observable ordering (test262 static-init-sequence.js)
                    // and for a top-level const referenced by a later
                    // field's initializer: the upfront `init_static_fields`
                    // pass at codegen.rs:3449 runs before any user `Let`
                    // bindings, so it captures unbound (undefined) values —
                    // these inline statements re-run with the correct values
                    // once we reach this point in source order. The
                    // codegen-side fallback in `init_static_fields_late` is
                    // kept for class expressions that bypass this
                    // declaration path; it skips blocks already invoked via
                    // this inline call.
                    module.init.extend(
                        crate::lower_decl::build_interleaved_static_init_stmts(
                            &class_decl.class.body,
                            &class.name,
                            &class.static_fields,
                            &class.static_methods,
                        ),
                    );
                    append_legacy_decorator_init_for_class(ctx, &mut module.init, &class);
                    push_class_dedup(module, class);
                }
                ast::Decl::TsEnum(enum_decl) => {
                    let en = lower_enum_decl(ctx, enum_decl, false)?;
                    module.enums.push(en);
                }
                ast::Decl::TsInterface(iface_decl) => {
                    let iface = lower_interface_decl(ctx, iface_decl, false)?;
                    module.interfaces.push(iface);
                }
                ast::Decl::TsTypeAlias(alias_decl) => {
                    let alias = lower_type_alias_decl(ctx, alias_decl, false)?;
                    module.type_aliases.push(alias);
                }
                ast::Decl::Using(using_decl) => {
                    // `using x = expr` / `await using x = expr` — TC39 Explicit
                    // Resource Management. Lower as const bindings. Disposal at
                    // block-scope exit is not yet automated — the variables are
                    // accessible but [Symbol.dispose/asyncDispose] isn't called.
                    // Treat as a const var declaration.
                    let fake_var = ast::VarDecl {
                        span: using_decl.span,
                        kind: ast::VarDeclKind::Const,
                        declare: false,
                        decls: using_decl.decls.clone(),
                        ctxt: Default::default(),
                    };
                    let mutable = false;
                    let _is_var = false;
                    for decl in &fake_var.decls {
                        if let Some(init) = &decl.init {
                            if let ast::Pat::Ident(bind_ident) = &decl.name {
                                let name = bind_ident.sym.to_string();
                                let init_expr = lower_expr(ctx, init)?;
                                let ty = Type::Any;
                                let id = ctx.fresh_local();
                                ctx.locals.push((name.clone(), id, ty.clone()));
                                module.init.push(Stmt::Let {
                                    id,
                                    name,
                                    ty,
                                    mutable,
                                    init: Some(init_expr),
                                });
                            }
                        }
                    }
                }
                ast::Decl::TsModule(ts_module)
                    // namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare => {
                        if let Some(ref body) = ts_module.body {
                            let ns_name = match &ts_module.id {
                                ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                                ast::TsModuleName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            let class =
                                lower_namespace_as_class(ctx, module, &ns_name, body, false)?;
                            push_class_dedup(module, class);
                        }
                    }
                // #853: `ast::Decl` is `#[non_exhaustive]` upstream — keep
                // this catch-all so a future SWC variant is dropped silently
                // (the supported variants above each have explicit handling).
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            module
                .init
                .extend(predeclare_implicit_assignment_targets(ctx, &expr_stmt.expr));
            // Check if this is a destructuring assignment that needs special handling
            let maybe_assign = match expr_stmt.expr.as_ref() {
                ast::Expr::Assign(assign) => Some(assign),
                ast::Expr::Paren(paren) => match paren.expr.as_ref() {
                    ast::Expr::Assign(assign) => Some(assign),
                    _ => None,
                },
                _ => None,
            };
            if let Some(assign) = maybe_assign {
                if let ast::AssignTarget::Pat(pat) = &assign.left {
                    // This is a destructuring assignment at statement level
                    // We can emit proper Let statements for temporaries
                    let stmts = lower_destructuring_assignment_stmt(ctx, pat, &assign.right)?;
                    module.init.extend(stmts);
                    return Ok(());
                }
                // #6071: a compound member/index assignment at statement level —
                // spill the base + computed key into Let temps so each is
                // evaluated once (the expression path lowers them twice).
                if let Some(stmts) =
                    crate::lower::expr_assign::hoist_compound_member_assign(ctx, assign)?
                {
                    module.init.extend(stmts);
                    return Ok(());
                }
            }
            let expr = lower_expr(ctx, &expr_stmt.expr)?;
            module.init.push(Stmt::Expr(expr));
        }
        ast::Stmt::If(if_stmt) => {
            module
                .init
                .extend(predeclare_implicit_assignment_targets(ctx, &if_stmt.test));
            let condition = lower_expr(ctx, &if_stmt.test)?;
            // Each branch introduces its own lexical scope. Skip extra push if
            // branch is a BlockStmt (handled there) or an If (else-if chain).
            let then_branch = if matches!(*if_stmt.cons, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &if_stmt.cons)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &if_stmt.cons)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            let else_branch = if_stmt
                .alt
                .as_ref()
                .map(|s| {
                    if matches!(**s, ast::Stmt::Block(_)) || matches!(**s, ast::Stmt::If(_)) {
                        lower_body_stmt(ctx, s)
                    } else {
                        let mark = ctx.push_block_scope();
                        let stmts = lower_body_stmt(ctx, s);
                        ctx.pop_block_scope(mark);
                        stmts
                    }
                })
                .transpose()?;
            module.init.push(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }
        ast::Stmt::While(while_stmt) => {
            let condition = lower_expr(ctx, &while_stmt.test)?;
            let body = if matches!(*while_stmt.body, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &while_stmt.body)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &while_stmt.body)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            module.init.push(Stmt::While { condition, body });
        }
        ast::Stmt::DoWhile(do_while_stmt) => {
            let body = lower_body_stmt(ctx, &do_while_stmt.body)?;
            let condition = lower_expr(ctx, &do_while_stmt.test)?;
            module.init.push(Stmt::DoWhile { body, condition });
        }
        ast::Stmt::Labeled(labeled_stmt) => {
            let label = labeled_stmt.label.sym.to_string();
            // #2383 + #5247: a labeled *non-loop* statement — a block
            // (`a: { ... break a; ... }`) or an `if`/`switch`/expression — exits via
            // `break a`. It is not a loop, so the loop-based labeled-break codegen
            // has nothing to bind the label to. Desugar any labeled non-loop to a
            // labeled run-once do-while so it has an exit block to target (also
            // correct when `break a` fires from inside a nested loop in the body).
            // Labeled LOOPS skip this and keep the label bound to the loop, so
            // `continue a` targets the loop. See the matching code in
            // lower_decl/body_stmt.rs.
            //
            // #5247: a label chain ending in a loop (`outer: inner: for (...)`)
            // is a loop label too — the outer label's immediate body is another
            // `Labeled`, not the loop, so unwrap nested labels before deciding.
            // Otherwise `outer` would desugar to a run-once do-while and
            // `continue outer` would target that instead of the real loop.
            let body_is_loop = labeled_body_targets_loop(&labeled_stmt.body);
            if !body_is_loop {
                let body = if let ast::Stmt::Block(block) = &*labeled_stmt.body {
                    lower_block_stmt_scoped(ctx, block)?
                } else {
                    lower_body_stmt(ctx, &labeled_stmt.body)?
                };
                module.init.push(Stmt::Labeled {
                    label,
                    body: Box::new(Stmt::DoWhile {
                        body,
                        condition: Expr::Bool(false),
                    }),
                });
                return Ok(());
            }
            let inner = lower_body_stmt(ctx, &labeled_stmt.body)?;
            if inner.len() == 1 {
                let body = inner.into_iter().next().unwrap();
                module.init.push(Stmt::Labeled {
                    label,
                    body: Box::new(body),
                });
            } else {
                let mut inner = inner;
                let last = inner.pop().unwrap();
                for s in inner {
                    module.init.push(s);
                }
                module.init.push(Stmt::Labeled {
                    label,
                    body: Box::new(last),
                });
            }
        }
        ast::Stmt::Break(break_stmt) => {
            if let Some(ref label) = break_stmt.label {
                module.init.push(Stmt::LabeledBreak(label.sym.to_string()));
            } else {
                module.init.push(Stmt::Break);
            }
        }
        ast::Stmt::Continue(continue_stmt) => {
            if let Some(ref label) = continue_stmt.label {
                module
                    .init
                    .push(Stmt::LabeledContinue(label.sym.to_string()));
            } else {
                module.init.push(Stmt::Continue);
            }
        }
        ast::Stmt::For(for_stmt) => {
            // Push a lexical scope covering init/test/update/body, so
            // `for (let i = 0; ...)` bindings don't leak to the outer scope.
            let for_scope_mark = ctx.push_block_scope();
            let init = if let Some(init) = &for_stmt.init {
                match init {
                    ast::VarDeclOrExpr::VarDecl(var_decl) => {
                        let is_var = var_decl.kind == ast::VarDeclKind::Var;
                        if is_var {
                            for decl in var_decl.decls.iter() {
                                if let Some(init_ast) = decl.init.as_ref() {
                                    module.init.extend(predeclare_implicit_assignment_targets(
                                        ctx, init_ast,
                                    ));
                                }
                                // A destructuring declarator (`for (var {a} = o; …)`)
                                // routes through the shared pattern-binding helper
                                // rather than `get_binding_name`, which only handles
                                // plain idents. The bound ids are var-hoisted so they
                                // escape the for's block scope, matching plain
                                // `var`-decl destructuring.
                                if is_destructuring_pattern(&decl.name) {
                                    let init_expr = decl
                                        .init
                                        .as_ref()
                                        .map(|e| lower_expr(ctx, e))
                                        .transpose()?
                                        .ok_or_else(|| {
                                            anyhow!("Destructuring requires an initializer")
                                        })?;
                                    let stmts = crate::destructuring::lower_pattern_binding(
                                        ctx, &decl.name, init_expr, true, true,
                                    )?;
                                    for stmt in &stmts {
                                        if let Stmt::Let { id, .. } = stmt {
                                            ctx.var_hoisted_ids.insert(*id);
                                        }
                                    }
                                    module.init.extend(stmts);
                                    continue;
                                }
                                let name = get_binding_name(&decl.name)?;
                                let init_expr =
                                    decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                ctx.var_hoisted_ids.insert(id);
                                module.init.push(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: init_expr,
                                });
                            }
                            None
                        } else {
                            for decl in var_decl.decls.iter().skip(1) {
                                if let Some(init_ast) = decl.init.as_ref() {
                                    module.init.extend(predeclare_implicit_assignment_targets(
                                        ctx, init_ast,
                                    ));
                                }
                                // `for (let {a} = o, i = 0; …)` — a destructuring
                                // declarator binds via the shared helper into the
                                // pre-loop init block.
                                if is_destructuring_pattern(&decl.name) {
                                    let init_expr = decl
                                        .init
                                        .as_ref()
                                        .map(|e| lower_expr(ctx, e))
                                        .transpose()?
                                        .ok_or_else(|| {
                                            anyhow!("Destructuring requires an initializer")
                                        })?;
                                    let stmts = crate::destructuring::lower_pattern_binding(
                                        ctx, &decl.name, init_expr, true, false,
                                    )?;
                                    module.init.extend(stmts);
                                    continue;
                                }
                                let name = get_binding_name(&decl.name)?;
                                let init_expr =
                                    decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                module.init.push(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: init_expr,
                                });
                            }
                            if let Some(decl) = var_decl.decls.first() {
                                if let Some(init_ast) = decl.init.as_ref() {
                                    module.init.extend(predeclare_implicit_assignment_targets(
                                        ctx, init_ast,
                                    ));
                                }
                                // A destructuring first-declarator can't be a single
                                // `Stmt::Let` (it lowers to several binds), so emit it
                                // into the pre-loop init block and leave the for's own
                                // init empty. It still runs exactly once before the
                                // first test, preserving for-init semantics.
                                if is_destructuring_pattern(&decl.name) {
                                    let init_expr = decl
                                        .init
                                        .as_ref()
                                        .map(|e| lower_expr(ctx, e))
                                        .transpose()?
                                        .ok_or_else(|| {
                                            anyhow!("Destructuring requires an initializer")
                                        })?;
                                    let stmts = crate::destructuring::lower_pattern_binding(
                                        ctx, &decl.name, init_expr, true, false,
                                    )?;
                                    module.init.extend(stmts);
                                    None
                                } else {
                                    let name = get_binding_name(&decl.name)?;
                                    let init_expr = decl
                                        .init
                                        .as_ref()
                                        .map(|e| lower_expr(ctx, e))
                                        .transpose()?;
                                    let id = ctx.define_local(name.clone(), Type::Any);
                                    Some(Box::new(Stmt::Let {
                                        id,
                                        name,
                                        ty: Type::Any,
                                        mutable: true,
                                        init: init_expr,
                                    }))
                                }
                            } else {
                                None
                            }
                        }
                    }
                    ast::VarDeclOrExpr::Expr(expr) => {
                        for stmt in predeclare_implicit_assignment_targets(ctx, expr) {
                            module.init.push(stmt);
                        }
                        Some(Box::new(Stmt::Expr(lower_expr(ctx, expr)?)))
                    }
                }
            } else {
                None
            };
            let condition = for_stmt
                .test
                .as_ref()
                .map(|e| lower_expr(ctx, e))
                .transpose()?;
            let update = for_stmt
                .update
                .as_ref()
                .map(|e| lower_expr(ctx, e))
                .transpose()?;
            let body = lower_body_stmt(ctx, &for_stmt.body)?;
            ctx.pop_block_scope(for_scope_mark);
            module.init.push(Stmt::For {
                init,
                condition,
                update,
                body,
            });
        }
        ast::Stmt::Block(block) => {
            // Bare block: introduce a lexical scope so inner let/const shadow
            // without leaking into the enclosing module scope.
            let stmts = lower_block_stmt_scoped(ctx, block)?;
            for stmt in stmts {
                module.init.push(stmt);
            }
        }
        ast::Stmt::Try(try_stmt) => {
            // try body is its own lexical scope
            let previous_optional_require_try_depth = ctx.optional_require_try_depth;
            ctx.optional_require_try_depth = previous_optional_require_try_depth.saturating_add(1);
            let body_result = lower_block_stmt_scoped(ctx, &try_stmt.block);
            ctx.optional_require_try_depth = previous_optional_require_try_depth;
            let body = body_result?;

            // Lower catch clause (if present)
            let catch = if let Some(ref catch_clause) = try_stmt.handler {
                let scope_mark = ctx.enter_scope();

                let mut binding_stmts: Vec<Stmt> = Vec::new();
                let param = if let Some(ref pat) = catch_clause.param {
                    let param_name = get_pat_name(pat)?;
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    // Destructured catch binding — `catch ([a, b = d()])` /
                    // `catch ({ message })`: bind the pattern leaves off the
                    // exception value before the user body runs.
                    if !matches!(pat, ast::Pat::Ident(_)) {
                        let mut leaves = Vec::new();
                        collect_for_of_pattern_leaves(ctx, pat, &mut leaves);
                        let mut idx = 0usize;
                        emit_for_of_pattern_binding(
                            ctx,
                            pat,
                            Expr::LocalGet(param_id),
                            &leaves,
                            &mut idx,
                            &mut binding_stmts,
                        )?;
                    }
                    Some((param_id, param_name))
                } else {
                    None
                };

                // Annex B B.3.4: track the (simple-ident) catch parameter name so
                // a `var <name> = init` inside the catch body targets the catch
                // binding, not the function-scoped hoisted `var`.
                let mut pushed_catch_scope = false;
                if let Some((_, ref pname)) = param {
                    if matches!(catch_clause.param, Some(ast::Pat::Ident(_))) {
                        let mut set = std::collections::HashSet::new();
                        set.insert(pname.clone());
                        ctx.catch_param_scopes.push(set);
                        pushed_catch_scope = true;
                    }
                }
                let mut catch_body = lower_block_stmt(ctx, &catch_clause.body)?;
                if pushed_catch_scope {
                    ctx.catch_param_scopes.pop();
                }
                for (i, stmt) in binding_stmts.into_iter().enumerate() {
                    catch_body.insert(i, stmt);
                }
                ctx.exit_scope(scope_mark);

                Some(CatchClause {
                    param,
                    body: catch_body,
                })
            } else {
                None
            };

            // finally block is its own lexical scope
            let finally = if let Some(ref finally_block) = try_stmt.finalizer {
                Some(lower_block_stmt_scoped(ctx, finally_block)?)
            } else {
                None
            };

            module.init.push(Stmt::Try {
                body,
                catch,
                finally,
            });
        }
        ast::Stmt::Throw(throw_stmt) => {
            let expr = lower_expr(ctx, &throw_stmt.arg)?;
            module.init.push(Stmt::Throw(expr));
        }
        ast::Stmt::Switch(switch_stmt) => {
            let discriminant = lower_expr(ctx, &switch_stmt.discriminant)?;
            let mut cases = Vec::new();
            let switch_scope_mark = ctx.push_block_scope();
            // Case statement-lists share the switch's block scope without
            // being a `BlockStmt`, so they don't pass through
            // `lower_block_stmt` — re-bind their pre-registered
            // forward-captured lets here (all cases up front: one scope).
            for case in &switch_stmt.cases {
                rebind_nested_forward_scope_lets(ctx, &case.cons);
            }

            for case in &switch_stmt.cases {
                let test = case.test.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;

                let mut body = Vec::new();
                for stmt in &case.cons {
                    body.extend(lower_body_stmt(ctx, stmt)?);
                }

                cases.push(SwitchCase { test, body });
            }

            ctx.pop_block_scope(switch_scope_mark);

            module.init.push(Stmt::Switch {
                discriminant,
                cases,
            });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            lower_stmt_for_of(ctx, module, for_of_stmt)?;
        }
        ast::Stmt::ForIn(for_in_stmt) => {
            lower_stmt_for_in(ctx, module, for_in_stmt)?;
        }
        ast::Stmt::With(with_stmt) => {
            if ctx.current_strict_mode() || ctx.current_strict {
                crate::lower_bail!(
                    with_stmt.span,
                    "`with` statement is forbidden in strict mode"
                );
            }
            let insert_at = module.init.len();
            let env_id = ctx.define_local("__perry_with_env".to_string(), Type::Any);
            module.init.push(Stmt::Let {
                id: env_id,
                name: format!("__perry_with_env_{}", env_id),
                ty: Type::Any,
                mutable: false,
                init: Some(lower_expr(ctx, &with_stmt.obj)?),
            });
            ctx.push_with_env(env_id);
            let body_result = lower_body_stmt(ctx, &with_stmt.body);
            ctx.pop_with_env();
            module.init.extend(body_result?);
            // Sentinel slots for implicit globals minted by with-set
            // fallbacks inside this body (see with_set_fallback_for_ident).
            for (i, (id, name)) in ctx.pending_with_implicit_inits.drain(..).enumerate() {
                module.init.insert(
                    insert_at + i,
                    crate::lower::with_implicit_unset_let(id, name),
                );
            }
        }
        _ => {}
    }
    Ok(())
}
