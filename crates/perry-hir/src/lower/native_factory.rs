//! Native module factories whose result is a native instance (#11568).
//!
//! A factory such as `http.createServer(...)` or `createHook({...})` returns
//! a value whose methods only dispatch when the receiver carries its native
//! class tag (`NativeMethodCall { class_name: Some(..), object: Some(..) }`,
//! matched against the `class_filter` rows of the codegen native table). A
//! `const h = createHook(...)` binding gets that tag from the var-decl arm;
//! the chained form `createHook(...).enable()` has no binding and has to
//! derive it from the factory call itself. Both consult this ONE table, so a
//! factory added for the bound form automatically works chained. (Before
//! #11568 the chained table was a separate five-row copy covering only the
//! http/https/http2/tls factories, and `createHook(...).enable()` read
//! `enable` off an untagged foreign pointer as `undefined`.)

use swc_ecma_ast as ast;

/// The `(instance module, class)` a call to `module.method(...)` returns, if
/// it is a native-instance factory. The instance module differs from the
/// factory's module only for `https.request`/`https.get`, whose
/// `ClientRequest` methods are registered under `"http"`.
pub(crate) fn native_factory_result_class(
    module: &str,
    method: &str,
) -> Option<(&'static str, &'static str)> {
    let class = match (module, method) {
        ("async_hooks", "createHook") => "AsyncHook",
        ("dns" | "dns/promises", "Resolver") => "Resolver",
        ("mysql2" | "mysql2/promise", "createPool") => "Pool",
        ("mysql2" | "mysql2/promise", "createConnection") => "Connection",
        ("pg", "connect") => "Client",
        // `http.request(...).on(...)` (#2208): the ClientRequest's methods
        // live under module "http" for both schemes.
        ("http" | "https", "request" | "get") => return Some(("http", "ClientRequest")),
        ("http", "createServer") => "HttpServer",
        ("https", "createServer") => "HttpsServer",
        ("tls", "createServer" | "Server") => "Server",
        ("http2", "createSecureServer") => "Http2SecureServer",
        // readline.createInterface() returns a handle whose .question/.on/
        // .close dispatch via the ("readline", class "Interface") rows.
        ("readline", "createInterface") => "Interface",
        // perry/tui state(initial) and the ink-shape hooks (#358, #679).
        ("perry/tui", "state") => "State",
        ("perry/tui", "useApp") => "TuiApp",
        ("perry/tui", "useStdout") => "TuiStdout",
        ("perry/tui", "useRef") => "RefBox",
        ("perry/tui", "useFocusManager") => "FocusManager",
        _ => return None,
    };
    Some((module_static(module)?, class))
}

/// The native class a factory CALL expression returns: the named-import form
/// (`createServer(...)`) and the namespace form (`http.createServer(...)`).
/// `None` for any other call, so non-factory chains fall through unchanged.
/// Issue #2041 (chained `createServer(...).listen()`), generalized by #11568.
pub(crate) fn factory_call_class(
    ctx: &crate::lower::LoweringContext,
    call: &ast::CallExpr,
) -> Option<(&'static str, &'static str)> {
    let callee_expr = match &call.callee {
        ast::Callee::Expr(e) => e.as_ref(),
        _ => return None,
    };
    // Resolve to `(module, method)`, handling both the named-import form
    // (`createServer(...)`) and the namespace form (`http.createServer(...)`).
    let (module, method): (String, String) = match callee_expr {
        ast::Expr::Member(member) => {
            let obj_ident = match member.obj.as_ref() {
                ast::Expr::Ident(i) => i,
                _ => return None,
            };
            let (module, _) = ctx.lookup_native_module(obj_ident.sym.as_ref())?;
            let method = match &member.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            (module.to_string(), method)
        }
        ast::Expr::Ident(ident) => {
            let (module, method_opt) = ctx.lookup_native_module(ident.sym.as_ref())?;
            (module.to_string(), method_opt?.to_string())
        }
        _ => return None,
    };
    native_factory_result_class(&module, &method)
}

/// Methods that return their receiver (Node's `hook.enable()` / `.disable()`
/// return the `AsyncHook`), so `createHook(...).enable()` still evaluates to
/// the native instance.
pub(crate) fn method_returns_receiver(module: &str, class: &str, method: &str) -> bool {
    matches!(
        (module, class, method),
        ("async_hooks", "AsyncHook", "enable" | "disable")
    )
}

