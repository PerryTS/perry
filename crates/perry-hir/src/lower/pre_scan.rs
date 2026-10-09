//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

mod class_decl_names;
mod function_ctor_reach;
mod weakref_locals;

pub(crate) use class_decl_names::pre_scan_class_decl_names;
pub(crate) use function_ctor_reach::pre_scan_function_ctor_reach;
pub(crate) use weakref_locals::pre_scan_weakref_locals;

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
        if fn_decl.function.this_param.is_some() || fn_decl.function.params.len() != 1 {
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
