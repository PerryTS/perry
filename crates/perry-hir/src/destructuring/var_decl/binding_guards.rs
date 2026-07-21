//! Strict-mode binding-identifier checks and native-instance/module
//! shadow-tombstones for a simple `let/const/var` identifier binding
//! (extracted from `var_decl.rs`'s `Pat::Ident` arm).

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

/// Applies the strict-mode early error and the native-instance/module
/// shadow tombstones for a fresh `name` binding. Mirrors the original
/// inline block verbatim.
pub(crate) fn apply_binding_guards(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    name: &str,
) -> Result<()> {
    // Strict-mode early error: `var eval` / `var arguments` (and the
    // let/const forms) are a SyntaxError (ECMA-262 BindingIdentifier
    // static semantics). Surfaced as a compile error so the test262
    // negative cases agree with Node (12.2.1-22-s).
    if ctx.current_strict && matches!(name, "eval" | "arguments") {
        anyhow::bail!(
            "SyntaxError: unexpected `{}` as a strict-mode binding identifier",
            name
        );
    }

    // A fresh binding of `name` must not inherit a stale
    // native-instance tag that an UNRELATED earlier binding of the
    // same name registered (e.g. a minified webpack bundle that
    // `new FormData()`-binds a local `i` in one factory and reuses
    // `var i = { exports: {} }` as the require-cache object in
    // another). `native_instances` is module-global + last-match-wins,
    // so push a tombstone to shadow the old tag here, BEFORE the
    // native-instance registration checks below — if THIS init is
    // itself a native instance, it re-registers after the tombstone
    // and last-match-wins keeps the correct tag. Without this, a plain
    // `i.exports` read mis-routes through the stale module's native
    // method dispatch and folds to 0 (Next.js app-page-turbo `require`
    // → React's `exports.Fragment = …` "read only property" throw).
    if ctx.lookup_native_instance(name).is_some() {
        ctx.shadow_native_instance(name.to_string());
    }

    // #wall5: same scope-leak for native MODULES. `native_modules_index`
    // is module-global + first-match-wins (no scope tracking), so a
    // local re-bind of a name a top-level `const url = require('url')`
    // registered (e.g. undici's `const util = require('./util')`, or a
    // local `const url = []` / a URL object) would mis-resolve
    // `util.isStream` / `url.push` through the node-module dispatch and
    // fire the unimplemented-API gate (Next.js app-page-turbo: 88× url.push,
    // 84× util.destroy, the url.o render throw). Shadow the module here —
    // UNLESS this very decl IS the native-module binding (`= require('url')`
    // of a node-core module), which must keep resolving as the module.
    if ctx.lookup_native_module(name).is_some() {
        let binds_native_module = decl.init.as_deref().is_some_and(|init| {
            if let ast::Expr::Call(call) = init {
                if let ast::Callee::Expr(callee) = &call.callee {
                    if let ast::Expr::Ident(id) = callee.as_ref() {
                        if &*id.sym == "require" {
                            if let Some(ast::Expr::Lit(ast::Lit::Str(s))) =
                                call.args.first().map(|a| a.expr.as_ref())
                            {
                                if let Some(spec) = s.value.as_str() {
                                    let bare = spec.strip_prefix("node:").unwrap_or(spec);
                                    return perry_api_manifest::is_node_core_module(bare);
                                }
                            }
                        }
                    }
                }
            }
            false
        });
        if !binds_native_module {
            ctx.shadow_native_module_if_present(name);
        }
    }

    Ok(())
}
