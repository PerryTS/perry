//! Init-expression source classification helpers for `var`/`let`/`const`
//! declaration lowering — extracted from `var_decl.rs` (2,000-LOC cap).

use super::*;

pub(super) fn is_global_this_value(ctx: &LoweringContext, expr: &Expr) -> bool {
    matches!(expr, Expr::GlobalGet(_))
        || matches!(
            expr,
            Expr::PropertyGet { object, property }
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && property == "globalThis"
        )
        || matches!(expr, Expr::LocalGet(id) if ctx.global_this_aliases.contains(id))
}

/// #3663: classic-stream constructor export names from `node:stream`.
pub(super) const STREAM_CTOR_NAMES: [&str; 5] =
    ["Readable", "Writable", "Duplex", "Transform", "PassThrough"];

/// #3663: the string argument of a `require("<literal>")` call, if any. Unlike
/// `is_require_builtin_module` (whose allowlist is just fs/path/crypto), this
/// returns the specifier verbatim so the caller can match the module it cares
/// about (`"stream"`).
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
                skip_local_bindings.push(binding);
            }
        }
    }
    skip_local_bindings
}
