//! Reactive-animation desugaring, extracted from `widget_decl.rs` to keep
//! that file under the 2000-line cap (CI `check_file_size.sh`). Pure move,
//! no logic change. `try_desugar_reactive_animate` keeps its `pub(super)`
//! visibility (re-exported from `widget_decl.rs`) so the
//! `pub(crate) use widget_decl::*` in `lower/mod.rs` still resolves it.

use crate::types::{LocalId, Type};
use anyhow::{anyhow, Result};
use swc_ecma_ast as ast;

use super::super::*;
use crate::ir::*;

/// Walk an AST expression and collect identifiers used as `<ident>.value`
/// where `<ident>` resolves to a `perry/ui` State native instance. Callers
/// use the collected names to register `stateOnChange` subscribers.
///
/// Covers the expression shapes most commonly found in animation arguments:
/// ternaries, binary/logical ops, parens, template literals, unary,
/// assignment RHS, call args, array/object literals, and member reads. The
/// catch-all silently skips unhandled shapes — worst case, a state read
/// inside an exotic expression just won't trigger reactivity (same
/// conservative failure mode as #104's template walker).
fn collect_state_value_reads(ctx: &LoweringContext, expr: &ast::Expr, out: &mut Vec<String>) {
    match expr {
        ast::Expr::Member(member) => {
            // `<ident>.value` where ident is a registered State.
            if let ast::MemberProp::Ident(prop) = &member.prop {
                if prop.sym.as_ref() == "value" {
                    if let ast::Expr::Ident(obj) = member.obj.as_ref() {
                        let name = obj.sym.to_string();
                        if matches!(
                            ctx.lookup_native_instance(&name),
                            Some(("perry/ui", "State"))
                        ) && !out.contains(&name)
                        {
                            out.push(name);
                            return;
                        }
                    }
                }
            }
            collect_state_value_reads(ctx, member.obj.as_ref(), out);
        }
        ast::Expr::Paren(p) => collect_state_value_reads(ctx, &p.expr, out),
        ast::Expr::Cond(c) => {
            collect_state_value_reads(ctx, &c.test, out);
            collect_state_value_reads(ctx, &c.cons, out);
            collect_state_value_reads(ctx, &c.alt, out);
        }
        ast::Expr::Bin(b) => {
            collect_state_value_reads(ctx, &b.left, out);
            collect_state_value_reads(ctx, &b.right, out);
        }
        ast::Expr::Unary(u) => collect_state_value_reads(ctx, &u.arg, out),
        ast::Expr::Tpl(t) => {
            for e in &t.exprs {
                collect_state_value_reads(ctx, e, out);
            }
        }
        ast::Expr::Call(c) => {
            if let ast::Callee::Expr(ce) = &c.callee {
                collect_state_value_reads(ctx, ce, out);
            }
            for a in &c.args {
                collect_state_value_reads(ctx, &a.expr, out);
            }
        }
        ast::Expr::Array(a) => {
            for el in a.elems.iter().flatten() {
                collect_state_value_reads(ctx, &el.expr, out);
            }
        }
        ast::Expr::Seq(s) => {
            for e in &s.exprs {
                collect_state_value_reads(ctx, e, out);
            }
        }
        ast::Expr::TsNonNull(n) => collect_state_value_reads(ctx, &n.expr, out),
        ast::Expr::TsAs(a) => collect_state_value_reads(ctx, &a.expr, out),
        ast::Expr::TsTypeAssertion(a) => collect_state_value_reads(ctx, &a.expr, out),
        _ => {}
    }
}

