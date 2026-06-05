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
                        ctx.proxy_locals.insert(name.clone());
                        // Track proxy target class for `new p(args)` fold.
                        if let Some(args) = new_expr.args.as_ref() {
                            if let Some(first) = args.first() {
                                if let ast::Expr::Ident(cls_ident) = first.expr.as_ref() {
                                    let cls_name = cls_ident.sym.to_string();
                                    ctx.proxy_target_classes.insert(name, cls_name);
                                }
                            }
                        }
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
