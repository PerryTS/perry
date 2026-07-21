use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

use super::super::super::{lower_expr, LoweringContext};

/// Issue #668 / #5216: a string-literal `require("<module>")` from user source.
///
/// When `<module>` statically resolves to a Perry-supported native/Node-builtin
/// module (`readline`, `node:fs`, `os`, `path`, `util`, …), lower the
/// `require(...)` *expression* to the same module-namespace value an `import *
/// as ns from "<module>"` binds (`Expr::NativeModuleRef(module)`), so inline
/// member access (`require("node:os").platform()`) and the statement-level
/// `const ns = require(...)` / `const { x } = require(...)` shapes (handled in
/// `destructuring::var_decl`) all reuse the existing native-module dispatch.
///
/// For a *non-literal* specifier or an *unresolvable* module the historical
/// behavior is preserved: user source bails at compile time with a fix-it
/// pointing at `import ...` (so the problem surfaces on the first build, not the
/// first prod request); `node_modules` sources and `require(...)` inside a
/// `try` (optional native addons) fall through silently to the legacy
/// unknown-callee path.
///
/// Returns `Some(expr)` when the require lowered to a namespace value, `None`
/// to fall through to the rest of call lowering.
pub(crate) fn try_require_literal(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    // Only the bare global `require` — a local/func/imported binding named
    // `require` (e.g. `createRequire(...)`) shadows it and is handled elsewhere.
    if ident.sym.as_ref() != "require"
        || ctx.lookup_local("require").is_some()
        || ctx.lookup_func("require").is_some()
        || ctx.lookup_imported_func("require").is_some()
        || call.args.len() != 1
        || call.args[0].spread.is_some()
    {
        return Ok(None);
    }
    let ast::Expr::Lit(ast::Lit::Str(s)) = call.args[0].expr.as_ref() else {
        return Ok(None);
    };
    let spec = s.value.as_str().unwrap_or("");

    // #5216: a string-literal require of a statically resolvable native/Node
    // builtin lowers to the module-namespace value — same as `import * as ns
    // from "<spec>"`. This works regardless of external-module / try context
    // (it is strictly correct: the result really is the namespace). Inline
    // member access (`require("node:os").platform()`) dispatches off the
    // `NativeModuleRef` exactly like a namespace import would.
    if let Some(module) = crate::destructuring::resolvable_native_module_for_spec(spec) {
        let native_source = if module == "process" {
            "process.namespace".to_string()
        } else {
            module
        };
        return Ok(Some(Expr::NativeModuleRef(native_source)));
    }

    // Issue #668: for an UNRESOLVABLE module, only enforce the compile-time
    // error for user-written source files. Many published packages (e.g.
    // `@perryts/redis`) deliberately use `require(literal)` inside a method
    // body to break import cycles; those calls only execute on opt-in code
    // paths and pre-fix simply returned undefined-and-failed-at-call-time.
    // Failing them at compile time would refuse to build any consumer of those
    // packages even if the require'd path is never reached. node_modules
    // sources keep the legacy behavior (silent fall-through to the
    // unknown-callee path), as does `require(...)` inside a `try` (optional
    // native addons, #optional_require_try_depth).
    if !ctx.is_external_module && ctx.optional_require_try_depth == 0 {
        // #925: when we have a module-specific hint (e.g. distinguishing "this
        // is in stdlib, just swap to ESM" from "this isn't shimmed at all"),
        // append it.
        let hint = super::super::super::unimpl_hints::require_module_hint(spec)
            .map(|h| format!(" {h}"))
            .unwrap_or_default();
        crate::lower_bail!(
            call.span,
            "CommonJS `require(\"{}\")` is not supported under `perry compile` \
             — use a static `import` instead \
             (e.g. `import * as m from \"{}\"` \
             or `import {{ x }} from \"{}\"`). Closes #668.{}",
            spec,
            spec,
            spec,
            hint,
        );
    }
    Ok(None)
}

/// #5389 Tier 2: a bare, **computed** `require(expr)` (non-literal specifier)
/// inside a compiled external / `compilePackages` module.
///
/// Literal specifiers are handled by `try_require_literal` (which runs first):
/// native builtins fold to `NativeModuleRef`, and the `createRequire`-alias /
/// destructuring transforms rewrite literal package requires to imports. A
/// non-literal specifier can't be rewritten statically, so route it through the
/// same synchronous dynamic-require path as dynamic `import()`: emit a
/// `DynamicImport { synchronous: true }` node whose `arg` `collect_modules`
/// const-folds (or globs) to a finite target set, registering each as a dynamic
/// import edge. Codegen then dispatches to the matching compiled-module
/// namespace **synchronously** (no Promise), with the Tier-1 ambient
/// createRequire-backed `require` as the no-match / unresolved fallthrough
/// (builtins resolve by string; unknown packages throw the descriptive
/// `ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE`).
///
/// Gated to external modules: in first-party source a bare `require` keeps the
/// deliberate compile-time behavior (#668). Returns `Some(expr)` when matched.
pub(crate) fn try_dynamic_require(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    if !ctx.is_external_module {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    // Only the bare unshadowed global `require` — a local/func/imported binding
    // named `require` shadows it (and matched an earlier lowering arm).
    if ident.sym.as_ref() != "require"
        || ctx.lookup_local("require").is_some()
        || ctx.lookup_func("require").is_some()
        || ctx.lookup_imported_func("require").is_some()
        || call.args.len() != 1
        || call.args[0].spread.is_some()
    {
        return Ok(None);
    }
    // Literal specifiers were already handled by `try_require_literal`.
    if matches!(call.args[0].expr.as_ref(), ast::Expr::Lit(ast::Lit::Str(_))) {
        return Ok(None);
    }
    let arg = lower_expr(ctx, call.args[0].expr.as_ref())?;
    Ok(Some(Expr::DynamicImport {
        paths: Vec::new(),
        arg: Box::new(arg),
        byte_offset: call.span.lo.0,
        deferred_error: None,
        synchronous: true,
    }))
}
