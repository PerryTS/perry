//! Pre-scan for `WeakRef` / `WeakMap` / `WeakSet` / `FinalizationRegistry` /
//! `Proxy` local bindings.
//!
//! Split out of `lower/pre_scan.rs` for the 2000-line file cap (#7828 pushed it
//! to 2011). Pure code move — no logic change.

use std::collections::HashSet;
use swc_ecma_ast as ast;

use crate::lower::*;

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
            } else if matches!(init_unwrapped, ast::Expr::Call(_) | ast::Expr::Await(_)) {
                // #7775: a name bound to a CALL result is just as ambiguous as
                // one bound to `new <OtherClass>()`, and it was not poisoned at
                // all. `const a = build(10)` in one function and
                // `const a = new Proxy(raw, {})` in another made the FIRST
                // function's `a.length` lower to `js_proxy_get` on a plain
                // array — `undefined`, so the read loop after it ran zero
                // iterations. The proxy function never even had to be called.
                //
                // Deliberately narrow: only call/await initializers, which are
                // the opaque ones. A literal, a member read or an identifier
                // copy stays unpoisoned, so the common `const p = new Proxy(…)`
                // in a module that also does `const p = { … }` elsewhere is
                // untouched by this arm.
                poison.insert(ident.id.sym.to_string());
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
        // #7775: `proxy_locals` was left in — the note above weighed a lost
        // codegen fast path against "no upside", because it only considered
        // METHOD dispatch (`.deref()`, `.register()`), where a poisoned name
        // costs speed. A PROPERTY read is the other half and the objection does
        // not hold there: an ambiguous name routed a NON-proxy receiver's
        // `a.length` to `js_proxy_get`, which answers `undefined`. That is a
        // wrong answer, not a slow one, and it is what a poisoned name buys
        // back. A genuine proxy keeps working through the ordinary dynamic
        // property path (asserted in
        // `test-files/test_gap_proxy_local_name_collision_7775.ts`).
        ctx.proxy_locals.remove(name);
    }
}
