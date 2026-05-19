//! Declaration lowering.
//!
//! Contains functions for lowering function declarations, class declarations,
//! enum declarations, interface declarations, type alias declarations,
//! constructors, class methods, getters, setters, and class properties.

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

/// Build `if (param === undefined) { param = default; }` stmts for every
/// param with a default value. Prepended to function/constructor bodies so
/// cross-module callers that pad missing args with `undefined` still observe
/// the intended default. Rest params are skipped (they're handled by the
/// call-site array bundling, not by scalar default substitution).
/// Recognise `WebAssembly.instantiate(...)` call shapes used as a var-decl
/// initializer. Used to populate `ctx.wasm_instance_locals` so the
/// standard `inst.exports.<method>(...)` syntactic match in
/// `lower/expr_call.rs` doesn't fire on unrelated `obj.exports.method()`
/// calls (notably CJS aggregator output). Issue #76.
/// Collect local IDs declared anywhere inside this statement tree (Let
/// statements, for-init Lets, catch-clause variables, etc.) — but do NOT
/// recurse into nested closures, since those introduce their own scope.
///
/// Used by the closure capture detector to filter out ids that are
/// shadowed by inner declarations. See the dayjs-#920 comment in the
/// closure-lowering site for context.
pub(crate) fn collect_let_decls_in_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_let_decls_in_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_let_decls_in_stmt(s, out);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                collect_let_decls_in_stmt(s, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_let_decls_in_stmt(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                collect_let_decls_in_stmt(init_stmt, out);
            }
            for s in body {
                collect_let_decls_in_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_let_decls_in_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                if let Some((id, _)) = catch_clause.param {
                    out.insert(id);
                }
                for s in &catch_clause.body {
                    collect_let_decls_in_stmt(s, out);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_let_decls_in_stmt(s, out);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_let_decls_in_stmt(s, out);
                }
            }
        }
        // Other forms don't introduce new bindings or only have expression payloads.
        _ => {}
    }
}

pub(crate) fn init_is_webassembly_instantiate(expr: &ast::Expr) -> bool {
    let call = match expr {
        ast::Expr::Call(c) => c,
        ast::Expr::Await(a) => return init_is_webassembly_instantiate(&a.arg),
        _ => return false,
    };
    let callee = match &call.callee {
        ast::Callee::Expr(e) => e.as_ref(),
        _ => return false,
    };
    let member = match callee {
        ast::Expr::Member(m) => m,
        _ => return false,
    };
    let obj = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return false,
    };
    let prop = match &member.prop {
        ast::MemberProp::Ident(i) => i,
        _ => return false,
    };
    obj.sym.as_ref() == "WebAssembly" && prop.sym.as_ref() == "instantiate"
}

pub(crate) fn build_default_param_stmts(params: &[Param]) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::new();
    for param in params {
        if param.is_rest {
            continue;
        }
        let Some(default_expr) = param.default.as_ref() else {
            continue;
        };
        out.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(param.id)),
                right: Box::new(Expr::Undefined),
            },
            then_branch: vec![Stmt::Expr(Expr::LocalSet(
                param.id,
                Box::new(default_expr.clone()),
            ))],
            else_branch: None,
        });
    }
    out
}

/// Detect the computed key `[Symbol.iterator]` in a class method / object
/// literal. Recognizes the standard `Symbol.iterator` form — doesn't try to
/// evaluate arbitrary expressions, which is enough for `*[Symbol.iterator]()`
/// as emitted by SWC for user code.
pub(crate) fn is_symbol_iterator_key(expr: &ast::Expr) -> bool {
    if let ast::Expr::Member(member) = expr {
        if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
            (member.obj.as_ref(), &member.prop)
        {
            return obj.sym.as_ref() == "Symbol" && prop.sym.as_ref() == "iterator";
        }
    }
    false
}

/// Detect the computed key `[Symbol.<well-known>]` in a class method (static
/// method, getter, regular method). Returns the short well-known name
/// ("toPrimitive", "hasInstance", "toStringTag", "iterator", "asyncIterator",
/// "dispose", "asyncDispose") if the expression matches `Symbol.X` for a
/// supported well-known.
pub(crate) fn symbol_well_known_key(expr: &ast::Expr) -> Option<&'static str> {
    if let ast::Expr::Member(member) = expr {
        if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
            (member.obj.as_ref(), &member.prop)
        {
            if obj.sym.as_ref() != "Symbol" {
                return None;
            }
            return match prop.sym.as_ref() {
                "toPrimitive" => Some("toPrimitive"),
                "hasInstance" => Some("hasInstance"),
                "toStringTag" => Some("toStringTag"),
                "iterator" => Some("iterator"),
                "asyncIterator" => Some("asyncIterator"),
                "dispose" => Some("dispose"),
                "asyncDispose" => Some("asyncDispose"),
                _ => None,
            };
        }
    }
    None
}

/// Pre-scan a function body to detect references to the `arguments` identifier.
/// Stops descent at nested function declarations and arrow functions, since
/// those have their own `arguments` binding (or, for arrows, inherit the
/// enclosing function's). For our purposes, "uses arguments anywhere in the
/// direct body or nested arrows" is sufficient — nested regular functions
/// shadow with their own arguments object.
pub(crate) fn body_uses_arguments(body: &[ast::Stmt]) -> bool {
    for stmt in body {
        if stmt_uses_arguments(stmt) {
            return true;
        }
    }
    false
}

fn stmt_uses_arguments(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Block(b) => body_uses_arguments(&b.stmts),
        ast::Stmt::Expr(e) => expr_uses_arguments(&e.expr),
        ast::Stmt::Return(r) => r.arg.as_deref().map(expr_uses_arguments).unwrap_or(false),
        ast::Stmt::If(i) => {
            expr_uses_arguments(&i.test)
                || stmt_uses_arguments(&i.cons)
                || i.alt.as_deref().map(stmt_uses_arguments).unwrap_or(false)
        }
        ast::Stmt::While(w) => expr_uses_arguments(&w.test) || stmt_uses_arguments(&w.body),
        ast::Stmt::DoWhile(w) => expr_uses_arguments(&w.test) || stmt_uses_arguments(&w.body),
        ast::Stmt::For(f) => {
            f.test.as_deref().map(expr_uses_arguments).unwrap_or(false)
                || f.update
                    .as_deref()
                    .map(expr_uses_arguments)
                    .unwrap_or(false)
                || stmt_uses_arguments(&f.body)
        }
        ast::Stmt::ForIn(f) => expr_uses_arguments(&f.right) || stmt_uses_arguments(&f.body),
        ast::Stmt::ForOf(f) => expr_uses_arguments(&f.right) || stmt_uses_arguments(&f.body),
        ast::Stmt::Try(t) => {
            body_uses_arguments(&t.block.stmts)
                || t.handler
                    .as_ref()
                    .map(|h| body_uses_arguments(&h.body.stmts))
                    .unwrap_or(false)
                || t.finalizer
                    .as_ref()
                    .map(|f| body_uses_arguments(&f.stmts))
                    .unwrap_or(false)
        }
        ast::Stmt::Switch(s) => {
            expr_uses_arguments(&s.discriminant)
                || s.cases.iter().any(|c| body_uses_arguments(&c.cons))
        }
        ast::Stmt::Decl(ast::Decl::Var(v)) => v
            .decls
            .iter()
            .any(|d| d.init.as_deref().map(expr_uses_arguments).unwrap_or(false)),
        ast::Stmt::Throw(t) => expr_uses_arguments(&t.arg),
        ast::Stmt::Labeled(l) => stmt_uses_arguments(&l.body),
        _ => false,
    }
}

fn expr_uses_arguments(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(i) => i.sym.as_ref() == "arguments",
        ast::Expr::Call(c) => {
            let callee_uses = match &c.callee {
                ast::Callee::Expr(e) => expr_uses_arguments(e),
                _ => false,
            };
            callee_uses || c.args.iter().any(|a| expr_uses_arguments(&a.expr))
        }
        ast::Expr::Member(m) => {
            expr_uses_arguments(&m.obj)
                || matches!(&m.prop, ast::MemberProp::Computed(c) if expr_uses_arguments(&c.expr))
        }
        ast::Expr::Bin(b) => expr_uses_arguments(&b.left) || expr_uses_arguments(&b.right),
        ast::Expr::Unary(u) => expr_uses_arguments(&u.arg),
        ast::Expr::Update(u) => expr_uses_arguments(&u.arg),
        ast::Expr::Cond(c) => {
            expr_uses_arguments(&c.test)
                || expr_uses_arguments(&c.cons)
                || expr_uses_arguments(&c.alt)
        }
        ast::Expr::Assign(a) => expr_uses_arguments(&a.right),
        ast::Expr::Paren(p) => expr_uses_arguments(&p.expr),
        ast::Expr::TsAs(t) => expr_uses_arguments(&t.expr),
        ast::Expr::TsNonNull(t) => expr_uses_arguments(&t.expr),
        ast::Expr::TsTypeAssertion(t) => expr_uses_arguments(&t.expr),
        ast::Expr::Tpl(t) => t.exprs.iter().any(|e| expr_uses_arguments(e)),
        ast::Expr::Array(a) => a.elems.iter().any(|el| {
            el.as_ref()
                .map(|e| expr_uses_arguments(&e.expr))
                .unwrap_or(false)
        }),
        ast::Expr::Object(o) => o.props.iter().any(|p| match p {
            ast::PropOrSpread::Spread(s) => expr_uses_arguments(&s.expr),
            ast::PropOrSpread::Prop(p) => {
                if let ast::Prop::KeyValue(kv) = p.as_ref() {
                    expr_uses_arguments(&kv.value)
                } else {
                    false
                }
            }
        }),
        ast::Expr::New(n) => n
            .args
            .as_ref()
            .map(|args| args.iter().any(|a| expr_uses_arguments(&a.expr)))
            .unwrap_or(false),
        // Arrows inherit `arguments` from the enclosing function (per spec).
        // If an inner arrow references `arguments`, the enclosing non-arrow
        // function must synthesize the binding so the arrow's closure
        // capture mechanism can see it via the scope chain.
        ast::Expr::Arrow(a) => match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => body_uses_arguments(&b.stmts),
            ast::BlockStmtOrExpr::Expr(e) => expr_uses_arguments(e),
        },
        // Don't descend into nested function declarations or function
        // expressions — those have their own `arguments` binding that
        // shadows the enclosing scope.
        _ => false,
    }
}

/// Synthesize a trailing `...arguments` rest parameter. Call after lowering
/// the user's parameters, before lowering the body, when the body references
/// `arguments` and the user hasn't already bound it (either explicitly or via
/// their own rest param).
pub(crate) fn append_synthetic_arguments_param(ctx: &mut LoweringContext, params: &mut Vec<Param>) {
    let arguments_id = ctx.define_local("arguments".to_string(), Type::Any);
    params.push(Param {
        id: arguments_id,
        name: "arguments".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: true,
    });
}

