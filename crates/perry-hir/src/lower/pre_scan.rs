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
    fn classify_new(new_expr: &ast::NewExpr, shadowed: &HashSet<String>) -> Option<&'static str> {
        if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
            let name = ident.sym.as_ref();
            // #6233: a user declaration of this name (`class Proxy {}`,
            // `function WeakRef() {}`, an import, …) shadows the global, so
            // `new <name>()` constructs the USER binding — don't track the
            // result as a weak/proxy intrinsic instance. Name-keyed and
            // scope-blind, matching this scan's own granularity.
            if shadowed.contains(name) {
                return None;
            }
            match name {
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
    // Names bound (anywhere in the module) to a `new <X>` whose `<X>` is NOT a
    // tracked weak type — i.e. an ordinary class/constructor. The weak-locals
    // sets are keyed by BARE NAME with no scope discrimination, so a name reused
    // across functions (extremely common in minified bundles, e.g. a one-letter
    // `z`) for both `new WeakMap()` in one function and `new SomeCache()` in
    // another would route the second function's `z.get/set` to the WeakMap
    // intrinsic — throwing `Invalid value used as weak map key` when the
    // non-weak cache is legitimately keyed by a string. Collect such ambiguous
    // names and subtract them from all weak-locals sets after the walk so the
    // call falls back to ordinary dynamic method dispatch (correct for both a
    // real WeakMap and the other constructor).
    fn record_var(
        decl: &ast::VarDeclarator,
        ctx: &mut LoweringContext,
        poison: &mut HashSet<String>,
        shadowed: &HashSet<String>,
    ) {
        if let (ast::Pat::Ident(ident), Some(init)) = (&decl.name, decl.init.as_ref()) {
            let init_unwrapped = unwrap_init(init);
            if let ast::Expr::New(new_expr) = init_unwrapped {
                let name = ident.id.sym.to_string();
                match classify_new(new_expr, shadowed) {
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
                    // `new <OtherClass>()` — this name is also used for a
                    // non-weak instance somewhere; mark it ambiguous.
                    None => {
                        poison.insert(name);
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
    fn walk_stmt(
        stmt: &ast::Stmt,
        ctx: &mut LoweringContext,
        poison: &mut HashSet<String>,
        shadowed: &HashSet<String>,
    ) {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    record_var(decl, ctx, poison, shadowed);
                }
            }
            ast::Stmt::Decl(ast::Decl::Using(using_decl)) => {
                for decl in &using_decl.decls {
                    record_var(decl, ctx, poison, shadowed);
                }
            }
            // Function declarations — descend into the body so `const
            // ref = new WeakRef(x)` inside a function is still tracked
            // and `ref.deref()` lowers to `Expr::WeakRefDeref` instead
            // of falling through to the generic method dispatch.
            ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
                if let Some(body) = &fn_decl.function.body {
                    for s in &body.stmts {
                        walk_stmt(s, ctx, poison, shadowed);
                    }
                }
            }
            ast::Stmt::Block(block) => {
                for s in &block.stmts {
                    walk_stmt(s, ctx, poison, shadowed);
                }
            }
            ast::Stmt::If(if_stmt) => {
                walk_stmt(&if_stmt.cons, ctx, poison, shadowed);
                if let Some(alt) = &if_stmt.alt {
                    walk_stmt(alt, ctx, poison, shadowed);
                }
            }
            ast::Stmt::While(w) => walk_stmt(&w.body, ctx, poison, shadowed),
            ast::Stmt::DoWhile(w) => walk_stmt(&w.body, ctx, poison, shadowed),
            ast::Stmt::For(f) => {
                if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                    for decl in &vd.decls {
                        record_var(decl, ctx, poison, shadowed);
                    }
                }
                walk_stmt(&f.body, ctx, poison, shadowed);
            }
            ast::Stmt::ForIn(f) => walk_stmt(&f.body, ctx, poison, shadowed),
            ast::Stmt::ForOf(f) => walk_stmt(&f.body, ctx, poison, shadowed),
            ast::Stmt::Try(t) => {
                for s in &t.block.stmts {
                    walk_stmt(s, ctx, poison, shadowed);
                }
                if let Some(catch) = &t.handler {
                    for s in &catch.body.stmts {
                        walk_stmt(s, ctx, poison, shadowed);
                    }
                }
                if let Some(finalizer) = &t.finalizer {
                    for s in &finalizer.stmts {
                        walk_stmt(s, ctx, poison, shadowed);
                    }
                }
            }
            ast::Stmt::Switch(s) => {
                for case in &s.cases {
                    for s in &case.cons {
                        walk_stmt(s, ctx, poison, shadowed);
                    }
                }
            }
            _ => {}
        }
    }
    // #6233: collect user declarations that shadow one of the tracked
    // constructor names — `class Proxy {}` / `function WeakRef() {}` /
    // `const WeakMap = …` / `import { WeakSet } from …`. This scan is
    // name-keyed with no scope discrimination (see the poison-set comment
    // below), so a shadow anywhere in the module suppresses tracking
    // module-wide and the affected locals fall back to ordinary dispatch.
    fn record_shadow(name: &str, shadowed: &mut HashSet<String>) {
        if matches!(
            name,
            "WeakRef" | "FinalizationRegistry" | "WeakMap" | "WeakSet" | "Proxy"
        ) {
            shadowed.insert(name.to_string());
        }
    }
    fn collect_shadowing_decls(stmt: &ast::Stmt, shadowed: &mut HashSet<String>) {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Class(cd)) => record_shadow(cd.ident.sym.as_ref(), shadowed),
            ast::Stmt::Decl(ast::Decl::Fn(fd)) => {
                record_shadow(fd.ident.sym.as_ref(), shadowed);
                if let Some(body) = &fd.function.body {
                    for s in &body.stmts {
                        collect_shadowing_decls(s, shadowed);
                    }
                }
            }
            ast::Stmt::Decl(ast::Decl::Var(vd)) => {
                for d in &vd.decls {
                    if let ast::Pat::Ident(ident) = &d.name {
                        // `const Proxy = …` — the declared NAME shadows; a
                        // `new Proxy()` bound to an ordinary local is handled
                        // by classify_new/poison, not here.
                        record_shadow(ident.id.sym.as_ref(), shadowed);
                    }
                }
            }
            ast::Stmt::Block(block) => {
                for s in &block.stmts {
                    collect_shadowing_decls(s, shadowed);
                }
            }
            ast::Stmt::If(if_stmt) => {
                collect_shadowing_decls(&if_stmt.cons, shadowed);
                if let Some(alt) = &if_stmt.alt {
                    collect_shadowing_decls(alt, shadowed);
                }
            }
            ast::Stmt::While(w) => collect_shadowing_decls(&w.body, shadowed),
            ast::Stmt::DoWhile(w) => collect_shadowing_decls(&w.body, shadowed),
            ast::Stmt::For(f) => collect_shadowing_decls(&f.body, shadowed),
            ast::Stmt::ForIn(f) => collect_shadowing_decls(&f.body, shadowed),
            ast::Stmt::ForOf(f) => collect_shadowing_decls(&f.body, shadowed),
            ast::Stmt::Try(t) => {
                for s in &t.block.stmts {
                    collect_shadowing_decls(s, shadowed);
                }
                if let Some(catch) = &t.handler {
                    for s in &catch.body.stmts {
                        collect_shadowing_decls(s, shadowed);
                    }
                }
                if let Some(finalizer) = &t.finalizer {
                    for s in &finalizer.stmts {
                        collect_shadowing_decls(s, shadowed);
                    }
                }
            }
            ast::Stmt::Switch(s) => {
                for case in &s.cases {
                    for s in &case.cons {
                        collect_shadowing_decls(s, shadowed);
                    }
                }
            }
            _ => {}
        }
    }
    let mut shadowed: HashSet<String> = HashSet::new();
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => collect_shadowing_decls(stmt, &mut shadowed),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                match &export_decl.decl {
                    ast::Decl::Class(cd) => record_shadow(cd.ident.sym.as_ref(), &mut shadowed),
                    ast::Decl::Fn(fd) => record_shadow(fd.ident.sym.as_ref(), &mut shadowed),
                    ast::Decl::Var(vd) => {
                        for d in &vd.decls {
                            if let ast::Pat::Ident(ident) = &d.name {
                                record_shadow(ident.id.sym.as_ref(), &mut shadowed);
                            }
                        }
                    }
                    _ => {}
                }
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import_decl)) => {
                for spec in &import_decl.specifiers {
                    let local = match spec {
                        ast::ImportSpecifier::Named(named) => named.local.sym.as_ref(),
                        ast::ImportSpecifier::Default(default) => default.local.sym.as_ref(),
                        ast::ImportSpecifier::Namespace(ns) => ns.local.sym.as_ref(),
                    };
                    record_shadow(local, &mut shadowed);
                }
            }
            _ => {}
        }
    }
    let mut poison: HashSet<String> = HashSet::new();
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => walk_stmt(stmt, ctx, &mut poison, &shadowed),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(var_decl) = &export_decl.decl {
                    for decl in &var_decl.decls {
                        record_var(decl, ctx, &mut poison, &shadowed);
                    }
                }
            }
            _ => {}
        }
    }
    // A name reused for both `new WeakMap()`/`new WeakSet()` and a non-weak
    // constructor is ambiguous: the bare-name weak-locals set can't tell the
    // two bindings apart, so routing `.set/.get/.has/.delete`/`.add` to the
    // weak intrinsic would be wrong for the non-weak instance (e.g. a
    // string-keyed cache → `Invalid value used as weak map key`). Drop such
    // ambiguous names so their method calls fall back to ordinary dynamic
    // dispatch. This is correct for a real WeakMap/WeakSet too: the runtime's
    // `WeakMap/WeakSet.prototype` thunks (collection_proto_thunks) re-validate
    // the receiver's class id and re-enter `js_weakmap_set` etc.
    //
    // Restricted to weakmap/weakset only: WeakRef.deref / FinalizationRegistry
    // .register / Proxy use distinct method names that don't collide with the
    // cache `.get/.set/.add` family, and (unlike WeakMap/WeakSet) have no
    // runtime method-dispatch fallback — they rely on the codegen fast path —
    // so dropping them could regress a genuine instance with no upside.
    for name in &poison {
        ctx.weakmap_locals.remove(name);
        ctx.weakset_locals.remove(name);
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
        ctx.mixin_funcs.insert(
            fn_name,
            crate::lower::MixinFn {
                param_name,
                class_expr_name: class_expr.ident.as_ref().map(|i| i.sym.to_string()),
                class_ast: Box::new((*class_expr.class).clone()),
            },
        );
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
    // name -> identifier span `lo` (stable AST identity) for keying hints, so a
    // hint never leaks onto an unrelated same-named declaration.
    let mut fn_spans: HashMap<String, u32> = HashMap::new();
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
            // Drop the TypeScript `this:` type-only param exactly as
            // `lower_fn_decl` does before enumerating real params. Call sites
            // never pass `this` positionally, so keeping it would shift every
            // hint/param index by one for `function f(this: T, req, wsId)`.
            .filter(|name| name.as_deref() != Some("this"))
            .collect();
        fn_spans.insert(name.clone(), fd.ident.span.lo.0);
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

    // Walk each gated upgrade handler's body with the `wsId` handle tainted,
    // recording a hint for every top-level user function the handle flows into.
    // The walk is scope-aware (`walk_taint_*`): descending into a nested
    // function/arrow drops any name that scope rebinds as a parameter, so a
    // shadowing `function later(wsId) {…}` cannot inherit the outer handle —
    // while legitimate captures (`wsId.on('message', m => helper(req, wsId))`)
    // still propagate. Hints are keyed by callee identity (span) and applied
    // after the walk so the immutable `fn_*` tables can stay borrowed throughout.
    let tcx = TaintCtx {
        fn_params: &fn_params,
        fn_bodies: &fn_bodies,
        fn_spans: &fn_spans,
    };
    let mut all_calls: Vec<&ast::CallExpr> = Vec::new();
    collect_calls_in_module(ast_module, &mut all_calls);
    let mut acc = TaintAcc {
        tcx: &tcx,
        applied: HashSet::new(),
        hints: Vec::new(),
        ws_fed: HashSet::new(),
        deps: HashMap::new(),
        source: None,
        guard: 0,
    };
    for call in &all_calls {
        if let Some((ws_id, handler)) = upgrade_handler_ws_id(call, &server_idents) {
            let mut taint: Taint = HashMap::new();
            taint.insert(ws_id, ("ws".to_string(), "Client".to_string()));
            walk_taint_callback_body(handler, &taint, &mut acc);
        }
    }

    // Polymorphism guard. A hint tags a function PARAMETER, which fixes the
    // dispatch for that parameter across EVERY call site of the function — not
    // just the upgrade-fed one. So only keep a hint when the function is
    // provably always handed the ws handle at that parameter. If any caller
    // passes a non-ws value there (or any caller uses a spread arg, whose
    // positional mapping is unreliable), tagging the param would silently
    // re-route that caller's `.send()/.on()/.close()` to the Client runtime and
    // drop the frame — the exact bug this pass fixes. Detect such functions and
    // drop their hints. The caller scan is scope-aware so a nested function that
    // SHADOWS a top-level name is not mistaken for a call to the top-level one.
    let candidate_keys: HashSet<(u32, usize)> = acc.hints.iter().map(|(k, _)| *k).collect();
    let mut shadowed_call_ids: HashSet<usize> = HashSet::new();
    collect_shadowed_callee_calls(ast_module, &fn_spans, &mut shadowed_call_ids);

    let mut invalid: HashSet<(u32, usize)> = HashSet::new();
    for call in &all_calls {
        let call_id = *call as *const ast::CallExpr as usize;
        // A call whose callee NAME is shadowed by a nearer binding does not
        // reach the top-level function — skip it.
        if shadowed_call_ids.contains(&call_id) {
            continue;
        }
        let Some(callee) = cross_fn_call_ident(call) else {
            continue;
        };
        let Some(&span) = fn_spans.get(&callee) else {
            continue;
        };
        if call.args.iter().any(|a| a.spread.is_some()) {
            // Spread args defeat positional matching — invalidate every
            // candidate param of this callee.
            for key in &candidate_keys {
                if key.0 == span {
                    invalid.insert(*key);
                }
            }
            continue;
        }
        for i in 0..call.args.len() {
            if candidate_keys.contains(&(span, i)) && !acc.ws_fed.contains(&(span, i, call_id)) {
                invalid.insert((span, i));
            }
        }
    }

    // Transitive demotion: a hint reached by following the handle THROUGH an
    // upstream param P is only as valid as P. Once P is invalid (polymorphic),
    // P can deliver a non-ws value downstream, so demote everything that
    // depended on it. Iterate to a fixpoint.
    loop {
        let mut changed = false;
        for (key, deps) in &acc.deps {
            if !invalid.contains(key) && deps.iter().any(|d| invalid.contains(d)) {
                invalid.insert(*key);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for (key, val) in acc.hints {
        if !invalid.contains(&key) {
            ctx.param_native_hints.insert(key, val);
        }
    }
}

/// Scope-aware pass that records the AST pointer identity of every bare-ident
/// call `name(...)` whose callee `name` is a top-level function (`fn_names`) but
/// is SHADOWED at the call site by a nearer binding (an enclosing
/// function/arrow/method param, or a function declaration hoisted into the
/// enclosing function body). Such a call does NOT reach the top-level function,
/// so the polymorphism guard must not treat it as a caller of it.
///
/// Conservative by construction: it only records calls it can prove are
/// shadowed. A missed shadow leaves the call counted as a real caller, which can
/// only DROP a hint (safe), never keep an unsound one.
fn collect_shadowed_callee_calls(
    ast_module: &ast::Module,
    fn_names: &HashMap<String, u32>,
    out: &mut HashSet<usize>,
) {
    // Shadowing names introduced by a function/arrow/method body: its params
    // plus the names of function declarations hoisted to the top of that body.
    fn body_shadows<'a>(
        params: impl Iterator<Item = &'a ast::Pat>,
        stmts: &[ast::Stmt],
        base: &HashSet<String>,
    ) -> HashSet<String> {
        let mut s = base.clone();
        for p in params {
            if let Some(n) = cross_fn_pat_name(p) {
                s.insert(n);
            }
        }
        for st in stmts {
            if let ast::Stmt::Decl(ast::Decl::Fn(f)) = st {
                s.insert(f.ident.sym.to_string());
            }
        }
        s
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(s) => shadow_scan_stmt(s, &HashSet::new(), fn_names, out),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => match &e.decl {
                ast::Decl::Var(v) => {
                    for d in &v.decls {
                        if let Some(init) = &d.init {
                            shadow_scan_expr(init, &HashSet::new(), fn_names, out);
                        }
                    }
                }
                ast::Decl::Fn(f) => {
                    if let Some(b) = &f.function.body {
                        let sh = body_shadows(
                            f.function.params.iter().map(|p| &p.pat),
                            &b.stmts,
                            &HashSet::new(),
                        );
                        for st in &b.stmts {
                            shadow_scan_stmt(st, &sh, fn_names, out);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn shadow_scan_stmt(
    stmt: &ast::Stmt,
    shadowed: &HashSet<String>,
    fn_names: &HashMap<String, u32>,
    out: &mut HashSet<usize>,
) {
    macro_rules! e {
        ($x:expr) => {
            shadow_scan_expr($x, shadowed, fn_names, out)
        };
    }
    macro_rules! s {
        ($x:expr) => {
            shadow_scan_stmt($x, shadowed, fn_names, out)
        };
    }
    match stmt {
        ast::Stmt::Expr(x) => e!(&x.expr),
        ast::Stmt::Return(r) => {
            if let Some(a) = &r.arg {
                e!(a);
            }
        }
        ast::Stmt::Decl(ast::Decl::Var(v)) => {
            for d in &v.decls {
                if let Some(init) = &d.init {
                    e!(init);
                }
            }
        }
        ast::Stmt::Decl(ast::Decl::Fn(f)) => {
            if let Some(b) = &f.function.body {
                let mut sh = shadowed.clone();
                for p in &f.function.params {
                    if let Some(n) = cross_fn_pat_name(&p.pat) {
                        sh.insert(n);
                    }
                }
                for st in &b.stmts {
                    if let ast::Stmt::Decl(ast::Decl::Fn(inner)) = st {
                        sh.insert(inner.ident.sym.to_string());
                    }
                }
                for st in &b.stmts {
                    shadow_scan_stmt(st, &sh, fn_names, out);
                }
            }
        }
        ast::Stmt::Block(b) => {
            for st in &b.stmts {
                s!(st);
            }
        }
        ast::Stmt::If(i) => {
            e!(&i.test);
            s!(&i.cons);
            if let Some(alt) = &i.alt {
                s!(alt);
            }
        }
        ast::Stmt::While(w) => {
            e!(&w.test);
            s!(&w.body);
        }
        ast::Stmt::DoWhile(w) => {
            s!(&w.body);
            e!(&w.test);
        }
        ast::Stmt::For(f) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                for d in &vd.decls {
                    if let Some(init) = &d.init {
                        e!(init);
                    }
                }
            } else if let Some(ast::VarDeclOrExpr::Expr(x)) = &f.init {
                e!(x);
            }
            if let Some(t) = &f.test {
                e!(t);
            }
            if let Some(u) = &f.update {
                e!(u);
            }
            s!(&f.body);
        }
        ast::Stmt::ForIn(f) => {
            e!(&f.right);
            s!(&f.body);
        }
        ast::Stmt::ForOf(f) => {
            e!(&f.right);
            s!(&f.body);
        }
        ast::Stmt::Throw(t) => e!(&t.arg),
        ast::Stmt::Try(t) => {
            for st in &t.block.stmts {
                s!(st);
            }
            if let Some(h) = &t.handler {
                for st in &h.body.stmts {
                    s!(st);
                }
            }
            if let Some(f) = &t.finalizer {
                for st in &f.stmts {
                    s!(st);
                }
            }
        }
        ast::Stmt::Switch(sw) => {
            e!(&sw.discriminant);
            for c in &sw.cases {
                if let Some(t) = &c.test {
                    e!(t);
                }
                for st in &c.cons {
                    s!(st);
                }
            }
        }
        ast::Stmt::Labeled(l) => s!(&l.body),
        _ => {}
    }
}

fn shadow_scan_expr(
    expr: &ast::Expr,
    shadowed: &HashSet<String>,
    fn_names: &HashMap<String, u32>,
    out: &mut HashSet<usize>,
) {
    macro_rules! e {
        ($x:expr) => {
            shadow_scan_expr($x, shadowed, fn_names, out)
        };
    }
    // Walk into a nested function/arrow/method body with its params (and hoisted
    // fn-decls) added to the shadow set.
    macro_rules! into_body {
        ($params:expr, $stmts:expr) => {{
            let mut sh = shadowed.clone();
            for p in $params {
                if let Some(n) = cross_fn_pat_name(p) {
                    sh.insert(n);
                }
            }
            for st in $stmts {
                if let ast::Stmt::Decl(ast::Decl::Fn(inner)) = st {
                    sh.insert(inner.ident.sym.to_string());
                }
            }
            for st in $stmts {
                shadow_scan_stmt(st, &sh, fn_names, out);
            }
        }};
    }
    match expr {
        ast::Expr::Call(c) => {
            if let ast::Callee::Expr(ce) = &c.callee {
                if let ast::Expr::Ident(id) = ce.as_ref() {
                    let name = id.sym.as_ref();
                    if fn_names.contains_key(name) && shadowed.contains(name) {
                        out.insert(c as *const ast::CallExpr as usize);
                    }
                }
                e!(ce);
            }
            for a in &c.args {
                e!(&a.expr);
            }
        }
        ast::Expr::New(n) => {
            e!(&n.callee);
            if let Some(args) = &n.args {
                for a in args {
                    e!(&a.expr);
                }
            }
        }
        ast::Expr::Member(m) => {
            e!(&m.obj);
            if let ast::MemberProp::Computed(c) = &m.prop {
                e!(&c.expr);
            }
        }
        ast::Expr::Bin(b) => {
            e!(&b.left);
            e!(&b.right);
        }
        ast::Expr::Unary(u) => e!(&u.arg),
        ast::Expr::Update(u) => e!(&u.arg),
        ast::Expr::Assign(a) => e!(&a.right),
        ast::Expr::Cond(c) => {
            e!(&c.test);
            e!(&c.cons);
            e!(&c.alt);
        }
        ast::Expr::Paren(p) => e!(&p.expr),
        ast::Expr::Seq(s) => {
            for x in &s.exprs {
                e!(x);
            }
        }
        ast::Expr::Await(a) => e!(&a.arg),
        ast::Expr::Yield(y) => {
            if let Some(a) = &y.arg {
                e!(a);
            }
        }
        ast::Expr::Tpl(t) => {
            for x in &t.exprs {
                e!(x);
            }
        }
        ast::Expr::TaggedTpl(t) => {
            e!(&t.tag);
            for x in &t.tpl.exprs {
                e!(x);
            }
        }
        ast::Expr::Array(a) => {
            for el in a.elems.iter().flatten() {
                e!(&el.expr);
            }
        }
        ast::Expr::Object(o) => {
            for prop in &o.props {
                match prop {
                    ast::PropOrSpread::Spread(s) => e!(&s.expr),
                    ast::PropOrSpread::Prop(p) => match p.as_ref() {
                        ast::Prop::KeyValue(kv) => e!(&kv.value),
                        ast::Prop::Method(m) => {
                            if let Some(b) = &m.function.body {
                                into_body!(m.function.params.iter().map(|p| &p.pat), &b.stmts);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        ast::Expr::Arrow(a) => match a.body.as_ref() {
            ast::BlockStmtOrExpr::BlockStmt(b) => {
                into_body!(a.params.iter(), &b.stmts);
            }
            ast::BlockStmtOrExpr::Expr(x) => {
                let mut sh = shadowed.clone();
                for p in &a.params {
                    if let Some(n) = cross_fn_pat_name(p) {
                        sh.insert(n);
                    }
                }
                shadow_scan_expr(x, &sh, fn_names, out);
            }
        },
        ast::Expr::Fn(f) => {
            if let Some(b) = &f.function.body {
                into_body!(f.function.params.iter().map(|p| &p.pat), &b.stmts);
            }
        }
        ast::Expr::OptChain(o) => match o.base.as_ref() {
            ast::OptChainBase::Member(m) => {
                e!(&m.obj);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    e!(&c.expr);
                }
            }
            ast::OptChainBase::Call(c) => {
                e!(&c.callee);
                for a in &c.args {
                    e!(&a.expr);
                }
            }
        },
        ast::Expr::TsAs(t) => e!(&t.expr),
        ast::Expr::TsNonNull(t) => e!(&t.expr),
        ast::Expr::TsTypeAssertion(t) => e!(&t.expr),
        ast::Expr::TsConstAssertion(t) => e!(&t.expr),
        ast::Expr::TsInstantiation(t) => e!(&t.expr),
        _ => {}
    }
}

/// Immutable top-level-function tables shared by the scope-aware taint walk.
struct TaintCtx<'a> {
    fn_params: &'a HashMap<String, Vec<Option<String>>>,
    fn_bodies: &'a HashMap<String, &'a ast::BlockStmt>,
    fn_spans: &'a HashMap<String, u32>,
}

type Taint = HashMap<String, (String, String)>;

/// Mutable accumulators + the immutable function tables for the scope-aware
/// taint walk, bundled so the recursive walkers take a single `&mut` and so the
/// "current upstream source" can be saved/restored around a cross-fn descent
/// without threading another argument through every arm.
struct TaintAcc<'a> {
    tcx: &'a TaintCtx<'a>,
    /// Dedup: a `(fn_span, param_idx, module, class)` hint is pushed / recursed
    /// into at most once.
    applied: HashSet<(u32, usize, String, String)>,
    /// Recorded hints, validated (polymorphism guard + transitive demotion) by
    /// the caller after the walk.
    hints: Vec<((u32, usize), (String, String))>,
    /// Call sites — by AST pointer identity — that passed a ws-tainted handle at
    /// `(callee_span, param_idx)`. After the walk, a hinted param whose function
    /// ALSO has a non-ws-feeding caller is dropped: tagging the param would
    /// mis-route that other caller's `.send()/.on()/.close()` to the Client
    /// runtime (a silent no-op for the non-ws value).
    ws_fed: HashSet<(u32, usize, usize)>,
    /// Validity dependencies: a hint recorded while following the handle THROUGH
    /// an upstream param P only holds when P is itself validly tagged. Seed-level
    /// hints carry no dep. Drives transitive demotion.
    deps: HashMap<(u32, usize), HashSet<(u32, usize)>>,
    /// The upstream param the current descent is flowing the handle from (`None`
    /// at the upgrade-handler seed level). Set when recursing into a callee body.
    source: Option<(u32, usize)>,
    guard: usize,
}

/// Clone `taint`, dropping any name the arrow rebinds as a parameter (shadowing).
fn prune_for_arrow(taint: &Taint, arrow: &ast::ArrowExpr) -> Taint {
    let mut t = taint.clone();
    for p in &arrow.params {
        if let Some(n) = cross_fn_pat_name(p) {
            t.remove(&n);
        }
    }
    t
}

/// Clone `taint`, dropping any name the function rebinds as a parameter.
fn prune_for_fn(taint: &Taint, func: &ast::Function) -> Taint {
    let mut t = taint.clone();
    for p in &func.params {
        if let Some(n) = cross_fn_pat_name(&p.pat) {
            t.remove(&n);
        }
    }
    t
}

/// Walk a callback expression's body (arrow/function) under the seed taint. The
/// callback's own params already account for the tainted handle, so no pruning.
fn walk_taint_callback_body(handler: &ast::Expr, taint: &Taint, acc: &mut TaintAcc) {
    match handler {
        ast::Expr::Arrow(a) => match a.body.as_ref() {
            ast::BlockStmtOrExpr::BlockStmt(b) => walk_taint_stmts(&b.stmts, taint, acc),
            ast::BlockStmtOrExpr::Expr(e) => walk_taint_expr(e, taint, acc),
        },
        ast::Expr::Fn(f) => {
            if let Some(b) = &f.function.body {
                walk_taint_stmts(&b.stmts, taint, acc);
            }
        }
        _ => {}
    }
}

fn walk_taint_stmts(stmts: &[ast::Stmt], taint: &Taint, acc: &mut TaintAcc) {
    for s in stmts {
        walk_taint_stmt(s, taint, acc);
    }
}

fn walk_taint_stmt(stmt: &ast::Stmt, taint: &Taint, acc: &mut TaintAcc) {
    macro_rules! e {
        ($x:expr) => {
            walk_taint_expr($x, taint, acc)
        };
    }
    macro_rules! s {
        ($x:expr) => {
            walk_taint_stmt($x, taint, acc)
        };
    }
    match stmt {
        ast::Stmt::Expr(x) => e!(&x.expr),
        ast::Stmt::Return(r) => {
            if let Some(a) = &r.arg {
                e!(a);
            }
        }
        ast::Stmt::Decl(ast::Decl::Var(v)) => {
            for d in &v.decls {
                if let Some(init) = &d.init {
                    e!(init);
                }
            }
        }
        // A nested function declaration is a new scope — prune its params.
        ast::Stmt::Decl(ast::Decl::Fn(f)) => {
            if let Some(b) = &f.function.body {
                let pruned = prune_for_fn(taint, &f.function);
                walk_taint_stmts(&b.stmts, &pruned, acc);
            }
        }
        ast::Stmt::Block(b) => walk_taint_stmts(&b.stmts, taint, acc),
        ast::Stmt::If(i) => {
            e!(&i.test);
            s!(&i.cons);
            if let Some(alt) = &i.alt {
                s!(alt);
            }
        }
        ast::Stmt::While(w) => {
            e!(&w.test);
            s!(&w.body);
        }
        ast::Stmt::DoWhile(w) => {
            s!(&w.body);
            e!(&w.test);
        }
        ast::Stmt::For(f) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                for d in &vd.decls {
                    if let Some(init) = &d.init {
                        e!(init);
                    }
                }
            } else if let Some(ast::VarDeclOrExpr::Expr(x)) = &f.init {
                e!(x);
            }
            if let Some(t) = &f.test {
                e!(t);
            }
            if let Some(u) = &f.update {
                e!(u);
            }
            s!(&f.body);
        }
        ast::Stmt::ForIn(f) => {
            e!(&f.right);
            s!(&f.body);
        }
        ast::Stmt::ForOf(f) => {
            e!(&f.right);
            s!(&f.body);
        }
        ast::Stmt::Throw(t) => e!(&t.arg),
        ast::Stmt::Try(t) => {
            walk_taint_stmts(&t.block.stmts, taint, acc);
            if let Some(h) = &t.handler {
                walk_taint_stmts(&h.body.stmts, taint, acc);
            }
            if let Some(f) = &t.finalizer {
                walk_taint_stmts(&f.stmts, taint, acc);
            }
        }
        ast::Stmt::Switch(sw) => {
            e!(&sw.discriminant);
            for c in &sw.cases {
                if let Some(t) = &c.test {
                    e!(t);
                }
                walk_taint_stmts(&c.cons, taint, acc);
            }
        }
        ast::Stmt::Labeled(l) => s!(&l.body),
        _ => {}
    }
}

fn walk_taint_expr(expr: &ast::Expr, taint: &Taint, acc: &mut TaintAcc) {
    acc.guard += 1;
    if acc.guard > 200_000 {
        return; // pathological-input backstop
    }
    // Copy the shared function tables out (a `&` is Copy) so reading them below
    // doesn't conflict with the `&mut acc` mutations in the call arm.
    let tcx = acc.tcx;
    macro_rules! e {
        ($x:expr) => {
            walk_taint_expr($x, taint, acc)
        };
    }
    match expr {
        ast::Expr::Call(c) => {
            // Record a cross-fn hint when this is a top-level user-fn call that
            // receives a currently-tainted ident argument.
            if let Some(callee) = cross_fn_call_ident(c) {
                if let (Some(params), Some(&span)) =
                    (tcx.fn_params.get(&callee), tcx.fn_spans.get(&callee))
                {
                    for (i, arg) in c.args.iter().enumerate() {
                        if arg.spread.is_some() {
                            continue;
                        }
                        let ast::Expr::Ident(id) = arg.expr.as_ref() else {
                            continue;
                        };
                        let Some((module, class)) = taint.get(id.sym.as_ref()) else {
                            continue;
                        };
                        let Some(Some(param_name)) = params.get(i) else {
                            continue;
                        };
                        let (module, class) = (module.clone(), class.clone());
                        // Record this ws-feeding call site (by AST pointer
                        // identity) BEFORE the dedup below, so EVERY ws caller is
                        // captured for the polymorphism guard even when the hint
                        // itself was already recorded from another seed.
                        let call_id = c as *const ast::CallExpr as usize;
                        acc.ws_fed.insert((span, i, call_id));
                        // This hint only holds if the upstream param we are
                        // flowing through is itself validly tagged.
                        if let Some(src) = acc.source {
                            acc.deps.entry((span, i)).or_default().insert(src);
                        }
                        if !acc.applied.insert((span, i, module.clone(), class.clone())) {
                            continue;
                        }
                        acc.hints.push(((span, i), (module.clone(), class.clone())));
                        // Follow the handle into the callee body with a FRESH
                        // scoped taint binding only its receiving parameter, and
                        // mark this callee param as the upstream source so any
                        // deeper hint records its dependency on us.
                        if let Some(body) = tcx.fn_bodies.get(&callee) {
                            let mut fresh: Taint = HashMap::new();
                            fresh.insert(param_name.clone(), (module, class));
                            let prev = acc.source;
                            acc.source = Some((span, i));
                            walk_taint_stmts(&body.stmts, &fresh, acc);
                            acc.source = prev;
                        }
                    }
                }
            }
            // Descend into the callee + args (this scope; nested closures inside
            // an arg get pruned by their own arms below).
            if let ast::Callee::Expr(ce) = &c.callee {
                e!(ce);
            }
            for arg in &c.args {
                e!(&arg.expr);
            }
        }
        ast::Expr::New(n) => {
            e!(&n.callee);
            if let Some(args) = &n.args {
                for a in args {
                    e!(&a.expr);
                }
            }
        }
        ast::Expr::Member(m) => {
            e!(&m.obj);
            if let ast::MemberProp::Computed(c) = &m.prop {
                e!(&c.expr);
            }
        }
        ast::Expr::Bin(b) => {
            e!(&b.left);
            e!(&b.right);
        }
        ast::Expr::Unary(u) => e!(&u.arg),
        ast::Expr::Update(u) => e!(&u.arg),
        ast::Expr::Assign(a) => e!(&a.right),
        ast::Expr::Cond(c) => {
            e!(&c.test);
            e!(&c.cons);
            e!(&c.alt);
        }
        ast::Expr::Paren(p) => e!(&p.expr),
        ast::Expr::Seq(s) => {
            for x in &s.exprs {
                e!(x);
            }
        }
        ast::Expr::Await(a) => e!(&a.arg),
        ast::Expr::Yield(y) => {
            if let Some(a) = &y.arg {
                e!(a);
            }
        }
        ast::Expr::Tpl(t) => {
            for x in &t.exprs {
                e!(x);
            }
        }
        ast::Expr::TaggedTpl(t) => {
            e!(&t.tag);
            for x in &t.tpl.exprs {
                e!(x);
            }
        }
        ast::Expr::Array(a) => {
            for el in a.elems.iter().flatten() {
                e!(&el.expr);
            }
        }
        ast::Expr::Object(o) => {
            for prop in &o.props {
                match prop {
                    ast::PropOrSpread::Spread(s) => e!(&s.expr),
                    ast::PropOrSpread::Prop(p) => match p.as_ref() {
                        ast::Prop::KeyValue(kv) => e!(&kv.value),
                        // Object methods are new scopes — prune their params.
                        ast::Prop::Method(m) => {
                            if let Some(b) = &m.function.body {
                                let pruned = prune_for_fn(taint, &m.function);
                                walk_taint_stmts(&b.stmts, &pruned, acc);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        // New lexical scope — drop any name it rebinds as a parameter.
        ast::Expr::Arrow(a) => {
            let pruned = prune_for_arrow(taint, a);
            match a.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => walk_taint_stmts(&b.stmts, &pruned, acc),
                ast::BlockStmtOrExpr::Expr(x) => walk_taint_expr(x, &pruned, acc),
            }
        }
        ast::Expr::Fn(f) => {
            if let Some(b) = &f.function.body {
                let pruned = prune_for_fn(taint, &f.function);
                walk_taint_stmts(&b.stmts, &pruned, acc);
            }
        }
        ast::Expr::OptChain(o) => match o.base.as_ref() {
            ast::OptChainBase::Member(m) => {
                e!(&m.obj);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    e!(&c.expr);
                }
            }
            ast::OptChainBase::Call(c) => {
                e!(&c.callee);
                for arg in &c.args {
                    e!(&arg.expr);
                }
            }
        },
        ast::Expr::TsAs(t) => e!(&t.expr),
        ast::Expr::TsNonNull(t) => e!(&t.expr),
        ast::Expr::TsTypeAssertion(t) => e!(&t.expr),
        ast::Expr::TsConstAssertion(t) => e!(&t.expr),
        ast::Expr::TsInstantiation(t) => e!(&t.expr),
        ast::Expr::TsSatisfies(t) => e!(&t.expr),
        // A class expression's member bodies are new scopes: prune each
        // method/constructor's params before recursing (like object methods);
        // field initializers and static blocks run in the surrounding taint.
        ast::Expr::Class(c) => {
            for member in &c.class.body {
                match member {
                    ast::ClassMember::Method(m) => {
                        if let Some(b) = &m.function.body {
                            let pruned = prune_for_fn(taint, &m.function);
                            walk_taint_stmts(&b.stmts, &pruned, acc);
                        }
                    }
                    ast::ClassMember::PrivateMethod(m) => {
                        if let Some(b) = &m.function.body {
                            let pruned = prune_for_fn(taint, &m.function);
                            walk_taint_stmts(&b.stmts, &pruned, acc);
                        }
                    }
                    ast::ClassMember::Constructor(ctor) => {
                        if let Some(b) = &ctor.body {
                            let mut pruned = taint.clone();
                            for p in &ctor.params {
                                if let ast::ParamOrTsParamProp::Param(param) = p {
                                    if let Some(n) = cross_fn_pat_name(&param.pat) {
                                        pruned.remove(&n);
                                    }
                                }
                            }
                            walk_taint_stmts(&b.stmts, &pruned, acc);
                        }
                    }
                    ast::ClassMember::ClassProp(p) => {
                        if let Some(v) = &p.value {
                            e!(v);
                        }
                    }
                    ast::ClassMember::PrivateProp(p) => {
                        if let Some(v) = &p.value {
                            e!(v);
                        }
                    }
                    ast::ClassMember::StaticBlock(s) => {
                        walk_taint_stmts(&s.body.stmts, taint, acc);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
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

/// Node HTTP-family module specifiers whose `createServer`/`createSecureServer`
/// produce a real `("http","HttpServer")` handle.
fn is_http_module_specifier(src: &str) -> bool {
    matches!(
        src,
        "http" | "node:http" | "https" | "node:https" | "http2" | "node:http2"
    )
}

/// References that resolve to an HTTP-family `createServer`/`createSecureServer`,
/// gathered from the module's imports and `require(...)`s. `bare` holds local
/// names that ARE the function (named import / destructured require); `ns` holds
/// namespace locals so `<ns>.createServer(...)` is recognised.
#[derive(Default)]
struct HttpCreateRefs {
    bare: HashSet<String>,
    ns: HashSet<String>,
}

/// Is `expr` a `require("<http-module>")` call? Returns the specifier text.
fn require_http_specifier(expr: &ast::Expr) -> Option<&str> {
    let ast::Expr::Call(call) = expr else {
        return None;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let ast::Expr::Ident(i) = callee.as_ref() else {
        return None;
    };
    if i.sym.as_ref() != "require" {
        return None;
    }
    let arg = call.args.first()?;
    let ast::Expr::Lit(ast::Lit::Str(s)) = arg.expr.as_ref() else {
        return None;
    };
    let src = s.value.as_str()?;
    is_http_module_specifier(src).then_some(src)
}

/// Build the set of HTTP-imported `createServer`/`createSecureServer` references
/// (named/default/namespace imports + `require(...)` aliases, top-level scan).
fn collect_http_create_server_refs(ast_module: &ast::Module) -> HttpCreateRefs {
    let mut refs = HttpCreateRefs::default();
    let note_require_binding = |pat: &ast::Pat, refs: &mut HttpCreateRefs| match pat {
        // `const http = require("node:http")` → namespace binding.
        ast::Pat::Ident(id) => {
            refs.ns.insert(id.id.sym.to_string());
        }
        // `const { createServer } = require("node:http")` → bare binding.
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                if let ast::ObjectPatProp::Assign(a) = prop {
                    let key = a.key.sym.as_ref();
                    if key == "createServer" || key == "createSecureServer" {
                        refs.bare.insert(a.key.sym.to_string());
                    }
                } else if let ast::ObjectPatProp::KeyValue(kv) = prop {
                    if let ast::PropName::Ident(k) = &kv.key {
                        let key = k.sym.as_ref();
                        if key == "createServer" || key == "createSecureServer" {
                            if let ast::Pat::Ident(local) = kv.value.as_ref() {
                                refs.bare.insert(local.id.sym.to_string());
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    };
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(imp)) => {
                if !is_http_module_specifier(imp.src.value.as_str().unwrap_or("")) {
                    continue;
                }
                for spec in &imp.specifiers {
                    match spec {
                        ast::ImportSpecifier::Named(n) => {
                            let imported = match &n.imported {
                                Some(ast::ModuleExportName::Ident(i)) => i.sym.to_string(),
                                Some(ast::ModuleExportName::Str(s)) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                                None => n.local.sym.to_string(),
                            };
                            if imported == "createServer" || imported == "createSecureServer" {
                                refs.bare.insert(n.local.sym.to_string());
                            }
                        }
                        ast::ImportSpecifier::Default(d) => {
                            refs.ns.insert(d.local.sym.to_string());
                        }
                        ast::ImportSpecifier::Namespace(ns) => {
                            refs.ns.insert(ns.local.sym.to_string());
                        }
                    }
                }
            }
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(v))) => {
                for d in &v.decls {
                    if let Some(init) = &d.init {
                        if require_http_specifier(init).is_some() {
                            note_require_binding(&d.name, &mut refs);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    refs
}

/// Record idents bound to an HTTP-family `createServer(...)` /
/// `createSecureServer(...)` call (verified import/require provenance) anywhere
/// in the module.
fn collect_server_idents_in_module(ast_module: &ast::Module, out: &mut HashSet<String>) {
    let refs = collect_http_create_server_refs(ast_module);
    fn is_create_server_call(expr: &ast::Expr, refs: &HttpCreateRefs) -> bool {
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
        match callee.as_ref() {
            // Bare `createServer(...)` — only when the name is an HTTP-imported
            // binding (named import or destructured require), not a user factory.
            ast::Expr::Ident(i) => refs.bare.contains(i.sym.as_ref()),
            // `<ns>.createServer(...)` — `<ns>` must be an HTTP namespace import.
            ast::Expr::Member(m) => {
                let ast::MemberProp::Ident(prop) = &m.prop else {
                    return false;
                };
                let prop = prop.sym.as_ref();
                if prop != "createServer" && prop != "createSecureServer" {
                    return false;
                }
                matches!(m.obj.as_ref(), ast::Expr::Ident(o) if refs.ns.contains(o.sym.as_ref()))
            }
            _ => false,
        }
    }
    fn record_decl(decl: &ast::VarDeclarator, refs: &HttpCreateRefs, out: &mut HashSet<String>) {
        if let (ast::Pat::Ident(id), Some(init)) = (&decl.name, decl.init.as_ref()) {
            if is_create_server_call(init, refs) {
                out.insert(id.id.sym.to_string());
            }
        }
    }
    fn walk_stmt(stmt: &ast::Stmt, refs: &HttpCreateRefs, out: &mut HashSet<String>) {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(v)) => {
                for d in &v.decls {
                    record_decl(d, refs, out);
                }
            }
            ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                if let Some(b) = &f.function.body {
                    for s in &b.stmts {
                        walk_stmt(s, refs, out);
                    }
                }
            }
            ast::Stmt::Expr(e) => walk_expr(&e.expr, refs, out),
            ast::Stmt::Block(b) => {
                for s in &b.stmts {
                    walk_stmt(s, refs, out);
                }
            }
            ast::Stmt::If(i) => {
                walk_stmt(&i.cons, refs, out);
                if let Some(alt) = &i.alt {
                    walk_stmt(alt, refs, out);
                }
            }
            ast::Stmt::While(w) => walk_stmt(&w.body, refs, out),
            ast::Stmt::DoWhile(w) => walk_stmt(&w.body, refs, out),
            ast::Stmt::For(f) => {
                if let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init {
                    for d in &vd.decls {
                        record_decl(d, refs, out);
                    }
                }
                walk_stmt(&f.body, refs, out);
            }
            ast::Stmt::ForIn(f) => walk_stmt(&f.body, refs, out),
            ast::Stmt::ForOf(f) => walk_stmt(&f.body, refs, out),
            ast::Stmt::Try(t) => {
                for s in &t.block.stmts {
                    walk_stmt(s, refs, out);
                }
                if let Some(h) = &t.handler {
                    for s in &h.body.stmts {
                        walk_stmt(s, refs, out);
                    }
                }
                if let Some(f) = &t.finalizer {
                    for s in &f.stmts {
                        walk_stmt(s, refs, out);
                    }
                }
            }
            ast::Stmt::Switch(s) => {
                for c in &s.cases {
                    for s in &c.cons {
                        walk_stmt(s, refs, out);
                    }
                }
            }
            ast::Stmt::Return(r) => {
                if let Some(a) = &r.arg {
                    walk_expr(a, refs, out);
                }
            }
            _ => {}
        }
    }
    // Descend into callback bodies (e.g. a server created inside `main()` or a
    // `.listen(0, () => { const server = createServer(...) })`).
    fn walk_expr(expr: &ast::Expr, refs: &HttpCreateRefs, out: &mut HashSet<String>) {
        match expr {
            ast::Expr::Call(c) => {
                for a in &c.args {
                    walk_expr(&a.expr, refs, out);
                }
            }
            ast::Expr::Arrow(a) => match a.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    for s in &b.stmts {
                        walk_stmt(s, refs, out);
                    }
                }
                ast::BlockStmtOrExpr::Expr(e) => walk_expr(e, refs, out),
            },
            ast::Expr::Fn(f) => {
                if let Some(b) = &f.function.body {
                    for s in &b.stmts {
                        walk_stmt(s, refs, out);
                    }
                }
            }
            ast::Expr::Paren(p) => walk_expr(&p.expr, refs, out),
            _ => {}
        }
    }
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(s) => walk_stmt(s, &refs, out),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => {
                if let ast::Decl::Var(v) = &e.decl {
                    for d in &v.decls {
                        record_decl(d, &refs, out);
                    }
                } else if let ast::Decl::Fn(f) = &e.decl {
                    if let Some(b) = &f.function.body {
                        for s in &b.stmts {
                            walk_stmt(s, &refs, out);
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