/// The native class an expression evaluates to when it is a factory call or a
/// receiver-returning method chained on one: `createHook(...)`,
/// `createHook(...).enable()`. Used to record `function f() { return
/// createHook(...).enable(); }` as returning a native instance, exactly like
/// `const h = createHook(...); h.enable(); return h;`, so `f().disable()` keeps
/// native dispatch instead of treating the raw handle as a JS object.
pub(crate) fn native_class_of_expr(
    ctx: &crate::lower::LoweringContext,
    expr: &swc_ecma_ast::Expr,
) -> Option<(&'static str, &'static str)> {
    let call = match expr {
        ast::Expr::Call(call) => call,
        ast::Expr::Paren(paren) => return native_class_of_expr(ctx, &paren.expr),
        _ => return None,
    };
    if let Some(found) = factory_call_class(ctx, call) {
        return Some(found);
    }
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let ast::Expr::Member(member) = callee.as_ref() else {
        return None;
    };
    let ast::MemberProp::Ident(method) = &member.prop else {
        return None;
    };
    let (module, class) = native_class_of_expr(ctx, &member.obj)?;
    method_returns_receiver(module, class, method.sym.as_ref()).then_some((module, class))
}

/// `module` as a `'static` string, for the modules the table above names.
fn module_static(module: &str) -> Option<&'static str> {
    Some(match module {
        "async_hooks" => "async_hooks",
        "dns" => "dns",
        "dns/promises" => "dns/promises",
        "mysql2" => "mysql2",
        "mysql2/promise" => "mysql2/promise",
        "pg" => "pg",
        "http" => "http",
        "https" => "https",
        "tls" => "tls",
        "http2" => "http2",
        "readline" => "readline",
        "perry/tui" => "perry/tui",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::native_factory_result_class as f;

    #[test]
    fn factories_map_to_their_instance_class() {
        assert_eq!(
            f("async_hooks", "createHook"),
            Some(("async_hooks", "AsyncHook"))
        );
        assert_eq!(f("http", "createServer"), Some(("http", "HttpServer")));
        assert_eq!(f("https", "get"), Some(("http", "ClientRequest")));
        assert_eq!(
            f("readline", "createInterface"),
            Some(("readline", "Interface"))
        );
        assert_eq!(f("perry/tui", "useApp"), Some(("perry/tui", "TuiApp")));
        assert_eq!(f("async_hooks", "executionAsyncId"), None);
        assert_eq!(f("fs", "readFileSync"), None);
    }

    fn lower(source: &str) -> String {
        let module = perry_parser::parse_typescript(source, "factory.ts").expect("source parses");
        let hir =
            crate::lower::lower_module(&module, "factory", "factory.ts").expect("source lowers");
        format!("{hir:?}")
    }

    /// #11568: the chained receiver gets the same class tag as the bound one,
    /// for every factory in the table — not only the http servers #2041 wired.
    #[test]
    fn chained_factory_result_is_a_tagged_native_receiver() {
        for (source, tag) in [
            (
                "import { createHook } from \"node:async_hooks\";\ncreateHook({ init() {} }).enable();\n",
                "NativeMethodCall { module: \"async_hooks\", class_name: Some(\"AsyncHook\"), object: Some(",
            ),
            (
                "import * as ah from \"async_hooks\";\nconst h = ah.createHook({}).enable();\n",
                "NativeMethodCall { module: \"async_hooks\", class_name: Some(\"AsyncHook\"), object: Some(",
            ),
            // The #11568 fixture shape: a helper returning the chained
            // result keeps native dispatch for the caller's `.disable()`.
            (
                "import { createHook } from \"node:async_hooks\";\nfunction track() { return createHook({ init() {} }).enable(); }\nconst first = track();\nfirst.disable();\n",
                "NativeMethodCall { module: \"async_hooks\", class_name: Some(\"AsyncHook\"), object: Some(LocalGet(",
            ),
            (
                "import * as readline from \"node:readline\";\nreadline.createInterface({ input: process.stdin }).close();\n",
                "NativeMethodCall { module: \"readline\", class_name: Some(\"Interface\"), object: Some(",
            ),
        ] {
            let dump = lower(source);
            assert!(dump.contains(tag), "missing `{tag}` for:\n{source}\n{dump}");
        }
    }
}