pub(crate) fn lower_fn_decl(ctx: &mut LoweringContext, fn_decl: &ast::FnDecl) -> Result<Function> {
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

/// Validate the legacy TypeScript decorator surface Perry implements. Perry
/// currently lowers class, method, property, and parameter decorators, which
/// is enough for Nest-style DI metadata canaries. Accessor (getter/setter)
/// decorators and private decoration points still fail loudly instead of
/// being dropped — the runtime path for descriptor replacement on accessors
/// is not implemented and silently ignoring them would mask real bugs in
/// user code.
fn validate_legacy_decorator_surface(class: &ast::Class, class_name: &str) -> Result<()> {
    for member in &class.body {
        match member {
            ast::ClassMember::Method(m) => {
                // SWC models getters/setters as Method with kind != Method.
                // Their decorators would expect descriptor replacement, which
                // Perry does not implement; reject rather than drop silently.
                if matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) {
                    if let Some(dec) = m.function.decorators.first() {
                        let name = decorator_name_hint(dec);
                        let key = method_key_hint(&m.key);
                        let kind = match m.kind {
                            ast::MethodKind::Getter => "getter",
                            ast::MethodKind::Setter => "setter",
                            _ => "accessor",
                        };
                        bail!(
                            "TypeScript {kind} decorators are not supported (found `@{name}` on `{class_name}.{key}`). \
                             See docs/src/language/decorators.md — accessor descriptor replacement is not implemented.",
                        );
                    }
                }
            }
            ast::ClassMember::PrivateMethod(m) => {
                if let Some(dec) = m.function.decorators.first() {
                    let name = decorator_name_hint(dec);
                    bail!(
                        "TypeScript private method decorators are not supported yet (found `@{name}` on private method of `{class_name}`).",
                    );
                }
            }
            ast::ClassMember::ClassProp(_) => {}
            ast::ClassMember::PrivateProp(p) => {
                if let Some(dec) = p.decorators.first() {
                    let name = decorator_name_hint(dec);
                    bail!(
                        "TypeScript private property decorators are not supported yet (found `@{name}` on a private property of `{class_name}`).",
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn method_key_hint(key: &ast::PropName) -> String {
    match key {
        ast::PropName::Ident(i) => i.sym.to_string(),
        ast::PropName::Str(s) => format!("{:?}", s.value),
        ast::PropName::Num(n) => n.value.to_string(),
        _ => "<method>".to_string(),
    }
}

fn decorator_name_hint(dec: &ast::Decorator) -> String {
    match dec.expr.as_ref() {
        ast::Expr::Ident(i) => i.sym.to_string(),
        ast::Expr::Call(c) => {
            if let ast::Callee::Expr(e) = &c.callee {
                if let ast::Expr::Ident(i) = e.as_ref() {
                    return i.sym.to_string();
                }
            }
            "<decorator>".to_string()
        }
        _ => "<decorator>".to_string(),
    }
}

/// Issue #212 / #740: classes nested inside a function may have method,
/// getter, setter, constructor, or field-initializer bodies that reference
/// enclosing-fn locals. Walk every instance member (methods, getters,
/// setters, constructor) AND field initializers / computed keys, union the
/// captured outer-scope LocalIds, and then:
///   1. Add a hidden `__perry_cap_<outer_id>` instance field per captured
///      outer id. The field name is keyed off the outer id so every
///      method/ctor agrees on which field reads which capture, independent
///      of the per-method fresh ids below.
///   2. For each method/getter/setter, allocate a FRESH method-local
///      LocalId per captured outer id, rewrite the body's `LocalGet(outer_id)`
///      / `LocalSet(outer_id, _)` / nested-closure `captures: [outer_id]` to
///      use the fresh id, and prepend `Stmt::Let { id: fresh_id, init:
///      PropertyGet(This, "__perry_cap_<outer_id>") }`. Per-method fresh
///      ids are essential — the boxed-vars analysis at codegen time runs
///      module-wide on a single global LocalId space; a `Stmt::Let { id:
///      outer_id }` inside a method that has a closure mutating the
///      captured value would mark `outer_id` as boxed *globally*, which
///      then makes the outer fn's plain (non-boxed) read of `outer_id`
///      segfault on a `js_box_get` of a non-box pointer.
///   3. Extend (or synthesize) the constructor: append a param with a
///      FRESH ctor-local LocalId per captured outer id, prepend
///      `this.__perry_cap_<outer_id> = LocalGet(fresh_ctor_id)`, and
///      rewrite the user-written ctor body's `LocalGet(outer_id)` to use
///      the fresh ctor id. Also rewrite field initializers and computed
///      key expressions using the same map so `apply_field_initializers_recursive`
///      can lower them inside the ctor's scope. For derived classes, the
///      capture assignment is placed after the first `super()` call so
///      `this` is initialized first.
///   4. Register the class in `ctx.class_captures` keyed by `outer_id`;
///      `Expr::New { class_name }` looks this up and appends
///      `LocalGet(outer_id)` per captured outer id at every construction
///      site (the outer scope's actual id, since we're lowering inside it).
///
/// Static methods aren't included because they have no `this` to read
/// captures from. Mutation note: `LocalSet(outer_id, ...)` inside a method
/// writes only to the method-local fresh-id slot, not back to the outer
/// scope — divergence from JS for primitive captures with reassignment;
/// reference-type captures (`array.push`, `obj.x = ...`) work because both
/// the method-local copy and the outer binding hold the same reference.
///
/// Extracted in #740 so both `lower_class_decl` (class declarations) and
/// `lower_class_from_ast` (anonymous class expressions like `const Inner =
/// class { ... }`) share the same capture machinery — without this, an
/// anon class capturing a function param had no `__perry_cap_*` ctor param
/// synthesized and `_tag = tag` field inits read garbage at runtime.
pub(crate) fn synthesize_class_captures(
    ctx: &mut LoweringContext,
    name: &str,
    extends_name: Option<&str>,
    fields: &mut Vec<ClassField>,
    methods: &mut Vec<Function>,
    getters: &mut Vec<(String, Function)>,
    setters: &mut Vec<(String, Function)>,
    constructor: &mut Option<Function>,
) {
    let module_level_ids = ctx.module_level_ids.clone();
    let outer_scope_ids: std::collections::HashSet<LocalId> =
        ctx.locals.iter().map(|(_, id, _)| *id).collect();
    let mut union_captures: std::collections::BTreeSet<LocalId> = std::collections::BTreeSet::new();
    for m in methods.iter() {
        for id in collect_method_captures(m, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    for (_, g) in getters.iter() {
        for id in collect_method_captures(g, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    for (_, s) in setters.iter() {
        for id in collect_method_captures(s, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    if let Some(ctor) = constructor.as_ref() {
        for id in collect_method_captures(ctor, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    // Issue #740: field initializers (`readonly _tag = tag` declared on
    // a class nested inside a function) also capture outer-scope locals.
    // Without this, `LocalGet(outer_id)` inside a field's init expression
    // would read a non-existent local in the ctor's scope when
    // `apply_field_initializers_recursive` lowers the initializer.
    // Collect refs from both the init expr and the computed key_expr.
    for field in fields.iter() {
        if let Some(init) = &field.init {
            let mut refs = Vec::new();
            let mut visited = std::collections::HashSet::new();
            crate::analysis::collect_local_refs_expr(init, &mut refs, &mut visited);
            for id in refs {
                if outer_scope_ids.contains(&id) && !module_level_ids.contains(&id) {
                    union_captures.insert(id);
                }
            }
        }
        if let Some(key) = &field.key_expr {
            let mut refs = Vec::new();
            let mut visited = std::collections::HashSet::new();
            crate::analysis::collect_local_refs_expr(key, &mut refs, &mut visited);
            for id in refs {
                if outer_scope_ids.contains(&id) && !module_level_ids.contains(&id) {
                    union_captures.insert(id);
                }
            }
        }
    }
    // Inherited captures: if this class extends a parent that registered
    // captures, the parent's instance methods read from
    // `this.__perry_cap_<inherited_id>` fields the parent ctor would have
    // initialized. With our synthesized constructor on this child class,
    // the parent ctor is no longer called automatically (lower_new only
    // walks parents when the child has *no* own constructor). Union the
    // parent's captures into our captures_vec so the child's synthesized
    // ctor takes the inherited capture as a param too — and the
    // `Expr::New { class_name: <child> }` site appends `LocalGet(id)`
    // for every captured id (own + inherited). The fields themselves are
    // still deduplicated below — the child only declares the OWN-not-
    // inherited subset, so a single keys-array entry exists per capture.
    if let Some(pname) = extends_name {
        if let Some(parent_caps) = ctx.lookup_class_captures(pname) {
            for id in parent_caps {
                union_captures.insert(*id);
            }
        }
    }
    let captures_vec: Vec<LocalId> = union_captures.into_iter().collect();

    if captures_vec.is_empty() {
        return;
    }

    // Walk the parent chain to find which `__perry_cap_<id>` fields
    // are already declared by an ancestor. Inherited fields share the
    // same instance slot via the runtime's by-name lookup; declaring
    // them again here would leave two same-named entries in the keys
    // array at different offsets and the parent's method body would
    // read the parent's index while the child's ctor wrote to the
    // child's index — the inherited-class-with-shared-capture case.
    // Parent classes also synthesize a constructor that takes the
    // capture as a param, so the child's constructor needs to
    // forward inherited capture args to `super(...)` rather than
    // store them itself.
    let mut inherited_cap_field_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if let Some(pname) = extends_name {
        if let Some(parent_fields) = ctx.lookup_class_field_names(pname) {
            for f in parent_fields {
                if f.starts_with("__perry_cap_") {
                    inherited_cap_field_names.insert(f.clone());
                }
            }
        }
    }
    let inherited_cap_ids: std::collections::HashSet<LocalId> = captures_vec
        .iter()
        .copied()
        .filter(|cid| inherited_cap_field_names.contains(&format!("__perry_cap_{}", cid)))
        .collect();

    // 1. Hidden fields keyed by outer id, skipping inherited.
    for &cid in &captures_vec {
        if inherited_cap_ids.contains(&cid) {
            continue;
        }
        fields.push(ClassField {
            name: format!("__perry_cap_{}", cid),
            key_expr: None,
            ty: Type::Any,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        });
    }
    if let Some(existing) = ctx.lookup_class_field_names(name) {
        let mut updated: Vec<String> = existing.to_vec();
        for &cid in &captures_vec {
            let field_name = format!("__perry_cap_{}", cid);
            if !updated.contains(&field_name) {
                updated.push(field_name);
            }
        }
        ctx.register_class_field_names(name.to_string(), updated);
    }

    // Look up the outer-scope type for each captured id so the
    // rebind let can preserve typed-array fast paths (`out.length`,
    // `out[i]`, etc.). Without this the rebind defaults to
    // `Type::Any`, the codegen `local_types` map records the rebind
    // as Any, and `out.length` on a `string[]` capture falls off the
    // typed-array fast path into generic object-field-by-name dispatch
    // — which on an array silently returns undefined or crashes.
    let captured_outer_types: std::collections::HashMap<LocalId, Type> = captures_vec
        .iter()
        .map(|&cid| {
            let ty = ctx
                .locals
                .iter()
                .rev()
                .find(|(_, id, _)| *id == cid)
                .map(|(_, _, t)| t.clone())
                .unwrap_or(Type::Any);
            (cid, ty)
        })
        .collect();

    // Field-propagation map keyed by OUTER ids. Every `LocalSet(outer_id, v)`
    // and `Expr::Update { id: outer_id, .. }` at a top-level expression
    // position inside a method body is rewritten to also propagate the
    // new value to `this.__perry_cap_<id>`. Without this, a setter
    // writing to a captured primitive (`set value(v) { stored = v; }`)
    // would only update the method-local rebind slot, and the next
    // getter call would re-read the field's stale snapshot. The
    // propagation only fires at top-level positions (statement-level
    // expression, return value, condition); nested captured writes
    // like `(stored = v).toString()` only update the local — rare
    // enough to defer to a follow-up.
    let field_propagation: std::collections::HashMap<LocalId, String> = captures_vec
        .iter()
        .map(|&cid| (cid, format!("__perry_cap_{}", cid)))
        .collect();

    // Helper closure: build a fresh-id map for one function's body,
    // rewrite the body refs (with field-write propagation), and
    // prepend the rebinding lets.
    let rewrite_method_body = |ctx: &mut LoweringContext, body: &mut Vec<Stmt>| {
        let mut id_map: std::collections::HashMap<LocalId, LocalId> =
            std::collections::HashMap::new();
        let mut prologue: Vec<Stmt> = Vec::new();
        for &outer_id in &captures_vec {
            let new_id = ctx.fresh_local();
            id_map.insert(outer_id, new_id);
            let ty = captured_outer_types
                .get(&outer_id)
                .cloned()
                .unwrap_or(Type::Any);
            prologue.push(Stmt::Let {
                id: new_id,
                name: format!("__perry_cap_{}", outer_id),
                ty,
                mutable: true,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: format!("__perry_cap_{}", outer_id),
                }),
            });
        }
        // Rewrite first (so closure captures lists pick up the new ids
        // at the same time as the body's refs), then prepend the let.
        crate::analysis::remap_local_ids_in_stmts_with_field_propagation(
            body,
            &id_map,
            &field_propagation,
        );
        prologue.append(body);
        *body = prologue;
    };

    // 2. Methods / getters / setters.
    for m in methods.iter_mut() {
        rewrite_method_body(ctx, &mut m.body);
    }
    for (_, g) in getters.iter_mut() {
        rewrite_method_body(ctx, &mut g.body);
    }
    for (_, s) in setters.iter_mut() {
        rewrite_method_body(ctx, &mut s.body);
    }

    // 3. Constructor.
    let mut ctor = constructor.take().unwrap_or_else(|| Function {
        id: ctx.fresh_func(),
        name: format!("{}::constructor", name),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Void,
        body: Vec::new(),
        is_async: false,
        is_generator: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    });
    let mut ctor_id_map: std::collections::HashMap<LocalId, LocalId> =
        std::collections::HashMap::new();
    let mut assignment_stmts: Vec<Stmt> = Vec::with_capacity(captures_vec.len());
    for &outer_id in &captures_vec {
        let fresh_param_id = ctx.fresh_local();
        ctor_id_map.insert(outer_id, fresh_param_id);
        let ty = captured_outer_types
            .get(&outer_id)
            .cloned()
            .unwrap_or(Type::Any);
        ctor.params.push(Param {
            id: fresh_param_id,
            name: format!("__perry_cap_{}", outer_id),
            ty,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
        });
        assignment_stmts.push(Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: format!("__perry_cap_{}", outer_id),
            value: Box::new(Expr::LocalGet(fresh_param_id)),
        }));
    }
    // Rewrite user-written ctor body BEFORE inserting the assignment
    // stmts (which already reference the fresh ids directly).
    crate::analysis::remap_local_ids_in_stmts(&mut ctor.body, &ctor_id_map);
    let super_pos = ctor
        .body
        .iter()
        .position(|s| matches!(s, Stmt::Expr(Expr::SuperCall(_))));
    let insert_at = super_pos.map(|p| p + 1).unwrap_or(0);
    for (i, stmt) in assignment_stmts.into_iter().enumerate() {
        ctor.body.insert(insert_at + i, stmt);
    }
    *constructor = Some(ctor);

    // Issue #740: rewrite field initializers and computed-key
    // expressions using the same `ctor_id_map`. Field initializers
    // are lowered inside the constructor body by
    // `apply_field_initializers_recursive`, so `LocalGet(outer_id)`
    // inside a field's init must be rewritten to read the fresh
    // ctor-local param that holds the captured value (synthesized
    // above). The ctor param is bound at every `new X(...)` call
    // site by `Expr::New`'s capture-args appending logic.
    for field in fields.iter_mut() {
        if let Some(init) = field.init.as_mut() {
            crate::analysis::remap_local_ids_in_expr(init, &ctor_id_map);
        }
        if let Some(key) = field.key_expr.as_mut() {
            crate::analysis::remap_local_ids_in_expr(key, &ctor_id_map);
        }
    }

    // 4. Register so `Expr::New { class_name }` appends
    //    `LocalGet(outer_id)` per captured outer id at every
    //    construction site.
    ctx.register_class_captures(name.to_string(), captures_vec);
}

pub(crate) fn lower_class_decl(
    ctx: &mut LoweringContext,
    class_decl: &ast::ClassDecl,
    is_exported: bool,
) -> Result<Class> {
    let name = class_decl.ident.sym.to_string();
    validate_legacy_decorator_surface(&class_decl.class, &name)?;
    let class_id = match ctx.lookup_class(&name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(name.clone(), id);
            id
        }
    };

    // Set current class for arrow function `this` capture tracking
    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.clone());

    // Issue #562: track the parent class identifier so the `super({...})`
    // pre-scan in expr_call.rs can register the controller param as a
    // readable_stream instance for stream subclass constructors. Set
    // here BEFORE constructor lowering so the body lowering picks it up.
    let old_super_ident = ctx.current_class_super_ident.take();
    ctx.current_class_super_ident = match class_decl.class.super_class.as_deref() {
        Some(ast::Expr::Ident(ident)) => Some(ident.sym.to_string()),
        _ => None,
    };

    // Extract type parameters from generic class declaration (e.g., class Box<T>)
    let type_params = class_decl
        .class
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type parameter scope for resolving T, U, etc. in member types
    ctx.enter_type_param_scope(&type_params);

    // Handle extends clause
    let (extends, extends_name, native_extends, extends_expr) = if let Some(ref super_class) =
        class_decl.class.super_class
    {
        if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            // First check if it's a native module class
            let native_parent = match parent_name.as_str() {
                "EventEmitter" => Some(("events".to_string(), "EventEmitter".to_string())),
                "AsyncLocalStorage" => {
                    Some(("async_hooks".to_string(), "AsyncLocalStorage".to_string()))
                }
                "AsyncResource" => Some(("async_hooks".to_string(), "AsyncResource".to_string())),
                "WebSocketServer" => Some(("ws".to_string(), "WebSocketServer".to_string())),
                // Issue #562: user classes extending the Web Streams
                // base classes get a runtime-side subclass-init shim
                // wired through `Expr::SuperCall` (codegen). The
                // `extends_name` is also retained so the existing
                // `native_extends.is_some()` branch below still
                // populates it for the inheritance walks elsewhere
                // (vtable, hasOwn, etc.) that key on the parent name.
                "ReadableStream" => {
                    Some(("readable_stream".to_string(), "ReadableStream".to_string()))
                }
                "WritableStream" => {
                    Some(("writable_stream".to_string(), "WritableStream".to_string()))
                }
                "TransformStream" => Some((
                    "transform_stream".to_string(),
                    "TransformStream".to_string(),
                )),
                _ => None,
            };
            if native_parent.is_some() {
                // Keep `extends_name` populated alongside `native_extends`
                // so SuperCall codegen + downstream chain walks still
                // see the parent name (mirrors how stream-class
                // dispatch resolves through the existing extends_name
                // path while the native_extends carries the (module,
                // class) tag for the runtime shim).
                (None, Some(parent_name), native_parent, None)
            } else {
                let parent_cid = ctx.lookup_class(&parent_name);
                if parent_cid.is_none() {
                    // Issue #711 part 2: the Ident doesn't resolve to
                    // any known class. The common case is Effect's
                    // `const Base = (function() { function Base(){}; Base.prototype = X; return Base })()` pattern —
                    // `Base` is a function value with a prototype
                    // object attached via `js_set_function_prototype`.
                    // Capture the Ident as `extends_expr` so the
                    // dynamic parent-registration helper can resolve
                    // it through `function_class_id` at runtime.
                    // `extends_name` stays populated for the rare
                    // cases where downstream code paths key on the
                    // textual parent name (super-call codegen, etc.)
                    // but extends_expr takes precedence on the
                    // method-dispatch path.
                    match lower_expr(ctx, super_class) {
                        Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                        Err(_) => (None, Some(parent_name), None, None),
                    }
                } else {
                    // Always capture the parent name for imported classes that may not have a ClassId
                    (parent_cid, Some(parent_name), None, None)
                }
            }
        } else if let ast::Expr::Member(member) = super_class.as_ref() {
            // Handle member expression like ethers.JsonRpcProvider or module.ClassName
            let parent_name = extract_member_class_name(member);
            // Refs #488 drizzle-sqlite: also try resolving the parent
            // class by name across modules. Pre-fix the Member arm set
            // `extends = None`, so `class SQLiteIntegerBuilder extends
            // import_mid.SQLiteColumnBuilder { ... }` lost its parent
            // link entirely — inherited methods (drizzle's
            // ColumnBuilder.setName etc.) were unreachable on instances.
            // Class names are unique enough in practice that `lookup_class`
            // resolves; if it doesn't, we fall back to the prior
            // name-only behavior (no regression for unknown parents).
            (
                ctx.lookup_class(&parent_name),
                Some(parent_name),
                None,
                None,
            )
        } else {
            // Issue #711: `class X extends fn(...)` / `class X extends
            // new Foo(...)` etc. The super-class expression isn't
            // statically resolvable to a known class. Lower the
            // expression so codegen can evaluate it at the class
            // declaration site and call
            // `js_register_class_parent_dynamic` to wire the parent
            // edge into CLASS_REGISTRY at runtime. Both `extends` and
            // `extends_name` stay None — the parent class_id is only
            // known once the expression evaluates. Lowering errors
            // here are non-fatal: fall back to a parentless class so
            // the rest of the program still compiles (the
            // method-dispatch catch-all in object.rs surfaces the
            // missing-method case clearly enough).
            match lower_expr(ctx, super_class) {
                Ok(expr) => (None, None, None, Some(Box::new(expr))),
                Err(_) => (None, None, None, None),
            }
        }
    } else {
        (None, None, None, None)
    };

    // First pass: collect static field/method names for early registration
    // This allows static method bodies to reference static fields
    let mut static_field_names = Vec::new();
    let mut static_method_names = Vec::new();
    for member in &class_decl.class.body {
        match member {
            ast::ClassMember::Method(method) if method.is_static => {
                if let ast::PropName::Ident(ident) = &method.key {
                    static_method_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::PrivateMethod(method) if method.is_static => {
                // Register as "#name" so WithPrivateStatic.#helper()
                // call-site lookup via has_static_method() succeeds.
                static_method_names.push(format!("#{}", method.key.name));
            }
            ast::ClassMember::ClassProp(prop) if prop.is_static => {
                if let ast::PropName::Ident(ident) = &prop.key {
                    static_field_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                static_field_names.push(format!("#{}", prop.key.name));
            }
            _ => {}
        }
    }

    // Register static members early so method bodies can reference them
    ctx.register_class_statics(name.clone(), static_field_names, static_method_names);

    // Issue #302: also collect instance field TYPES early so method bodies'
    // `for (... of this.someField)` lowering can detect Map/Set field types
    // BEFORE method bodies are lowered. The full `fields` Vec gets populated
    // during the next pass starting at line 672 (with init exprs etc.); for
    // type-name lookup we only need (name, declared type) which is cheap to
    // pluck from `prop.type_ann`. Registered again at end-of-class
    // (line ~1058) once `fields` is complete in case any field types got
    // refined during body lowering.
    //
    // Issue #305 (re-repro on v0.5.415): fields without an explicit
    // annotation but WITH an initializer like `private map = new Map<K,V>()`
    // also need their generic type registered, otherwise `for-of this.map`
    // and `const m = this.map; for-of m` (whose type inference consults this
    // registry via lower_types::infer_type_from_expr's `this.<field>` arm)
    // both fall off the Map fast path and the loop body never executes.
    // Falls back to `infer_type_from_expr` on the AST initializer when the
    // annotation is absent — this is the same routine used elsewhere for
    // `let m = new Map<K,V>()`, so the two shapes now agree.
    let mut early_field_types: Vec<(String, Type)> = Vec::new();
    for member in &class_decl.class.body {
        if let ast::ClassMember::ClassProp(prop) = member {
            if prop.is_static {
                continue;
            }
            let field_name = match &prop.key {
                ast::PropName::Ident(i) => i.sym.to_string(),
                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                _ => continue,
            };
            let ty = match prop.type_ann.as_ref() {
                Some(ann) => extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)),
                None => prop
                    .value
                    .as_ref()
                    .map(|v| infer_type_from_expr(v, ctx))
                    .unwrap_or(Type::Any),
            };
            early_field_types.push((field_name, ty));
        }
    }
    ctx.register_class_field_types(name.clone(), early_field_types);

    let mut fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut constructor = None;
    let mut methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();

    // Second pass: actually lower the class members
    for member in &class_decl.class.body {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                constructor = Some(lower_constructor(ctx, &name, ctor)?);
            }
            ast::ClassMember::Method(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                // Get the property name for getters/setters. Computed
                // keys are accepted for `[Symbol.iterator]` (registered
                // under `@@iterator`), and for `[Symbol.hasInstance]` /
                // `[Symbol.toStringTag]` (lifted to top-level functions
                // with a `__perry_wk_<hook>_<class>` prefix so the LLVM
                // backend's `init_static_fields` picks them up and
                // registers them with the runtime).
                let prop_name = match &method.key {
                    ast::PropName::Ident(ident) => ident.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    ast::PropName::Computed(computed) => {
                        if is_symbol_iterator_key(&computed.expr) {
                            "@@iterator".to_string()
                        } else if let Some(wk) = symbol_well_known_key(&computed.expr) {
                            // hasInstance (static method): lift the method
                            // body to a top-level function named
                            // `__perry_wk_hasinstance_<class>`. Signature:
                            // `(value: f64) -> f64` — no `this`.
                            if wk == "hasInstance"
                                && method.is_static
                                && matches!(method.kind, ast::MethodKind::Method)
                            {
                                let mut func = lower_class_method(ctx, method)?;
                                func.name = format!("__perry_wk_hasinstance_{}", name);
                                ctx.pending_functions.push(func);
                                continue;
                            }
                            // toStringTag (instance getter): lift the
                            // getter body to a top-level function named
                            // `__perry_wk_tostringtag_<class>`. Signature:
                            // `(this: f64) -> f64` — getter takes `this`
                            // as an explicit first parameter and returns
                            // a string.
                            if wk == "toStringTag"
                                && !method.is_static
                                && matches!(method.kind, ast::MethodKind::Getter)
                            {
                                let getter = lower_getter_method(ctx, method)?;
                                // Inject a `this` parameter at position 0 and rewrite
                                // any `Expr::This` in the body to `LocalGet(this_id)`.
                                let this_id = ctx.fresh_local();
                                let mut new_params = Vec::with_capacity(getter.params.len() + 1);
                                new_params.push(Param {
                                    id: this_id,
                                    name: "this".to_string(),
                                    ty: Type::Named(name.clone()),
                                    default: None,
                                    decorators: Vec::new(),
                                    is_rest: false,
                                });
                                new_params.extend(getter.params.into_iter());
                                let mut body = getter.body;
                                crate::analysis::replace_this_in_stmts(&mut body, this_id);
                                let top_fn = Function {
                                    id: ctx.fresh_func(),
                                    name: format!("__perry_wk_tostringtag_{}", name),
                                    type_params: Vec::new(),
                                    params: new_params,
                                    return_type: Type::Any,
                                    body,
                                    is_async: false,
                                    is_generator: false,
                                    was_plain_async: false,
                                    was_unrolled: false,
                                    is_exported: false,
                                    captures: Vec::new(),
                                    decorators: Vec::new(),
                                };
                                ctx.pending_functions.push(top_fn);
                                continue;
                            }
                            // `[Symbol.dispose]()` / `[Symbol.asyncDispose]()`:
                            // ES2024 explicit-resource-management dispose hooks.
                            // Rename the method to a stable string-keyed name so
                            // the using-block desugarer can call it via plain
                            // method dispatch (`obj.__perry_dispose__()` /
                            // `obj.__perry_async_dispose__()`). Falls through to
                            // the regular method-pushing path below with the
                            // renamed key.
                            if (wk == "dispose" || wk == "asyncDispose")
                                && !method.is_static
                                && matches!(method.kind, ast::MethodKind::Method)
                            {
                                if wk == "asyncDispose" {
                                    "__perry_async_dispose__".to_string()
                                } else {
                                    "__perry_dispose__".to_string()
                                }
                            } else {
                                // Other well-known (toPrimitive, asyncIterator)
                                // on a class: not yet implemented, skip.
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };

                match method.kind {
                    ast::MethodKind::Getter => {
                        // Getter: no parameters, returns a value
                        let func = lower_getter_method(ctx, method)?;
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        // Setter: takes one parameter
                        let func = lower_setter_method(ctx, method)?;
                        setters.push((prop_name, func));
                    }
                    ast::MethodKind::Method => {
                        let mut func = lower_class_method(ctx, method)?;
                        // Issue #212 fixed the broader class-method-captures-
                        // outer-fn-local codegen gap, so the dispose family no
                        // longer needs a silent-drop fallback — the same
                        // hidden-field rewrite that lets `log() { captured.push(...) }`
                        // work also lets `[Symbol.dispose]() { disposed.push(...) }`
                        // work. The pre-fix gate at this site (`scope_depth > 0
                        // && method_body_captures_outer(...)` → `continue`) was
                        // removed in v0.5.319. See the v0.5.317 entry for the
                        // history and `test_issue_154_using_dispose.ts` for the
                        // regression test.
                        // `*[Symbol.iterator]()` — lift to a top-level
                        // generator function with `this` as an explicit
                        // first parameter. The generator transform
                        // (which only visits `module.functions`) then
                        // rewrites it to return the `{next, return,
                        // throw}` closure triple. For-of sites use
                        // `iterator_func_for_class` to dispatch.
                        if prop_name == "@@iterator" && func.is_generator && !method.is_static {
                            let this_id = ctx.fresh_local();
                            let mut new_params = Vec::with_capacity(func.params.len() + 1);
                            new_params.push(Param {
                                id: this_id,
                                name: "this".to_string(),
                                ty: Type::Named(name.clone()),
                                default: None,
                                decorators: Vec::new(),
                                is_rest: false,
                            });
                            new_params.append(&mut func.params);

                            let mut body = std::mem::take(&mut func.body);
                            crate::analysis::replace_this_in_stmts(&mut body, this_id);

                            let top_name = format!("__perry_iter_{}", name);
                            let top_fn_id = ctx.fresh_func();
                            let top_fn = Function {
                                id: top_fn_id,
                                name: top_name,
                                type_params: Vec::new(),
                                params: new_params,
                                return_type: Type::Any,
                                body,
                                is_async: false,
                                is_generator: true,
                                was_plain_async: false,
                                was_unrolled: false,
                                is_exported: false,
                                captures: Vec::new(),
                                decorators: Vec::new(),
                            };
                            ctx.pending_functions.push(top_fn);
                            ctx.iterator_func_for_class.insert(name.clone(), top_fn_id);
                            continue;
                        }
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                }
            }
            ast::ClassMember::ClassProp(prop) => {
                // Computed-key fields (`[Symbol.for("k")] = init`) flow through
                // here for both instance AND static positions.
                // `lower_class_prop` captures the key expression in
                // `ClassField.key_expr` for runtime evaluation. Refs #420 —
                // drizzle's `static [entityKind] = "Table"` is the canonical
                // static-computed-key pattern; codegen's `init_static_fields`
                // detects `key_expr.is_some()` and emits a runtime
                // registration into the class-static-symbol side table.
                let field = lower_class_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                let field = lower_private_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateMethod(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                match method.kind {
                    ast::MethodKind::Method => {
                        let func = lower_private_method(ctx, method)?;
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                    ast::MethodKind::Getter => {
                        // Store under "#name" so PropertyGet on "#name"
                        // can hit the getter registry (which keys on
                        // the property name, not `get_#name`).
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_getter(ctx, method)?;
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_setter(ctx, method)?;
                        setters.push((prop_name, func));
                    }
                }
            }
            ast::ClassMember::StaticBlock(block) => {
                // `static { ... }` — lower the body and attach it as
                // a synthetic static method whose name is
                // `__perry_static_init_N`. `codegen.rs :: init_static_fields`
                // later recognizes the prefix and emits a call to each
                // such method right after static field init, so they
                // run once at module startup.
                let scope_mark = ctx.enter_scope();
                let body = lower_block_stmt(ctx, &block.body)?;
                ctx.exit_scope(scope_mark);

                let block_idx = static_methods
                    .iter()
                    .filter(|m| m.name.starts_with("__perry_static_init_"))
                    .count();
                let synthetic_name = format!("__perry_static_init_{}", block_idx);
                static_methods.push(Function {
                    id: ctx.fresh_func(),
                    name: synthetic_name,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: Type::Void,
                    body,
                    is_async: false,
                    is_generator: false,
                    was_plain_async: false,
                    was_unrolled: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                });
            }
            _ => {}
        }
    }

    // Detect fields from TypeScript parameter properties (e.g., constructor(public name: string)).
    // SWC represents these as TsParamProp in the AST. They must be registered as class fields
    // so that `this.name` access in methods can find them by field index.
    {
        let declared_field_names: std::collections::HashSet<String> =
            fields.iter().map(|f| f.name.clone()).collect();
        for member in &class_decl.class.body {
            if let ast::ClassMember::Constructor(ctor) = member {
                for param in &ctor.params {
                    if let ast::ParamOrTsParamProp::TsParamProp(ts_prop) = param {
                        let (param_name, param_type) = match &ts_prop.param {
                            ast::TsParamPropParam::Ident(ident) => {
                                let pname = ident.id.sym.to_string();
                                let ty = ident
                                    .type_ann
                                    .as_ref()
                                    .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                                    .unwrap_or(Type::Any);
                                (pname, ty)
                            }
                            ast::TsParamPropParam::Assign(assign) => {
                                let pname = get_pat_name(&assign.left).unwrap_or_default();
                                let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                                (pname, ty)
                            }
                        };
                        if !param_name.is_empty() && !declared_field_names.contains(&param_name) {
                            fields.push(ClassField {
                                name: param_name,
                                key_expr: None,
                                ty: param_type,
                                init: None,
                                is_private: false,
                                is_readonly: ts_prop.readonly,
                                decorators: lower_decorators(ctx, &ts_prop.decorators),
                            });
                        }
                    }
                }
            }
        }
    }

    // Detect fields from constructor body `this.xxx = ...` assignments.
    // JavaScript classes (e.g., transpiled from TypeScript) often don't have ClassProp
    // declarations; instead they assign to `this` in the constructor body.
    //
    // IMPORTANT: Also exclude fields inherited from parent classes. If the parent already
    // declares `kind` and the subclass writes `this.kind = ...`, the subclass must NOT
    // add `kind` as a new own field. Otherwise, codegen's resolve_class_fields later
    // merges parent and own indices and the subclass's shadow `kind` gets a different
    // offset from the parent's, leaving TWO `kind` slots that disagree at runtime.
    {
        // Collect inherited field names by walking the parent chain via the extends_name.
        // Previous lower_class_decl calls have registered each class's full (own+inherited)
        // field set, so a single lookup on the direct parent yields the complete chain.
        let mut inherited_field_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if let Some(ref parent_name) = extends_name {
            if let Some(parent_fields) = ctx.lookup_class_field_names(parent_name) {
                for f in parent_fields {
                    inherited_field_names.insert(f.clone());
                }
            }
        }

        // Issue #665 (sixth pass): collect own + inherited accessor (getter+setter)
        // property names. Real-world packages like rate-limiter-flexible
        // declare a `set points(v)` accessor AND write `this.points = opts.points`
        // from the constructor body. Pre-fix the bare-this scan below
        // mis-categorised `points` as an own data field, allocating an
        // inline slot that surfaced via `Object.keys` and shadowed the
        // accessor when a subclass instance's `.points` was read across
        // modules (the runtime's setter dispatch walks the class vtable
        // chain correctly, but the spurious own-data slot wins lookup).
        let mut accessor_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for member in &class_decl.class.body {
            match member {
                ast::ClassMember::Method(m)
                    if matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) =>
                {
                    let key = match &m.key {
                        ast::PropName::Ident(i) => i.sym.to_string(),
                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                        _ => continue,
                    };
                    accessor_names.insert(key);
                }
                ast::ClassMember::PrivateMethod(m)
                    if matches!(m.kind, ast::MethodKind::Getter | ast::MethodKind::Setter) =>
                {
                    accessor_names.insert(format!("#{}", m.key.name));
                }
                _ => {}
            }
        }
        // Pull in accessor names from the parent chain. The parent's
        // registration stored the own+inherited union, so a single lookup
        // on the direct parent suffices.
        if let Some(ref parent_name) = extends_name {
            if let Some(parent_accessors) = ctx.lookup_class_accessor_names(parent_name) {
                for a in parent_accessors {
                    accessor_names.insert(a.clone());
                }
            }
        }

        let declared_field_names: std::collections::HashSet<String> =
            fields.iter().map(|f| f.name.clone()).collect();
        for member in &class_decl.class.body {
            if let ast::ClassMember::Constructor(ctor) = member {
                if let Some(ref body) = ctor.body {
                    for stmt in &body.stmts {
                        if let ast::Stmt::Expr(expr_stmt) = stmt {
                            if let ast::Expr::Assign(assign) = &*expr_stmt.expr {
                                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(
                                    mem,
                                )) = &assign.left
                                {
                                    if let ast::Expr::This(_) = &*mem.obj {
                                        if let ast::MemberProp::Ident(prop_ident) = &mem.prop {
                                            let fname = prop_ident.sym.to_string();
                                            if !declared_field_names.contains(&fname)
                                                && !inherited_field_names.contains(&fname)
                                                && !accessor_names.contains(&fname)
                                            {
                                                fields.push(ClassField {
                                                    name: fname,
                                                    key_expr: None,
                                                    ty: Type::Any,
                                                    init: None,
                                                    is_private: false,
                                                    is_readonly: false,
                                                    decorators: Vec::new(),
                                                });
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
        // Dedup fields: keep first occurrence of each name
        let mut seen = std::collections::HashSet::new();
        fields.retain(|f| seen.insert(f.name.clone()));

        // Register this class's complete field set (own + inherited) so subclasses that
        // extend it can see the full inheritance chain during their own lowering.
        let mut complete_field_names: Vec<String> = inherited_field_names.into_iter().collect();
        for f in &fields {
            if !complete_field_names.contains(&f.name) {
                complete_field_names.push(f.name.clone());
            }
        }
        ctx.register_class_field_names(name.clone(), complete_field_names);

        // Issue #665: register own+inherited accessor names so subclasses
        // lowered after this one can also skip them when scanning ctor
        // bodies. `accessor_names` already contains the union from the
        // parent-chain lookup above.
        let accessor_list: Vec<String> = accessor_names.into_iter().collect();
        ctx.register_class_accessor_names(name.clone(), accessor_list);

        // Issue #302: also register field TYPES so the for-of arm can
        // detect `for (... of this.someMap)` patterns. Only own fields are
        // registered here; inherited field types fall through to whichever
        // ancestor class registered them (sub-class lookups walk via the
        // class hierarchy elsewhere if needed).
        let field_types: Vec<(String, Type)> = fields
            .iter()
            .map(|f| (f.name.clone(), f.ty.clone()))
            .collect();
        ctx.register_class_field_types(name.clone(), field_types);
    }

    // Exit type parameter scope
    ctx.exit_type_param_scope();

    // Issue #562: stash native_extends so the `let x = new <subclass>()`
    // path in destructuring.rs can route the local through the parent
    // stream module. Done here (not at the call site) so the registry
    // lookup is always available regardless of declaration order.
    if let Some((module, class)) = native_extends.as_ref() {
        ctx.register_class_native_extends(name.clone(), module.clone(), class.clone());
    }

    // Restore previous current_class
    ctx.current_class = old_class;
    // Issue #562: restore the prior super-ident slot.
    ctx.current_class_super_ident = old_super_ident;

    // Issue #212: classes nested inside a function may have method bodies
    // that reference enclosing-fn locals. See `synthesize_class_captures`
    // for the full doc (extracted in #740 so anonymous class expressions
    // can use the same machinery).
    synthesize_class_captures(
        ctx,
        &name,
        extends_name.as_deref(),
        &mut fields,
        &mut methods,
        &mut getters,
        &mut setters,
        &mut constructor,
    );

    // Phase 4.1: register each method's and getter's return type so
    // call-site inference (`infer_call_return_type`'s Member arm) can
    // resolve `obj.method()` when obj's type is Type::Named(name).
    // Feeds off Phase 4's body-based inference — any method without an
    // explicit annotation whose body returned a known type lands here too.
    for m in &methods {
        if !matches!(m.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.clone(),
                m.name.clone(),
                m.return_type.clone(),
            );
        }
    }
    for (prop_name, g) in &getters {
        if !matches!(g.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.clone(),
                prop_name.clone(),
                g.return_type.clone(),
            );
        }
    }

    Ok(Class {
        id: class_id,
        name,
        type_params,
        extends,
        extends_name,
        native_extends,
        extends_expr,
        fields,
        constructor,
        methods,
        getters,
        setters,
        static_fields,
        static_methods,
        decorators: lower_decorators(ctx, &class_decl.class.decorators),
        is_exported,
        aliases: Vec::new(),
    })
}

/// Lower a class expression (ast::Class) to HIR.
/// Used for anonymous class expressions like `new (class extends Command { ... })()`.
pub(crate) fn lower_class_from_ast(
    ctx: &mut LoweringContext,
    class: &ast::Class,
    name: &str,
    is_exported: bool,
) -> Result<Class> {
    validate_legacy_decorator_surface(class, name)?;
    let class_id = match ctx.lookup_class(name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(name.to_string(), id);
            id
        }
    };

    let old_class = ctx.current_class.take();
    ctx.current_class = Some(name.to_string());

    // Issue #562: same as the parallel `lower_class_decl` arm — track the
    // parent class identifier so super({...}) controller-param pre-scan
    // fires for stream subclasses.
    let old_super_ident = ctx.current_class_super_ident.take();
    ctx.current_class_super_ident = match class.super_class.as_deref() {
        Some(ast::Expr::Ident(ident)) => Some(ident.sym.to_string()),
        _ => None,
    };

    let type_params = class
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    ctx.enter_type_param_scope(&type_params);

    let (extends, extends_name, native_extends, extends_expr) = if let Some(ref super_class) =
        class.super_class
    {
        if let ast::Expr::Ident(ident) = super_class.as_ref() {
            let parent_name = ident.sym.to_string();
            let native_parent = match parent_name.as_str() {
                "EventEmitter" => Some(("events".to_string(), "EventEmitter".to_string())),
                "AsyncLocalStorage" => {
                    Some(("async_hooks".to_string(), "AsyncLocalStorage".to_string()))
                }
                "AsyncResource" => Some(("async_hooks".to_string(), "AsyncResource".to_string())),
                "WebSocketServer" => Some(("ws".to_string(), "WebSocketServer".to_string())),
                // Issue #562: keep in lockstep with the parallel arm in
                // `lower_class_decl` above.
                "ReadableStream" => {
                    Some(("readable_stream".to_string(), "ReadableStream".to_string()))
                }
                "WritableStream" => {
                    Some(("writable_stream".to_string(), "WritableStream".to_string()))
                }
                "TransformStream" => Some((
                    "transform_stream".to_string(),
                    "TransformStream".to_string(),
                )),
                _ => None,
            };
            if native_parent.is_some() {
                (None, Some(parent_name), native_parent, None)
            } else {
                let parent_cid = ctx.lookup_class(&parent_name);
                if parent_cid.is_none() {
                    // Issue #711 part 2: see the parallel arm in
                    // `lower_class_decl` above. Unknown Ident super-class
                    // falls through to extends_expr capture so a
                    // function-with-prototype value can be resolved at
                    // runtime via `function_class_id`.
                    match lower_expr(ctx, super_class) {
                        Ok(expr) => (None, Some(parent_name), None, Some(Box::new(expr))),
                        Err(_) => (None, Some(parent_name), None, None),
                    }
                } else {
                    (parent_cid, Some(parent_name), None, None)
                }
            }
        } else if let ast::Expr::Member(member) = super_class.as_ref() {
            // Refs #488 drizzle-sqlite: try cross-module class lookup. See
            // the matching arm in `lower_class_decl` (above) for the full
            // rationale — without this, the parent link is lost and
            // inherited methods don't reach instances.
            let parent_name = extract_member_class_name(member);
            (
                ctx.lookup_class(&parent_name),
                Some(parent_name),
                None,
                None,
            )
        } else {
            // Issue #711: see the matching arm in `lower_class_decl` above
            // for the full rationale. Capture the lowered extends
            // expression so codegen can evaluate it at the class
            // declaration site and call
            // `js_register_class_parent_dynamic` at runtime.
            match lower_expr(ctx, super_class) {
                Ok(expr) => (None, None, None, Some(Box::new(expr))),
                Err(_) => (None, None, None, None),
            }
        }
    } else {
        (None, None, None, None)
    };

    let mut static_field_names = Vec::new();
    let mut static_method_names = Vec::new();
    for member in &class.body {
        match member {
            ast::ClassMember::Method(method) if method.is_static => {
                if let ast::PropName::Ident(ident) = &method.key {
                    static_method_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::PrivateMethod(method) if method.is_static => {
                static_method_names.push(format!("#{}", method.key.name));
            }
            ast::ClassMember::ClassProp(prop) if prop.is_static => {
                if let ast::PropName::Ident(ident) = &prop.key {
                    static_field_names.push(ident.sym.to_string());
                }
            }
            ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                static_field_names.push(format!("#{}", prop.key.name));
            }
            _ => {}
        }
    }
    ctx.register_class_statics(name.to_string(), static_field_names, static_method_names);

    let mut fields = Vec::new();
    let mut static_fields = Vec::new();
    let mut constructor = None;
    let mut methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();

    for member in &class.body {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                constructor = Some(lower_constructor(ctx, name, ctor)?);
            }
            ast::ClassMember::Method(method) => {
                // Skip TypeScript overload declarations (no body)
                if method.function.body.is_none() {
                    continue;
                }
                let prop_name = match &method.key {
                    ast::PropName::Ident(ident) => ident.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                match method.kind {
                    ast::MethodKind::Getter => {
                        let func = lower_getter_method(ctx, method)?;
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let func = lower_setter_method(ctx, method)?;
                        setters.push((prop_name, func));
                    }
                    ast::MethodKind::Method => {
                        let func = lower_class_method(ctx, method)?;
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                }
            }
            ast::ClassMember::ClassProp(prop) => {
                // Computed-key fields (`[Symbol.for("k")] = init`) flow through
                // here for both instance AND static positions.
                // `lower_class_prop` captures the key expression in
                // `ClassField.key_expr` for runtime evaluation. Refs #420 —
                // drizzle's `static [entityKind] = "Table"` is the canonical
                // static-computed-key pattern; codegen's `init_static_fields`
                // detects `key_expr.is_some()` and emits a runtime
                // registration into the class-static-symbol side table.
                let field = lower_class_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                let field = lower_private_prop(ctx, prop)?;
                if prop.is_static {
                    static_fields.push(field);
                } else {
                    fields.push(field);
                }
            }
            ast::ClassMember::PrivateMethod(method) => {
                if method.function.body.is_none() {
                    continue;
                }
                match method.kind {
                    ast::MethodKind::Method => {
                        let func = lower_private_method(ctx, method)?;
                        if method.is_static {
                            static_methods.push(func);
                        } else {
                            methods.push(func);
                        }
                    }
                    ast::MethodKind::Getter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_getter(ctx, method)?;
                        getters.push((prop_name, func));
                    }
                    ast::MethodKind::Setter => {
                        let prop_name = format!("#{}", method.key.name);
                        let func = lower_private_setter(ctx, method)?;
                        setters.push((prop_name, func));
                    }
                }
            }
            ast::ClassMember::StaticBlock(block) => {
                let scope_mark = ctx.enter_scope();
                let body = lower_block_stmt(ctx, &block.body)?;
                ctx.exit_scope(scope_mark);

                let block_idx = static_methods
                    .iter()
                    .filter(|m| m.name.starts_with("__perry_static_init_"))
                    .count();
                let synthetic_name = format!("__perry_static_init_{}", block_idx);
                static_methods.push(Function {
                    id: ctx.fresh_func(),
                    name: synthetic_name,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: Type::Void,
                    body,
                    is_async: false,
                    is_generator: false,
                    was_plain_async: false,
                    was_unrolled: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                });
            }
            _ => {}
        }
    }

    ctx.exit_type_param_scope();
    // Issue #562: see the parallel site in `lower_class_decl` — register
    // native_extends so subclass instances of the three Web Stream base
    // classes route through the parent stream module's dispatch table.
    if let Some((module, class)) = native_extends.as_ref() {
        ctx.register_class_native_extends(name.to_string(), module.clone(), class.clone());
    }
    ctx.current_class = old_class;
    // Issue #562: restore prior super-ident slot.
    ctx.current_class_super_ident = old_super_ident;

    // Phase 4.1: register method + getter return types — see the parallel
    // site in lower_class_decl.
    for m in &methods {
        if !matches!(m.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.to_string(),
                m.name.clone(),
                m.return_type.clone(),
            );
        }
    }
    for (prop_name, g) in &getters {
        if !matches!(g.return_type, Type::Any) {
            ctx.register_class_method_return_type(
                name.to_string(),
                prop_name.clone(),
                g.return_type.clone(),
            );
        }
    }

    // Issue #740: synthesize __perry_cap_* capture machinery for class
    // expressions that reference enclosing-fn locals (e.g. `const Inner =
    // class { _tag = tag }` inside `function makeFactory(tag)`). Without
    // this, anon class expressions silently dropped captures while named
    // class declarations had the machinery via `lower_class_decl`. See
    // the helper's doc comment for the full description.
    synthesize_class_captures(
        ctx,
        name,
        extends_name.as_deref(),
        &mut fields,
        &mut methods,
        &mut getters,
        &mut setters,
        &mut constructor,
    );

    Ok(Class {
        id: class_id,
        name: name.to_string(),
        type_params,
        extends,
        extends_name,
        native_extends,
        extends_expr,
        fields,
        constructor,
        methods,
        getters,
        setters,
        static_fields,
        static_methods,
        decorators: lower_decorators(ctx, &class.decorators),
        is_exported,
        aliases: Vec::new(),
    })
}

pub(crate) fn lower_enum_decl(
    ctx: &mut LoweringContext,
    enum_decl: &ast::TsEnumDecl,
    is_exported: bool,
) -> Result<Enum> {
    let name = enum_decl.id.sym.to_string();
    let enum_id = ctx.fresh_enum();

    let mut members = Vec::new();
    let mut next_value: i64 = 0;

    for member in &enum_decl.members {
        // Get member name
        let member_name = match &member.id {
            ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            ast::TsEnumMemberId::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        };

        // Get member value
        let value = if let Some(ref init) = member.init {
            match init.as_ref() {
                ast::Expr::Lit(ast::Lit::Num(n)) => {
                    let v = n.value as i64;
                    next_value = v + 1;
                    EnumValue::Number(v)
                }
                ast::Expr::Lit(ast::Lit::Str(s)) => {
                    EnumValue::String(s.value.as_str().unwrap_or("").to_string())
                }
                ast::Expr::Unary(unary) if unary.op == ast::UnaryOp::Minus => {
                    // Handle negative numbers like -1
                    if let ast::Expr::Lit(ast::Lit::Num(n)) = unary.arg.as_ref() {
                        let v = -(n.value as i64);
                        next_value = v + 1;
                        EnumValue::Number(v)
                    } else {
                        // Default to auto-increment
                        let v = next_value;
                        next_value += 1;
                        EnumValue::Number(v)
                    }
                }
                _ => {
                    // For complex expressions, default to auto-increment
                    let v = next_value;
                    next_value += 1;
                    EnumValue::Number(v)
                }
            }
        } else {
            // Auto-increment
            let v = next_value;
            next_value += 1;
            EnumValue::Number(v)
        };

        members.push(EnumMember {
            name: member_name,
            value,
        });
    }

    // Register the enum in the context for later lookups
    let member_values: Vec<(String, EnumValue)> = members
        .iter()
        .map(|m| (m.name.clone(), m.value.clone()))
        .collect();
    ctx.define_enum(name.clone(), enum_id, member_values);

    Ok(Enum {
        id: enum_id,
        name,
        members,
        is_exported,
    })
}

pub(crate) fn lower_interface_decl(
    ctx: &mut LoweringContext,
    iface_decl: &ast::TsInterfaceDecl,
    is_exported: bool,
) -> Result<Interface> {
    let name = iface_decl.id.sym.to_string();
    let iface_id = ctx.fresh_interface();

    // Extract type parameters
    let type_params = iface_decl
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type param scope for resolving type references in body
    ctx.enter_type_param_scope(&type_params);

    // Extract extended interfaces
    let extends: Vec<Type> = iface_decl
        .extends
        .iter()
        .map(|ext| {
            let base_name = match &*ext.expr {
                ast::Expr::Ident(id) => id.sym.to_string(),
                _ => "unknown".to_string(),
            };
            // Handle type arguments if present
            if let Some(ref type_args) = ext.type_args {
                let args: Vec<Type> = type_args
                    .params
                    .iter()
                    .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                    .collect();
                if args.is_empty() {
                    Type::Named(base_name)
                } else {
                    Type::Generic {
                        base: base_name,
                        type_args: args,
                    }
                }
            } else {
                Type::Named(base_name)
            }
        })
        .collect();

    // Extract properties and methods from interface body
    let mut properties = Vec::new();
    let mut methods = Vec::new();

    for member in &iface_decl.body.body {
        match member {
            ast::TsTypeElement::TsPropertySignature(prop) => {
                let prop_name = match &*prop.key {
                    ast::Expr::Ident(id) => id.sym.to_string(),
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                let prop_type = prop
                    .type_ann
                    .as_ref()
                    .map(|ta| extract_ts_type_with_ctx(&ta.type_ann, Some(ctx)))
                    .unwrap_or(Type::Any);
                properties.push(InterfaceProperty {
                    name: prop_name,
                    ty: prop_type,
                    optional: prop.optional,
                    readonly: prop.readonly,
                });
            }
            ast::TsTypeElement::TsMethodSignature(method) => {
                let method_name = match &*method.key {
                    ast::Expr::Ident(id) => id.sym.to_string(),
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };

                // Method's own type parameters
                let method_type_params = method
                    .type_params
                    .as_ref()
                    .map(|tp| extract_type_params(tp))
                    .unwrap_or_default();

                // Enter method's type param scope
                ctx.enter_type_param_scope(&method_type_params);

                // Extract parameters
                let params: Vec<(String, Type, bool)> = method
                    .params
                    .iter()
                    .map(|p| {
                        let (name, ty) = get_fn_param_name_and_type_with_ctx(p, Some(ctx));
                        let optional = matches!(p, ast::TsFnParam::Ident(id) if id.optional);
                        (name, ty, optional)
                    })
                    .collect();

                // Extract return type
                let return_type = method
                    .type_ann
                    .as_ref()
                    .map(|ta| extract_ts_type_with_ctx(&ta.type_ann, Some(ctx)))
                    .unwrap_or(Type::Void);

                ctx.exit_type_param_scope();

                methods.push(InterfaceMethod {
                    name: method_name,
                    type_params: method_type_params,
                    params,
                    return_type,
                });
            }
            _ => {} // Skip other member types for now
        }
    }

    ctx.exit_type_param_scope();

    // Register interface in context
    ctx.interfaces.push((name.clone(), iface_id));

    // Issue #179 typed-parse: record field names in source order so
    // `JSON.parse<Name[]>` codegen can emit a shape hint that matches
    // how `JSON.stringify` lays them out on the wire.
    let source_keys: Vec<String> = properties.iter().map(|p| p.name.clone()).collect();
    if !source_keys.is_empty() {
        ctx.interface_source_keys.insert(name.clone(), source_keys);
    }
    // Also materialize an ObjectType so `resolve_typed_parse_ty` can
    // expand `Named("Item")` → `Object{fields}` for codegen.
    let mut obj_props: std::collections::HashMap<String, perry_types::PropertyInfo> =
        std::collections::HashMap::new();
    for p in &properties {
        obj_props.insert(
            p.name.clone(),
            perry_types::PropertyInfo {
                ty: p.ty.clone(),
                optional: p.optional,
                readonly: p.readonly,
            },
        );
    }
    if !obj_props.is_empty() {
        ctx.interface_object_types.insert(
            name.clone(),
            perry_types::ObjectType {
                name: Some(name.clone()),
                properties: obj_props,
                index_signature: None,
            },
        );
    }

    Ok(Interface {
        id: iface_id,
        name,
        type_params,
        extends,
        properties,
        methods,
        is_exported,
    })
}

pub(crate) fn lower_type_alias_decl(
    ctx: &mut LoweringContext,
    alias_decl: &ast::TsTypeAliasDecl,
    is_exported: bool,
) -> Result<TypeAlias> {
    let name = alias_decl.id.sym.to_string();
    let alias_id = ctx.fresh_type_alias();

    // Extract type parameters
    let type_params = alias_decl
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter type param scope for resolving type references
    ctx.enter_type_param_scope(&type_params);

    // Extract the aliased type
    let ty = extract_ts_type_with_ctx(&alias_decl.type_ann, Some(ctx));

    ctx.exit_type_param_scope();

    // Register type alias in context
    ctx.type_aliases
        .push((name.clone(), alias_id, type_params.clone(), ty.clone()));

    Ok(TypeAlias {
        id: alias_id,
        name,
        type_params,
        ty,
        is_exported,
    })
}

pub(crate) fn lower_constructor(
    ctx: &mut LoweringContext,
    class_name: &str,
    ctor: &ast::Constructor,
) -> Result<Function> {
    let scope_mark = ctx.enter_scope();

    // Track that we're inside a constructor body so `new.target` can resolve
    // to a placeholder object with `.name = class_name`. Saved/restored in
    // case constructors are nested via class expressions.
    let saved_ctor_class = ctx.in_constructor_class.take();
    ctx.in_constructor_class = Some(class_name.to_string());

    // Add 'this' as a special local
    let _this_id = ctx.define_local("this".to_string(), Type::Any);

    // Lower parameters with type extraction (using context for class type param resolution)
    let mut params = Vec::new();
    // Track TsParamProp params so we can synthesize `this.field = param` assignments
    let mut param_prop_assignments: Vec<(LocalId, String)> = Vec::new();
    // Issue #572: track destructuring patterns on plain `Param` ctor args so
    // the body sees the destructured names (TsParamProp can't be a destructure).
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &ctor.params {
        match param {
            ast::ParamOrTsParamProp::Param(p) => {
                let param_name = get_pat_name(&p.pat)?;
                let param_type = extract_param_type_with_ctx(&p.pat, Some(ctx));
                let param_default = get_param_default(ctx, &p.pat)?;
                let is_rest = is_rest_param(&p.pat);
                let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_type,
                    default: param_default,
                    decorators: lower_decorators(ctx, &p.decorators),
                    is_rest,
                });
                let inner_pat = if let ast::Pat::Assign(assign) = &p.pat {
                    assign.left.as_ref()
                } else {
                    &p.pat
                };
                if is_destructuring_pattern(inner_pat) {
                    destructuring_params.push((param_id, inner_pat.clone()));
                }
            }
            ast::ParamOrTsParamProp::TsParamProp(ts_prop) => {
                // Handle parameter properties (e.g., constructor(public x: number))
                let (param_name, param_type) = match &ts_prop.param {
                    ast::TsParamPropParam::Ident(ident) => {
                        let name = ident.id.sym.to_string();
                        let ty = ident
                            .type_ann
                            .as_ref()
                            .map(|ann| extract_ts_type_with_ctx(&ann.type_ann, Some(ctx)))
                            .unwrap_or(Type::Any);
                        (name, ty)
                    }
                    ast::TsParamPropParam::Assign(assign) => {
                        let name = get_pat_name(&assign.left)?;
                        let ty = extract_param_type_with_ctx(&assign.left, Some(ctx));
                        (name, ty)
                    }
                };
                let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                // Record this param for synthesizing `this.field = param` assignment
                param_prop_assignments.push((param_id, param_name.clone()));
                params.push(Param {
                    id: param_id,
                    name: param_name,
                    ty: param_type,
                    default: None,
                    decorators: lower_decorators(ctx, &ts_prop.decorators),
                    is_rest: false, // TsParamProp cannot be a rest parameter
                });
            }
        }
    }

    // #677: synthesize `arguments` if the body references it and no user
    // param already binds it (TsParamProp can't be a rest, so the only
    // conflicts come from explicit `arguments` params or other rest params).
    let user_has_arguments_param = params.iter().any(|p| p.name == "arguments");
    let user_has_rest = params.iter().any(|p| p.is_rest);
    let needs_arguments_synth = !user_has_arguments_param
        && !user_has_rest
        && ctor
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params);
    }

    // Issue #572: generate destructuring extractions BEFORE lowering the
    // body so destructured names are in scope for identifier resolution.
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    if !destructuring_params.is_empty() {
        for (param_id, pat) in &destructuring_params {
            let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
            destructuring_stmts.extend(stmts);
        }
    }

    // Lower body — issue #569; constructor body may contain hoisted
    // inner function declarations that need PreallocateBoxes for sibling
    // captures.
    let mut body = if let Some(ref block) = ctor.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Synthesize `this.field = param` assignments for parameter properties.
    // In TypeScript, `constructor(public name: string)` automatically assigns
    // `this.name = name` at the start of the constructor body.
    if !param_prop_assignments.is_empty() {
        let mut synthetic_stmts: Vec<Stmt> = Vec::new();
        for (param_id, field_name) in &param_prop_assignments {
            synthetic_stmts.push(Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: field_name.clone(),
                value: Box::new(Expr::LocalGet(*param_id)),
            }));
        }
        // Prepend synthetic assignments before the user-written constructor body
        synthetic_stmts.append(&mut body);
        body = synthetic_stmts;
    }

    // Issue #572: prepend destructuring extractions for ctor params (the
    // stmts were generated before body lowering so locals are in scope).
    if !destructuring_stmts.is_empty() {
        destructuring_stmts.append(&mut body);
        body = destructuring_stmts;
    }

    // Prepend defaulted-parameter application: for every param with a
    // default, emit `if (param === undefined) { param = default; }` at the
    // very top of the constructor body. Needed for cross-module `new C(...)`
    // calls that pass fewer args than the constructor declares — the
    // codegen call site pads missing args with TAG_UNDEFINED, so without
    // body-side default application the param reads as `undefined`. The
    // in-module HIR `fill_default_arguments` pass already fills the args at
    // same-module call sites, so this check is a no-op there.
    let default_stmts = build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    ctx.exit_scope(scope_mark);
    ctx.in_constructor_class = saved_ctor_class;

    Ok(Function {
        id: ctx.fresh_func(),
        name: format!("{}::constructor", class_name),
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Issue #212: list outer-scope LocalIds referenced by a method/getter/
/// setter/constructor body. An id is "captured" when it's referenced inside
/// the body (or any nested closure inside the body — `collect_local_refs_*`
/// descends), but isn't one of the function's own params, isn't `this`, and
/// wasn't declared inside the body itself. Module-level ids are excluded
/// because codegen reads those directly from globals — they don't need
/// per-instance snapshotting.
///
/// `outer_scope_ids` is the snapshot of `ctx.locals` at the post-class
/// point (when this analysis runs). Refs that aren't in this set must
/// belong to inner closures' params/locals — `collect_local_refs_*`
/// descends into closure bodies indiscriminately, and without this filter
/// we'd wrongly capture inner closures' own arg ids.
fn collect_method_captures(
    func: &Function,
    outer_scope_ids: &std::collections::HashSet<LocalId>,
    module_level_ids: &std::collections::HashSet<LocalId>,
) -> Vec<LocalId> {
    let mut own_locals: std::collections::HashSet<LocalId> =
        func.params.iter().map(|p| p.id).collect();
    fn collect_let_ids(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
        for s in stmts {
            match s {
                Stmt::Let { id, .. } => {
                    out.insert(*id);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_let_ids(then_branch, out);
                    if let Some(e) = else_branch {
                        collect_let_ids(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_let_ids(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                            out.insert(*id);
                        }
                    }
                    collect_let_ids(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_let_ids(body, out);
                    if let Some(c) = catch {
                        collect_let_ids(&c.body, out);
                    }
                    if let Some(f) = finally {
                        collect_let_ids(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        collect_let_ids(&case.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    collect_let_ids(std::slice::from_ref(body.as_ref()), out)
                }
                _ => {}
            }
        }
    }
    collect_let_ids(&func.body, &mut own_locals);

    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for stmt in &func.body {
        crate::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    let mut captures: Vec<LocalId> = refs
        .into_iter()
        .filter(|id| {
            outer_scope_ids.contains(id)
                && !own_locals.contains(id)
                && !module_level_ids.contains(id)
        })
        .collect();
    captures.sort();
    captures.dedup();
    captures
}

/// Conservative outer-capture check used to gate `[Symbol.dispose]` /
/// `[Symbol.asyncDispose]` lowering: returns true when the method body
/// references any LocalId that isn't `this` or one of the method's own
/// parameters. Class-method-captures-outer-local has a pre-existing codegen
/// gap; for the dispose family we silently drop the method when this is true,
/// so test programs that previously compiled (with empty disposed output)
/// keep compiling.
#[allow(dead_code)]
fn method_body_captures_outer(func: &Function, ctx: &LoweringContext) -> bool {
    let mut own_locals: std::collections::HashSet<LocalId> =
        func.params.iter().map(|p| p.id).collect();
    // Also include `this` if it was registered (instance methods).
    if let Some(this_id) = ctx
        .locals
        .iter()
        .rev()
        .find(|(name, _, _)| name == "this")
        .map(|(_, id, _)| *id)
    {
        own_locals.insert(this_id);
    }
    // Locals defined inside the body (e.g., `let x = ...` inside the method)
    // also need to be treated as own-locals so they don't trip the capture
    // check. Walk the body collecting Let ids.
    fn collect_let_ids(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
        for s in stmts {
            match s {
                Stmt::Let { id, .. } => {
                    out.insert(*id);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_let_ids(then_branch, out);
                    if let Some(e) = else_branch {
                        collect_let_ids(e, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => collect_let_ids(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(init_stmt) = init {
                        if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                            out.insert(*id);
                        }
                    }
                    collect_let_ids(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_let_ids(body, out);
                    if let Some(c) = catch {
                        collect_let_ids(&c.body, out);
                    }
                    if let Some(f) = finally {
                        collect_let_ids(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        collect_let_ids(&case.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    collect_let_ids(std::slice::from_ref(body.as_ref()), out)
                }
                _ => {}
            }
        }
    }
    collect_let_ids(&func.body, &mut own_locals);

    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for stmt in &func.body {
        crate::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    refs.iter().any(|id| !own_locals.contains(id))
}

pub(crate) fn lower_class_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => ident.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        ast::PropName::Computed(computed) if is_symbol_iterator_key(&computed.expr) => {
            "@@iterator".to_string()
        }
        ast::PropName::Computed(computed) => {
            // Well-known symbols (hasInstance, toStringTag, toPrimitive,
            // asyncIterator) get a synthetic `@@<short>` name. The caller
            // is responsible for renaming / lifting the returned Function
            // as needed — see the well-known handling in lower_class_decl.
            // `dispose` / `asyncDispose` get stable string names so the
            // using-block desugarer can dispatch via plain method-call.
            if let Some(wk) = symbol_well_known_key(&computed.expr) {
                match wk {
                    "dispose" => "__perry_dispose__".to_string(),
                    "asyncDispose" => "__perry_async_dispose__".to_string(),
                    other => format!("@@{}", other),
                }
            } else {
                return Err(anyhow!("Unsupported method key"));
            }
        }
        _ => return Err(anyhow!("Unsupported method key")),
    };

    // Lower decorators from the method's function
    let decorators = lower_decorators(ctx, &method.function.decorators);

    // Extract method-level type parameters (e.g., method<U>(x: U): T)
    // Note: Class-level type params are already in scope from lower_class_decl
    let type_params = method
        .function
        .type_params
        .as_ref()
        .map(|tp| extract_type_params(tp))
        .unwrap_or_default();

    // Enter method's type param scope (nested inside class scope if applicable)
    ctx.enter_type_param_scope(&type_params);

    let scope_mark = ctx.enter_scope();

    // Add 'this' for instance methods
    if !method.is_static {
        ctx.define_local("this".to_string(), Type::Any);
    }

    // Lower parameters with type extraction (using context for type param resolution)
    let mut params = Vec::new();
    // Issue #572: track destructuring patterns so the body sees real bindings
    // rather than reading from a synthetic `__obj_destruct_*` local that the
    // method body can't reach by name.
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_default = get_param_default(ctx, &param.pat)?;
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: param_default,
            decorators: lower_decorators(ctx, &param.decorators),
            is_rest,
        });
        // Mirror the lower_fn_decl shape: an `Assign` pattern can wrap a
        // destructure (e.g. `({ a } = {}) => ...`). Unwrap before testing.
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }

    // #677: synthesize `arguments` if the method body references it.
    let user_has_arguments_param = method
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let user_has_rest = method.function.params.iter().any(|p| is_rest_param(&p.pat));
    let needs_arguments_synth = !user_has_arguments_param
        && !user_has_rest
        && method
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params);
    }

    // Extract return type (with context). Phase 4: when the method has no
    // explicit annotation, fall back to body-based inference after body
    // lowering so parameters and locals are visible to `infer_type_from_expr`.
    let has_explicit_return_annotation = method.function.return_type.is_some();
    let mut return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Pre-register the explicit-annotated return type BEFORE lowering the
    // body so a sibling method appearing later in the class (or this method
    // calling itself recursively) can resolve `this.method()` through
    // `infer_call_return_type`. The post-loop registration in
    // `lower_class_decl` only sees lowered methods, which is too late for
    // intra-class call-site inference inside the body we're about to lower.
    if has_explicit_return_annotation && !matches!(return_type, Type::Any) {
        if let Some(class_name) = ctx.current_class.clone() {
            ctx.register_class_method_return_type(class_name, name.clone(), return_type.clone());
        }
    }

    // Issue #572: generate destructuring extractions BEFORE lowering the
    // body, so the destructured names land in `ctx.locals` and identifier
    // references inside the body resolve to those LocalIds (matching the
    // order used by lower_fn_decl). Doing this after body lowering would
    // produce "unknown identifier" warnings + GlobalGet(0) refs.
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

    // Issue #235: prepend `if (param === undefined) param = default;` for
    // every default-value param so a caller passing fewer args (which
    // codegen now pads with TAG_UNDEFINED — see `lower_call.rs` dispatch
    // tower) gets the declared default instead of literal `undefined`.
    // Pre-fix the desugaring fired only on free functions (`lower_fn_decl`)
    // and constructors, never on instance/static class methods. The
    // standalone Calc.add(a, b = 10) regression case below printed `b` as
    // `undefined` post-padding because the method body just did `return a + b`
    // with no default check.
    let default_stmts = build_default_param_stmts(&params);
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.extend(body);
        body = new_body;
    }

    // Phase 4 (expansion): body-based return-type inference for unannotated
    // methods. Same pattern as `lower_fn_decl`: skip when annotation is
    // present or when the method is a generator; wrap inferred type in
    // Promise<T> for async methods. Feeds the class's `Function.return_type`
    // which is then consumed by call-site inference at receiver.method()
    // sites (currently limited — bare-method call-site inference isn't
    // wired through `infer_call_return_type` yet; this commit only
    // populates the field so class methods stop showing Type::Any when
    // callers inspect them via receiver_class_name + class.methods lookup).
    if !has_explicit_return_annotation
        && matches!(return_type, Type::Any)
        && !method.function.is_generator
    {
        if let Some(ref block) = method.function.body {
            if let Some(inferred) = infer_body_return_type(&block.stmts, ctx) {
                return_type = if method.function.is_async {
                    Type::Promise(Box::new(inferred))
                } else {
                    inferred
                };
            }
        }
    }

    ctx.exit_scope(scope_mark);

    // Exit method's type param scope
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
        is_exported: false,
        captures: Vec::new(),
        decorators,
        was_plain_async: false,
        was_unrolled: false,
    })
}

/// Lower a getter method (get propertyName(): Type { ... })
pub(crate) fn lower_getter_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => format!("get_{}", ident.sym),
        ast::PropName::Str(s) => format!("get_{}", s.value.as_str().unwrap_or("")),
        ast::PropName::Computed(computed) => {
            // Well-known symbol getters (e.g., `get [Symbol.toStringTag]()`)
            // get a synthetic `get_@@<short>` name. The caller is
            // responsible for lifting / renaming as needed.
            if let Some(wk) = symbol_well_known_key(&computed.expr) {
                format!("get_@@{}", wk)
            } else {
                return Err(anyhow!("Unsupported getter key"));
            }
        }
        _ => return Err(anyhow!("Unsupported getter key")),
    };

    let scope_mark = ctx.enter_scope();

    // Add 'this' for instance getters
    ctx.define_local("this".to_string(), Type::Any);

    // Getters have no parameters

    // Extract return type. Phase 4: body-based inference when no annotation.
    let has_explicit_return_annotation = method.function.return_type.is_some();
    let mut return_type = method
        .function
        .return_type
        .as_ref()
        .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
        .unwrap_or(Type::Any);

    // Lower body — see issue #569.
    let body = if let Some(ref block) = method.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };

    // Phase 4: getters can't be async/generator by JS syntax, so just the
    // plain body-walk + unify path. Feeds `class.getters[i].1.return_type`
    // which `receiver_class_name`-style codegen consults to pick Return
    // types through `obj.prop` chains.
    if !has_explicit_return_annotation && matches!(return_type, Type::Any) {
        if let Some(ref block) = method.function.body {
            if let Some(inferred) = infer_body_return_type(&block.stmts, ctx) {
                return_type = inferred;
            }
        }
    }

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
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Lower a setter method (set propertyName(value: Type) { ... })
pub(crate) fn lower_setter_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
) -> Result<Function> {
    let name = match &method.key {
        ast::PropName::Ident(ident) => format!("set_{}", ident.sym),
        ast::PropName::Str(s) => format!("set_{}", s.value.as_str().unwrap_or("")),
        _ => return Err(anyhow!("Unsupported setter key")),
    };

    let scope_mark = ctx.enter_scope();

    // Add 'this' for instance setters
    ctx.define_local("this".to_string(), Type::Any);

    // Setters have exactly one parameter
    let mut params = Vec::new();
    // Issue #572: setter param can be a destructuring pattern (`set v({ x }) {...}`).
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

    // Generate destructuring stmts BEFORE body lowering (issue #572).
    let mut destructuring_stmts: Vec<Stmt> = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
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
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub(crate) fn lower_class_prop(
    ctx: &mut LoweringContext,
    prop: &ast::ClassProp,
) -> Result<ClassField> {
    // Computed property keys (`[Symbol.for("k")]`, `[Parent.Symbol.X]`, etc.)
    // can't be reduced to a string at compile time — the key expression is
    // evaluated at construction time. We capture the lowered key expression
    // in `key_expr` and synthesize a placeholder `name` for HIR identity
    // (string-keyed lookup paths skip these fields via `key_expr.is_some()`).
    let (name, key_expr) = match &prop.key {
        ast::PropName::Ident(ident) => (ident.sym.to_string(), None),
        ast::PropName::Str(s) => (s.value.as_str().unwrap_or("").to_string(), None),
        ast::PropName::Computed(c) => {
            let key = lower_expr(ctx, &c.expr)?;
            // Synthetic name — uniqueness within a class is enforced by the
            // caller appending to fields/static_fields in source order; the
            // HIR's field-list iterators that key on `name` will still see
            // distinct entries because each computed-key field lowers in its
            // own call.
            let synth = format!("__computed_field_{}_{}", c.span.lo.0, c.span.hi.0);
            (synth, Some(key))
        }
        _ => return Err(anyhow!("Unsupported property key")),
    };

    // Extract type from type annotation (using context for class type param resolution).
    // Issue #305: when the annotation is absent, fall back to inferring the type from the
    // initializer (`= new Map<K,V>()`, `= new Set<T>()`, `= []`, ...) so the late
    // `register_class_field_types` call doesn't clobber the early-registered correct type
    // with Type::Any.
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
        key_expr,
        ty,
        init,
        is_private: false, // TODO: check accessibility
        is_readonly: prop.readonly,
        decorators: lower_decorators(ctx, &prop.decorators),
    })
}

/// Lower a private method (e.g. `#secret(): number { ... }`) — this mirrors
/// `lower_class_method` but for `ast::PrivateMethod`. The resulting function
/// is stored with the name prefixed by `#` so method dispatch (which keys on
/// `(class_name, "#secret")`) can find it.
pub(crate) fn lower_private_method(
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

    // Add 'this' for instance methods
    if !method.is_static {
        ctx.define_local("this".to_string(), Type::Any);
    }

    // Lower parameters with type extraction
    let mut params = Vec::new();
    // Issue #572 — private methods follow the same destructure-extraction shape
    // as public methods.
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &method.function.params {
        let param_name = get_pat_name(&param.pat)?;
        let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
        let param_default = get_param_default(ctx, &param.pat)?;
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), param_type.clone());
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: param_type,
            default: param_default,
            decorators: Vec::new(),
            is_rest,
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

    // #677: synthesize `arguments` if the private method body references it.
    let user_has_arguments_param = method
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let user_has_rest = method.function.params.iter().any(|p| is_rest_param(&p.pat));
    let needs_arguments_synth = !user_has_arguments_param
        && !user_has_rest
        && method
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        append_synthetic_arguments_param(ctx, &mut params);
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
pub(crate) fn lower_private_getter(
    ctx: &mut LoweringContext,
    method: &ast::PrivateMethod,
) -> Result<Function> {
    let name = format!("get_#{}", method.key.name);
    let scope_mark = ctx.enter_scope();
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
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

/// Lower a private setter method (e.g. `set #value(v: number) { ... }`).
pub(crate) fn lower_private_setter(
    ctx: &mut LoweringContext,
    method: &ast::PrivateMethod,
) -> Result<Function> {
    let name = format!("set_#{}", method.key.name);
    let scope_mark = ctx.enter_scope();
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
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    })
}

pub(crate) fn lower_private_prop(
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

pub(crate) fn lower_block_stmt(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Result<Vec<Stmt>> {
    lower_stmts_using_aware(ctx, &block.stmts)
}

/// Lower a function-body block, with support for ECMAScript function-decl
/// hoisting (issue #569). Pre-defines locals for every non-generator
/// `function name() {...}` at the block's top level so forward-reference
/// callsites resolve at HIR lowering time, then after the body is lowered
/// rearranges the resulting `Vec<Stmt>` so the hoisted FnDecls' `Stmt::Let`
/// entries appear before any other top-level statement (matching JS spec
/// "function declarations are hoisted AND initialized at function entry").
///
/// Sibling/forward captures need their box pre-allocated at the function
/// entry so the hoisted closure's `captures` list can stash a stable box
/// pointer instead of a TAG_UNDEFINED snapshot of the not-yet-run `Stmt::
/// Let`. We compute the set of (a) hoisted FnDecl ids referenced from any
/// closure body in the function, plus (b) function-body lets/consts
/// captured by any hoisted closure, and emit a synthetic `Stmt::Preallocate
/// Boxes(...)` at the very top of the result. Codegen consumes that variant
/// to alloca a slot+box for each id before any user statement runs.
pub(crate) fn lower_fn_body_block_stmt(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Result<Vec<Stmt>> {
    use std::collections::HashSet;

    // Phase 1: pre-define hoisted FnDecl locals so forward references in
    // any earlier statement resolve via `lookup_local`. Generator and
    // async-generator FnDecls are excluded — those go through the
    // hoist-to-top-level + FuncRef path in `lower_body_stmt` and aren't
    // closure-bound at the source position.
    let mut hoisted_id_set: HashSet<LocalId> = HashSet::new();
    for stmt in &block.stmts {
        if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
            if fn_decl.function.body.is_none() || fn_decl.function.is_generator {
                continue;
            }
            let name = fn_decl.ident.sym.to_string();
            let local_id = if let Some(existing) = ctx.lookup_local(&name) {
                existing
            } else {
                ctx.define_local(name.clone(), Type::Any)
            };
            hoisted_id_set.insert(local_id);
        }
    }

    // Phase 2: lower the body. The inner FnDecl arm in `lower_body_stmt`
    // calls `lookup_local(name)` and reuses our pre-defined id.
    let body = lower_block_stmt(ctx, block)?;

    if hoisted_id_set.is_empty() {
        return Ok(body);
    }

    // Phase 3: split — pull every top-level `Stmt::Let` whose id is in the
    // hoisted set to the front (preserving relative source order).
    let mut hoisted_lets: Vec<Stmt> = Vec::new();
    let mut other: Vec<Stmt> = Vec::new();
    for s in body {
        let is_hoisted = matches!(
            &s,
            Stmt::Let { id, init: Some(Expr::Closure { .. }), .. }
                if hoisted_id_set.contains(id)
        );
        if is_hoisted {
            hoisted_lets.push(s);
        } else {
            other.push(s);
        }
    }

    // Phase 4: compute the prealloc-box set via shared helper.
    let combined: Vec<Stmt> = hoisted_lets.iter().chain(other.iter()).cloned().collect();
    let prealloc = compute_prealloc_for_hoisted_closures(&combined, &hoisted_id_set);

    // Phase 5: assemble the final body — PreallocateBoxes (if any),
    // then the hoisted FnDecl Lets, then everything else.
    let mut result: Vec<Stmt> = Vec::new();
    if !prealloc.is_empty() {
        result.push(Stmt::PreallocateBoxes(prealloc));
    }
    result.extend(hoisted_lets);
    result.extend(other);
    Ok(result)
}

/// Compute the prealloc-box set for a function/arrow/fn-expr body that
/// performs ECMAScript function-decl hoisting. `body` is the already-
/// hoisted body (with FnDecl `Stmt::Let`s ahead of other top-level
/// stmts); `hoisted_id_set` is the set of LocalIds those FnDecls were
/// hoisted under. Returns the sorted list of LocalIds that need a
/// pre-allocated box at function entry — covers both (a) hoisted FnDecl
/// ids referenced from any closure body in this function (sibling
/// recursion), and (b) function-body let/const ids captured by any
/// hoisted closure (the closure literal is built before the let's source
/// position, so the let's box must already exist).
///
/// Issue #633 followup: previously only `lower_fn_body_block_stmt`
/// (function-decl bodies) emitted the prealloc; arrow-fn and fn-expr
/// bodies did their own hoisting inline and skipped this analysis,
/// leading to capture-of-uninitialized-slot for hoisted async fn decls
/// that captured outer `let`s (the dispatch chain pattern in hono
/// `compose()`).
pub(crate) fn compute_prealloc_for_hoisted_closures(
    body: &[Stmt],
    hoisted_id_set: &std::collections::HashSet<LocalId>,
) -> Vec<LocalId> {
    use std::collections::HashSet;

    let mut closure_body_refs: HashSet<LocalId> = HashSet::new();
    for s in body {
        collect_refs_in_closure_bodies_stmt(s, &mut closure_body_refs);
    }

    let mut body_let_ids: HashSet<LocalId> = HashSet::new();
    for s in body {
        collect_top_level_let_ids_stmt(s, &mut body_let_ids);
    }

    let mut prealloc_set: HashSet<LocalId> = HashSet::new();
    for &id in hoisted_id_set {
        if closure_body_refs.contains(&id) {
            prealloc_set.insert(id);
        }
    }
    for s in body {
        if let Stmt::Let {
            id,
            init: Some(Expr::Closure { captures, .. }),
            ..
        } = s
        {
            if hoisted_id_set.contains(id) {
                for &cap in captures {
                    if body_let_ids.contains(&cap) && !hoisted_id_set.contains(&cap) {
                        prealloc_set.insert(cap);
                    }
                }
            }
        }
    }

    let mut prealloc: Vec<LocalId> = prealloc_set.into_iter().collect();
    prealloc.sort();
    prealloc
}

/// Collect every `LocalId` referenced (LocalGet / LocalSet / Update / etc.)
/// from inside any `Expr::Closure` body found within `stmt`. Used by
/// `lower_fn_body_block_stmt` to decide which hoisted FnDecl ids need a
/// pre-allocated box.
pub(crate) fn collect_refs_in_closure_bodies_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => collect_refs_in_closure_bodies_expr(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_refs_in_closure_bodies_expr(e, out);
            }
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_refs_in_closure_bodies_expr(e, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_refs_in_closure_bodies_expr(condition, out);
            for s in then_branch {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_refs_in_closure_bodies_expr(condition, out);
            for s in body {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                collect_refs_in_closure_bodies_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_refs_in_closure_bodies_expr(c, out);
            }
            if let Some(u) = update {
                collect_refs_in_closure_bodies_expr(u, out);
            }
            for s in body {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_refs_in_closure_bodies_expr(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_refs_in_closure_bodies_expr(t, out);
                }
                for s in &case.body {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_refs_in_closure_bodies_stmt(body, out),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn collect_refs_in_closure_bodies_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    if let Expr::Closure { body, .. } = expr {
        // Inside a closure body — collect every reference (including refs
        // from any further-nested closures, since those run when the outer
        // closure runs, after the function body has set up bindings).
        let mut tmp_refs: Vec<LocalId> = Vec::new();
        let mut visited = std::collections::HashSet::new();
        for s in body {
            collect_local_refs_stmt(s, &mut tmp_refs, &mut visited);
        }
        for id in tmp_refs {
            out.insert(id);
        }
        return;
    }
    crate::walker::walk_expr_children(expr, &mut |child| {
        collect_refs_in_closure_bodies_expr(child, out)
    });
}

/// Collect `LocalId`s declared by a top-level `Stmt::Let` in `stmt`. Does
/// NOT recurse into nested blocks (those are block-scoped — their lets
/// aren't hoisted to function-entry).
pub(crate) fn collect_top_level_let_ids_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<LocalId>,
) {
    if let Stmt::Let { id, .. } = stmt {
        out.insert(*id);
    }
}

/// Lower a block statement that introduces its own lexical scope for
/// `let`/`const`. Inner bindings shadow outer ones and are removed on exit.
/// `var` declarations remain visible (function-scoped).
pub(crate) fn lower_block_stmt_scoped(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Result<Vec<Stmt>> {
    let mark = ctx.push_block_scope();
    let stmts = lower_stmts_using_aware(ctx, &block.stmts)?;
    ctx.pop_block_scope(mark);
    Ok(stmts)
}

/// Lower a sequence of body statements, desugaring `using` / `await using`
/// declarations into nested try/finally blocks that invoke the bound value's
/// `[Symbol.dispose]()` (sync `using`) or `await [Symbol.asyncDispose]()`
/// (`await using`) on block exit, in reverse declaration order. Issue #154.
///
/// Class methods written as `[Symbol.dispose]()` / `[Symbol.asyncDispose]()`
/// are renamed at lowering time (`lower_class_method`) to the stable string
/// names `__perry_dispose__` / `__perry_async_dispose__` so this desugarer
/// can dispatch via plain `obj.__perry_dispose__()` method calls.
///
/// Bindings whose initializer evaluates to `null` or `undefined` are skipped
/// per spec (no dispose call, no error). Multi-binding using declarations
/// (`using a = e1, b = e2`) are unrolled left-to-right with each binding
/// getting its own try/finally so the rightmost disposes first. SuppressedError
/// chaining when a body throw is followed by a dispose throw is not yet
/// implemented — the dispose throw shadows the original.
pub(crate) fn lower_stmts_using_aware(
    ctx: &mut LoweringContext,
    stmts: &[ast::Stmt],
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if let ast::Stmt::Decl(ast::Decl::Using(using_decl)) = stmt {
            let is_async = using_decl.is_await;
            let mut binding_ids: Vec<LocalId> = Vec::new();
            for decl in &using_decl.decls {
                if !matches!(&decl.name, ast::Pat::Ident(_)) {
                    bail!("`using` / `await using` requires an identifier binding");
                }
                // Reuse lower_var_decl_with_destructuring so the binding's type
                // is inferred from `new ClassName(...)` initializers — that
                // makes `obj.__perry_dispose__()` route through static class-
                // method dispatch (`receiver_class_name` returns the class name
                // for `Type::Named` locals; without inference it stays `Any`
                // and the call goes nowhere on missing-method).
                let stmts = lower_var_decl_with_destructuring(ctx, decl, false)?;
                for s in &stmts {
                    if let Stmt::Let { id, .. } = s {
                        binding_ids.push(*id);
                    }
                }
                result.extend(stmts);
            }
            // Recursively lower remaining stmts as the try body.
            let body_stmts = lower_stmts_using_aware(ctx, &stmts[i + 1..])?;
            // Wrap each binding in its own try/finally — innermost (rightmost
            // binding) finally runs first, giving reverse-declaration disposal.
            let mut wrapped = body_stmts;
            for &id in binding_ids.iter().rev() {
                let method_name = if is_async {
                    "__perry_async_dispose__"
                } else {
                    "__perry_dispose__"
                };
                // if (id !== null && id !== undefined) [await] id.<method>()
                let null_check = Expr::Logical {
                    op: LogicalOp::And,
                    left: Box::new(Expr::Compare {
                        op: CompareOp::Ne,
                        left: Box::new(Expr::LocalGet(id)),
                        right: Box::new(Expr::Null),
                    }),
                    right: Box::new(Expr::Compare {
                        op: CompareOp::Ne,
                        left: Box::new(Expr::LocalGet(id)),
                        right: Box::new(Expr::Undefined),
                    }),
                };
                let mut call_expr = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(id)),
                        property: method_name.to_string(),
                    }),
                    args: Vec::new(),
                    type_args: Vec::new(),
                };
                if is_async {
                    call_expr = Expr::Await(Box::new(call_expr));
                }
                let finally_stmts = vec![Stmt::If {
                    condition: null_check,
                    then_branch: vec![Stmt::Expr(call_expr)],
                    else_branch: None,
                }];
                wrapped = vec![Stmt::Try {
                    body: wrapped,
                    catch: None,
                    finally: Some(finally_stmts),
                }];
            }
            result.extend(wrapped);
            return Ok(result);
        }
        result.extend(lower_body_stmt(ctx, stmt)?);
    }
    Ok(result)
}

pub(crate) fn lower_body_stmt(ctx: &mut LoweringContext, stmt: &ast::Stmt) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match stmt {
        ast::Stmt::Return(ret) => {
            let value = ret.arg.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            result.push(Stmt::Return(value));
        }
        ast::Stmt::If(if_stmt) => {
            let condition = lower_expr(ctx, &if_stmt.test)?;
            // Each branch introduces its own lexical scope for let/const.
            // Skip the extra push if the branch is already a BlockStmt (which
            // will push its own scope via lower_block_stmt_scoped), or another
            // If (else-if chain) which handles its own scoping.
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
            result.push(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }
        ast::Stmt::Block(block) => {
            // Bare block: introduce a lexical scope so let/const shadow
            // without leaking into the enclosing scope.
            result.extend(lower_block_stmt_scoped(ctx, block)?);
        }
        ast::Stmt::Expr(expr_stmt) => {
            // Desugar this.field.splice(...) to:
            //   let __temp = this.field;
            //   __temp.splice(...);
            //   this.field = __temp;
            // This avoids a codegen issue where calling js_array_splice directly
            // on a class field pointer corrupts the object memory.
            if let ast::Expr::Call(call) = expr_stmt.expr.as_ref() {
                if let ast::Callee::Expr(callee) = &call.callee {
                    if let ast::Expr::Member(member) = callee.as_ref() {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            if method_ident.sym.as_ref() == "splice" {
                                if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
                                    if let ast::Expr::This(_) = inner_member.obj.as_ref() {
                                        if let ast::MemberProp::Ident(field_ident) =
                                            &inner_member.prop
                                        {
                                            let field_name = field_ident.sym.to_string();
                                            // Create temp local
                                            let temp_id = ctx.fresh_local();
                                            let temp_name = format!("__splice_temp_{}", field_name);
                                            ctx.locals.push((
                                                temp_name.clone(),
                                                temp_id,
                                                Type::Array(Box::new(Type::Any)),
                                            ));

                                            // Stmt 1: let __temp = this.field;
                                            result.push(Stmt::Let {
                                                id: temp_id,
                                                name: temp_name.clone(),
                                                ty: Type::Array(Box::new(Type::Any)),
                                                mutable: true,
                                                init: Some(Expr::PropertyGet {
                                                    object: Box::new(Expr::This),
                                                    property: field_name.clone(),
                                                }),
                                            });

                                            // Stmt 2: __temp.splice(args...)
                                            let mut args_iter = call
                                                .args
                                                .iter()
                                                .map(|a| lower_expr(ctx, &a.expr))
                                                .collect::<Result<Vec<Expr>>>()?
                                                .into_iter();
                                            if let Some(start) = args_iter.next() {
                                                let delete_count = args_iter.next();
                                                let items: Vec<Expr> = args_iter.collect();
                                                result.push(Stmt::Expr(Expr::ArraySplice {
                                                    array_id: temp_id,
                                                    start: Box::new(start),
                                                    delete_count: delete_count.map(Box::new),
                                                    items,
                                                }));
                                            }

                                            // Stmt 3: this.field = __temp;
                                            result.push(Stmt::Expr(Expr::PropertySet {
                                                object: Box::new(Expr::This),
                                                property: field_name,
                                                value: Box::new(Expr::LocalGet(temp_id)),
                                            }));

                                            return Ok(result);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a destructuring assignment that needs special handling
            if let ast::Expr::Assign(assign) = expr_stmt.expr.as_ref() {
                if let ast::AssignTarget::Pat(pat) = &assign.left {
                    // This is a destructuring assignment at statement level
                    // We can emit proper Let statements for temporaries
                    let stmts = lower_destructuring_assignment_stmt(ctx, pat, &assign.right)?;
                    result.extend(stmts);
                    return Ok(result);
                }
            }
            let expr = lower_expr(ctx, &expr_stmt.expr)?;
            result.push(Stmt::Expr(expr));
        }
        ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
            let mutable = var_decl.kind != ast::VarDeclKind::Const;
            let is_var = var_decl.kind == ast::VarDeclKind::Var;
            for decl in &var_decl.decls {
                // Issue #76 — pre-tag locals that hold the result of
                // `WebAssembly.instantiate(...)` so the standard
                // `inst.exports.<method>(...)` syntactic match in
                // `lower/expr_call.rs` only fires for genuine wasm
                // instances (not CJS-style `module.exports.foo()`).
                if let (ast::Pat::Ident(binding), Some(init_expr)) =
                    (&decl.name, decl.init.as_deref())
                {
                    if init_is_webassembly_instantiate(init_expr) {
                        ctx.wasm_instance_locals
                            .insert(binding.id.sym.as_ref().to_string());
                    }
                }
                let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable)?;
                // `var` is function-scoped: mark each defined local so
                // `pop_block_scope` preserves it when leaving an inner block.
                if is_var {
                    for s in &stmts {
                        if let Stmt::Let { id, .. } = s {
                            ctx.var_hoisted_ids.insert(*id);
                        }
                    }
                }
                // Issue #769 — mirror the class-tagging that
                // `lower::lower_stmt` does for top-level var decls,
                // but inside closure bodies (function-body path).
                // `const req = http.request(...)` declared inside a
                // callback like `server.listen(port, () => { ... })`
                // wouldn't otherwise be tagged as ClientRequest, so
                // `req.on/.end/...` would fall through and dispatch as
                // a generic property-call — never reaching
                // `js_http_client_request_end`. Mirrors the equivalent
                // arm for `net.createConnection` / `net.connect` /
                // `tls.connect` / `new net.Socket()`.
                for s in &stmts {
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
                        let socket_class = match (mod_name.as_str(), method.as_str()) {
                            ("net", "createConnection" | "connect") => Some(("net", "Socket")),
                            ("tls", "connect") => Some(("net", "Socket")),
                            ("net", "Socket") => Some(("net", "Socket")),
                            _ => None,
                        };
                        if let Some((m, c)) = socket_class {
                            ctx.register_native_instance(
                                name.clone(),
                                m.to_string(),
                                c.to_string(),
                            );
                        }
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
                    // Issue #1123 followup — mirror the top-level
                    // `Expr::NetCreateServer` registration in lower.rs
                    // for the inside-function case. Without this,
                    // `function main() { const s = createServer(...);
                    // s.listen(port, cb); }` would have `s` unregistered
                    // and `s.listen` would fall through dispatch.
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
                    }
                }
                result.extend(stmts);
            }
        }
        ast::Stmt::Decl(ast::Decl::Using(using_decl)) => {
            // `using` / `await using` — lower as const bindings.
            for decl in &using_decl.decls {
                let stmts = lower_var_decl_with_destructuring(ctx, decl, false)?;
                result.extend(stmts);
            }
        }
        ast::Stmt::Decl(ast::Decl::Class(class_decl)) => {
            // Class declared inside a function body (e.g., noble-curves' Point class)
            let class_name = class_decl.ident.sym.to_string();
            // Skip if a class with the same name already exists (avoids duplicate definitions
            // when the same class name appears at both module level and function body level)
            let already_exists = ctx.pending_classes.iter().any(|c| c.name == class_name)
                || ctx.classes_index.contains_key(&class_name);
            if !already_exists {
                let class = lower_class_decl(ctx, class_decl, false)?;
                ctx.pending_classes.push(class);
            }
        }
        ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
            // Inner function declarations are compiled as closures and assigned to local variables.
            // EXCEPTION: nested **generator** declarations (`function*` /
            // `async function*`) cannot be lowered as closures because the
            // generator-state-machine transform in `perry-transform/src/
            // generator.rs` only operates on top-level `Function`s in
            // `hir.functions`. Closures with `yield` in their body would
            // never run through the transform and would silently call the
            // raw IR (returning 0). Hoist them to top-level via
            // `lower_fn_decl` + `pending_functions` and register the local
            // as a FuncRef so the for-of / Array.fromAsync iterator path
            // detects them via `generator_func_names`.
            if fn_decl.function.body.is_some() && fn_decl.function.is_generator {
                let func_name = fn_decl.ident.sym.to_string();
                let func = lower_fn_decl(ctx, fn_decl)?;
                let func_id = func.id;
                ctx.register_func(func_name.clone(), func_id);
                ctx.pending_functions.push(func);
                // Also bind the local name so a downstream `LocalGet(name)`
                // resolves to the FuncRef. We use a Let with `init: Some(FuncRef)`
                // so existing code that does `let it = gen()` lowers via
                // the LocalGet path → FuncRef → known generator name.
                let local_id = ctx
                    .lookup_local(&func_name)
                    .unwrap_or_else(|| ctx.define_local(func_name.clone(), Type::Any));
                ctx.function_valued_locals.insert(local_id);
                result.push(Stmt::Let {
                    id: local_id,
                    name: func_name,
                    ty: Type::Any,
                    init: Some(Expr::FuncRef(func_id)),
                    mutable: false,
                });
                return Ok(result);
            }
            if fn_decl.function.body.is_some() {
                let func_name = fn_decl.ident.sym.to_string();
                let func_id = ctx.fresh_func();

                // Register the function name temporarily so self-recursive calls
                // inside the body resolve to FuncRef(func_id).
                ctx.register_func(func_name.clone(), func_id);

                // Define the local for the function name BEFORE lowering the body,
                // so self-recursive references inside the body resolve to
                // LocalGet(local_id) rather than FuncRef(func_id). This ensures
                // the LLVM backend's boxed-var analysis sees the same LocalId at
                // both the declaration and self-reference sites.
                let local_id = ctx
                    .lookup_local(&func_name)
                    .unwrap_or_else(|| ctx.define_local(func_name.clone(), Type::Any));

                let scope_mark = ctx.enter_scope();

                // Track outer locals for capture detection
                let outer_locals: Vec<(String, LocalId)> = ctx
                    .locals
                    .iter()
                    .map(|(name, id, _)| (name.clone(), *id))
                    .collect();

                // Lower parameters. Skip the TypeScript `this:` annotation —
                // it has no runtime existence (see the sibling site above for
                // the full rationale).
                let mut params = Vec::new();
                let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
                for param in &fn_decl.function.params {
                    let param_name = get_pat_name(&param.pat)?;
                    if param_name == "this" {
                        continue;
                    }
                    let param_default = get_param_default(ctx, &param.pat)?;
                    let is_rest = is_rest_param(&param.pat);
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    params.push(Param {
                        id: param_id,
                        name: param_name,
                        ty: Type::Any,
                        default: param_default,
                        decorators: Vec::new(),
                        is_rest,
                    });
                    if is_destructuring_pattern(&param.pat) {
                        destructuring_params.push((param_id, param.pat.clone()));
                    }
                }

                // #677: synthesize `arguments` for nested function decls.
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
                if needs_arguments_synth {
                    append_synthetic_arguments_param(ctx, &mut params);
                }

                // Generate destructuring stmts
                let mut destructuring_stmts = Vec::new();
                for (param_id, pat) in &destructuring_params {
                    let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
                    destructuring_stmts.extend(stmts);
                }

                // Lower body — see issue #569; hoist nested function-decl
                // statements within this inner fn body to the top so
                // forward refs and sibling captures work end-to-end.
                let mut body = if let Some(ref block) = fn_decl.function.body {
                    lower_fn_body_block_stmt(ctx, block)?
                } else {
                    Vec::new()
                };

                if !destructuring_stmts.is_empty() {
                    let mut new_body = destructuring_stmts;
                    new_body.append(&mut body);
                    body = new_body;
                }

                ctx.exit_scope(scope_mark);

                // Detect captured variables
                let mut all_refs = Vec::new();
                let mut visited_closures = std::collections::HashSet::new();
                for stmt in &body {
                    collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
                }

                let outer_local_ids: std::collections::HashSet<LocalId> =
                    outer_locals.iter().map(|(_, id)| *id).collect();
                let param_ids: std::collections::HashSet<LocalId> =
                    params.iter().map(|p| p.id).collect();

                // dayjs (issue: format() returned `292278994-08`): local
                // IDs are scope-local — see expr_function.rs
                // compute_closure_captures for the long explanation.
                // Strip locally-declared ids from the capture set so an
                // inner `var i = ...` doesn't collide with a same-id
                // outer constant.
                let inner_decls: std::collections::HashSet<LocalId> = {
                    let mut s = std::collections::HashSet::new();
                    for stmt in &body {
                        collect_let_decls_in_stmt(stmt, &mut s);
                    }
                    s
                };

                let mut captures: Vec<LocalId> = all_refs
                    .into_iter()
                    .filter(|id| {
                        outer_local_ids.contains(id)
                            && !param_ids.contains(id)
                            && !inner_decls.contains(id)
                    })
                    .collect();
                captures.sort();
                captures.dedup();
                captures = ctx.filter_module_level_captures(captures);

                // Detect mutable captures
                let mut all_assigned = Vec::new();
                for stmt in &body {
                    collect_assigned_locals_stmt(stmt, &mut all_assigned);
                }
                let assigned_set: std::collections::HashSet<LocalId> =
                    all_assigned.into_iter().collect();
                let mutable_captures: Vec<LocalId> = captures
                    .iter()
                    .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                    .copied()
                    .collect();

                // Issue #838 followup (b): tag the function-decl's
                // local id as function-valued so the assignment
                // recogniser routes `M.prototype.x = fn` (and the
                // `var m = M.prototype` aliased form) through the
                // function-classic prototype-method path. Babel's
                // class-from-function emit pattern and dayjs's
                // minified bundle both lower `function M(){}` inside
                // an IIFE to exactly this `Stmt::Let { init:
                // Some(Closure{…}) }` shape — the destructuring.rs
                // path only fires for `var/let/const` lets, so the
                // tag has to be applied here too.
                ctx.function_valued_locals.insert(local_id);

                let closure = Expr::Closure {
                    func_id,
                    params,
                    return_type: Type::Any,
                    body,
                    captures,
                    mutable_captures,
                    captures_this: false,
                    enclosing_class: None,
                    is_async: fn_decl.function.is_async,
                };
                result.push(Stmt::Let {
                    id: local_id,
                    name: func_name,
                    ty: Type::Any,
                    init: Some(closure),
                    mutable: false,
                });
            }
        }
        ast::Stmt::While(while_stmt) => {
            let condition = lower_expr(ctx, &while_stmt.test)?;
            // While body introduces its own lexical scope.
            let body = if matches!(*while_stmt.body, ast::Stmt::Block(_)) {
                lower_body_stmt(ctx, &while_stmt.body)?
            } else {
                let mark = ctx.push_block_scope();
                let stmts = lower_body_stmt(ctx, &while_stmt.body)?;
                ctx.pop_block_scope(mark);
                stmts
            };
            result.push(Stmt::While { condition, body });
        }
        ast::Stmt::DoWhile(do_while_stmt) => {
            let body = lower_body_stmt(ctx, &do_while_stmt.body)?;
            let condition = lower_expr(ctx, &do_while_stmt.test)?;
            result.push(Stmt::DoWhile { body, condition });
        }
        ast::Stmt::Labeled(labeled_stmt) => {
            let label = labeled_stmt.label.sym.to_string();
            let inner = lower_body_stmt(ctx, &labeled_stmt.body)?;
            // If the body lowered to a single statement, wrap it directly.
            // Otherwise wrap the first statement (preserving any hoisted lets before it).
            if inner.len() == 1 {
                let body = inner.into_iter().next().unwrap();
                result.push(Stmt::Labeled {
                    label,
                    body: Box::new(body),
                });
            } else {
                // Multiple statements — take the last "real" loop/block as the labeled target,
                // and emit any preceding statements (e.g., hoisted lets from for-of/for-in desugar) first.
                let mut inner = inner;
                let last = inner.pop().unwrap();
                for s in inner {
                    result.push(s);
                }
                result.push(Stmt::Labeled {
                    label,
                    body: Box::new(last),
                });
            }
        }
        ast::Stmt::Break(break_stmt) => {
            if let Some(ref label) = break_stmt.label {
                result.push(Stmt::LabeledBreak(label.sym.to_string()));
            } else {
                result.push(Stmt::Break);
            }
        }
        ast::Stmt::Continue(continue_stmt) => {
            if let Some(ref label) = continue_stmt.label {
                result.push(Stmt::LabeledContinue(label.sym.to_string()));
            } else {
                result.push(Stmt::Continue);
            }
        }
        ast::Stmt::For(for_stmt) => {
            // Push a block scope covering init/test/update/body, so
            // `for (let i = 0; ...)` bindings don't leak to the enclosing scope.
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
                                result.push(Stmt::Let {
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
                                result.push(Stmt::Let {
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
            result.push(Stmt::For {
                init,
                condition,
                update,
                body,
            });
        }
        ast::Stmt::Try(try_stmt) => {
            // try body is its own lexical scope
            let body = lower_block_stmt_scoped(ctx, &try_stmt.block)?;

            // Lower catch clause (if present)
            let catch = if let Some(ref catch_clause) = try_stmt.handler {
                let scope_mark = ctx.enter_scope();

                // Lower catch parameter (if present)
                let param = if let Some(ref pat) = catch_clause.param {
                    let param_name = get_pat_name(pat)?;
                    let param_id = ctx.define_local(param_name.clone(), Type::Any);
                    Some((param_id, param_name))
                } else {
                    None
                };

                // Lower catch body
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

            result.push(Stmt::Try {
                body,
                catch,
                finally,
            });
        }
        ast::Stmt::Throw(throw_stmt) => {
            let expr = lower_expr(ctx, &throw_stmt.arg)?;
            result.push(Stmt::Throw(expr));
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

            result.push(Stmt::Switch {
                discriminant,
                cases,
            });
        }
        ast::Stmt::ForOf(for_of_stmt) => {
            // --- Issue #237: `for await (const c of <ReadableStream>)` ---
            // Lower to a getReader/read loop so the body sees Uint8Array
            // chunks. Detect by checking the iterable's registered native
            // instance type. Falls through to the generic async-iterator
            // path if not a ReadableStream.
            if for_of_stmt.is_await {
                let is_readable_stream = if let ast::Expr::Ident(ident) = &*for_of_stmt.right {
                    matches!(
                        ctx.lookup_native_instance(ident.sym.as_ref()),
                        Some((_, "ReadableStream"))
                    )
                } else {
                    false
                };

                if is_readable_stream {
                    let scope_mark = ctx.push_block_scope();
                    let stream_expr = lower_expr(ctx, &for_of_stmt.right)?;

                    // const __reader = stream.getReader();
                    let reader_id = ctx.fresh_local();
                    ctx.locals
                        .push((format!("__reader_{}", reader_id), reader_id, Type::Any));
                    ctx.register_native_instance(
                        format!("__reader_{}", reader_id),
                        "readable_stream_reader".to_string(),
                        "ReadableStreamDefaultReader".to_string(),
                    );
                    result.push(Stmt::Let {
                        id: reader_id,
                        name: format!("__reader_{}", reader_id),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::NativeMethodCall {
                            module: "readable_stream".to_string(),
                            class_name: Some("ReadableStream".to_string()),
                            object: Some(Box::new(stream_expr)),
                            method: "getReader".to_string(),
                            args: vec![],
                        }),
                    });

                    // let __res = await __reader.read();
                    let res_id = ctx.fresh_local();
                    ctx.locals
                        .push((format!("__res_{}", res_id), res_id, Type::Any));
                    let read_call = || {
                        Expr::Await(Box::new(Expr::NativeMethodCall {
                            module: "readable_stream_reader".to_string(),
                            class_name: Some("ReadableStreamDefaultReader".to_string()),
                            object: Some(Box::new(Expr::LocalGet(reader_id))),
                            method: "read".to_string(),
                            args: vec![],
                        }))
                    };
                    result.push(Stmt::Let {
                        id: res_id,
                        name: format!("__res_{}", res_id),
                        ty: Type::Any,
                        mutable: true,
                        init: Some(read_call()),
                    });

                    let item_name = if let ast::ForHead::VarDecl(var_decl) = &for_of_stmt.left {
                        if let Some(decl) = var_decl.decls.first() {
                            if let ast::Pat::Ident(ident) = &decl.name {
                                ident.id.sym.to_string()
                            } else {
                                "__chunk".to_string()
                            }
                        } else {
                            "__chunk".to_string()
                        }
                    } else {
                        "__chunk".to_string()
                    };
                    let item_id = ctx.define_local(item_name.clone(), Type::Any);

                    let mut body_stmts: Vec<Stmt> = Vec::new();
                    body_stmts.push(Stmt::Let {
                        id: item_id,
                        name: item_name,
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(res_id)),
                            property: "value".to_string(),
                        }),
                    });
                    let user_body = lower_body_stmt(ctx, &for_of_stmt.body)?;
                    body_stmts.extend(user_body);
                    body_stmts.push(Stmt::Expr(Expr::LocalSet(res_id, Box::new(read_call()))));

                    result.push(Stmt::While {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(Expr::PropertyGet {
                                object: Box::new(Expr::LocalGet(res_id)),
                                property: "done".to_string(),
                            }),
                        },
                        body: body_stmts,
                    });

                    // reader.releaseLock(); — best-effort cleanup so the
                    // stream stays usable after the loop body falls out.
                    result.push(Stmt::Expr(Expr::NativeMethodCall {
                        module: "readable_stream_reader".to_string(),
                        class_name: Some("ReadableStreamDefaultReader".to_string()),
                        object: Some(Box::new(Expr::LocalGet(reader_id))),
                        method: "releaseLock".to_string(),
                        args: vec![],
                    }));

                    ctx.pop_block_scope(scope_mark);
                    return Ok(result);
                }
            }

            // --- Iterator-protocol path for generator function calls ---
            // Detect: `for [await] (const x of genFunc(...))` where genFunc is
            // function* / async function*. Without this path the for-of falls
            // through to the array-index desugar which segfaults on a real
            // iterator object. Mirrors `lower::lower_stmt`'s ForOf branch.
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
                let scope_mark = ctx.push_block_scope();
                let iter_expr_raw = lower_expr(ctx, &for_of_stmt.right)?;
                let iter_expr = if let Some(iter_fn_id) = iter_from_class {
                    Expr::Call {
                        callee: Box::new(Expr::FuncRef(iter_fn_id)),
                        args: vec![iter_expr_raw],
                        type_args: vec![],
                    }
                } else {
                    iter_expr_raw
                };
                let iter_id = ctx.fresh_local();
                ctx.locals
                    .push((format!("__iter_{}", iter_id), iter_id, Type::Any));
                result.push(Stmt::Let {
                    id: iter_id,
                    name: format!("__iter_{}", iter_id),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(iter_expr),
                });

                let result_id = ctx.fresh_local();
                ctx.locals
                    .push((format!("__result_{}", result_id), result_id, Type::Any));
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
                result.push(Stmt::Let {
                    id: result_id,
                    name: format!("__result_{}", result_id),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(next_call.clone()),
                });

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

                let mut body_stmts: Vec<Stmt> = Vec::new();
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
                let user_body = lower_body_stmt(ctx, &for_of_stmt.body)?;
                body_stmts.extend(user_body);
                body_stmts.push(Stmt::Expr(Expr::LocalSet(result_id, Box::new(next_call))));

                result.push(Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: body_stmts,
                });

                ctx.pop_block_scope(scope_mark);
                return Ok(result);
            }

            // Desugar for-of to a regular for loop (same as in lower_stmt).
            // Push a block scope so loop variables and internal temporaries don't leak.
            let for_scope_mark = ctx.push_block_scope();

            // Detect string iteration BEFORE lowering — each iteration yields a
            // 1-char string via str[i] rather than an array element.
            let is_string_iter = crate::lower::is_ast_string_expr(ctx, &for_of_stmt.right);

            let arr_expr = lower_expr(ctx, &for_of_stmt.right)?;

            // Issue #302: resolve the iterable's declared type. Was
            // limited to `Ident` (local variable lookup) so
            // `for (const [k, v] of this.someMap)` produced a raw Map
            // handle that the for-loop's `.length` read returned 0 on,
            // silently skipping the loop body. Now also resolves
            // `Member { obj: This, prop: ident }` via the class field
            // type registry so class instance fields work too.
            // Issue #311 extends to plain object property access
            // (`obj.m` where `obj` is a local with an inferred
            // `Type::Object` shape) — same silent-zero-iterations
            // symptom as #302, just a different missing arm.
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

            // Fast path: `for (const [k, v] of mapExpr)` reads flat entries
            // directly via `MapEntryKeyAt` / `MapEntryValueAt` — no pair-Array
            // materialization. Mirrors the same detection in `lower::lower_stmt`.
            // Issue #542/#543: also accept `Type::Union([Generic{Map}, Void])`
            // (the shape produced by `Map<K, V> | undefined` parameters /
            // return types). After the `if (!m) return;` narrowing, the type
            // is morally `Map<K, V>` but perry doesn't propagate the narrow
            // through the union type, so `for (const [k, v] of m)` would fall
            // through to array iteration and read garbage from MapHeader bytes.
            let type_contains_map =
                |ty: &Type| -> bool { matches!(ty, Type::Generic { base, .. } if base == "Map") };
            let is_iterable_map = match &iterable_type {
                Some(Type::Generic { base, .. }) if base == "Map" => true,
                Some(Type::Union(variants)) => variants.iter().any(type_contains_map),
                _ => false,
            };
            // Map fast path also fires for the single-binding shapes
            //   for (const [k] of map)        — only key
            //   for (const [, v] of map)      — only value
            // Each non-empty slot must be a plain Ident; nested patterns
            // / object patterns / defaults fall through to the materialized
            // MapEntries path so destructuring stays correct.
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
            // Fast path: `for (const x of setExpr)` reads elements directly
            // via `SetValueAt` (→ `js_set_value_at`) instead of materializing
            // the buffer with `js_set_to_array`.
            // Issue #542/#543: also accept `Type::Union([Generic{Set}, Void])`.
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
            // If the iterable is a Map or Set, wrap in MapEntries / SetValues
            // to materialize it as an array for the index-based loop.
            // Fast-path Map+[k,v] / Set+ident iterables stay unwrapped — the
            // loop reads entries directly via runtime helpers below.
            // Issue #542/#543: handle `Map | undefined` / `Set | undefined`
            // shapes via the same `is_iterable_map` / `is_iterable_set` flags
            // (which now Union-aware) instead of a fresh narrow match here.
            // Issue #578: typed-array iterables in function-body for-of.
            // Same materialization fix as the module-init lowering path —
            // wrap in `Expr::ArrayFrom` so iteration sees the byte values
            // not the byte buffer reinterpreted as f64s.
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

            // For string iteration the __arr holder is typed as String (so codegen
            // uses string.length + js_string_char_at via the existing str[i] path).
            // For an identifier iterable like `for (const word of words)` where
            // `words: string[]`, extract the element type from the local's
            // declared Array<T> so the loop variable gets the right type.
            let inferred_elem_type: Option<Type> = match &iterable_type {
                Some(Type::Array(elem)) => Some((**elem).clone()),
                Some(Type::Generic { base, type_args })
                    if base == "Array" && type_args.len() == 1 =>
                {
                    Some(type_args[0].clone())
                }
                Some(Type::Generic { base, type_args })
                    if base == "Map" && type_args.len() >= 2 =>
                {
                    // for-of over Map yields [K, V] tuples
                    Some(Type::Tuple(vec![
                        type_args[0].clone(),
                        type_args[1].clone(),
                    ]))
                }
                Some(Type::Generic { base, type_args })
                    if base == "Set" && !type_args.is_empty() =>
                {
                    Some(type_args[0].clone())
                }
                _ => None,
            };
            // For the Map fast path the holder must be typed Map so
            // `__m.size` resolves through `is_map_expr` to `js_map_size`.
            let holder_type = if is_string_iter {
                Type::String
            } else if map_kv_fastpath {
                if let Some(Type::Generic { base, type_args }) = iterable_type.clone() {
                    if base == "Map" && type_args.len() >= 2 {
                        Type::Generic {
                            base: "Map".to_string(),
                            type_args,
                        }
                    } else {
                        Type::Any
                    }
                } else {
                    Type::Any
                }
            } else if set_fastpath {
                // Holder typed as Set so `__s.size` resolves through
                // `is_set_expr` to `js_set_size` instead of `.length`.
                if let Some(Type::Generic { base, type_args }) = iterable_type.clone() {
                    if base == "Set" {
                        Type::Generic {
                            base: "Set".to_string(),
                            type_args,
                        }
                    } else {
                        Type::Any
                    }
                } else {
                    Type::Any
                }
            } else if let Some(ref elem) = inferred_elem_type {
                Type::Array(Box::new(elem.clone()))
            } else {
                Type::Array(Box::new(Type::Any))
            };
            let item_hir_type = if is_string_iter {
                Type::String
            } else if is_iterable_typed_array {
                // Issue #578: typed-array element values are always Number.
                Type::Number
            } else if let Some(elem) = inferred_elem_type {
                elem
            } else {
                Type::Any
            };

            let arr_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            ctx.locals
                .push((format!("__arr_{}", arr_id), arr_id, holder_type.clone()));
            ctx.locals
                .push((format!("__idx_{}", idx_id), idx_id, Type::Number));

            // Store array reference
            result.push(Stmt::Let {
                id: arr_id,
                name: format!("__arr_{}", arr_id),
                ty: holder_type.clone(),
                mutable: false,
                init: Some(arr_expr),
            });

            // IMPORTANT: Define iteration variables BEFORE lowering the body
            let item_id = ctx.fresh_local();
            ctx.locals.push((
                format!("__item_{}", item_id),
                item_id,
                item_hir_type.clone(),
            ));

            // Pre-define all variables from the pattern
            let var_ids: Vec<(String, u32)> = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        match &decl.name {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                let id = ctx.define_local(name.clone(), item_hir_type.clone());
                                vec![(name, id)]
                            }
                            ast::Pat::Array(arr_pat) => {
                                let mut ids = Vec::new();
                                for elem_pat in arr_pat.elems.iter().flatten() {
                                    if let ast::Pat::Ident(ident) = elem_pat {
                                        let name = ident.id.sym.to_string();
                                        let id = ctx.define_local(name.clone(), Type::Any);
                                        ids.push((name, id));
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
                                                // Recurse so leaves get pre-defined and the
                                                // body can reference them. Issue #554 (the
                                                // function-body counterpart of the lower.rs
                                                // top-level fix in v0.5.629).
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

            // NOW lower the body
            let mut loop_body = lower_body_stmt(ctx, &for_of_stmt.body)?;

            // Build binding statements using pre-defined variable IDs
            let binding_stmts = match &for_of_stmt.left {
                ast::ForHead::VarDecl(var_decl) => {
                    if let Some(decl) = var_decl.decls.first() {
                        // `for await (const x of arr)`: spec ECMA-262 §14.7.5.10
                        // — each iteration awaits the value yielded by the
                        // iterator. For plain-array iterables (the common
                        // shape; the iterator-protocol path above already
                        // handles `for await ... of asyncGen()`), wrap the
                        // per-element `arr[i]` access in `Expr::Await` so a
                        // `Promise.resolve(n)` element is unwrapped to `n`
                        // before binding. Without this, `for await (const x of
                        // [Promise.resolve(1), …]) sum += x` binds `x` to a
                        // raw Promise object and `sum += x` produces NaN.
                        // Mirrors the same fix in `lower.rs::lower_stmt`'s
                        // module-init for-of arm.
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
                                    ty: item_hir_type.clone(),
                                    mutable: false,
                                    init: Some(init),
                                }]
                            }
                            ast::Pat::Array(arr_pat) => {
                                if map_kv_fastpath {
                                    // Map [k, v] / [k] / [, v] fast path: read
                                    // each requested entry slot directly. No
                                    // `__item` Array materialization. Skipped
                                    // slots emit no binding.
                                    let mut stmts: Vec<Stmt> = Vec::new();
                                    let mut var_idx = 0;
                                    for (slot, elem) in arr_pat.elems.iter().enumerate() {
                                        let Some(ast::Pat::Ident(_)) = elem else {
                                            continue;
                                        };
                                        let (name, id) = var_ids[var_idx].clone();
                                        var_idx += 1;
                                        let init = if slot == 0 {
                                            Expr::MapEntryKeyAt {
                                                map: Box::new(Expr::LocalGet(arr_id)),
                                                idx: Box::new(Expr::LocalGet(idx_id)),
                                            }
                                        } else {
                                            Expr::MapEntryValueAt {
                                                map: Box::new(Expr::LocalGet(arr_id)),
                                                idx: Box::new(Expr::LocalGet(idx_id)),
                                            }
                                        };
                                        stmts.push(Stmt::Let {
                                            id,
                                            name,
                                            ty: Type::Any,
                                            mutable: false,
                                            init: Some(init),
                                        });
                                    }
                                    stmts
                                } else {
                                    let mut stmts = vec![Stmt::Let {
                                        id: item_id,
                                        name: format!("__item_{}", item_id),
                                        ty: Type::Any,
                                        mutable: false,
                                        init: Some(item_expr),
                                    }];
                                    let mut var_idx = 0;
                                    for (idx, elem) in arr_pat.elems.iter().enumerate() {
                                        if let Some(elem_pat) = elem {
                                            if let ast::Pat::Ident(_) = elem_pat {
                                                let (name, id) = var_ids[var_idx].clone();
                                                var_idx += 1;
                                                stmts.push(Stmt::Let {
                                                    id,
                                                    name,
                                                    ty: Type::Any,
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
                                let mut stmts = vec![Stmt::Let {
                                    id: item_id,
                                    name: format!("__item_{}", item_id),
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(item_expr),
                                }];
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
                                                // Issue #554 (function-body path).
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

            // Loop bound: Map/Set fast paths use `.size` (codegen-recognized,
            // lowered to js_map_size / js_set_size), regular path uses .length.
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
            // Create the for loop
            result.push(Stmt::For {
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
            // Desugar for-in to a for-of over Object.keys(obj) (same as in lower_stmt).
            // Push a block scope so loop variables don't leak.
            let for_scope_mark = ctx.push_block_scope();
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

            let obj_expr = lower_expr(ctx, &for_in_stmt.right)?;
            let keys_expr = Expr::ObjectKeys(Box::new(obj_expr));
            let keys_id = ctx.fresh_local();
            let idx_id = ctx.fresh_local();
            let key_id = ctx.define_local(key_name.clone(), Type::String);

            // Store keys array reference
            result.push(Stmt::Let {
                id: keys_id,
                name: format!("__keys_{}", keys_id),
                ty: Type::Array(Box::new(Type::String)),
                mutable: false,
                init: Some(keys_expr),
            });

            // Lower the body and prepend key assignment
            let mut loop_body = lower_body_stmt(ctx, &for_in_stmt.body)?;
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

            // Create the for loop
            result.push(Stmt::For {
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
        // Empty statement (`;`) — nothing to lower.
        ast::Stmt::Empty(_) => {}
        // `debugger;` is a no-op in AOT compilation.
        ast::Stmt::Debugger(_) => {}
        // Type-only declarations are fully erased at compile time.
        ast::Stmt::Decl(ast::Decl::TsInterface(_)) | ast::Stmt::Decl(ast::Decl::TsTypeAlias(_)) => {
        }
        // Body-local enum / namespace are valid TS but Perry only registers them
        // at module scope (see lower.rs::lower_module). Silently dropping them
        // here produced runtime ReferenceErrors at the use site instead of a
        // compile diagnostic — fail loud so the user knows to hoist the decl.
        ast::Stmt::Decl(ast::Decl::TsEnum(enum_decl)) => {
            crate::lower_bail!(
                enum_decl.span,
                "enum declared inside a function body is not supported; declare it at module scope"
            );
        }
        ast::Stmt::Decl(ast::Decl::TsModule(ts_module)) => {
            crate::lower_bail!(
                ts_module.span,
                "namespace/module declared inside a function body is not supported; declare it at module scope"
            );
        }
        // `with` is forbidden under TS strict-mode (the implicit default for
        // ES modules) — Perry does not implement dynamic scope chains.
        ast::Stmt::With(with_stmt) => {
            crate::lower_bail!(
                with_stmt.span,
                "`with` statement is not supported (also forbidden in strict mode)"
            );
        }
        // Final catch-all: any genuinely unexpected variant (e.g. a future
        // swc Stmt variant we haven't enumerated) bails instead of silently
        // dropping the statement. #853: `ast::Stmt` is `#[non_exhaustive]`
        // upstream — keep the catch-all even though current SWC variants
        // are covered.
        #[allow(unreachable_patterns)]
        other => {
            return Err(anyhow!(
                "lower_body_stmt: unhandled statement variant {:?}",
                std::mem::discriminant(other)
            ));
        }
    }

    Ok(result)
}

/// Scan AST statements for `return <ident>` where the ident is a native instance.
/// Registers the containing function in `func_return_native_instances` so callers
/// can track `const db = initDb()` as returning a native handle.
fn find_native_return_in_stmts(
    stmts: &[ast::Stmt],
    ctx: &mut LoweringContext,
    func_name: &str,
    ni_start: usize,
) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Return(ret_stmt) => {
                if let Some(ref arg) = ret_stmt.arg {
                    if let ast::Expr::Ident(ident) = arg.as_ref() {
                        let var = ident.sym.as_ref();
                        for i in ni_start..ctx.native_instances.len() {
                            if ctx.native_instances[i].0 == var {
                                ctx.func_return_native_instances.push((
                                    func_name.to_string(),
                                    ctx.native_instances[i].1.clone(),
                                    ctx.native_instances[i].2.clone(),
                                ));
                                return;
                            }
                        }
                    }
                }
            }
            // Recurse into blocks that may contain returns
            ast::Stmt::Block(block) => {
                find_native_return_in_stmts(&block.stmts, ctx, func_name, ni_start);
            }
            ast::Stmt::If(if_stmt) => {
                if let ast::Stmt::Block(ref block) = *if_stmt.cons {
                    find_native_return_in_stmts(&block.stmts, ctx, func_name, ni_start);
                }
                if let Some(ref alt) = if_stmt.alt {
                    if let ast::Stmt::Block(ref block) = **alt {
                        find_native_return_in_stmts(&block.stmts, ctx, func_name, ni_start);
                    }
                }
            }
            ast::Stmt::Try(try_stmt) => {
                find_native_return_in_stmts(&try_stmt.block.stmts, ctx, func_name, ni_start);
                if let Some(ref handler) = try_stmt.handler {
                    find_native_return_in_stmts(&handler.body.stmts, ctx, func_name, ni_start);
                }
            }
            _ => {}
        }
        // Stop once registered (early return in Return arm handles the direct case;
        // check here for nested finds)
        if ctx
            .func_return_native_instances
            .iter()
            .any(|(n, _, _)| n == func_name)
        {
            return;
        }
    }
}
