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
