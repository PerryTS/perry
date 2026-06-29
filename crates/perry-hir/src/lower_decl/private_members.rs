use anyhow::Result;
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

/// Pre-scan a class body and build the private-name scope for it: every
/// private field / method / accessor it declares, with its kind and
/// static-ness. Getter+setter pairs of the same name collapse to
/// `PrivKind::GetSet`. Used to push a `PrivateScope` for the duration of the
/// class body lowering so `obj.#name` accesses can brand-check on the correct
/// declaring class and reject illegal read/write operations.
pub fn build_private_scope(class: &ast::Class, class_name: &str) -> crate::lower::PrivateScope {
    use crate::lower::{PrivKind, PrivMember, PrivateScope};
    let mut members: std::collections::HashMap<String, PrivMember> =
        std::collections::HashMap::new();
    for member in &class.body {
        match member {
            ast::ClassMember::PrivateProp(prop) => {
                let name = format!("#{}", prop.key.name);
                members.insert(
                    name,
                    PrivMember {
                        kind: PrivKind::Field,
                        is_static: prop.is_static,
                    },
                );
            }
            ast::ClassMember::PrivateMethod(method) => {
                let name = format!("#{}", method.key.name);
                let kind = match method.kind {
                    ast::MethodKind::Getter => PrivKind::Get,
                    ast::MethodKind::Setter => PrivKind::Set,
                    ast::MethodKind::Method => PrivKind::Method,
                };
                // Collapse a getter+setter pair declared under the same name
                // into GetSet so a read or a write is legal for either.
                let merged = match members.get(&name).map(|m| m.kind) {
                    Some(PrivKind::Get) if kind == PrivKind::Set => PrivKind::GetSet,
                    Some(PrivKind::Set) if kind == PrivKind::Get => PrivKind::GetSet,
                    Some(existing) => existing,
                    None => kind,
                };
                members.insert(
                    name,
                    PrivMember {
                        kind: merged,
                        is_static: method.is_static,
                    },
                );
            }
            _ => {}
        }
    }
    PrivateScope {
        class_name: class_name.to_string(),
        members,
    }
}

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
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
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
        ctx.shadow_native_instance_if_present(&param_name);
        ctx.shadow_native_module_if_present(&param_name);
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

    // Capture the destructuring-prologue length before it is drained into the
    // body so generator methods can replay param binding synchronously at call
    // time (see the `gen_param_prologue_len` recording below).
    let destructuring_prologue_len = destructuring_stmts.len();
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
    let default_prologue_len = default_stmts.len();
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.extend(body);
        body = new_body;
    }

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;
    ctx.exit_type_param_scope();

    let func_id = ctx.fresh_func();
    // Record the param-prologue length for private generator methods so the
    // generator transform replays param binding synchronously at call time
    // (spec FunctionDeclarationInstantiation order). See the matching comment
    // in `lower_class_method_with_name` (test262 class/dstr private-gen-meth-*).
    if method.function.is_generator {
        let prologue_len = default_prologue_len + destructuring_prologue_len;
        if prologue_len > 0 {
            ctx.gen_param_prologue_len.insert(func_id, prologue_len);
        }
    }

    Ok(Function {
        id: func_id,
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
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
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
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

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
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;
    ctx.enter_strict_mode(true);
    ctx.define_local("this".to_string(), Type::Any);

    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        ctx.shadow_native_instance_if_present(&param_name);
        ctx.shadow_native_module_if_present(&param_name);
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
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

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

    // Lower initializer expression if present — field-initializer context for
    // the direct-eval `arguments` early error (see `lower_class_prop`).
    // Also set `in_nonarrow_fn` so a direct eval in the initializer allows
    // `new.target` (ES2025 §sec-performeval-rules-in-initializer: the eval
    // runs "inside a function" for new.target purposes).
    // NamedEvaluation: an anonymous function initializer takes the private
    // field's name including the `#` (`static #field = function(){}` →
    // `.name === "#field"`, test262 static-field-anonymous-function-name).
    let saved_field_init = ctx.in_class_field_init;
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_class_field_init = true;
    ctx.in_nonarrow_fn = true;
    let init = prop
        .value
        .as_ref()
        .map(|e| {
            if crate::lower::expr_assign::rhs_accepts_assignment_name(e) {
                let old = ctx.assignment_inferred_name.replace(name.clone());
                let result = lower_expr(ctx, e);
                ctx.assignment_inferred_name = old;
                result
            } else {
                lower_expr(ctx, e)
            }
        })
        .transpose();
    ctx.in_class_field_init = saved_field_init;
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;
    let init = init?;

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
