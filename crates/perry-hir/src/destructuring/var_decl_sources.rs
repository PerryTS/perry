//! Init-expression source classification helpers for `var`/`let`/`const`
//! declaration lowering — extracted from `var_decl.rs` (2,000-LOC cap).

use super::*;

pub(super) fn is_global_this_value(ctx: &LoweringContext, expr: &Expr) -> bool {
    matches!(expr, Expr::GlobalGet(_))
        || matches!(
            expr,
            Expr::PropertyGet { object, property, .. }
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && property == "globalThis"
        )
        || matches!(expr, Expr::LocalGet(id) if ctx.global_this_aliases.contains(id))
}

/// #3663: classic-stream constructor export names from `node:stream`.
pub(super) const STREAM_CTOR_NAMES: [&str; 5] =
    ["Readable", "Writable", "Duplex", "Transform", "PassThrough"];

/// #3663: the string argument of a `require("<literal>")` call, if any —
/// returned verbatim so callers can match the module they care about
/// (`"stream"`) or resolve it (`require_resolvable_native_specifier`).
pub(super) fn require_literal_specifier(init: &ast::Expr) -> Option<String> {
    let ast::Expr::Call(call) = init else {
        return None;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let ast::Expr::Ident(ident) = callee.as_ref() else {
        return None;
    };
    if ident.sym.as_ref() != "require" {
        return None;
    }
    let arg = call.args.first()?;
    if arg.spread.is_some() {
        return None;
    }
    let ast::Expr::Lit(ast::Lit::Str(s)) = arg.expr.as_ref() else {
        return None;
    };
    s.value.as_str().map(|s| s.to_string())
}

/// #5216: the `node:`-stripped specifier of a `require("<literal>")` call iff
/// the module statically resolves to a Perry-supported native/Node-builtin
/// module. Returns `None` for non-literal args, packages/files Perry can't
/// resolve as a native module, or anything else — those keep the legacy
/// compile-time `require(...)` refusal / fall-through. Prioritizes Node
/// builtins (`readline`, `os`, `path`, `util`, `fs`, …) which real apps hit
/// via `require(...)`.
pub(crate) fn require_resolvable_native_specifier(init: &ast::Expr) -> Option<String> {
    resolvable_native_module_for_spec(&require_literal_specifier(init)?)
}

/// #8342: is the bare global `require` shadowed by a local / function-scoped /
/// imported binding named `require`? This is exactly the situation inside a
/// CJS-wrapped module, where the wrap injects a synthetic
/// `function require(specifier) { ... }` (with a `createRequire`-backed
/// built-in arm, see `cjs_wrap::wrap`) into the IIFE body. Mirrors the guard in
/// `expr_call::intrinsics::try_require_literal` — which bails on the same
/// shadowing — but the destructuring `var`/`let`/`const` paths run BEFORE call
/// lowering and intercept `let x = require("process")` first, so without this
/// check they would register `x` as a native-module namespace binding and drop
/// the runtime local. In a CJS-wrapped module the native-module namespace is
/// not initialized, so `x` resolves to nothing at runtime
/// (`ReferenceError: node_process is not defined`). Returning `true` here tells
/// the callers to let the `require(...)` call flow through to the synthetic
/// require at runtime, which resolves builtins via `createRequire`.
pub(crate) fn require_is_shadowed_by_local(ctx: &LoweringContext) -> bool {
    // #8465: a local `require` created by node:module's `createRequire` IS the
    // module-scoped require — for builtin specifiers it returns exactly the
    // native namespace, so it must not suppress the static namespace fast
    // path (pre-#8343 behavior for this idiom; regressed net.connect reached
    // as a bound value to a runtime dispatch arm that is null in
    // default-feature builds).
    let local_shadow =
        ctx.lookup_local("require").is_some() && !ctx.require_local_is_create_require;
    local_shadow
        || ctx.lookup_func("require").is_some()
        || ctx.lookup_imported_func("require").is_some()
}

/// The CJS-to-ESM wrapper's synthetic `require` is deliberately a real local
/// function, so the ordinary native-require fast paths must not steal calls
/// from it (see #8342).  There is one narrower exception: a destructured
/// constructor supplied by a Perry native npm shim has no runtime namespace
/// value to destructure in the first place.  The wrapper-generated helper
/// pair identifies that compiler-owned function without mistaking an ordinary
/// user `function require(...) { ... }` for the intrinsic.
fn require_is_perry_cjs_wrapper(ctx: &LoweringContext) -> bool {
    ctx.lookup_func("require").is_some()
        && ctx.lookup_func("__perry_cjs_require_error").is_some()
        && ctx.lookup_func("__perry_cjs_require_is_builtin").is_some()
}

/// Native npm-shim destructures that can be lowered exactly like named ESM
/// imports even inside Perry's CJS wrapper.  Keep this an explicit surface:
/// broadening it to every native module would regress #8342's builtin-module
/// namespace semantics, while broadening it to arbitrary lru-cache exports
/// would pretend the partial shim implements API that it does not have.
fn cjs_wrapper_static_native_destructure(
    ctx: &LoweringContext,
    init: &ast::Expr,
    obj_pat: &ast::ObjectPat,
) -> bool {
    if !require_is_perry_cjs_wrapper(ctx)
        || require_literal_specifier(init).as_deref() != Some("lru-cache")
    {
        return false;
    }

    !obj_pat.props.is_empty()
        && obj_pat.props.iter().all(|prop| match prop {
            ast::ObjectPatProp::Assign(assign) => assign.key.sym.as_ref() == "LRUCache",
            ast::ObjectPatProp::KeyValue(kv) => match &kv.key {
                ast::PropName::Ident(key) => key.sym.as_ref() == "LRUCache",
                ast::PropName::Str(key) => key.value.as_str() == Some("LRUCache"),
                _ => false,
            },
            ast::ObjectPatProp::Rest(_) => false,
        })
}

/// #5216: the canonical (`node:`-stripped) native module name for a require
/// specifier `raw`, iff it resolves to a Perry-supported native/Node-builtin
/// module; otherwise `None`. `node:`-prefixed specifiers must name a real Node
/// builtin (parity with the ESM import path, which bails on
/// `node:<not-a-builtin>`).
pub(crate) fn resolvable_native_module_for_spec(raw: &str) -> Option<String> {
    let normalized = raw.strip_prefix("node:").unwrap_or(raw).to_string();
    if raw.starts_with("node:") && !is_node_builtin_module(&normalized) {
        return None;
    }
    if is_native_module(&normalized) {
        Some(normalized)
    } else {
        None
    }
}

/// #5216: register a `const <local> = require("<spec>")` binding exactly as the
/// equivalent `import * as <local> from "<spec>"` namespace import would, so the
/// require result behaves like a module-namespace value (member dispatch,
/// `typeof`, etc.) and reuses the existing native-module machinery. `spec` must
/// already be a resolved native module name (see
/// `require_resolvable_native_specifier`); the caller emits NO runtime `let`,
/// matching how namespace imports of native modules bind nothing observable.
pub(crate) fn register_require_namespace_binding(
    ctx: &mut LoweringContext,
    local: &str,
    spec: &str,
) {
    // Mirror `module_decl.rs`'s `ImportSpecifier::Namespace` native branch.
    let native_source = if spec == "process" {
        "process.namespace".to_string()
    } else {
        spec.to_string()
    };
    ctx.register_native_module(local.to_string(), native_source, None);
    ctx.register_builtin_module_alias(local.to_string(), spec.to_string());
    // The top-level pre-scan may have already registered `local` as a module
    // var (it can't know the initializer is a require yet). Drop that local so
    // a bare `local` / `local.member` read resolves to the native module rather
    // than an always-`undefined` `LocalGet` — `import * as local` never creates
    // a local, so this is exact namespace-import parity.
    ctx.remove_local_binding(local);
}

/// #3663: resolve the builtin module that a destructuring RHS reads from.
/// Handles `const { Readable } = require('stream')` (CJS), and the namespace
/// forms `const { Readable } = stream` where `stream` is an `import * as` /
/// `const stream = require('stream')` alias. Returns the canonical module name.
pub(super) fn destructure_builtin_module_source(
    ctx: &LoweringContext,
    init: &ast::Expr,
) -> Option<String> {
    if let Some(module) = require_literal_specifier(init) {
        return Some(module);
    }
    if let ast::Expr::Ident(ident) = init {
        let name = ident.sym.as_ref();
        if let Some(module) = ctx.lookup_builtin_module_alias(name) {
            return Some(module.to_string());
        }
        if let Some((module, None)) = ctx.lookup_native_module(name) {
            return Some(module.to_string());
        }
    }
    None
}

/// #3663 / #4905: register destructured builtin-module members as
/// native-module aliases, mirroring what ESM named imports get
/// generically in `module_decl.rs` (`import { connect } from 'net'`).
/// Without the alias, the binding only holds the runtime property read
/// off the reified module object — which is `undefined` for exports
/// whose value-read path isn't reified (`net.connect`), so the
/// canonical CJS corpus idiom `const { connect } = require('net')`
/// threw `value is not a function` at the call site.
///
/// Returns the binding names that must NOT also bind a runtime local:
/// a local would shadow the alias at call sites (the call lowers as a
/// closure call of the undefined local instead of the native-table
/// row). ESM named imports never create a local, so skipping the
/// binding is exact parity. Stream ctors keep their local (their
/// runtime member read works, and #3663 shipped with it).
pub(super) fn register_destructured_stream_ctors(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
) -> Vec<String> {
    let ast::Pat::Object(obj_pat) = &decl.name else {
        return Vec::new();
    };
    let Some(init) = decl.init.as_deref() else {
        return Vec::new();
    };

    // #8342: inside a CJS-wrapped module the wrap's synthetic
    // `function require(...)` shadows the bare global `require`, and its
    // built-in arm resolves `require("process")` etc. via `createRequire` at
    // runtime. Don't register destructured members as native-module aliases
    // here — the native namespace isn't initialized in a CJS-wrapped module,
    // so the bindings would be undefined at runtime. Let the destructure run
    // off the runtime `require(...)` call result instead.
    if require_is_shadowed_by_local(ctx)
        && require_literal_specifier(init).is_some()
        && !cjs_wrapper_static_native_destructure(ctx, init, obj_pat)
    {
        return Vec::new();
    }

    // #5216: `const { createInterface } = require("readline")` — when the RHS is
    // a `require("<native-spec>")` literal, register EVERY destructured member
    // as a native named member, exactly as `import { createInterface } from
    // "readline"` does (`register_native_module(binding, module, Some(key))`).
    // This generalizes the stream/net special-cases below to all resolvable
    // native/Node-builtin modules. Skip every bound local so call sites route
    // through the static native table (a runtime local read is `undefined` for
    // value-unreified exports — exact ESM-named-import parity).
    if let Some(module) = require_resolvable_native_specifier(init) {
        // `stream` and `net` retain their tuned allowlist + local-binding
        // behavior below (stream ctors keep their runtime local); fall through.
        if module != "stream" && module != "net" {
            let mut skip_local_bindings = Vec::new();
            for prop in &obj_pat.props {
                let (key, binding) = match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        (name.clone(), name)
                    }
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(i) => i.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            _ => continue,
                        };
                        let ast::Pat::Ident(binding) = kv.value.as_ref() else {
                            continue;
                        };
                        (key, binding.id.sym.to_string())
                    }
                    // Rest (`...rest`) has no static key — leave it on the
                    // runtime-binding path (it reads the reified module object).
                    _ => continue,
                };
                ctx.register_native_module(binding.clone(), module.clone(), Some(key));
                // #5364 interaction: the module-level forward-declaration pass
                // now pre-registers destructuring leaves as module-var locals.
                // For a native-alias leaf that local is never written (the
                // runtime destructuring is skipped below), so a bare
                // `binding` / `typeof binding` read would resolve to that stale
                // `undefined` local and shadow the native alias. Drop it so the
                // name resolves to the native table, exactly as the simple-ident
                // `register_require_namespace_binding` path does.
                ctx.remove_local_binding(&binding);
                skip_local_bindings.push(binding);
            }
            return skip_local_bindings;
        }
    }

    let Some(module) = destructure_builtin_module_source(ctx, init) else {
        return Vec::new();
    };
    let allowed: &[&str] = match module.as_str() {
        "stream" => &STREAM_CTOR_NAMES,
        // #4905: net's factory exports — call sites lower through the
        // static native table rows, so the alias works even though the
        // runtime member read is undefined.
        "net" => &["connect", "createConnection"],
        _ => return Vec::new(),
    };
    let mut skip_local_bindings = Vec::new();
    for prop in &obj_pat.props {
        let (key, binding) = match prop {
            ast::ObjectPatProp::Assign(assign) => {
                let name = assign.key.sym.to_string();
                (name.clone(), name)
            }
            ast::ObjectPatProp::KeyValue(kv) => {
                let key = match &kv.key {
                    ast::PropName::Ident(i) => i.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                let ast::Pat::Ident(binding) = kv.value.as_ref() else {
                    continue;
                };
                (key, binding.id.sym.to_string())
            }
            _ => continue,
        };
        if allowed.contains(&key.as_str()) {
            ctx.register_native_module(binding.clone(), module.clone(), Some(key));
            if module == "net" {
                // Same #5364 interaction as the generic native branch above:
                // drop the pre-registered module-var local for skipped leaves
                // so the name resolves to the native alias, not a stale
                // `undefined` local. (Stream ctors keep their runtime local and
                // are not skipped, so they are intentionally left untouched.)
                ctx.remove_local_binding(&binding);
                skip_local_bindings.push(binding);
            }
        }
    }
    skip_local_bindings
}
