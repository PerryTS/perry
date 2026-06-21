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

/// `let/const x = new FinalizationRegistry(...)` bindings into the lowering
/// context. This is used by `obj.method()` lowering to recognise these instances
/// without requiring type inference (Perry's existing var-decl type inference
/// doesn't extend to WeakRef/FinalizationRegistry).
pub(crate) fn pre_scan_weakref_locals(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    fn classify_new(new_expr: &ast::NewExpr) -> Option<&'static str> {
        if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
            match ident.sym.as_ref() {
                "WeakRef" => Some("WeakRef"),
                "FinalizationRegistry" => Some("FinalizationRegistry"),
                "WeakMap" => Some("WeakMap"),
                "WeakSet" => Some("WeakSet"),
                "Proxy" => Some("Proxy"),
                _ => None,
            }
        } else {
            None
        }
    }
    fn unwrap_init(mut e: &ast::Expr) -> &ast::Expr {
        loop {
            match e {
                ast::Expr::TsAs(ts_as) => e = &ts_as.expr,
                ast::Expr::TsTypeAssertion(ta) => e = &ta.expr,
                ast::Expr::TsNonNull(nn) => e = &nn.expr,
                ast::Expr::TsConstAssertion(ca) => e = &ca.expr,
                ast::Expr::Paren(p) => e = &p.expr,
                _ => break,
            }
        }
        e
    }
    fn record_var(decl: &ast::VarDeclarator, ctx: &mut LoweringContext) {
        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, decl.init.as_ref()) {
            let init_unwrapped = unwrap_init(init);
            if let ast::Expr::New(new_expr) = init_unwrapped {
                let name = ident.id.sym.to_string();
                match classify_new(new_expr) {
                    Some("WeakRef") => {
                        ctx.weakref_locals.insert(name);
                    }
                    Some("FinalizationRegistry") => {
                        ctx.finreg_locals.insert(name);
                    }
                    Some("WeakMap") => {
                        ctx.weakmap_locals.insert(name);
                    }
                    Some("WeakSet") => {
                        ctx.weakset_locals.insert(name);
                    }
                    Some("Proxy") => {
                        ctx.proxy_locals.insert(name);
                    }
                    _ => {}
                }
            } else if let ast::Expr::Member(member) = init_unwrapped {
                // #1750: `const w = path.win32` / `const p = path.posix`.
                // Record the alias so `w.normalize(...)` later dispatches like
                // `path.win32.normalize(...)`. The root ident is stored
                // unresolved; the `path` check is deferred to call lowering.
                if let (ast::Expr::Ident(root), ast::MemberProp::Ident(sub_prop)) =
                    (member.obj.as_ref(), &member.prop)
                {
                    let sub = sub_prop.sym.as_ref();
                    if sub == "win32" || sub == "posix" {
                        ctx.register_subns_path_alias(
                            ident.id.sym.to_string(),
                            root.sym.to_string(),
                            sub.to_string(),
                        );
                    }
                }
                // #3144: `const m = [].map` / `const s = "".slice` /
                // `const f = Array.prototype.filter` — track the local so a
                // later `m.call(arr, ...)` / `m.apply(arr, [...])` rewrites to a
                // direct call. Uses the same receiver rule as the existing
                // `.call`/`.apply` builtin-prototype rewrite.
                if let Some(method) =
                    crate::lower::expr_call::intrinsics::as_builtin_proto_method_ref(
                        ctx,
                        init_unwrapped,
                    )
                {
                    ctx.builtin_proto_method_locals
                        .insert(ident.id.sym.to_string(), method);
                }
            }
        }
    }
    fn walk_stmt(stmt: &ast::Stmt, ctx: &mut LoweringContext) {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    record_var(decl, ctx);
                }
            }
            ast::Stmt::Decl(ast::Decl::Using(using_decl)) => {
                for decl in &using_decl.decls {
                    record_var(decl, ctx);
                }
            }
            // Function declarations — descend into the body so `const
            // ref = new WeakRef(x)` inside a function is still tracked
            // and `ref.deref()` lowers to `Expr::WeakRefDeref` instead
            // of falling through to the generic method dispatch.
            ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
                if let Some(body) = &fn_decl.function.body {
                    for s in &body.stmts {
                        walk_stmt(s, ctx);
                    }
                }
            }
            ast::Stmt::Block(block) => {
                for s in &block.stmts {
                    walk_stmt(s, ctx);
                }
            }
            ast::Stmt::If(if_stmt) => {
                walk_stmt(&if_stmt.cons, ctx);
                if let Some(alt) = &if_stmt.alt {
                    walk_stmt(alt, ctx);
                }
            }
            ast::Stmt::While(w) => walk_stmt(&w.body, ctx),
            ast::Stmt::DoWhile(w) => walk_stmt(&w.body, ctx),
            ast::Stmt::For(f) => {
                if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                    for decl in &vd.decls {
                        record_var(decl, ctx);
                    }
                }
                walk_stmt(&f.body, ctx);
            }
            ast::Stmt::ForIn(f) => walk_stmt(&f.body, ctx),
            ast::Stmt::ForOf(f) => walk_stmt(&f.body, ctx),
            ast::Stmt::Try(t) => {
                for s in &t.block.stmts {
                    walk_stmt(s, ctx);
                }
                if let Some(catch) = &t.handler {
                    for s in &catch.body.stmts {
                        walk_stmt(s, ctx);
                    }
                }
                if let Some(finalizer) = &t.finalizer {
                    for s in &finalizer.stmts {
                        walk_stmt(s, ctx);
                    }
                }
            }
            ast::Stmt::Switch(s) => {
                for case in &s.cases {
                    for s in &case.cons {
                        walk_stmt(s, ctx);
                    }
                }
            }
            _ => {}
        }
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => walk_stmt(stmt, ctx),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(var_decl) = &export_decl.decl {
                    for decl in &var_decl.decls {
                        record_var(decl, ctx);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Pre-scan top-level function declarations for the standard TypeScript
/// mixin pattern:
///
///   function Foo<T extends Constructor>(Base: T) {
///     return class extends Base {
///       greet(): string { return "..."; }
///     };
///   }
///
/// Records the function name → (base_param_name, class_ast) so that calls
/// like `const Mixed = Foo(BaseClass)` can synthesize a real class.
pub(crate) fn pre_scan_mixin_functions(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    fn try_record_fn(fn_decl: &ast::FnDecl, ctx: &mut LoweringContext) {
        if fn_decl.function.params.len() != 1 {
            return;
        }
        let param_name = match &fn_decl.function.params[0].pat {
            ast::Pat::Ident(ident) => ident.id.sym.to_string(),
            _ => return,
        };
        let body = match &fn_decl.function.body {
            Some(b) => b,
            None => return,
        };
        if body.stmts.len() != 1 {
            return;
        }
        let return_arg = match &body.stmts[0] {
            ast::Stmt::Return(r) => match &r.arg {
                Some(arg) => arg.as_ref(),
                None => return,
            },
            _ => return,
        };
        let mut e = return_arg;
        loop {
            match e {
                ast::Expr::Paren(p) => e = &p.expr,
                _ => break,
            }
        }
        let class_expr = match e {
            ast::Expr::Class(ce) => ce,
            _ => return,
        };
        let extends_param = match &class_expr.class.super_class {
            Some(sc) => {
                if let ast::Expr::Ident(ident) = sc.as_ref() {
                    ident.sym.as_ref() == param_name
                } else {
                    false
                }
            }
            None => false,
        };
        if !extends_param {
            return;
        }
        let fn_name = fn_decl.ident.sym.to_string();
        ctx.mixin_funcs
            .insert(fn_name, (param_name, Box::new((*class_expr.class).clone())));
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => {
                try_record_fn(fn_decl, ctx);
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                if let ast::Decl::Fn(fn_decl) = &export.decl {
                    try_record_fn(fn_decl, ctx);
                }
            }
            _ => {}
        }
    }
}

/// Cross-function native-instance propagation — the `("ws","Client")` upgrade
/// handle delivered to `server.on("upgrade", (req, wsId, head) => …)` only
/// dispatches its `.send()/.on()/.close()` to the `js_ws_*_client_i64` runtime
/// when codegen statically knows the receiver is the upgrade `Client`. That
/// class is tagged at the upgrade callback's parameter, but is NOT propagated
/// when `wsId` is handed to a helper (`handleConnection(req, wsId)`): inside the
/// callee the parameter is plain/`any`, the `class_filter: Some("Client")` row
/// no longer matches, and `wsId.send(...)` silently lowers to a generic no-op —
/// the frame is dropped, no error thrown.
///
/// This pre-pass runs BEFORE any function body is lowered. It seeds from the
/// HTTP-upgrade idiom (gated on the receiver being a `createServer(...)` result,
/// mirroring the runtime `("http","HttpServer")` check) and follows the handle
/// through subsequent `userFn(…, wsId, …)` calls — transitively — recording a
/// `(callee_name, param_index) -> (module, class)` hint in
/// `ctx.param_native_hints`. `lower_fn_decl` consults the hint to register the
/// otherwise-untyped parameter as a native instance, so the callee's
/// `wsId.send(...)` dispatches to `js_ws_send_client_i64` exactly like the
/// inline call would.
pub(crate) fn pre_scan_cross_fn_native_params(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    // Top-level user functions: name -> ordered param names (None marks a
    // destructuring/rest param that can't be a simple handle binding), and
    // name -> body block to follow the handle into.
    let mut fn_params: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let mut fn_bodies: HashMap<String, &ast::BlockStmt> = HashMap::new();
    for item in &ast_module.body {
        let fd = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fd))) => fd,
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => {
                if let ast::Decl::Fn(fd) = &e.decl {
                    fd
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        let Some(body) = &fd.function.body else {
            continue;
        };
        let name = fd.ident.sym.to_string();
        let names: Vec<Option<String>> = fd
            .function
            .params
            .iter()
            .map(|p| cross_fn_pat_name(&p.pat))
            .collect();
        fn_params.insert(name.clone(), names);
        fn_bodies.insert(name, body);
    }
    if fn_params.is_empty() {
        return;
    }

    // Idents bound to a `createServer(...)` / `createSecureServer(...)` result
    // — the static proxy for a `("http","HttpServer")` native instance.
    let mut server_idents: HashSet<String> = HashSet::new();
    collect_server_idents_in_module(ast_module, &mut server_idents);

    // Seed work items from each gated upgrade handler.
    let mut work: Vec<(Vec<&ast::CallExpr>, HashMap<String, (String, String)>)> = Vec::new();
    let mut all_calls: Vec<&ast::CallExpr> = Vec::new();
    collect_calls_in_module(ast_module, &mut all_calls);
    for call in &all_calls {
        if let Some((ws_id, handler)) = upgrade_handler_ws_id(call, &server_idents) {
            let mut calls = Vec::new();
            collect_calls_in_callback_body(handler, &mut calls);
            let mut tainted = HashMap::new();
            tainted.insert(ws_id, ("ws".to_string(), "Client".to_string()));
            work.push((calls, tainted));
        }
    }

    // Fixpoint propagation. `applied` dedups (callee, param, module, class) so
    // mutual recursion / repeated call sites terminate.
    let mut applied: HashSet<(String, usize, String, String)> = HashSet::new();
    let mut guard = 0usize;
    while let Some((calls, tainted)) = work.pop() {
        guard += 1;
        if guard > 50_000 {
            break; // pathological-input backstop
        }
        for call in calls {
            let Some(callee) = cross_fn_call_ident(call) else {
                continue;
            };
            let Some(params) = fn_params.get(&callee) else {
                continue;
            };
            for (i, arg) in call.args.iter().enumerate() {
                if arg.spread.is_some() {
                    continue;
                }
                let ast::Expr::Ident(id) = arg.expr.as_ref() else {
                    continue;
                };
                let Some((module, class)) = tainted.get(id.sym.as_ref()) else {
                    continue;
                };
                let Some(Some(param_name)) = params.get(i) else {
                    continue;
                };
                let key = (callee.clone(), i, module.clone(), class.clone());
                if !applied.insert(key) {
                    continue;
                }
                ctx.param_native_hints
                    .insert((callee.clone(), i), (module.clone(), class.clone()));
                // Follow the handle into the callee: its param[i] is now tainted.
                if let Some(body) = fn_bodies.get(&callee) {
                    let mut next_calls = Vec::new();
                    collect_calls_in_stmts(&body.stmts, &mut next_calls);
                    let mut next = HashMap::new();
                    next.insert(param_name.clone(), (module.clone(), class.clone()));
                    work.push((next_calls, next));
                }
            }
        }
    }
}

