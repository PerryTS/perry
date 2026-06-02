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

use super::helpers::{async_iterator_method_call, is_filehandle_readlines_for_await_target};
use super::*;

fn class_computed_member_registration_expr(class_name: &str, member: &ClassComputedMember) -> Expr {
    match member.kind {
        ClassComputedMemberKind::Method => Expr::RegisterClassComputedMethod {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            method_name: member.function.name.clone(),
            is_static: member.is_static,
            param_count: member.function.params.len() as u32,
            has_rest: member
                .function
                .params
                .last()
                .map(|p| p.is_rest)
                .unwrap_or(false),
        },
        ClassComputedMemberKind::Getter => Expr::RegisterClassComputedAccessor {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            getter_name: Some(member.function.name.clone()),
            setter_name: None,
            is_static: member.is_static,
        },
        ClassComputedMemberKind::Setter => Expr::RegisterClassComputedAccessor {
            class_name: class_name.to_string(),
            key_expr: Box::new(member.key_expr.clone()),
            getter_name: None,
            setter_name: Some(member.function.name.clone()),
            is_static: member.is_static,
        },
    }
}

mod nested_fn_decl;

fn unwrap_stream_expr(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(ts_as) => &ts_as.expr,
            ast::Expr::TsNonNull(non_null) => &non_null.expr,
            ast::Expr::TsConstAssertion(assertion) => &assertion.expr,
            ast::Expr::TsTypeAssertion(assertion) => &assertion.expr,
            ast::Expr::Paren(paren) => &paren.expr,
            _ => break,
        };
    }
    expr
}

fn web_readable_stream_values_receiver(expr: &ast::Expr) -> Option<&ast::Expr> {
    let ast::Expr::Call(call) = unwrap_stream_expr(expr) else {
        return None;
    };
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return None;
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return None;
    };
    if !matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "values") {
        return None;
    }
    Some(member.obj.as_ref())
}

fn is_web_readable_stream_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match unwrap_stream_expr(expr) {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            matches!(
                ctx.lookup_native_instance(name),
                Some((_, "ReadableStream"))
            ) || matches!(
                ctx.lookup_local_type(name),
                Some(Type::Named(n)) if n == "ReadableStream"
            )
        }
        ast::Expr::New(new_expr) => matches!(
            new_expr.callee.as_ref(),
            ast::Expr::Ident(callee) if callee.sym.as_ref() == "ReadableStream"
        ),
        _ => false,
    }
}

fn strip_for_of_expr_wrappers(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(x) => &x.expr,
            ast::Expr::TsNonNull(x) => &x.expr,
            ast::Expr::TsConstAssertion(x) => &x.expr,
            ast::Expr::Paren(x) => &x.expr,
            _ => return expr,
        };
    }
}

fn is_node_readable_class_ref(expr: &ast::Expr) -> bool {
    match strip_for_of_expr_wrappers(expr) {
        ast::Expr::Ident(ident) => ident.sym.as_ref() == "Readable",
        ast::Expr::Member(member) => {
            matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "Readable")
        }
        _ => false,
    }
}

fn is_node_readable_static_factory(expr: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = strip_for_of_expr_wrappers(expr) else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    matches!(prop.sym.as_ref(), "from" | "of") && is_node_readable_class_ref(&member.obj)
}

fn is_node_readable_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    if is_node_readable_static_factory(expr) {
        return true;
    }
    matches!(
        crate::lower_types::infer_type_from_expr(strip_for_of_expr_wrappers(expr), ctx),
        Type::Named(name) if name == "Readable"
    )
}

fn iterator_return_call(iter_id: LocalId, needs_await: bool) -> Expr {
    let call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "return".to_string(),
        }),
        args: vec![],
        type_args: vec![],
    };
    if needs_await {
        Expr::Await(Box::new(call))
    } else {
        call
    }
}

fn insert_iterator_return_before_abrupts(
    stmts: &mut Vec<Stmt>,
    iter_id: LocalId,
    needs_await: bool,
) {
    let mut rewritten = Vec::with_capacity(stmts.len());
    for stmt in stmts.drain(..) {
        match stmt {
            Stmt::Break => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Break);
            }
            Stmt::LabeledBreak(label) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::LabeledBreak(label));
            }
            Stmt::Return(value) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Return(value));
            }
            Stmt::Throw(expr) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Throw(expr));
            }
            Stmt::If {
                condition,
                mut then_branch,
                mut else_branch,
            } => {
                insert_iterator_return_before_abrupts(&mut then_branch, iter_id, needs_await);
                if let Some(else_stmts) = else_branch.as_mut() {
                    insert_iterator_return_before_abrupts(else_stmts, iter_id, needs_await);
                }
                rewritten.push(Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                });
            }
            other => rewritten.push(other),
        }
    }
    *stmts = rewritten;
}

