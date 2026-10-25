//! Bridge direct eval to live native bindings using ordinary accessors.
//!
//! A sloppy function whose body holds a direct eval of source that is not a
//! compile-time constant has a dynamic scope: the evaluated code may declare
//! `var`s in it. Such a function gets a scope object (null prototype) created
//! at entry; the interpreter declares the eval's `var`s and functions there,
//! and every identifier of the function that is not one of its own bindings
//! reads and writes through it first (a `with`-environment frame), so later
//! native code sees what the eval created. Functions without such an eval
//! lower exactly as before.
use super::{lower_expr, LoweringContext};
use crate::{
    types::{LocalId, Type},
    Expr, Stmt,
};
use anyhow::Result;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Does this function body (arrow bodies included, nested non-arrow
/// functions and classes excluded) call `eval` directly? `any_source` also
/// counts constant source, which needs the scope only inside an enclosing
/// dynamic scope (the constant fold is off there).
fn has_direct_eval(stmts: &[ast::Stmt], any_source: bool) -> bool {
    struct Find {
        any_source: bool,
        found: bool,
    }
    impl Visit for Find {
        fn visit_function(&mut self, _: &ast::Function) {}
        fn visit_class(&mut self, _: &ast::Class) {}
        fn visit_call_expr(&mut self, c: &ast::CallExpr) {
            if let ast::Callee::Expr(e) = &c.callee {
                if matches!(e.as_ref(), ast::Expr::Ident(i) if i.sym.as_ref() == "eval")
                    && !c.args.is_empty()
                    && (self.any_source
                        || crate::dynamic_code::constant_source(&c.args, true).is_none())
                {
                    self.found = true;
                }
            }
            c.visit_children_with(self);
        }
    }
    let mut f = Find {
        any_source,
        found: false,
    };
    for s in stmts {
        s.visit_with(&mut f);
    }
    f.found
}

/// Enter a function body. When it has a dynamic scope, define the scope
/// object, open its frame, and return the statement that creates it (to run
/// first). Pair with [`exit_function`].
pub(crate) fn enter_function(
    ctx: &mut LoweringContext,
    stmts: &[ast::Stmt],
    strict: bool,
) -> Option<Stmt> {
    // Functions without a dynamic scope use a sentinel stack entry. Their
    // children still inherit the nearest real enclosing scope, if any.
    let enclosing = ctx
        .dynamic_eval_scopes
        .iter()
        .rev()
        .find_map(|(id, _)| (*id != LocalId::MAX).then_some(*id));
    if strict || ctx.lookup_local("eval").is_some() || !has_direct_eval(stmts, enclosing.is_some())
    {
        ctx.dynamic_eval_scopes.push((LocalId::MAX, usize::MAX));
        return None;
    }
    let mark = ctx.scope_local_marks.last().copied().unwrap_or(0);
    let proto = enclosing.map_or(Expr::Null, Expr::LocalGet);
    let id = ctx.define_local("__perry_eval_scope".to_string(), Type::Any);
    ctx.push_with_env_at(id, mark);
    ctx.dynamic_eval_scopes.push((id, mark));
    Some(Stmt::Let {
        id,
        name: "__perry_eval_scope".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::ObjectCreate(Box::new(proto), None)),
    })
}

pub(crate) fn exit_function(ctx: &mut LoweringContext) {
    if let Some((id, _)) = ctx.dynamic_eval_scopes.pop() {
        if id != LocalId::MAX {
            ctx.pop_with_env();
        }
    }
}