/// Plain-ident name of a pattern, else `None` (destructuring / rest).
fn cross_fn_pat_name(pat: &ast::Pat) -> Option<String> {
    match pat {
        ast::Pat::Ident(i) => Some(i.id.sym.to_string()),
        _ => None,
    }
}

/// `g(...)` where the callee is a bare identifier — returns `g`.
fn cross_fn_call_ident(call: &ast::CallExpr) -> Option<String> {
    match &call.callee {
        ast::Callee::Expr(e) => match e.as_ref() {
            ast::Expr::Ident(i) => Some(i.sym.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// `X.on("upgrade", (req, wsId, head) => …)` / `.addListener` / `.once`, where
/// `X` is a `createServer(...)` result. Returns the `wsId` param name and the
/// handler expression (whose body carries the handle in scope).
fn upgrade_handler_ws_id<'a>(
    call: &'a ast::CallExpr,
    server_idents: &HashSet<String>,
) -> Option<(String, &'a ast::Expr)> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let ast::Expr::Member(member) = callee.as_ref() else {
        return None;
    };
    // Receiver must be a known http server ident.
    let ast::Expr::Ident(obj) = member.obj.as_ref() else {
        return None;
    };
    if !server_idents.contains(obj.sym.as_ref()) {
        return None;
    }
    let method = match &member.prop {
        ast::MemberProp::Ident(i) => i.sym.as_ref(),
        _ => return None,
    };
    if method != "on" && method != "addListener" && method != "once" {
        return None;
    }
    let event = call.args.first()?;
    if event.spread.is_some() {
        return None;
    }
    let is_upgrade = matches!(
        event.expr.as_ref(),
        ast::Expr::Lit(ast::Lit::Str(s)) if s.value.as_str() == Some("upgrade")
    );
    if !is_upgrade {
        return None;
    }
    let handler = call.args.get(1)?;
    if handler.spread.is_some() {
        return None;
    }
    let ws_id = match handler.expr.as_ref() {
        ast::Expr::Arrow(a) => a.params.get(1).and_then(cross_fn_pat_name),
        ast::Expr::Fn(f) => f
            .function
            .params
            .get(1)
            .and_then(|p| cross_fn_pat_name(&p.pat)),
        _ => None,
    }?;
    Some((ws_id, handler.expr.as_ref()))
}

/// Record idents bound to a `createServer(...)` / `createSecureServer(...)`
/// call (bare or `http.`/`https.`/`http2.`-qualified) anywhere in the module.
fn collect_server_idents_in_module(ast_module: &ast::Module, out: &mut HashSet<String>) {
    fn is_create_server_call(expr: &ast::Expr) -> bool {
        let mut e = expr;
        loop {
            match e {
                ast::Expr::Paren(p) => e = &p.expr,
                ast::Expr::TsAs(t) => e = &t.expr,
                ast::Expr::TsNonNull(t) => e = &t.expr,
                // `createServer(...).listen(...)` still binds the server to the
                // chained receiver, but the bound ident is the chain result, not
                // the server — so only treat a *direct* call result as a server.
                _ => break,
            }
        }
        let ast::Expr::Call(call) = e else {
            return false;
        };
        let ast::Callee::Expr(callee) = &call.callee else {
            return false;
        };
        let name = match callee.as_ref() {
            ast::Expr::Ident(i) => i.sym.to_string(),
            ast::Expr::Member(m) => match &m.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return false,
            },
            _ => return false,
        };
        name == "createServer" || name == "createSecureServer"
    }
    fn record_decl(decl: &ast::VarDeclarator, out: &mut HashSet<String>) {
        if let (ast::Pat::Ident(id), Some(init)) = (&decl.name, decl.init.as_ref()) {
            if is_create_server_call(init) {
                out.insert(id.id.sym.to_string());
            }
        }
    }
    fn walk_stmt(stmt: &ast::Stmt, out: &mut HashSet<String>) {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(v)) => {
                for d in &v.decls {
                    record_decl(d, out);
                }
            }
            ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                if let Some(b) = &f.function.body {
                    for s in &b.stmts {
                        walk_stmt(s, out);
                    }
                }
            }
            ast::Stmt::Expr(e) => walk_expr(&e.expr, out),
            ast::Stmt::Block(b) => {
                for s in &b.stmts {
                    walk_stmt(s, out);
                }
            }
            ast::Stmt::If(i) => {
                walk_stmt(&i.cons, out);
                if let Some(alt) = &i.alt {
                    walk_stmt(alt, out);
                }
            }
            ast::Stmt::While(w) => walk_stmt(&w.body, out),
            ast::Stmt::DoWhile(w) => walk_stmt(&w.body, out),
            ast::Stmt::For(f) => {
                if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                    for d in &vd.decls {
                        record_decl(d, out);
                    }
                }
                walk_stmt(&f.body, out);
            }
            ast::Stmt::ForIn(f) => walk_stmt(&f.body, out),
            ast::Stmt::ForOf(f) => walk_stmt(&f.body, out),
            ast::Stmt::Try(t) => {
                for s in &t.block.stmts {
                    walk_stmt(s, out);
                }
                if let Some(h) = &t.handler {
                    for s in &h.body.stmts {
                        walk_stmt(s, out);
                    }
                }
                if let Some(f) = &t.finalizer {
                    for s in &f.stmts {
                        walk_stmt(s, out);
                    }
                }
            }
            ast::Stmt::Switch(s) => {
                for c in &s.cases {
                    for s in &c.cons {
                        walk_stmt(s, out);
                    }
                }
            }
            ast::Stmt::Return(r) => {
                if let Some(a) = &r.arg {
                    walk_expr(a, out);
                }
            }
            _ => {}
        }
    }
    // Descend into callback bodies (e.g. a server created inside `main()` or a
    // `.listen(0, () => { const server = createServer(...) })`).
    fn walk_expr(expr: &ast::Expr, out: &mut HashSet<String>) {
        match expr {
            ast::Expr::Call(c) => {
                for a in &c.args {
                    walk_expr(&a.expr, out);
                }
            }
            ast::Expr::Arrow(a) => match a.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    for s in &b.stmts {
                        walk_stmt(s, out);
                    }
                }
                ast::BlockStmtOrExpr::Expr(e) => walk_expr(e, out),
            },
            ast::Expr::Fn(f) => {
                if let Some(b) = &f.function.body {
                    for s in &b.stmts {
                        walk_stmt(s, out);
                    }
                }
            }
            ast::Expr::Paren(p) => walk_expr(&p.expr, out),
            _ => {}
        }
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(s) => walk_stmt(s, out),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => {
                if let ast::Decl::Var(v) = &e.decl {
                    for d in &v.decls {
                        record_decl(d, out);
                    }
                } else if let ast::Decl::Fn(f) = &e.decl {
                    if let Some(b) = &f.function.body {
                        for s in &b.stmts {
                            walk_stmt(s, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Collect every `CallExpr` reachable in the module (statement position and
/// nested closures) — used to find the upgrade-handler seed.
fn collect_calls_in_module<'a>(ast_module: &'a ast::Module, out: &mut Vec<&'a ast::CallExpr>) {
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(s) => collect_calls_in_stmt(s, out),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => match &e.decl {
                ast::Decl::Var(v) => {
                    for d in &v.decls {
                        if let Some(init) = &d.init {
                            collect_calls_in_expr(init, out);
                        }
                    }
                }
                ast::Decl::Fn(f) => {
                    if let Some(b) = &f.function.body {
                        collect_calls_in_stmts(&b.stmts, out);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// Collect calls in a callback expression's body (arrow or function).
fn collect_calls_in_callback_body<'a>(handler: &'a ast::Expr, out: &mut Vec<&'a ast::CallExpr>) {
    match handler {
        ast::Expr::Arrow(a) => match a.body.as_ref() {
            ast::BlockStmtOrExpr::BlockStmt(b) => collect_calls_in_stmts(&b.stmts, out),
            ast::BlockStmtOrExpr::Expr(e) => collect_calls_in_expr(e, out),
        },
        ast::Expr::Fn(f) => {
            if let Some(b) = &f.function.body {
                collect_calls_in_stmts(&b.stmts, out);
            }
        }
        _ => {}
    }
}

fn collect_calls_in_stmts<'a>(stmts: &'a [ast::Stmt], out: &mut Vec<&'a ast::CallExpr>) {
    for s in stmts {
        collect_calls_in_stmt(s, out);
    }
}

fn collect_calls_in_stmt<'a>(stmt: &'a ast::Stmt, out: &mut Vec<&'a ast::CallExpr>) {
    match stmt {
        ast::Stmt::Expr(e) => collect_calls_in_expr(&e.expr, out),
        ast::Stmt::Return(r) => {
            if let Some(a) = &r.arg {
                collect_calls_in_expr(a, out);
            }
        }
        ast::Stmt::Decl(ast::Decl::Var(v)) => {
            for d in &v.decls {
                if let Some(init) = &d.init {
                    collect_calls_in_expr(init, out);
                }
            }
        }
        ast::Stmt::Decl(ast::Decl::Fn(f)) => {
            if let Some(b) = &f.function.body {
                collect_calls_in_stmts(&b.stmts, out);
            }
        }
        ast::Stmt::Block(b) => collect_calls_in_stmts(&b.stmts, out),
        ast::Stmt::If(i) => {
            collect_calls_in_expr(&i.test, out);
            collect_calls_in_stmt(&i.cons, out);
            if let Some(alt) = &i.alt {
                collect_calls_in_stmt(alt, out);
            }
        }
        ast::Stmt::While(w) => {
            collect_calls_in_expr(&w.test, out);
            collect_calls_in_stmt(&w.body, out);
        }
        ast::Stmt::DoWhile(w) => {
            collect_calls_in_stmt(&w.body, out);
            collect_calls_in_expr(&w.test, out);
        }
        ast::Stmt::For(f) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                for d in &vd.decls {
                    if let Some(init) = &d.init {
                        collect_calls_in_expr(init, out);
                    }
                }
            } else if let Some(ast::VarDeclOrExpr::Expr(e)) = &f.init {
                collect_calls_in_expr(e, out);
            }
            if let Some(t) = &f.test {
                collect_calls_in_expr(t, out);
            }
            if let Some(u) = &f.update {
                collect_calls_in_expr(u, out);
            }
            collect_calls_in_stmt(&f.body, out);
        }
        ast::Stmt::ForIn(f) => {
            collect_calls_in_expr(&f.right, out);
            collect_calls_in_stmt(&f.body, out);
        }
        ast::Stmt::ForOf(f) => {
            collect_calls_in_expr(&f.right, out);
            collect_calls_in_stmt(&f.body, out);
        }
        ast::Stmt::Throw(t) => collect_calls_in_expr(&t.arg, out),
        ast::Stmt::Try(t) => {
            collect_calls_in_stmts(&t.block.stmts, out);
            if let Some(h) = &t.handler {
                collect_calls_in_stmts(&h.body.stmts, out);
            }
            if let Some(f) = &t.finalizer {
                collect_calls_in_stmts(&f.stmts, out);
            }
        }
        ast::Stmt::Switch(s) => {
            collect_calls_in_expr(&s.discriminant, out);
            for c in &s.cases {
                if let Some(t) = &c.test {
                    collect_calls_in_expr(t, out);
                }
                collect_calls_in_stmts(&c.cons, out);
            }
        }
        ast::Stmt::Labeled(l) => collect_calls_in_stmt(&l.body, out),
        _ => {}
    }
}

fn collect_calls_in_expr<'a>(expr: &'a ast::Expr, out: &mut Vec<&'a ast::CallExpr>) {
    match expr {
        ast::Expr::Call(c) => {
            out.push(c);
            if let ast::Callee::Expr(e) = &c.callee {
                collect_calls_in_expr(e, out);
            }
            for a in &c.args {
                collect_calls_in_expr(&a.expr, out);
            }
        }
        ast::Expr::New(n) => {
            collect_calls_in_expr(&n.callee, out);
            if let Some(args) = &n.args {
                for a in args {
                    collect_calls_in_expr(&a.expr, out);
                }
            }
        }
        ast::Expr::Member(m) => {
            collect_calls_in_expr(&m.obj, out);
            if let ast::MemberProp::Computed(c) = &m.prop {
                collect_calls_in_expr(&c.expr, out);
            }
        }
        ast::Expr::Bin(b) => {
            collect_calls_in_expr(&b.left, out);
            collect_calls_in_expr(&b.right, out);
        }
        ast::Expr::Unary(u) => collect_calls_in_expr(&u.arg, out),
        ast::Expr::Update(u) => collect_calls_in_expr(&u.arg, out),
        // Assignment LHS rarely carries a handle-passing call; follow the RHS.
        ast::Expr::Assign(a) => collect_calls_in_expr(&a.right, out),
        ast::Expr::Cond(c) => {
            collect_calls_in_expr(&c.test, out);
            collect_calls_in_expr(&c.cons, out);
            collect_calls_in_expr(&c.alt, out);
        }
        ast::Expr::Paren(p) => collect_calls_in_expr(&p.expr, out),
        ast::Expr::Seq(s) => {
            for e in &s.exprs {
                collect_calls_in_expr(e, out);
            }
        }
        ast::Expr::Await(a) => collect_calls_in_expr(&a.arg, out),
        ast::Expr::Yield(y) => {
            if let Some(a) = &y.arg {
                collect_calls_in_expr(a, out);
            }
        }
        ast::Expr::Tpl(t) => {
            for e in &t.exprs {
                collect_calls_in_expr(e, out);
            }
        }
        ast::Expr::TaggedTpl(t) => {
            collect_calls_in_expr(&t.tag, out);
            for e in &t.tpl.exprs {
                collect_calls_in_expr(e, out);
            }
        }
        ast::Expr::Array(a) => {
            for el in a.elems.iter().flatten() {
                collect_calls_in_expr(&el.expr, out);
            }
        }
        ast::Expr::Object(o) => {
            for p in &o.props {
                match p {
                    ast::PropOrSpread::Spread(s) => collect_calls_in_expr(&s.expr, out),
                    ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                        ast::Prop::KeyValue(kv) => collect_calls_in_expr(&kv.value, out),
                        ast::Prop::Method(m) => {
                            if let Some(b) = &m.function.body {
                                collect_calls_in_stmts(&b.stmts, out);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        ast::Expr::Arrow(a) => match a.body.as_ref() {
            ast::BlockStmtOrExpr::BlockStmt(b) => collect_calls_in_stmts(&b.stmts, out),
            ast::BlockStmtOrExpr::Expr(e) => collect_calls_in_expr(e, out),
        },
        ast::Expr::Fn(f) => {
            if let Some(b) = &f.function.body {
                collect_calls_in_stmts(&b.stmts, out);
            }
        }
        ast::Expr::OptChain(o) => match o.base.as_ref() {
            ast::OptChainBase::Member(m) => {
                collect_calls_in_expr(&m.obj, out);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    collect_calls_in_expr(&c.expr, out);
                }
            }
            ast::OptChainBase::Call(c) => {
                collect_calls_in_expr(&c.callee, out);
                for a in &c.args {
                    collect_calls_in_expr(&a.expr, out);
                }
            }
        },
        ast::Expr::TsAs(t) => collect_calls_in_expr(&t.expr, out),
        ast::Expr::TsNonNull(t) => collect_calls_in_expr(&t.expr, out),
        ast::Expr::TsTypeAssertion(t) => collect_calls_in_expr(&t.expr, out),
        ast::Expr::TsConstAssertion(t) => collect_calls_in_expr(&t.expr, out),
        ast::Expr::TsInstantiation(t) => collect_calls_in_expr(&t.expr, out),
        _ => {}
    }
}

/// #4510: pre-register module-level `enum` declarations so a forward
/// reference (an enum used in a function body or earlier statement, before its
/// textual declaration) resolves instead of falling through to the
/// "unknown identifier → GlobalGet(0) → 0" silent-miscompile path. Enum
/// bindings are module-scoped in TypeScript, so a function declared above the
/// `enum` may legally compare against `Enum.Member`. Member values are computed
/// purely (`compute_enum_members`), so registering here produces the same id +
/// values the real declaration site would, and `lower_enum_decl` reuses this
/// registration rather than minting a duplicate.
pub(crate) fn pre_register_module_enums(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    for item in &ast_module.body {
        let enum_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsEnum(e))) => Some(e),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                if let ast::Decl::TsEnum(e) = &export.decl {
                    Some(e)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(e) = enum_decl {
            // `declare enum` / `const enum` ambient declarations still carry
            // member values usable as constants; register them too.
            let name = e.id.sym.to_string();
            if ctx.lookup_enum(&name).is_some() {
                continue;
            }
            let members = crate::lower_decl::compute_enum_members(e);
            let member_values: Vec<(String, EnumValue)> =
                members.into_iter().map(|m| (m.name, m.value)).collect();
            let id = ctx.fresh_enum();
            ctx.define_enum(name, id, member_values);
        }
    }
}
