//! #10363: TypeScript ambient variable declarations.
//!
//! `declare const x: T;` (and `declare let` / `declare var`) describes a
//! binding the host supplies: a global object property installed by the
//! runtime, a polyfill, or a script loaded earlier. TypeScript erases the
//! declaration, so it binds NOTHING, and every reference must resolve exactly
//! as if the line were absent, which means through the global object. Lowering
//! it as a real binding shadows that global with `undefined`, and a
//! `declare var` would also publish a non-configurable global property that
//! makes a later `Object.defineProperty(globalThis, "x", …)` throw.
//!
//! Every pass that models the module's bindings (source-position lowering,
//! the forward pre-registration passes, `var` hoisting, TDZ and shadow scans)
//! has to agree on this, so each one asks [`declarator_binds_nothing`].
//!
//! The one exception is Perry's compile-time constants. `declare const
//! __platform__: number` is the documented way to ask for the target's
//! platform id: the backends fold it from the declaration's `Stmt::Let`, which
//! must carry no initializer (see `compile_time_constants` in perry-codegen,
//! and the JS, WASM and ArkTS emitters).

use swc_ecma_ast as ast;

use crate::ir::Stmt;

/// A compile-time constant whose ambient declaration keeps its binding.
fn is_compile_time_constant(name: &str) -> bool {
    matches!(name, "__platform__" | "__plugins__")
}

/// Whether `decl` is an ambient declarator of a compile-time constant.
pub(crate) fn declarator_is_compile_time_constant(
    var: &ast::VarDecl,
    decl: &ast::VarDeclarator,
) -> bool {
    var.declare
        && matches!(&decl.name, ast::Pat::Ident(ident)
            if is_compile_time_constant(ident.id.sym.as_ref()))
}

/// Whether `decl` (one declarator of `var`) is erased at run time: it declares
/// no binding, and references to its names resolve to the global object.
pub(crate) fn declarator_binds_nothing(var: &ast::VarDecl, decl: &ast::VarDeclarator) -> bool {
    var.declare && !declarator_is_compile_time_constant(var, decl)
}

/// Record the names of a declarator that [`declarator_binds_nothing`] as known
/// globals, so a reference to them does not print the unknown-identifier
/// warning. They use the same by-name runtime lookup as any other global.
pub(crate) fn note_ambient_globals(ctx: &mut super::LoweringContext, decl: &ast::VarDeclarator) {
    let mut names = Vec::new();
    crate::lower_patterns::collect_binding_names(&decl.name, &mut names);
    ctx.platform_globals.extend(names);
}

/// Give a compile-time constant's lowered `Stmt::Let` back the no-initializer
/// shape the backends key on. #6871 made every uninitialized `let`/`const`
/// materialize `undefined`, which is right for real bindings but hid the
/// declaration from constant folding, so `__platform__` read `undefined`.
pub(crate) fn restore_compile_time_constant_shape(stmts: &mut [Stmt]) {
    for stmt in stmts {
        if let Stmt::Let { init, .. } = stmt {
            if matches!(init, Some(crate::ir::Expr::Undefined)) {
                *init = None;
            }
        }
    }
}
