use anyhow::Result;
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{capture_function_source, native_instance_from_return_type, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

fn function_has_use_strict(func: &ast::Function) -> bool {
    let Some(block) = func.body.as_ref() else {
        return false;
    };
    for stmt in &block.stmts {
        let Some(directive) = crate::lower::string_directive_stmt_lit(stmt) else {
            break;
        };
        if crate::lower::is_raw_use_strict_directive(directive) {
            return true;
        }
    }
    false
}

pub fn lower_fn_decl(ctx: &mut LoweringContext, fn_decl: &ast::FnDecl) -> Result<Function> {
    let name = fn_decl.ident.sym.to_string();
    // A function declaration's name must be resolvable inside its own body
    // (recursion / self-reference) and to sibling statements (hoisting). The
    // pre-scan hoists top-level/shallow decls, but deeply-nested decls inside
    // closures can slip through — and then `fresh_func()` here would mint an id
    // WITHOUT registering the name, so `lookup_func(name)` stays None and a
    // self-reference `o(...)` / member read on the func name lowers to an
    // unresolved global (→ `ReferenceError: o is not defined` deep in Next.js's
    // webpack-bundled modules). Register the name now when it wasn't already.
    let func_id = match ctx.lookup_func(&name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_func();
            ctx.register_func(name.clone(), id);
            id
        }
    };

    // #4101: retain the original source text so `fn.toString()` reconstructs
    // it. Slice the module source against the function's AST span; prepend the
    // `async` keyword when the span starts at `function` (SWC's `Function.span`
    // excludes the leading `async` modifier).
    if fn_decl.function.body.is_some() {
        capture_function_source(
            ctx,
            func_id,
            &fn_decl.function.span,
            fn_decl.function.is_async,
        );
    }

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
    let saved_in_nonarrow_fn = ctx.in_nonarrow_fn;
    ctx.in_nonarrow_fn = true;

    // Pre-scan body for `arguments` references. If the function references
    // `arguments`, we synthesize a hidden raw-arguments parameter so
    // `Expr::Ident("arguments")` resolves to a LocalGet at lowering time.
    // Skipped only if the user already declared a parameter named `arguments`;
    // user rest/default params still get a real ECMAScript arguments object.
    let user_has_arguments_param = fn_decl
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let strict = fn_decl
        .function
        .body
        .as_ref()
        .map(|b| ctx.current_strict_mode() || body_has_use_strict(&b.stmts))
        .unwrap_or(false);
    ctx.enter_strict_mode(strict);
    let simple_parameters = params_are_simple_arguments_list(&fn_decl.function.params);
    let needs_arguments_synth = !user_has_arguments_param
        && (fn_decl
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false)
            || params_use_arguments(&fn_decl.function.params));

    // Lower parameters with type extraction (using context for type param resolution)
    //
    // Mirrors the `expr_function.rs` site: TypeScript's `this: T` is a
    // TYPE-only marker (SWC emits it as a regular `Param { pat: Ident("this") }`),
    // so skip it up front. Without this skip, `function greet(this: ..., prefix)`
    // is lowered as a 2-arg function and `.call(obj, 'Hi')` binds `this=obj,
    // prefix=undefined` — which breaks `Function.prototype.{call,apply}` on
    // FnDecls that use TS `this:` annotations.
    let mut params = Vec::new();
    let mut default_param_pats: Vec<ast::Pat> = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in fn_decl.function.params.iter() {
        let param_name = get_pat_name(&param.pat)?;
        if param_name == "this" {
            continue;
        }
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        ctx.shadow_native_instance_if_present(&param_name);
        ctx.shadow_native_module_if_present(&param_name);
        let is_rest = is_rest_param(&param.pat);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: None,
            decorators: lower_decorators(ctx, &param.decorators),
            is_rest,
            arguments_object: None,
        });
        default_param_pats.push(param.pat.clone());
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
    // If the body (or a parameter default) references `arguments`, append the
    // hidden raw-arguments input — BEFORE the defaults are lowered below so
    // `arguments` inside a default expression resolves to the synthetic local.
    if needs_arguments_synth {
        let mapped = !strict && simple_parameters;
        let mapped_parameter_ids = if mapped {
            mapped_argument_parameter_ids(&params)
        } else {
            Vec::new()
        };
        append_synthetic_arguments_param(
            ctx,
            &mut params,
            strict,
            simple_parameters,
            !mapped,
            mapped_parameter_ids,
        );
    }

    for (param, pat) in params.iter_mut().zip(default_param_pats.iter()) {
        param.default = get_param_default(ctx, pat)?;
    }

    // Register parameters with known native types as native instances
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            // #6003: a param typed with a USER class that shares a native
            // name (`class Headers { ... }; function f(h: Headers)`) must
            // dispatch through the user class, not the native FFI.
            if ctx.lookup_class(type_name).is_some() {
                continue;
            }
            let native_info = match type_name.as_str() {
                "PluginApi" => Some(("perry/plugin", "PluginApi")),
                "WebSocket" | "WebSocketServer" => Some(("ws", type_name.as_str())),
                "Redis" => Some(("ioredis", "Redis")),
                "EventEmitter" => Some(("events", "EventEmitter")),
                "EventEmitterAsyncResource" => Some(("events", "EventEmitterAsyncResource")),
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

    // Cross-function native-handle propagation: a parameter that receives a
    // known native handle across a call boundary (e.g. the `("ws","Client")`
    // upgrade `wsId` passed as `handleConnection(req, wsId)`) is untyped here,
    // so the type-annotation loop above can't tag it. `pre_scan_cross_fn_native_params`
    // recorded a `(fn_ident_span_lo, param_index) -> (module, class)` hint before
    // any body lowered; apply it now (before the body lowers) so `wsId.send(...)`
    // inside this function dispatches to `js_ws_send_client_i64` like the inline
    // call. The key is this declaration's identifier span (not its name), so a
    // hint seeded for a same-named but distinct function never leaks here. Only
    // registers when the param isn't already a native instance, so an explicit
    // annotation always wins.
    if !ctx.param_native_hints.is_empty() {
        let fn_span_lo = fn_decl.ident.span.lo.0;
        for (idx, param) in params.iter().enumerate() {
            if let Some((module, class)) = ctx.param_native_hints.get(&(fn_span_lo, idx)).cloned() {
                if ctx.lookup_native_instance(&param.name).is_none() {
                    ctx.register_native_instance(param.name.clone(), module, class);
                }
            }
        }
    }

    // #1483: perry/ui widget parameters (e.g. `canvas: Canvas` or, via a
    // type-only import alias, `canvas: CanvasType`) must dispatch instance
    // methods through perry/ui `NativeMethodCall` exactly like a local
    // `const canvas = Canvas(...)`. The match above only covers non-UI
    // native types; resolve the (possibly import-aliased) widget type here.
    // Resolution requires an actual perry/ui import, so a user class that
    // happens to be named `Canvas`/`Table`/`Picker` is not mis-tagged.
    for param in &params {
        if let Type::Named(type_name) = &param.ty {
            if let Some(widget) = ctx.resolve_perry_ui_widget_type(type_name) {
                ctx.register_native_instance(param.name.clone(), "perry/ui".to_string(), widget);
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
                ctx.push_func_return_native_instance((
                    name.clone(),
                    module_name.to_string(),
                    class_name.to_string(),
                ));
            }
        } else {
            // Bare type name check (e.g., `Redis` instead of `ioredis.Redis`).
            // Delegates to the shared table in `lower/misc.rs` rather than a
            // local copy: this list had drifted behind the parameter paths and
            // the local-`const`/factory paths (which already knew Socket and the
            // Fastify types), so an exported `function buildServer():
            // FastifyInstance` was never recorded as returning a native
            // instance. Keep new native return types in that one table.
            if let Some((module, class)) = native_instance_from_return_type(&return_type) {
                ctx.push_func_return_native_instance((
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
    let destructuring_prologue_len = destructuring_stmts.len();

    let outer_strict = ctx.current_strict;
    let is_strict = outer_strict || function_has_use_strict(&fn_decl.function);
    ctx.current_strict = is_strict;

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
    ctx.current_strict = outer_strict;

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
    // Record the param-prologue length for generators so the generator
    // transform can run param binding synchronously at call time (spec
    // FunctionDeclarationInstantiation order). Prologue = default guards +
    // destructuring stmts, both prepended below. Only generators need this;
    // for plain functions / async functions the prologue stays in the body.
    if fn_decl.function.is_generator {
        let prologue_len = default_stmts.len() + destructuring_prologue_len;
        if prologue_len > 0 {
            ctx.gen_param_prologue_len.insert(func_id, prologue_len);
        }
    }
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

    ctx.exit_strict_mode();
    ctx.exit_scope(scope_mark);
    ctx.in_nonarrow_fn = saved_in_nonarrow_fn;

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
        is_strict,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}