pub fn lower_body_stmt(ctx: &mut LoweringContext, stmt: &ast::Stmt) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match stmt {
        ast::Stmt::Return(ret) => {
            let value = ret.arg.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            result.push(Stmt::Return(value));
        }
        ast::Stmt::If(if_stmt) => {
            result.extend(predeclare_implicit_assignment_targets(ctx, &if_stmt.test));
            // #2277: `typeof x === "string"` narrowing — see typeof_narrow.rs.
            let stmt = typeof_narrow::lower_if_with_narrowing(ctx, if_stmt, lower_body_stmt)?;
            result.push(stmt);
        }
        ast::Stmt::Block(block) => {
            // Bare block: introduce a lexical scope so let/const shadow
            // without leaking into the enclosing scope.
            result.extend(lower_block_stmt_scoped(ctx, block)?);
        }
        ast::Stmt::Expr(expr_stmt) => {
            result.extend(predeclare_implicit_assignment_targets(ctx, &expr_stmt.expr));
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
                let stmts = lower_var_decl_with_destructuring(ctx, decl, mutable, is_var)?;
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
                        let native_class = match (mod_name.as_str(), method.as_str()) {
                            ("net", "createConnection" | "connect") => Some(("net", "Socket")),
                            ("tls", "connect") => Some(("net", "Socket")),
                            ("tls", "createServer" | "Server") => Some(("tls", "Server")),
                            ("net", "Socket") => Some(("net", "Socket")),
                            ("net", "Server") => Some(("net", "Server")),
                            _ => None,
                        };
                        if let Some((m, c)) = native_class {
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
                let stmts = lower_var_decl_with_destructuring(ctx, decl, false, false)?;
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
                if let Some(extends_expr) = &class.extends_expr {
                    result.push(Stmt::Expr(Expr::RegisterClassParentDynamic {
                        class_name: class.name.clone(),
                        parent_expr: extends_expr.clone(),
                    }));
                }
                for member in &class.computed_members {
                    result.push(Stmt::Expr(class_computed_member_registration_expr(
                        &class.name,
                        member,
                    )));
                }
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
                nested_fn_decl::lower_nested_fn_decl(ctx, fn_decl, &mut result)?;
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
            // #2383: a labeled *block* — `a: { ... break a; ... }` — exits the
            // block via `break a` (valid JS/TS; heavily used by minified React).
            // It is NOT a loop, so the loop-based labeled-break codegen has
            // nothing to bind the label to. Desugar to a labeled run-once
            // do-while: `a: do { ... } while (false)`. The do-while's exit block
            // becomes the labeled-break target, the body runs exactly once, and
            // the `while (false)` falls straight through to the exit. `continue
            // a` against a block label is a JS early SyntaxError, so it never
            // reaches here.
            if let ast::Stmt::Block(block) = &*labeled_stmt.body {
                let body = lower_block_stmt_scoped(ctx, block)?;
                result.push(Stmt::Labeled {
                    label,
                    body: Box::new(Stmt::DoWhile {
                        body,
                        condition: Expr::Bool(false),
                    }),
                });
                return Ok(result);
            }
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
                                if let Some(init_ast) = decl.init.as_ref() {
                                    result.extend(predeclare_implicit_assignment_targets(
                                        ctx, init_ast,
                                    ));
                                }
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
                                if let Some(init_ast) = decl.init.as_ref() {
                                    result.extend(predeclare_implicit_assignment_targets(
                                        ctx, init_ast,
                                    ));
                                }
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
                                if let Some(init_ast) = decl.init.as_ref() {
                                    result.extend(predeclare_implicit_assignment_targets(
                                        ctx, init_ast,
                                    ));
                                }
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
                        result.extend(predeclare_implicit_assignment_targets(ctx, expr));
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
            let previous_optional_require_try_depth = ctx.optional_require_try_depth;
            ctx.optional_require_try_depth = previous_optional_require_try_depth.saturating_add(1);
            let body_result = lower_block_stmt_scoped(ctx, &try_stmt.block);
            ctx.optional_require_try_depth = previous_optional_require_try_depth;
            let body = body_result?;

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
                // #1646: peel `as T` / `!` / `as const` / parens so
                // `for await (const v of rs as any)` (a common Web-Streams
                // idiom — the WHATWG ReadableStream async-iterator isn't in
                // the lib.dom.d.ts types Perry sees) is still recognised as a
                // ReadableStream and lowered to the getReader/read loop below.
                let stream_source = web_readable_stream_values_receiver(&for_of_stmt.right)
                    .unwrap_or(&for_of_stmt.right);
                let mut iter_inner: &ast::Expr = stream_source;
                loop {
                    iter_inner = match iter_inner {
                        ast::Expr::TsAs(x) => &x.expr,
                        ast::Expr::TsNonNull(x) => &x.expr,
                        ast::Expr::TsConstAssertion(x) => &x.expr,
                        ast::Expr::Paren(x) => &x.expr,
                        _ => break,
                    };
                }
                let is_readable_stream = match iter_inner {
                    ast::Expr::Ident(_) | ast::Expr::New(_) => {
                        is_web_readable_stream_expr(ctx, iter_inner)
                    }
                    // #1670: `for await (const c of res.body)` — the stream
                    // arrives as a bare `Member` (Any-typed). Recognise
                    // `<obj>.body` on a Response/Request and `<ts>.readable` on
                    // a TransformStream, mirroring `var_decl`'s native-instance
                    // property mapping for typed-local binds.
                    ast::Expr::Member(member) => {
                        if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(prop_ident)) =
                            (member.obj.as_ref(), &member.prop)
                        {
                            let prop = prop_ident.sym.as_ref();
                            let class = ctx
                                .lookup_native_instance(obj_ident.sym.as_ref())
                                .map(|(_, c)| c);
                            matches!(
                                (prop, class),
                                ("body", Some("Response"))
                                    | ("body", Some("Request"))
                                    | ("readable", Some("TransformStream"))
                            )
                        } else {
                            false
                        }
                    }
                    _ => false,
                };

                if is_readable_stream {
                    let scope_mark = ctx.push_block_scope();
                    let stream_expr = lower_expr(ctx, stream_source)?;

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

            let is_timer_promises_interval_call = for_of_stmt.is_await
                && if let ast::Expr::Call(call) = &*for_of_stmt.right {
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        match &**callee_expr {
                            ast::Expr::Ident(ident) => {
                                ctx.lookup_native_module(ident.sym.as_ref()).is_some_and(
                                    |(module, method)| {
                                        module.strip_prefix("node:").unwrap_or(module)
                                            == "timers/promises"
                                            && method == Some("setInterval")
                                    },
                                ) || ctx
                                    .lookup_imported_func(ident.sym.as_ref())
                                    .is_some_and(|imported| imported == "setInterval")
                            }
                            ast::Expr::Member(member) => {
                                if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                                    (&*member.obj, &member.prop)
                                {
                                    prop.sym.as_ref() == "setInterval"
                                        && ctx.lookup_local(obj.sym.as_ref()).is_none()
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

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

            let is_node_readable_for_await =
                for_of_stmt.is_await && is_node_readable_for_await_target(ctx, &for_of_stmt.right);
            let is_filehandle_readlines_for_await = for_of_stmt.is_await
                && is_filehandle_readlines_for_await_target(ctx, &for_of_stmt.right);

            if is_generator_call
                || iter_from_class.is_some()
                || is_timer_promises_interval_call
                || is_node_readable_for_await
                || is_filehandle_readlines_for_await
            {
                let scope_mark = ctx.push_block_scope();
                let iter_expr_raw = lower_expr(ctx, &for_of_stmt.right)?;
                let iter_expr = if let Some(iter_fn_id) = iter_from_class {
                    Expr::Call {
                        callee: Box::new(Expr::FuncRef(iter_fn_id)),
                        args: vec![iter_expr_raw],
                        type_args: vec![],
                    }
                } else if is_filehandle_readlines_for_await {
                    async_iterator_method_call(iter_expr_raw)
                } else if is_node_readable_for_await {
                    Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(iter_expr_raw),
                            property: "iterator".to_string(),
                        }),
                        args: vec![],
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
                let mut user_body = lower_body_stmt(ctx, &for_of_stmt.body)?;
                if is_node_readable_for_await || is_filehandle_readlines_for_await {
                    insert_iterator_return_before_abrupts(&mut user_body, iter_id, needs_await);
                }
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
                    | "Float16Array" | "Float32Array" | "Float64Array"
                )
            );
            // #321: when the receiver's static type can NOT be proven to
            // be a plain Array — an `any`-typed Map/Set (effect's
            // `for (const [tag, s] of that.unsafeMap)`, where `that` is an
            // untyped function parameter), an object carrying a custom
            // `[Symbol.iterator]`, etc. — the index-based loop below would
            // read `.length` off the wrong handle (Map/Set → 0) and
            // iterate zero times. Route those through the runtime default
            // iterator (`js_for_of_to_array`). Strings, typed arrays, and
            // statically-proven Map/Set/Array keep their existing fast
            // paths. Mirrors the module-init path in
            // `lower::stmt_loops::lower_stmt_for_of`.
            let proven_array = match &iterable_type {
                Some(Type::Array(_)) => true,
                Some(Type::Generic { base, .. }) => base == "Array",
                _ => false,
            };
            let needs_runtime_iterator = !is_string_iter
                && !is_iterable_map
                && !is_iterable_set
                && !is_iterable_typed_array
                && !proven_array;
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
            } else if needs_runtime_iterator {
                Expr::ForOfToArray(Box::new(arr_expr))
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
pub fn find_native_return_in_stmts(
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
