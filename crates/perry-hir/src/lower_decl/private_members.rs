use anyhow::{anyhow, bail, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::*;
use crate::destructuring::*;
use crate::ir::*;
use crate::lower::{
    collect_for_of_pattern_leaves, emit_for_of_pattern_binding, lower_expr, LoweringContext,
};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

pub fn lower_private_method(
    ctx: &mut LoweringContext,
    method: &ast::PrivateMethod,
) -> Result<Function> {
    let name = format!("#{}", method.key.name);

    // Extract method-level type parameters (e.g., #helper<U>(x: U): T)
    let type_params = method
        .function
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    ctx.enter_type_param_scope(&type_params);

    let scope_mark = ctx.enter_scope();
    ctx.enter_strict_mode(true);

    // Add 'this' for instance methods
    if !method.is_static {
        ctx.define_local("this".to_string(), Type::Any);
    }

    // Lower parameters with type extraction
    let mut params = Vec::new();
    // Issue #572 — private methods follow the same destructure-extraction shape
    // as public methods.
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    let mut default_param_pats: Vec<ast::Pat> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: None,
            decorators: Vec::new(),
            is_rest,
            arguments_object: None,
        });
        default_param_pats.push(param.pat.clone());
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }
    for (param, pat) in params.iter_mut().zip(default_param_pats.iter()) {
        param.default = get_param_default(ctx, pat)?;
    }

    // #677: synthesize `arguments` if the private method body references it.
    let user_has_arguments_param = method
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let needs_arguments_synth = !user_has_arguments_param
        && method
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params, true, false, true, Vec::new());
    }

    // Extract return type
    let return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Issue #572: generate destructuring stmts BEFORE body lowering so the
    // destructured names land in `ctx.locals` for identifier resolution.
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    if !destructuring_params.is_empty() {
        for (param_id, pat) in &destructuring_params {
            let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
            destructuring_stmts.extend(stmts);
        }
    }

    // Lower body — see issue #569.
    let mut body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    if !destructuring_stmts.is_empty() {
        destructuring_stmts.append(&mut body);
        body = destructuring_stmts;
    }

    // Default-parameter prologue (`if (p === undefined) p = <default>`) — this
    // was missing for private methods, so `#m(a = 1)` silently dropped the
    // default and `#m([x] = [])` left the destructuring source `undefined`
    // (→ a thrown TypeError under the iterator protocol). Prepend BEFORE the
    // destructuring prologue, matching the public-method ordering in
    // `lower_method`.
    let default_stmts = build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.extend(body);
        body = new_body;
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.exit_type_param_scope();

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params,
        params,
        return_type,
        body,
        is_async: method.function.is_async,
        is_generator: method.function.is_generator,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Lower a private getter method (e.g. `get #value(): number { ... }`).
/// Returned function has `name` set to `get_#value` so that the codegen's
/// getter-mangling convention (`__get_<name>`) stays consistent with the
/// dispatch registry.
pub fn lower_private_getter(
    ctx: &mut LoweringContext,
    method: &ast::PrivateMethod,
) -> Result<Function> {
    let name = format!("get_#{}", method.key.name);
    let scope_mark = ctx.enter_scope();
    ctx.enter_strict_mode(true);
    ctx.define_local("this".to_string(), Type::Any);

    let return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    let body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params: Vec::new(),
        params: Vec::new(),
        return_type,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Lower a private setter method (e.g. `set #value(v: number) { ... }`).
pub fn lower_private_setter(
    ctx: &mut LoweringContext,
    method: &ast::PrivateMethod,
) -> Result<Function> {
    let name = format!("set_#{}", method.key.name);
    let scope_mark = ctx.enter_scope();
    ctx.enter_strict_mode(true);
    ctx.define_local("this".to_string(), Type::Any);

    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        });
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }

    // Issue #572 — generate destructuring stmts before body lowering.
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    let mut body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    if !destructuring_stmts.is_empty() {
        destructuring_stmts.append(&mut body);
        body = destructuring_stmts;
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);

    Ok(Function {
        id: ctx.fresh_func(),
        name,
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub fn lower_private_prop(
    ctx: &mut LoweringContext,
    prop: &ast::PrivateProp,
) -> Result<ClassField> {
    // Private fields use PrivateName which has a `name` field (without the # prefix in SWC)
    // We store the name with the # prefix to distinguish private fields
    let name = format!("#{}", prop.key.name);

    // Extract type from type annotation (using context for class type param resolution).
    // Issue #305 (private-field shape): same initializer-fallback as the public class-prop
    // path so `#map = new Map<K,V>()` and friends keep their generic type past the
    // late `register_class_field_types` re-registration.
    let ty = match prop.type_ann.as_ref() {
        Some(ann) => extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)),
        None => prop
            .value
            .as_ref()
            .map(|v| infer_type_from_expr(v, ctx))
            .unwrap_or(Type::Any),
    };

    // Lower initializer expression if present
    let init = prop
        .value
        .as_ref()
        .map(|e| lower_expr(ctx, e))
        .transpose()?;

    Ok(ClassField {
        name,
        key_expr: None,
        ty,
        init,
        is_private: true,
        is_readonly: prop.readonly,
        decorators: lower_decorators(ctx, &prop.decorators),
    })
}
