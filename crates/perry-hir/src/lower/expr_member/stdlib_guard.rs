//! #503 stdlib-namespace dynamic-dispatch guard recognisers.
//!
//! Split out of `expr_member.rs` (pure code move).

use swc_ecma_ast as ast;

/// #503 — Node-core stdlib namespace receivers whose dynamic (`obj[x]`)
/// member access is refused at compile time. These are the namespaces
/// the issue calls out: the well-known shapes used by string-based
/// obfuscation in malicious npm packages. Globals (`process`, `Buffer`)
/// and `require`-imported core modules are both covered — Buffer is
/// intentionally omitted because it is a class constructor (`new Buffer`)
/// rather than a namespace; the meaningful attack surface there is the
/// constructor itself, not dynamic property access. Keep this list in
/// sync with the docs in `docs/src/security/dynamic-dispatch.md`.
const STDLIB_NAMESPACE_NAMES: &[&str] = &[
    "process",
    "fs",
    "crypto",
    "child_process",
    "dgram",
    "net",
    "os",
    "path",
    "http",
    "https",
    "http2",
    "stream",
    "url",
    "util",
    "events",
    "dns",
    "tls",
    "querystring",
    "zlib",
    "async_hooks",
    "readline",
    "string_decoder",
    "test",
    "tty",
    "worker_threads",
];

/// #1723 — is `member` the auditable `ns[dynamicKey].staticMember` shape, where
/// the dynamic index merely selects a stdlib SUB-namespace (e.g. `path.win32` /
/// `path.posix`) and the member actually used is a *source-visible* static name?
///
/// This is the legit counterpart of the #503 obfuscation pattern
/// `ns[runtimeVar]()` — which HIDES the called method behind a runtime string.
/// Here the method name is in plaintext (`.matchesGlob`, or a literal-string
/// key that folds to a static property), and the dynamic index only picks among
/// a namespace's tiny, known set of sub-namespaces, every one of which exposes
/// the same API surface the static member already names. So nothing is hidden,
/// and the #503 refusal should not fire on the nested `ns[dynamicKey]`. The
/// discriminator is the *enclosing access shape*, not the binding origin, so
/// `require()`, `import * as`, and default-import forms all behave identically.
///
/// Returns true only when:
///   - `member.prop` is static — an `Ident` or a computed STRING-LITERAL key
///     (a numeric/dynamic key would not be auditable), AND
///   - `member.obj` (transparent TS/paren wrappers peeled) is `recv[<nonLiteral>]`
///     where `recv` resolves to a stdlib namespace.
///
/// `ns[d1][d2]` (chained dynamic — the enclosing prop is a non-literal computed
/// key) is NOT matched and stays refused. Surfaced by the #800 node-core radar:
/// `test-path-glob.js` does `path[platform].matchesGlob(path, glob)`.
pub(crate) fn stdlib_ns_subnamespace_static_access(
    ctx: &super::LoweringContext,
    member: &ast::MemberExpr,
) -> bool {
    // Enclosing access must name a STATIC (auditable) property.
    let prop_is_static = match &member.prop {
        ast::MemberProp::Ident(_) => true,
        ast::MemberProp::Computed(c) => matches!(*c.expr, ast::Expr::Lit(ast::Lit::Str(_))),
        _ => false,
    };
    if !prop_is_static {
        return false;
    }
    // Object must be `<stdlib-ns>[<nonLiteralKey>]`.
    let mut obj = member.obj.as_ref();
    loop {
        match obj {
            ast::Expr::Paren(p) => obj = p.expr.as_ref(),
            ast::Expr::TsAs(a) => obj = a.expr.as_ref(),
            ast::Expr::TsNonNull(a) => obj = a.expr.as_ref(),
            ast::Expr::TsTypeAssertion(a) => obj = a.expr.as_ref(),
            ast::Expr::TsConstAssertion(a) => obj = a.expr.as_ref(),
            ast::Expr::TsSatisfies(a) => obj = a.expr.as_ref(),
            _ => break,
        }
    }
    let inner = match obj {
        ast::Expr::Member(m) => m,
        _ => return false,
    };
    let inner_is_dynamic = match &inner.prop {
        ast::MemberProp::Computed(c) => !matches!(*c.expr, ast::Expr::Lit(ast::Lit::Str(_))),
        _ => false,
    };
    if !inner_is_dynamic {
        return false;
    }
    stdlib_namespace_receiver(ctx, inner.obj.as_ref()).is_some()
}