/// An object literal with a getter (and, for mutable bindings, a setter) per
/// name, so the interpreter reads and writes the native binding itself.
fn bindings_object(
    ctx: &mut LoweringContext,
    names: &[(String, bool)],
    explicit_scope: bool,
) -> Result<Expr> {
    let mut members = String::new();
    for (name, settable) in names {
        members.push_str(&format!("get [{name:?}]() {{ return {name}; }},"));
        if *settable {
            members.push_str(&format!(
                "set [{name:?}](__perry_eval_value) {{ {name} = __perry_eval_value; }},"
            ));
        }
    }
    let source = format!("({{{members}}});");
    let parsed = perry_parser::parse_typescript(&source, "<eval bindings>.cjs")?;
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(e)) = &parsed.body[0] else {
        unreachable!()
    };
    // The interpreter resolves the explicitly supplied eval scope (including
    // its prototype chain) before these outer native bindings. Repeating that
    // lookup inside each accessor adds captures and safepoints unnecessarily.
    // Keep ordinary `with` frames, and keep all frames when a strict eval has
    // no explicit scope object to perform that lookup.
    let saved_frames = ctx.with_env_stack.clone();
    if explicit_scope {
        ctx.with_env_stack.retain(|frame| {
            !ctx.dynamic_eval_scopes
                .iter()
                .any(|(id, _)| *id == frame.local_id)
        });
    }
    let result = lower_expr(ctx, &e.expr);
    ctx.with_env_stack = saved_frames;
    Ok(inline_bindings_builder(result?))
}

/// The synthetic accessors need closures, but constructing their container
/// does not. Keep the container rooted in the caller instead of emitting a
/// one-shot builder function and its GC metadata for every direct eval.
fn inline_bindings_builder(expr: Expr) -> Expr {
    let Expr::Call { callee, args, .. } = &expr else {
        return expr;
    };
    let Expr::Closure {
        params,
        body,
        captures_this: false,
        ..
    } = callee.as_ref()
    else {
        return expr;
    };
    if params.len() != 1 || args.len() != 1 {
        return expr;
    }
    let id = params[0].id;
    let Some((Stmt::Return(Some(Expr::LocalGet(result))), definitions)) = body.split_last() else {
        return expr;
    };
    // Only the compiler-generated static accessor definitions qualify. If
    // object-literal lowering changes shape, retain its normal semantics.
    if *result != id
        || !definitions.iter().all(|stmt| {
            matches!(stmt,
                Stmt::Expr(Expr::Call { callee, args, .. })
                    if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                        if name == "js_object_define_accessor")
                    && args.len() == 4
                    && matches!(&args[0], Expr::LocalGet(target) if *target == id)
                    && matches!(&args[1], Expr::String(_))
            )
        })
    {
        return expr;
    }
    let Expr::Call {
        callee, mut args, ..
    } = expr
    else {
        unreachable!()
    };
    let Expr::Closure { body, .. } = *callee else {
        unreachable!()
    };
    let sequence = body
        .into_iter()
        .map(|stmt| match stmt {
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) => expr,
            _ => unreachable!(),
        })
        .collect();
    Expr::ScopedTemp {
        id,
        value: Box::new(args.remove(0)),
        body: Box::new(Expr::Sequence(sequence)),
    }
}