/// Desugar `widget.animateOpacity(<expr>, dur)` / `.animatePosition(...)`
/// into an IIFE that runs the initial animation and registers a
/// `stateOnChange` subscriber per `State` read in the args, so toggling the
/// state re-fires the animation.
///
/// Generated HIR shape (animateOpacity with one state dependency):
/// ```text
/// (() => {
///     const __h = <widget>;
///     widgetAnimateOpacity(__h, target, dur);       // initial
///     stateOnChange(state1, (__v) => widgetAnimateOpacity(__h, fresh_target, dur));
///     return undefined;
/// })()
/// ```
///
/// Like the reactive-Text desugar (#104), the target expression is re-lowered
/// for the subscriber body so it reads the *current* state value at fire time.
pub(crate) fn try_desugar_reactive_animate(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    let (method_name, expected_arity) = match prop.sym.as_ref() {
        "animateOpacity" => ("widgetAnimateOpacity", 2),
        "animatePosition" => ("widgetAnimatePosition", 3),
        _ => return Ok(None),
    };
    if call.args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    if call.args.len() != expected_arity {
        return Ok(None);
    }

    // Collect unique state names whose `.value` is read anywhere in the args.
    // Preserving insertion order keeps subscriber registration deterministic.
    let mut state_names: Vec<String> = Vec::new();
    for arg in &call.args {
        collect_state_value_reads(ctx, &arg.expr, &mut state_names);
    }
    if state_names.is_empty() {
        return Ok(None);
    }

    // Lower the receiver once; store in an IIFE local so the initial call and
    // every subscriber share the same widget handle without re-evaluating
    // side-effectful receiver expressions.
    let widget_expr = lower_expr(ctx, member.obj.as_ref())?;

    let outer_func_id = ctx.fresh_func();
    let outer_scope = ctx.enter_scope();
    let widget_id = ctx.define_local("__perry_anim_widget".to_string(), Type::Any);

    let mut outer_body: Vec<Stmt> = Vec::new();
    outer_body.push(Stmt::Let {
        id: widget_id,
        name: "__perry_anim_widget".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(widget_expr),
    });

    let mut initial_args: Vec<Expr> = Vec::with_capacity(expected_arity + 1);
    initial_args.push(Expr::LocalGet(widget_id));
    for a in &call.args {
        initial_args.push(lower_expr(ctx, &a.expr)?);
    }
    outer_body.push(Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        method: method_name.to_string(),
        object: None,
        args: initial_args,
        class_name: None,
    }));

    for state_name in &state_names {
        let state_local = ctx
            .lookup_local(state_name)
            .ok_or_else(|| anyhow!("reactive animate: state '{}' not in scope", state_name))?;

        let inner_func_id = ctx.fresh_func();
        let inner_scope = ctx.enter_scope();
        let v_param_id = ctx.define_local("__v".to_string(), Type::Any);
        let v_param = Param {
            id: v_param_id,
            name: "__v".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        };

        let mut fresh_args: Vec<Expr> = Vec::with_capacity(expected_arity + 1);
        fresh_args.push(Expr::LocalGet(widget_id));
        for a in &call.args {
            fresh_args.push(lower_expr(ctx, &a.expr)?);
        }
        let animate_call = Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: method_name.to_string(),
            object: None,
            args: fresh_args,
            class_name: None,
        };
        let inner_body = vec![Stmt::Expr(animate_call)];
        ctx.exit_scope(inner_scope);

        let mut inner_refs = Vec::new();
        let mut inner_visited = std::collections::HashSet::new();
        for stmt in &inner_body {
            collect_local_refs_stmt(stmt, &mut inner_refs, &mut inner_visited);
        }
        let mut inner_captures: Vec<LocalId> = inner_refs
            .into_iter()
            .filter(|id| *id != v_param_id)
            .collect();
        inner_captures.sort();
        inner_captures.dedup();
        inner_captures = ctx.filter_module_level_captures(inner_captures);

        let inner_closure = Expr::Closure {
            func_id: inner_func_id,
            params: vec![v_param],
            return_type: Type::Any,
            body: inner_body,
            captures: inner_captures,
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: ctx.current_strict,
        };

        outer_body.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            method: "stateOnChange".to_string(),
            object: None,
            args: vec![Expr::LocalGet(state_local), inner_closure],
            class_name: None,
        }));
    }

    outer_body.push(Stmt::Return(Some(Expr::Undefined)));
    ctx.exit_scope(outer_scope);

    let mut outer_refs = Vec::new();
    let mut outer_visited = std::collections::HashSet::new();
    for stmt in &outer_body {
        collect_local_refs_stmt(stmt, &mut outer_refs, &mut outer_visited);
    }
    let mut outer_captures: Vec<LocalId> = outer_refs
        .into_iter()
        .filter(|id| *id != widget_id)
        .collect();
    outer_captures.sort();
    outer_captures.dedup();
    outer_captures = ctx.filter_module_level_captures(outer_captures);

    let outer_closure = Expr::Closure {
        func_id: outer_func_id,
        params: vec![],
        return_type: Type::Any,
        body: outer_body,
        captures: outer_captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: ctx.current_strict,
    };

    Ok(Some(Expr::Call {
        callee: Box::new(outer_closure),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    }))
}
