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
            for elem in &arr_pat.elems {
                if let Some(ep) = elem {
                    if let ast::Pat::Rest(rest) = ep {
                        collect_for_of_pattern_leaves(ctx, &rest.arg, out);
                    } else {
                        collect_for_of_pattern_leaves(ctx, ep, out);
                    }
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
                        ctx.func_return_native_instances.push((
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
                        .is_some_and(|p| p.is_rest && p.name == "arguments");
                    ctx.func_defaults.push((
                        func.id,
                        defaults,
                        param_ids,
                        rest_idx,
                        has_synth_args,
                    ));
                    module.functions.push(func);
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
                        // Track locals assigned from `regex.exec(...)` so .index/.groups
                        // accesses route to the bare RegExpExecIndex/Groups variants.
                        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, &decl.init) {
                            if is_regex_exec_init(ctx, init) {
                                ctx.regex_exec_locals.insert(ident.id.sym.to_string());
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
                                    push_class_dedup(module, lowered_class);
                                    // Register the alias so `new X()` → `new X()`
                                    // (no-op lookup, but marks the binding as a class).
                                    ctx.class_expr_aliases
                                        .insert(bind_name.clone(), bind_name.clone());
                                    // We intentionally DO NOT push a Stmt::Let for
                                    // this binding — the class itself takes the
                                    // role of a "static value" referenced by name.
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
                        let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable)?;
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
                                    _ => None,
                                };
                                if let Some(cn) = class_name {
                                    // Register under `"net"` (the module the Socket class belongs to)
                                    // regardless of which module the factory lived in, so method
                                    // dispatch resolves correctly.
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "net".to_string(),
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
                                    ctx.module_native_instances.push((
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
                                ctx.module_native_instances.push((
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
                    let class = lower_class_decl(ctx, class_decl, false)?;
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
                    // Inject static-field-init statements at the source
                    // position of the class declaration. Per ES spec, a
                    // class declaration's static initializers run when the
                    // declaration evaluates — i.e., here in source order,
                    // not at the top of module init. This matters when a
                    // static field's initializer references a top-level
                    // const declared earlier in the module: the upfront
                    // `init_static_fields` pass at codegen.rs:3449 runs
                    // before any user `Let` bindings, so it captures
                    // unbound (undefined) values. The inline statements
                    // re-run with the correct values once we reach this
                    // point in source order.
                    for sf in &class.static_fields {
                        if let Some(init) = &sf.init {
                            if let Some(key) = sf.key_expr.as_ref() {
                                module.init.push(Stmt::Expr(Expr::ClassStaticSymbolSet {
                                    class_name: class.name.clone(),
                                    key: Box::new(key.clone()),
                                    value: Box::new(init.clone()),
                                }));
                            } else {
                                module.init.push(Stmt::Expr(Expr::StaticFieldSet {
                                    class_name: class.name.clone(),
                                    field_name: sf.name.clone(),
                                    value: Box::new(init.clone()),
                                }));
                            }
                        }
                    }
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
                ast::Decl::TsModule(ts_module) => {
                    // namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare {
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
                }
                // #853: `ast::Decl` is `#[non_exhaustive]` upstream — keep
                // this catch-all so a future SWC variant is dropped silently
                // (the supported variants above each have explicit handling).
                #[allow(unreachable_patterns)]
                _ => {}
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            // Check if this is a destructuring assignment that needs special handling
            if let ast::Expr::Assign(assign) = expr_stmt.expr.as_ref() {
                if let ast::AssignTarget::Pat(pat) = &assign.left {
                    // This is a destructuring assignment at statement level
                    // We can emit proper Let statements for temporaries
                    let stmts = lower_destructuring_assignment_stmt(ctx, pat, &assign.right)?;
                    module.init.extend(stmts);
                    return Ok(());
                }
            }
            let expr = lower_expr(ctx, &expr_stmt.expr)?;
            module.init.push(Stmt::Expr(expr));
        }
        ast::Stmt::If(if_stmt) => {
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
                                let name = get_binding_name(&decl.name)?;
                                let init_expr =
                                    decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                Some(Box::new(Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: true,
                                    init: init_expr,
                                }))
                            } else {
                                None
                            }
                        }
                    }
                    ast::VarDeclOrExpr::Expr(expr) => {
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
            let body = lower_block_stmt_scoped(ctx, &try_stmt.block)?;

            // Lower catch clause (if present)
            let catch = if let Some(ref catch_clause) = try_stmt.handler {
                let scope_mark = ctx.enter_scope();

                let param = if let Some(ref pat) = catch_clause.param {
                    let param_name = get_pat_name(pat)?;
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    Some((param_id, param_name))
                } else {
                    None
                };

                let catch_body = lower_block_stmt(ctx, &catch_clause.body)?;
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

            for case in &switch_stmt.cases {
                let test = case.test.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;

                let mut body = Vec::new();
                for stmt in &case.cons {
                    body.extend(lower_body_stmt(ctx, stmt)?);
                }

                cases.push(SwitchCase { test, body });
            }

            module.init.push(Stmt::Switch {
                discriminant,
                cases,
            });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            // --- Iterator protocol path for generators ---
            // Detect: for (const x of genFunc(...)) where genFunc is function*
            let is_generator_call = if let ast::Expr::Call(call) = &*for_of_stmt.right {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = &**callee_expr {
                        ctx.generator_func_names.contains(ident.sym.as_ref())
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            // Detect whether the called generator was an `async function*`.
            // Async generators always return `Promise<{value, done}>` from
            // `.next()`, so the iterator-protocol loop must `await` each
            // call before reading `.value` / `.done`. Either the user
            // wrote `for await (...)` (SWC `is_await`) or the callee was
            // declared async — both must trigger awaiting.
            let callee_is_async_gen = if let ast::Expr::Call(call) = &*for_of_stmt.right {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = &**callee_expr {
                        ctx.async_generator_func_names.contains(ident.sym.as_ref())
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            let needs_await = for_of_stmt.is_await || callee_is_async_gen;

            // Also detect: for (const x of new Range(...)) where Range
            // defines `*[Symbol.iterator]()`. We lowered that method as
            // a synthesized top-level generator function taking `this`
            // as its first parameter; the for-of here dispatches by
            // calling that function with the lowered receiver.
            let iter_from_class: Option<perry_types::FuncId> =
                if let ast::Expr::New(new_expr) = &*for_of_stmt.right {
                    if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
                        let class_name = ident.sym.to_string();
                        ctx.iterator_func_for_class.get(&class_name).copied()
                    } else {
                        None
                    }
                } else {
                    None
                };

            if is_generator_call || iter_from_class.is_some() {
                // Lower to iterator protocol:
                //   let __iter = genFunc(...);                     // generator-fn path
                //   let __iter = __perry_iter_Range(new Range(...));  // class path
                //   let __result = __iter.next();
                //   while (!__result.done) { const x = __result.value; body; __result = __iter.next(); }
                let for_scope_mark = ctx.push_block_scope();
                let iter_expr = lower_expr(ctx, &for_of_stmt.right)?;
                // For the class path we wrap the lowered `new Range(..)`
                // in a direct FuncRef call to the synthesized iterator
                // function (which has `this` as its first parameter).
                let iter_expr = if let Some(iter_fn_id) = iter_from_class {
                    Expr::Call {
                        callee: Box::new(Expr::FuncRef(iter_fn_id)),
                        args: vec![iter_expr],
                        type_args: vec![],
                    }
                } else {
                    iter_expr
                };
                let iter_id = ctx.fresh_local();
                ctx.locals
                    .push((format!("__iter_{}", iter_id), iter_id, Type::Any));
                module.init.push(Stmt::Let {
                    id: iter_id,
                    name: format!("__iter_{}", iter_id),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(iter_expr),
                });

                let result_id = ctx.fresh_local();
                ctx.locals
                    .push((format!("__result_{}", result_id), result_id, Type::Any));
                // __result = __iter.next()
                // For async generators / `for await ... of`, wrap the
                // call in `Expr::Await` so the resolved iter-result
                // (`{value, done}`) is what's stored, not the Promise.
                let raw_next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };
                let next_call = if needs_await {
                    Expr::Await(Box::new(raw_next_call))
                } else {
                    raw_next_call
                };
                module.init.push(Stmt::Let {
                    id: result_id,
                    name: format!("__result_{}", result_id),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(next_call.clone()),
                });

                // Extract the loop variable binding
                let item_name = if let ast::ForHead::VarDecl(var_decl) = &for_of_stmt.left {
                    if let Some(decl) = var_decl.decls.first() {
                        if let ast::Pat::Ident(ident) = &decl.name {
                            ident.id.sym.to_string()
                        } else {
                            "__gen_item".to_string()
                        }
                    } else {
                        "__gen_item".to_string()
                    }
                } else {
                    "__gen_item".to_string()
                };
                let item_id = ctx.define_local(item_name.clone(), Type::Any);

                // Lower loop body
                let mut body_stmts = Vec::new();
                // const x = __result.value
                body_stmts.push(Stmt::Let {
                    id: item_id,
                    name: item_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(result_id)),
                        property: "value".to_string(),
                    }),
                });
                // Lower user body statements. lower_stmt appends to module.init,
                // so we snapshot and drain to capture the body stmts.
                // Handle both Block bodies (`for (...) { ... }`) AND single-statement
                // bodies (`for (...) console.log(v);`). Pre-fix the brace-less
                // form was silently dropped — `for (const v of gen()) doThing(v);`
                // produced no output at all.
                let init_before = module.init.len();
                if let ast::Stmt::Block(block) = &*for_of_stmt.body {
                    for s in &block.stmts {
                        lower_stmt(ctx, module, s)?;
                    }
                } else {
                    lower_stmt(ctx, module, &for_of_stmt.body)?;
                }
                let mut user_body: Vec<Stmt> = module.init.drain(init_before..).collect();
                body_stmts.append(&mut user_body);
                // __result = __iter.next()
                body_stmts.push(Stmt::Expr(Expr::LocalSet(result_id, Box::new(next_call))));

                // while (!__result.done) { body }
                module.init.push(Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: body_stmts,
                });

                ctx.pop_block_scope(for_scope_mark);
                return Ok(());
            }

            // --- Standard array-based for-of path ---
            // Desugar for-of to a regular for loop:
            // for (const x of arr) { body }
            // becomes:
            // { let __arr = arr; for (let __i = 0; __i < __arr.length; __i++) { const x = __arr[__i]; body } }
            // Push a block scope so loop variables and internal temporaries don't leak.
            let for_scope_mark = ctx.push_block_scope();

            // Detect string iteration BEFORE lowering (so we can use the AST-level type info).
            // for (const ch of "hello") — each iteration yields a 1-char string via str[i].
            let is_string_iter = is_ast_string_expr(ctx, &for_of_stmt.right);

            // `for (const [k, v] of h)` where h is a Headers handle: WHATWG
            // Fetch spec says iteration of a Headers object yields `[key,
            // value]` pairs sorted by key. Without this rewrite, for-of falls
            // through to the generic array path and reads `.length` on the
            // raw handle (returns 0 → silent empty loop). Refs #576.
            let is_headers_iter = match &*for_of_stmt.right {
                ast::Expr::Ident(ident) => matches!(
                    ctx.lookup_native_instance(ident.sym.as_ref()),
                    Some((_, "Headers"))
                ),
                _ => false,
            };

            // `for (const [k, v] of params)` where `params` is a
            // URLSearchParams local. Same shape as the Headers case but
            // tracked via `lookup_local_type` (Type::Named) instead of the
            // native-instance registry. Refs #575.
            let is_urlsp_iter = match &*for_of_stmt.right {
                ast::Expr::Ident(ident) => matches!(
                    ctx.lookup_local_type(ident.sym.as_ref()),
                    Some(Type::Named(n)) if n == "URLSearchParams"
                ),
                ast::Expr::New(new_expr) => matches!(
                    new_expr.callee.as_ref(),
                    ast::Expr::Ident(c) if c.sym.as_ref() == "URLSearchParams"
                ),
                _ => false,
            };

            // Lower the iterable expression (the array)
            let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;
            let arr_expr = if is_headers_iter {
                Expr::NativeMethodCall {
                    module: "Headers".to_string(),
                    class_name: Some("Headers".to_string()),
                    object: Some(Box::new(arr_expr)),
                    method: "entries".to_string(),
                    args: vec![],
                }
            } else if is_urlsp_iter {
                Expr::UrlSearchParamsEntries(Box::new(arr_expr))
            } else {
                arr_expr
            };

            // Issue #302: resolve iterable type from either local var or
            // class instance field (`this.someMap`). Was limited to
            // `Ident` only. Issue #311 extends to plain object property
            // access (`obj.m` where `obj` is a local with an inferred
            // `Type::Object` shape) — without this arm `for (const x of
            // obj.m)` fell through to `None`, the loop read `.length` on
            // a raw Map handle (returns 0), and silently iterated zero
            // times.
            let iterable_type: Option<Type> = match &*for_of_stmt.right {
                ast::Expr::Ident(ident) => ctx.lookup_local_type(ident.sym.as_ref()).cloned(),
                ast::Expr::Member(m) => {
                    if matches!(m.obj.as_ref(), ast::Expr::This(_)) {
                        if let (Some(cls), ast::MemberProp::Ident(p)) =
                            (ctx.current_class.clone(), &m.prop)
                        {
                            ctx.lookup_class_field_type(&cls, p.sym.as_ref()).cloned()
                        } else {
                            None
                        }
                    } else if let ast::MemberProp::Ident(p) = &m.prop {
                        let obj_ty = crate::lower_types::infer_type_from_expr(&m.obj, ctx);
                        match obj_ty {
                            Type::Object(ot) => {
                                ot.properties.get(p.sym.as_ref()).map(|pi| pi.ty.clone())
                            }
                            // Class instance: receiver is `new Example()` or
                            // a local typed `Example`. Consult the same
                            // class_field_types registry the `this.<field>`
                            // arm uses (populated for #302).
                            Type::Named(cls) => {
                                ctx.lookup_class_field_type(&cls, p.sym.as_ref()).cloned()
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };

            // If the iterable is a Map, wrap in MapEntries to convert to array
            // This handles: for (const [k, v] of myMap) { ... } AND
            // for (const [k, v] of this.classMap) { ... } per #302.
            let mut map_key_type: Option<Type> = None;
            let mut map_val_type: Option<Type> = None;
            // Issue #542/#543: also accept Type::Union containing Map (the
            // shape produced by `Map<K, V> | undefined` parameters/returns).
            let type_contains_map =
                |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Map") };
            let is_iterable_map = match &iterable_type {
                Some(Type::Generic { base, .. }) if base == "Map" => true,
                Some(Type::Union(variants)) => variants.iter().any(type_contains_map),
                _ => false,
            };
            // Fast path: `for (const [k, v] of mapExpr)` with an exact two-element
            // identifier destructure can iterate the Map's flat entries buffer
            // directly via `MapEntryKeyAt` / `MapEntryValueAt`, skipping the N+1
            // small Array allocations that `MapEntries` would do per iteration.
            // Detected here so we can keep the iterable expression unwrapped
            // and emit a different binding/bound shape below.
            // Map fast path also fires for the single-binding shapes
            //   for (const [k] of map)        — only key
            //   for (const [, v] of map)      — only value
            // Each non-empty slot must be a plain Ident (no nested patterns).
            // Anything else falls through to the MapEntries materialization
            // path so destructuring semantics for objects / nested arrays
            // / defaults stay correct.
            let map_kv_fastpath = is_iterable_map
                && match &for_of_stmt.left {
                    ast::ForHead::VarDecl(var_decl) => match var_decl.decls.first() {
                        Some(decl) => match &decl.name {
                            ast::Pat::Array(arr_pat) => {
                                let len = arr_pat.elems.len();
                                (len == 1 || len == 2)
                                    && arr_pat.elems.iter().all(|e| {
                                        e.is_none() || matches!(e, Some(ast::Pat::Ident(_)))
                                    })
                            }
                            _ => false,
                        },
                        None => false,
                    },
                    _ => false,
                };
            // Fast path: `for (const x of setExpr)` with a single-Ident
            // binding. Reads elements directly via `SetValueAt` (→
            // `js_set_value_at`) instead of materializing the buffer with
            // `js_set_to_array`. ECS hot paths (changeset.removes, etc.)
            // iterate Sets repeatedly; this saves an Array alloc per loop.
            // Issue #542/#543: also accept Type::Union containing Set.
            let type_contains_set =
                |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Set") };
            let is_iterable_set = match &iterable_type {
                Some(Type::Generic { base, .. }) if base == "Set" => true,
                Some(Type::Union(variants)) => variants.iter().any(type_contains_set),
                _ => false,
            };
            let set_fastpath = is_iterable_set
                && match &for_of_stmt.left {
                    ast::ForHead::VarDecl(var_decl) => match var_decl.decls.first() {
                        Some(decl) => matches!(&decl.name, ast::Pat::Ident(_)),
                        None => false,
                    },
                    _ => false,
                };
            // Issue #542/#543: dispatch on `is_iterable_map` / `is_iterable_set`
            // so the Union-with-Map / Union-with-Set shapes also wrap correctly
            // (matches the same fix applied to `lower_decl.rs`'s for-of arm).
            // Extract the Map's K/V type args from whichever variant carries
            // them (direct Generic or the Union's Map arm).
            let map_type_args: Option<Vec<Type>> = if is_iterable_map {
                match &iterable_type {
                    Some(Type::Generic { base, type_args }) if base == "Map" => {
                        Some(type_args.clone())
                    }
                    Some(Type::Union(variants)) => variants.iter().find_map(|v| match v {
                        Type::Generic { base, type_args } if base == "Map" => {
                            Some(type_args.clone())
                        }
                        _ => None,
                    }),
                    _ => None,
                }
            } else {
                None
            };
            // Issue #578: typed-array iterables. Wrap in `Expr::ArrayFrom`
            // so the holder is a regular Array of materialized element values.
            // Without this, the generated `for (let i=0; i<__arr.length; ++i)
            // __item = __arr[i]` loop reads f64s straight off the typed
            // array's byte-packed storage and yields raw bit reinterpretations.
            // `js_array_clone` (the runtime backing of `ArrayFrom`) detects the
            // typed-array tag and materializes through the per-kind accessor.
            let is_iterable_typed_array = matches!(
                &iterable_type,
                Some(Type::Named(name)) if matches!(name.as_str(),
                    "Uint8Array" | "Int8Array" | "Uint8ClampedArray"
                    | "Uint16Array" | "Int16Array"
                    | "Uint32Array" | "Int32Array"
                    | "Float32Array" | "Float64Array"
                )
            );
            let arr_expr = if is_iterable_map {
                if let Some(args) = map_type_args.as_ref() {
                    if args.len() >= 2 {
                        map_key_type = Some(args[0].clone());
                        map_val_type = Some(args[1].clone());
                    }
                }
                if map_kv_fastpath {
                    arr_expr
                } else {
                    Expr::MapEntries(Box::new(arr_expr))
                }
            } else if is_iterable_set {
                if set_fastpath {
                    arr_expr
                } else {
                    Expr::SetValues(Box::new(arr_expr))
                }
            } else if is_iterable_typed_array {
                Expr::ArrayFrom(Box::new(arr_expr))
            } else {
                arr_expr
            };

            // Determine the array element type: String for strings, Tuple(K, V) for Maps, Any otherwise.
            // For an identifier iterable like `for (const word of words)` where
            // `words: string[]`, extract the element type from the local's
            // declared Array<T> so the synthesized iteration variable gets
            // the right type (was always Any, breaking `word.length` etc.).
            // #302: also draws Set + class-field Array element types
            // from the resolved `iterable_type` above instead of
            // re-doing the Ident lookup here.
            let elem_type = if is_string_iter {
                Type::String
            } else if let (Some(ref k), Some(ref v)) = (&map_key_type, &map_val_type) {
                Type::Tuple(vec![k.clone(), v.clone()])
            } else if is_iterable_typed_array {
                // Issue #578: typed-array element values are always Number.
                Type::Number
            } else {
                match &iterable_type {
                    Some(Type::Array(elem)) => (**elem).clone(),
                    Some(Type::Generic { base, type_args })
                        if base == "Array" && type_args.len() == 1 =>
                    {
                        type_args[0].clone()
                    }
                    Some(Type::Generic { base, type_args })
                        if base == "Set" && !type_args.is_empty() =>
                    {
                        type_args[0].clone()
                    }
                    _ => Type::Any,
                }
            };
            // The __arr holder's type: String for string iteration, Map for
            // the Map-fast-path so `__m.size` resolves through `is_map_expr`,
            // Array otherwise.
            let arr_type = if is_string_iter {
                Type::String
            } else if map_kv_fastpath {
                Type::Generic {
                    base: "Map".to_string(),
                    type_args: vec![
                        map_key_type.clone().unwrap_or(Type::Any),
                        map_val_type.clone().unwrap_or(Type::Any),
                    ],
                }
            } else if set_fastpath {
                Type::Generic {
                    base: "Set".to_string(),
                    type_args: vec![elem_type.clone()],
                }
            } else {
                Type::Array(Box::new(elem_type.clone()))
            };

            // Create internal variables for the array and index
            let arr_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            // Register these in the context so they can be looked up
            ctx.locals
                .push((format!("__arr_{}", arr_id), arr_id, arr_type.clone()));
            ctx.locals
                .push((format!("__idx_{}", idx_id), idx_id, Type::Number));

            // Store array reference: let __arr = arr
            module.init.push(Stmt::Let {
                id: arr_id,
                name: format!("__arr_{}", arr_id),
                ty: arr_type,
                mutable: false,
                init: Some(arr_expr),
            });

            // IMPORTANT: Define iteration variables BEFORE lowering the body
            // so the body can reference them
            let item_id = ctx.fresh_local();
            ctx.locals
                .push((format!("__item_{}", item_id), item_id, elem_type.clone()));

            // Pre-define all variables from the pattern so body can reference them
            let var_ids: Vec<(String, u32)> = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        match &decl.name {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                let id = ctx.define_local(name.clone(), elem_type.clone());
                                vec![(name, id)]
                            }
                            ast::Pat::Array(arr_pat) => {
                                let mut ids = Vec::new();
                                for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                    if let Some(elem_pat) = elem {
                                        if let ast::Pat::Ident(ident) = elem_pat {
                                            let name = ident.id.sym.to_string();
                                            // For Map destructuring [k, v], use key type for idx 0, value type for idx 1
                                            let var_type = if let Type::Tuple(ref types) = elem_type
                                            {
                                                types.get(idx).cloned().unwrap_or(Type::Any)
                                            } else {
                                                Type::Any
                                            };
                                            let id = ctx.define_local(name.clone(), var_type);
                                            ids.push((name, id));
                                        }
                                    }
                                }
                                ids
                            }
                            ast::Pat::Object(obj_pat) => {
                                let mut ids = Vec::new();
                                for prop in &obj_pat.props {
                                    match prop {
                                        ast::ObjectPatProp::Assign(assign) => {
                                            let name = assign.key.sym.to_string();
                                            let id = ctx.define_local(name.clone(), Type::Any);
                                            ids.push((name, id));
                                        }
                                        ast::ObjectPatProp::KeyValue(kv) => {
                                            if let ast::Pat::Ident(ident) = &*kv.value {
                                                let name = ident.id.sym.to_string();
                                                let id = ctx.define_local(name.clone(), Type::Any);
                                                ids.push((name, id));
                                            } else {
                                                // Nested pattern (e.g. `key: [a, b]`).
                                                // Recurse so leaves get pre-defined and
                                                // the body can reference them. Issue #554.
                                                collect_for_of_pattern_leaves(
                                                    ctx, &kv.value, &mut ids,
                                                );
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                ids
                            }
                            _ => {
                                let name = get_binding_name(&decl.name)?;
                                let id = ctx.define_local(name.clone(), Type::Any);
                                vec![(name, id)]
                            }
                        }
                    } else {
                        return Err(anyhow!("for-of requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(pat) => {
                    let name = get_pat_name(pat)?;
                    let id = ctx.define_local(name.clone(), Type::Any);
                    vec![(name, id)]
                }
                _ => return Err(anyhow!("Unsupported for-of left-hand side")),
            };

            // NOW lower the body - variables are defined so body can reference them
            let mut loop_body = lower_body_stmt(ctx, &for_of_stmt.body)?;

            // Build binding statements using the pre-defined variable IDs
            let binding_stmts = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        // `for await (const x of arr)`: spec ECMA-262 §14.7.5.10
                        // says each iteration must Await the value yielded by
                        // the iterator. For a plain-array iterable that means
                        // `await arr[i]` — unwraps a Promise element into its
                        // resolved value before binding. Without this, `for
                        // await (const x of [Promise.resolve(1), …])` would
                        // bind `x = <Promise object>` and any numeric op would
                        // see NaN. The iterator-protocol path above already
                        // wraps the `__iter.next()` call in `Expr::Await` for
                        // async generators; this brings the array-iteration
                        // path to parity.
                        let raw_item_expr = Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        };
                        let item_expr = if for_of_stmt.is_await {
                            Expr::Await(Box::new(raw_item_expr))
                        } else {
                            raw_item_expr
                        };

                        match &decl.name {
                            ast::Pat::Ident(_) => {
                                // Simple binding: for (const x of arr)
                                let (name, id) = var_ids[0].clone();
                                let init = if set_fastpath {
                                    Expr::SetValueAt {
                                        set: Box::new(Expr::LocalGet(arr_id)),
                                        idx: Box::new(Expr::LocalGet(idx_id)),
                                    }
                                } else {
                                    item_expr
                                };
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: elem_type.clone(),
                                    mutable: false,
                                    init: Some(init),
                                }]
                            }
                            ast::Pat::Array(arr_pat) => {
                                if map_kv_fastpath {
                                    // Map [k, v] / [k] / [, v] fast path: read
                                    // each requested entry slot directly from
                                    // the Map's flat buffer at the loop index.
                                    // No `__item` Array materialization. Skipped
                                    // slots ([,v] etc.) emit no binding.
                                    let key_ty = map_key_type.clone().unwrap_or(Type::Any);
                                    let val_ty = map_val_type.clone().unwrap_or(Type::Any);
                                    let mut stmts: Vec<Stmt> = Vec::new();
                                    let mut var_idx = 0;
                                    for (slot, elem) in arr_pat.elems.iter().enumerate() {
                                        let Some(ast::Pat::Ident(_)) = elem else {
                                            continue;
                                        };
                                        let (name, id) = var_ids[var_idx].clone();
                                        var_idx += 1;
                                        let (ty, init) = if slot == 0 {
                                            (
                                                key_ty.clone(),
                                                Expr::MapEntryKeyAt {
                                                    map: Box::new(Expr::LocalGet(arr_id)),
                                                    idx: Box::new(Expr::LocalGet(idx_id)),
                                                },
                                            )
                                        } else {
                                            (
                                                val_ty.clone(),
                                                Expr::MapEntryValueAt {
                                                    map: Box::new(Expr::LocalGet(arr_id)),
                                                    idx: Box::new(Expr::LocalGet(idx_id)),
                                                },
                                            )
                                        };
                                        stmts.push(Stmt::Let {
                                            id,
                                            name,
                                            ty,
                                            mutable: false,
                                            init: Some(init),
                                        });
                                    }
                                    stmts
                                } else {
                                    // Array destructuring: for (const [a, b] of arr)
                                    let mut stmts = vec![Stmt::Let {
                                        id: item_id,
                                        name: format!("__item_{}", item_id),
                                        ty: elem_type.clone(),
                                        mutable: false,
                                        init: Some(item_expr),
                                    }];

                                    // Extract each element using pre-defined IDs
                                    let mut var_idx = 0;
                                    for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                        if let Some(elem_pat) = elem {
                                            if let ast::Pat::Ident(_) = elem_pat {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                // For Map destructuring, use the Tuple element type
                                                let var_type =
                                                    if let Type::Tuple(ref types) = elem_type {
                                                        types.get(idx).cloned().unwrap_or(Type::Any)
                                                    } else {
                                                        Type::Any
                                                    };
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: var_type,
                                                    mutable: false,
                                                    init: Some(Expr::IndexGet {
                                                        object: Box::new(Expr::LocalGet(item_id)),
                                                        index: Box::new(Expr::Number(idx as f64)),
                                                    }),
                                                });
                                            }
                                        }
                                    }
                                    stmts
                                }
                            }
                            ast::Pat::Object(obj_pat) => {
                                // Object destructuring: for (const { a, b } of arr)
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }];

                                // Extract each property using pre-defined IDs
                                let mut var_idx = 0;
                                for prop in &obj_pat.props {
                                    match prop {
                                        ast::ObjectPatProp::Assign(assign) => {
                                            let prop_name = assign.key.sym.to_string();
                                            let (name, id) = var_ids[var_idx].clone();
                                            var_idx += 1;
                                            let init_value = if let Some(default_expr) =
                                                &assign.value
                                            {
                                                let prop_access = Expr::PropertyGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    property: prop_name,
                                                };
                                                let default_val = lower_expr(ctx, default_expr)?;
                                                let condition = Expr::Compare {
                                                    op: CompareOp::Ne,
                                                    left: Box::new(prop_access.clone()),
                                                    right: Box::new(Expr::Undefined),
                                                };
                                                Expr::Conditional {
                                                    condition: Box::new(condition),
                                                    then_expr: Box::new(prop_access),
                                                    else_expr: Box::new(default_val),
                                                }
                                            } else {
                                                Expr::PropertyGet {
                                                    object: Box::new(Expr::LocalGet(item_id)),
                                                    property: prop_name,
                                                }
                                            };
                                            stmts.push(Stmt::Let {
                                                id,
                                                name,
                                                ty: Type::Any,
                                                mutable: false,
                                                init: Some(init_value),
                                            });
                                        }
                                        ast::ObjectPatProp::KeyValue(kv) => {
                                            let key = match &kv.key {
                                                ast::PropName::Ident(ident) => {
                                                    ident.sym.to_string()
                                                }
                                                ast::PropName::Str(s) => {
                                                    s.value.as_str().unwrap_or("").to_string()
                                                }
                                                _ => continue,
                                            };
                                            let key_source = Expr::PropertyGet {
                                                object: Box::new(Expr::LocalGet(item_id)),
                                                property: key,
                                            };
                                            if let ast::Pat::Ident(_) = &*kv.value {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: Type::Any,
                                                    mutable: false,
                                                    init: Some(key_source),
                                                });
                                            } else {
                                                // Nested pattern (e.g. `key: [a, b]`).
                                                // Issue #554.
                                                emit_for_of_pattern_binding(
                                                    ctx,
                                                    &kv.value,
                                                    key_source,
                                                    &var_ids,
                                                    &mut var_idx,
                                                    &mut stmts,
                                                )?;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                stmts
                            }
                            _ => {
                                let (name, id) = var_ids[0].clone();
                                vec![Stmt::Let {
                                    id,
                                    name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(Expr::IndexGet {
                                        object: Box::new(Expr::LocalGet(arr_id)),
                                        index: Box::new(Expr::LocalGet(idx_id)),
                                    }),
                                }]
                            }
                        }
                    } else {
                        return Err(anyhow!("for-of requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(_) => {
                    let (name, id) = var_ids[0].clone();
                    vec![Stmt::Let {
                        id,
                        name,
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(arr_id)),
                            index: Box::new(Expr::LocalGet(idx_id)),
                        }),
                    }]
                }
                _ => return Err(anyhow!("Unsupported for-of left-hand side")),
            };

            // Prepend the binding statements to the loop body
            for (i, stmt) in binding_stmts.into_iter().enumerate() {
                loop_body.insert(i, stmt);
            }

            // Loop bound. Map/Set fast paths read `.size` (lowered by
            // codegen to `js_map_size` / `js_set_size`); regular path uses
            // `__arr.length` against the materialized iterable.
            let bound_expr = if map_kv_fastpath || set_fastpath {
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(arr_id)),
                    property: "size".to_string(),
                }
            } else {
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(arr_id)),
                    property: "length".to_string(),
                }
            };
            // Create the for loop:
            // for (let __i = 0; __i < __arr.length; __i++) { ... }
            module.init.push(Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: idx_id,
                    name: format!("__idx_{}", idx_id),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(bound_expr),
                }),
                update: Some(Expr::Update {
                    id: idx_id,
                    op: UpdateOp::Increment,
                    prefix: true,
                }),
                body: loop_body,
            });
            ctx.pop_block_scope(for_scope_mark);
        }
        ast::Stmt::ForIn(for_in_stmt) => {
            // Desugar for-in to a for-of over Object.keys(obj):
            // for (const key in obj) { body }
            // becomes:
            // { let __keys = Object.keys(obj); for (let __i = 0; __i < __keys.length; __i++) { const key = __keys[__i]; body } }
            // Push a block scope so the loop key and internal temporaries don't leak.
            let for_scope_mark = ctx.push_block_scope();

            // Get the iteration variable name
            let key_name = match &for_in_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        get_binding_name(&decl.name)?
                    } else {
                        return Err(anyhow!("for-in requires a variable declaration"));
                    }
                }
                ast::ForHead::Pat(pat) => get_pat_name(pat)?,
                _ => return Err(anyhow!("Unsupported for-in left-hand side")),
            };

            // Lower the object expression
            let obj_expr = lower_expr(ctx, &for_in_stmt.right)?;

            // Create Object.keys(obj) expression to get the array of keys
            let keys_expr = Expr::ObjectKeys(Box::new(obj_expr));

            // Create internal variables for the keys array and index
            let keys_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            let key_id = ctx.define_local(key_name.clone(), Type::String);

            // Store keys array reference: let __keys = Object.keys(obj)
            module.init.push(Stmt::Let {
                id: keys_id,
                name: format!("__keys_{}", keys_id),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                init: Some(keys_expr),
            });

            // Lower the body
            let mut loop_body = lower_body_stmt(ctx, &for_in_stmt.body)?;

            // Prepend: const key = __keys[__i]
            loop_body.insert(
                0,
                Stmt::Let {
                    id: key_id,
                    name: key_name,
                    ty: Type::String,
                    mutable: false,
                    init: Some(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(keys_id)),
                        index: Box::new(Expr::LocalGet(idx_id)),
                    }),
                },
            );

            // Create the for loop:
            // for (let __i = 0; __i < __keys.length; __i++) { ... }
            module.init.push(Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: idx_id,
                    name: format!("__idx_{}", idx_id),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(keys_id)),
                        property: "length".to_string(),
                    }),
                }),
                update: Some(Expr::Update {
                    id: idx_id,
                    op: UpdateOp::Increment,
                    prefix: true,
                }),
                body: loop_body,
            });
            ctx.pop_block_scope(for_scope_mark);
        }
        _ => {}
    }
    Ok(())
}