pub(crate) fn lower(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Result<Expr> {
    // Getter/setter closures go through normal capture conversion, so eval
    // closures that outlive this call retain the same cells as native code.
    // The function's own bindings come before its eval scope object, which
    // comes before every enclosing binding (the spec's scope order).
    let scope = ctx.dynamic_eval_scopes.last().copied().filter(|(id, _)| {
        *id != LocalId::MAX && !(ctx.current_strict || ctx.current_strict_mode())
    });
    let own_from = scope.map_or(usize::MAX, |(_, mark)| mark);
    let mut seen = std::collections::BTreeSet::new();
    let mut own = Vec::new();
    let mut outer = Vec::new();
    let locals: Vec<(usize, String, bool)> = ctx
        .locals
        .iter()
        .enumerate()
        .map(|(i, (name, id, _))| (i, name.clone(), ctx.sloppy_implicit_global_ids.contains(id)))
        .collect();
    for (index, name, implicit_global) in locals.into_iter().rev() {
        // A pending class binding hidden from the member being lowered is no
        // name the source can spell (the generated bindings object would not
        // parse); the member reads that class statically.
        if !seen.insert(name.clone())
            || name.starts_with("__perry")
            || crate::lower_decl::is_hidden_pending_binding(&name)
        {
            continue;
        }
        // A sloppy implicit global is a global binding wherever it was
        // minted: it comes after the eval scope object.
        if index >= own_from && !implicit_global {
            own.push((name, true));
        } else {
            outer.push((name, true));
        }
    }
    for (name, _, _) in ctx.globals.iter().rev() {
        if seen.insert(name.clone()) && !name.starts_with("__perry") {
            outer.push((name.clone(), true));
        }
    }
    for (name, _) in ctx.functions.iter().chain(ctx.classes.iter()) {
        if seen.insert(name.clone()) {
            outer.push((name.clone(), false));
        }
    }
    let (own_bindings, outer_bindings, scope_object) = match scope {
        Some((id, _)) => (
            bindings_object(ctx, &own, true)?,
            bindings_object(ctx, &outer, true)?,
            Expr::LocalGet(id),
        ),
        None => {
            own.extend(outer);
            (
                bindings_object(ctx, &own, false)?,
                Expr::Undefined,
                Expr::Undefined,
            )
        }
    };
    let mut args = Vec::new();
    for a in &call.args {
        args.push(lower_expr(ctx, &a.expr)?);
    }
    let spread = call.args.iter().any(|a| a.spread.is_some());
    let spread_temp = spread.then(|| ctx.fresh_local());
    let spread_value = spread.then(|| {
        Expr::ArraySpread(
            call.args
                .iter()
                .zip(&args)
                .map(|(a, e)| {
                    if a.spread.is_some() {
                        crate::ArrayElement::Spread(e.clone())
                    } else {
                        crate::ArrayElement::Expr(e.clone())
                    }
                })
                .collect(),
        )
    });
    let source = if let Some(id) = spread_temp {
        args.clear();
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(id)),
            index: Box::new(Expr::Number(0.0)),
        }
    } else if args.is_empty() {
        Expr::Undefined
    } else {
        args.remove(0)
    };
    let caller_this = lower_expr(ctx, &ast::Expr::This(ast::ThisExpr { span: call.span }))?;
    let result = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_eval_in_scope".into(),
            param_types: vec![Type::Any; 6],
            return_type: Type::Any,
        }),
        args: vec![
            source,
            own_bindings,
            outer_bindings,
            scope_object,
            caller_this,
            Expr::Bool(ctx.current_strict || ctx.current_strict_mode()),
        ],
        type_args: Vec::new(),
        byte_offset: call.span.lo.0,
    };
    // Extra eval arguments are evaluated, though only the first is source.
    // ScopedTemp keeps the first argument live while the extras run.
    if let (Some(id), Some(value)) = (spread_temp, spread_value) {
        return Ok(Expr::ScopedTemp {
            id,
            value: Box::new(value),
            body: Box::new(result),
        });
    }
    if args.is_empty() {
        return Ok(result);
    }
    let id = ctx.fresh_local();
    let Expr::Call {
        args: mut eval_args,
        callee,
        type_args,
        byte_offset,
    } = result
    else {
        unreachable!()
    };
    let value = eval_args.remove(0);
    eval_args.insert(0, Expr::LocalGet(id));
    args.push(Expr::Call {
        callee,
        args: eval_args,
        type_args,
        byte_offset,
    });
    Ok(Expr::ScopedTemp {
        id,
        value: Box::new(value),
        body: Box::new(Expr::Sequence(args)),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn eval_binding_objects_do_not_emit_builder_functions() {
        let source = "function run(source) { let value = 3; eval(source); return value; }";
        let parsed = perry_parser::parse_typescript(source, "eval-builders.cjs").unwrap();
        let hir =
            super::super::lower_module(&parsed, "eval-builders", "eval-builders.cjs").unwrap();
        let dump = format!("{hir:?}");
        assert!(dump.contains("js_eval_in_scope"));
        assert!(dump.contains("js_object_define_accessor"));
        assert!(!dump.contains("name: \"__perry_obj_iife\""), "{dump}");
    }
}
