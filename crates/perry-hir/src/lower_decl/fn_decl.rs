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

pub fn lower_fn_decl(ctx: &mut LoweringContext, fn_decl: &ast::FnDecl) -> Result<Function> {
    let name = fn_decl.ident.sym.to_string();
    let func_id = ctx.lookup_func(&name).unwrap_or_else(|| ctx.fresh_func());

    // Extract type parameters from generic function declaration (e.g., function foo<T, U>(...))
    let type_params = fn_decl
        .function
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type parameter scope for resolving T, U, etc. in body types
    ctx.enter_type_param_scope(&type_params);

    let scope_mark = ctx.enter_scope();

    // Pre-scan body for `arguments` references. If the function references
    // `arguments`, we synthesize a trailing rest parameter named "arguments"
    // so callers automatically bundle their args into an array — and
    // `Expr::Ident("arguments")` resolves to a LocalGet at lowering time.
    // Skipped if the user already declared a parameter named `arguments` or
    // already has a rest param (which would conflict with the synthetic one).
    let user_has_arguments_param = fn_decl
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let user_has_rest = fn_decl
        .function
        .params
        .iter()
        .any(|p| is_rest_param(&p.pat));
    let needs_arguments_synth = !user_has_arguments_param
        && !user_has_rest
        && fn_decl
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);

    // Lower parameters with type extraction (using context for type param resolution)
    //
    // Mirrors the `expr_function.rs` site: TypeScript's `this: T` is a
    // TYPE-only marker (SWC emits it as a regular `Param { pat: Ident("this") }`),
    // so skip it up front. Without this skip, `function greet(this: ..., prefix)`
    // is lowered as a 2-arg function and `.call(obj, 'Hi')` binds `this=obj,
    // prefix=undefined` — which breaks `Function.prototype.{call,apply}` on
    // FnDecls that use TS `this:` annotations.
    let mut params = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in fn_decl.function.params.iter() {
        let param_name = get_pat_name(&param.pat)?;
        if param_name == "this" {
            continue;
        }
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_default = get_param_default(ctx, &param.pat)?;
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        let is_rest = is_rest_param(&param.pat);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: param_default,
            decorators: lower_decorators(ctx, &param.decorators),
            is_rest,
        });
        // Track destructuring patterns (or an Assign wrapping one) for extraction stmts
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }

    // If the body references `arguments`, append a synthetic trailing
    // rest parameter named "arguments". The call site already bundles
    // trailing args into an array for any rest param, and `Expr::Ident("arguments")`
    // resolves to a LocalGet of this param.
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params);
    }

    // Register parameters with known native types as native instances
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            let native_info = match type_name.as_str() {
                "PluginApi" => Some(("perry/plugin", "PluginApi")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
                // Web Fetch API: Request / Response / Headers as function
                // params — same registration the local-init paths get
                // (destructuring.rs:1457+ for `const r = new Request(…)`).
                // Without this, hono's `fetch(request)` body reads
                // `request.url` through the generic-object-property-get
                // fallback which interprets the runtime handle as an
                // object pointer, returning undefined and TypeErroring on
                // the downstream `url.indexOf("/")` (issue #519 follow-up).
                "Request" => Some(("Request", "Request")),
                "Response" => Some(("fetch", "Response")),
                "Headers" => Some(("Headers", "Headers")),
                // Fastify types
                "FastifyInstance" => Some(("fastify", "App")),
                "FastifyRequest" => Some(("fastify", "Request")),
                "FastifyReply" => Some(("fastify", "Reply")),
                _ => None,
            };
            if let Some((module, class)) = native_info {
                ctx.register_native_instance(
                    param.name.clone(),
                    module.to_string(),
                    class.to_string(),
                );
            }
        }
    }

    // Extract return type from function's type annotation (with context).
    // Body-based inference for unannotated functions is filled in after body
    // lowering below, once parameters and body locals are visible to
    // `infer_type_from_expr`. Track whether the user wrote an explicit
    // annotation so we don't "override" an explicit `: any` with inference.
    let has_explicit_return_annotation = fn_decl.function.return_type.is_some();
    let mut return_type = fn_decl
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Check if return type is a native module type (e.g., mysql.Pool, mysql.PoolConnection)
    // For async functions, unwrap Promise<T> first
    let check_type = match &return_type {
        Type::Generic { base, type_args } if base == "Promise" => {
            type_args.first().unwrap_or(&return_type)
        }
        Type::Promise(inner) => inner.as_ref(),
        other => other,
    };
    if let Type::Named(type_name) = check_type {
        if let Some(dot_pos) = type_name.find('.') {
            let module_alias = &type_name[..dot_pos];
            let class_name = &type_name[dot_pos + 1..];
            if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                ctx.func_return_native_instances.push((
                    name.clone(),
                    module_name.to_string(),
                    class_name.to_string(),
                ));
            }
        } else {
            // Bare type name check (e.g., `Redis` instead of `ioredis.Redis`)
            let module_info = match type_name.as_str() {
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
                "Pool" => Some(("mysql2/promise", "Pool")),
                "PoolConnection" => Some(("mysql2/promise", "PoolConnection")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                _ => None,
            };
            if let Some((module, class)) = module_info {
                ctx.func_return_native_instances.push((
                    name.clone(),
                    module.to_string(),
                    class.to_string(),
                ));
            }
        }
    }

    // Generate destructuring statements for patterns in parameters BEFORE lowering body
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }

    // Lower body — `lower_fn_body_block_stmt` handles ECMAScript function-
    // declaration hoisting (issue #569): inner `function name() {...}`
    // statements are pulled to the top of the result so forward references
    // resolve, and a synthetic `Stmt::PreallocateBoxes` is emitted for any
    // sibling/forward captures that need a box pre-allocated.
    let mut body = if let Some(ref block) = fn_decl.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Prepend destructuring statements to body
    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // Prepend defaulted-parameter application (see lower_constructor for the
    // rationale). Without this, cross-module callers that pad missing args
    // with TAG_UNDEFINED read the param as `undefined` instead of its default.
    let default_stmts = build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // After body lowering, check if any return statement returns a native instance.
    // This handles patterns like: function initDb() { const d = new Database(...); return d; }
    // where the return type annotation is `any` but the actual value is a native handle.
    let ni_start = scope_mark.1;
    if ctx.native_instances.len() > ni_start {
        if let Some(ref block) = fn_decl.function.body {
            find_native_return_in_stmts(&block.stmts, ctx, &name, ni_start);
        }
    }

    // Body-based return-type inference: when the function has no explicit
    // annotation, walk its return statements and unify. Enables call-site
    // type inference for unannotated user functions and — combined with Phase 1
    // literal-shape inference — makes `function make() { return {x:0, y:0} }`
    // flow Point-shaped values to callers.
    if !has_explicit_return_annotation
        && matches!(return_type, Type::Any)
        && !fn_decl.function.is_generator
    {
        if let Some(ref block) = fn_decl.function.body {
            if let Some(inferred) = infer_body_return_type(&block.stmts, ctx) {
                return_type = if fn_decl.function.is_async {
                    Type::Promise(Box::new(inferred))
                } else {
                    inferred
                };
            }
        }
    }

    ctx.exit_scope(scope_mark);

    // Exit type parameter scope
    ctx.exit_type_param_scope();

    // Track generator functions so for-of can use iterator protocol.
    // Async generators are tracked separately so for-of paths can wrap
    // `__iter.next()` in `Expr::Await` (`async function*` returns
    // `Promise<{value, done}>`).
    if fn_decl.function.is_generator {
        ctx.generator_func_names.insert(name.clone());
        if fn_decl.function.is_async {
            ctx.async_generator_func_names.insert(name.clone());
        }
    }

    Ok(Function {
        id: func_id,
        name,
        type_params,
        params,
        return_type,
        body,
        is_async: fn_decl.function.is_async,
        is_generator: fn_decl.function.is_generator,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}