/// #503 — does the given AST receiver expression resolve to a known
/// stdlib namespace? Recognised shapes:
///   - bare ident matching one of `STDLIB_NAMESPACE_NAMES` (global
///     `process` or top-level imported `fs` etc.),
///   - bare ident bound to a stdlib alias via `import x from 'fs'`
///     (`ctx.builtin_module_aliases` populated by `require()` and ESM
///     default imports), or
///   - bare ident bound to a namespace import (`import * as fs from
///     'fs'`) via `ctx.native_modules` with a `None` method-name.
///
/// Returns the canonical stdlib namespace name (e.g. `"fs"`) when a
/// match is found, so the diagnostic can name the namespace concretely.
pub(crate) fn stdlib_namespace_receiver(
    ctx: &super::LoweringContext,
    obj: &ast::Expr,
) -> Option<&'static str> {
    // TS type-position wrappers like `(process as any)` and
    // `<any>process` parse as `TsAsExpr` / `TsTypeAssertion`, and the
    // `(...)` itself shows up as a `Paren`. Strip them so an idiomatic
    // `(process as any)[k]()` still surfaces `process` as the receiver.
    let mut current = obj;
    loop {
        match current {
            ast::Expr::Paren(p) => current = p.expr.as_ref(),
            ast::Expr::TsAs(a) => current = a.expr.as_ref(),
            ast::Expr::TsTypeAssertion(a) => current = a.expr.as_ref(),
            ast::Expr::TsNonNull(a) => current = a.expr.as_ref(),
            ast::Expr::TsConstAssertion(a) => current = a.expr.as_ref(),
            ast::Expr::TsSatisfies(a) => current = a.expr.as_ref(),
            _ => break,
        }
    }
    let ident = match current {
        ast::Expr::Ident(ident) => ident,
        _ => return None,
    };
    let name = ident.sym.as_ref();

    // #1701: a LOCAL binding (function param / `let` / `const`) that merely
    // shares a name with a stdlib namespace is NOT the namespace — it shadows
    // it. hono's trie-router has `path` (a URL-path string param) and does
    // `path[0] === "/"`; treating that local as the `node:path` namespace
    // false-fired the #503 refusal and blocked the whole package from
    // compiling. A real stdlib namespace is never a local: it's the global
    // (`process`) or an import, which the alias / namespace-import branches
    // below resolve. So skip the direct name-match when `name` is shadowed by
    // a local. (If a package shadows `process` with its own local, that local
    // is genuinely theirs and likewise shouldn't be refused.)
    if ctx.lookup_local(name).is_some() {
        return None;
    }

    // Direct global / module specifier match.
    if let Some(canon) = STDLIB_NAMESPACE_NAMES.iter().find(|n| **n == name) {
        return Some(*canon);
    }

    // `require()` / default-import alias: `import fs from 'fs'` →
    // builtin_module_aliases["fs"] = "fs", but the user may rename:
    // `import myFs from 'fs'` → ["myFs"] = "fs". Resolve to the
    // canonical specifier.
    for (local, module) in ctx.builtin_module_aliases.iter() {
        if local == name {
            if let Some(canon) = STDLIB_NAMESPACE_NAMES
                .iter()
                .find(|n| **n == module.as_str())
            {
                return Some(*canon);
            }
        }
    }

    // Namespace import: `import * as fs from 'fs'` — tracked as a
    // native_modules entry with method_name = None.
    for (local, module, method) in ctx.native_modules.iter() {
        if local == name && method.is_none() {
            if let Some(canon) = STDLIB_NAMESPACE_NAMES
                .iter()
                .find(|n| **n == module.as_str())
            {
                return Some(*canon);
            }
        }
    }

    None
}
