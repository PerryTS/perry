//! Lowering of variable declarations that may carry destructuring patterns.

use super::*;

use super::var_decl_sources::*;

/// Lower a variable declaration, handling array destructuring patterns.
/// Returns a vector of statements (multiple for destructuring, single for simple bindings).
pub(crate) fn lower_var_decl_with_destructuring(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    mutable: bool,
    is_var_decl: bool,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match &decl.name {
        ast::Pat::Ident(ident) => {
            // Simple binding: let x = expr
            let name = ident.id.sym.to_string();

            // Strict-mode early error: `var eval` / `var arguments` (and the
            // let/const forms) are a SyntaxError (ECMA-262 BindingIdentifier
            // static semantics). Surfaced as a compile error so the test262
            // negative cases agree with Node (12.2.1-22-s).
            if ctx.current_strict && matches!(name.as_str(), "eval" | "arguments") {
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
            if ctx.lookup_native_instance(&name).is_some() {
                ctx.shadow_native_instance(name.clone());
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
            if ctx.lookup_native_module(&name).is_some() {
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
                    ctx.shadow_native_module_if_present(&name);
                }
            }

            // #809: tag locals provably bound to a plain object (an object
            // literal or `Object.create(...)`). `static_receiver_class`
            // consults this so `x.toJSON()` / `.toString()` / `.valueOf()`
            // etc. on such a local fall through to generic dynamic dispatch
            // instead of the Date intrinsics (which would interpret the
            // object pointer's bits as a timestamp).
            if let Some(init_expr) = decl.init.as_deref() {
                let is_plain_object = match init_expr {
                    ast::Expr::Object(_) => true,
                    ast::Expr::Call(call) => {
                        if let ast::Callee::Expr(callee) = &call.callee {
                            if let ast::Expr::Member(m) = callee.as_ref() {
                                let obj_is = |name: &str| matches!(m.obj.as_ref(), ast::Expr::Ident(o) if o.sym.as_ref() == name);
                                let prop_is = |name: &str| matches!(&m.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == name);
                                // Object.create(...) — #809.
                                (obj_is("Object") && prop_is("create"))
                                    // #1387: `performance.mark(...)` /
                                    // `performance.measure(...)` return a
                                    // PerformanceEntry — a plain shaped object,
                                    // never a Date — so `entry.toJSON()` (and
                                    // `.toString()`/`.valueOf()`) must skip the
                                    // ambiguous-Date arms and fall through to
                                    // generic dispatch (which finds the
                                    // synthesized PerformanceEntry#toJSON).
                                    || (obj_is("performance")
                                        && (prop_is("mark") || prop_is("measure")))
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if is_plain_object {
                    ctx.plain_object_locals.insert(name.clone());
                }
            }
            let mut ty = ident
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or_else(|| {
                    // No type annotation: try local inference from initializer
                    if let Some(init_expr) = &decl.init {
                        let inferred = infer_type_from_expr(init_expr, ctx);
                        if !matches!(inferred, Type::Any) {
                            return inferred;
                        }
                        // Fall back to tsgo resolved types if available
                        if let Some(resolved) = ctx.resolved_types.as_ref() {
                            if let Some(resolved_ty) = resolved.get(&(ident.id.span.lo.0)) {
                                return resolved_ty.clone();
                            }
                        }
                    }
                    Type::Any
                });

            // If no type annotation, infer from new Set<T>() or new Map<K, V>() or new URLSearchParams() expressions
            if matches!(ty, Type::Any) {
                if let Some(init_expr) = &decl.init {
                    if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                        if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                            let class_name = class_ident.sym.as_ref();
                            if class_name == "Set" || class_name == "Map" {
                                // Extract type arguments from new Set<T>() or new Map<K, V>()
                                let type_args: Vec<Type> = new_expr
                                    .type_args
                                    .as_ref()
                                    .map(|ta| {
                                        ta.params.iter().map(|t| extract_ts_type(t)).collect()
                                    })
                                    .unwrap_or_default();
                                ty = Type::Generic {
                                    base: class_name.to_string(),
                                    type_args,
                                };
                            } else if class_name == "URLSearchParams" {
                                ty = Type::Named("URLSearchParams".to_string());
                            } else if class_name == "TextEncoder" {
                                ty = Type::Named("TextEncoder".to_string());
                            } else if class_name == "TextDecoder" {
                                ty = Type::Named("TextDecoder".to_string());
                            } else if matches!(
                                class_name,
                                "EventTarget" | "Event" | "CustomEvent" | "DOMException"
                            ) {
                                ty = Type::Named(class_name.to_string());
                            } else if matches!(
                                class_name,
                                "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                            ) {
                                ty = Type::Named(class_name.to_string());
                            } else if class_name == "Uint8Array" || class_name == "Buffer" {
                                ty = Type::Named("Uint8Array".to_string());
                            } else if matches!(
                                class_name,
                                "Int8Array"
                                    | "Int16Array"
                                    | "Uint16Array"
                                    | "Int32Array"
                                    | "Uint32Array"
                                    | "Float16Array"
                                    | "Float32Array"
                                    | "Float64Array"
                            ) {
                                ty = Type::Named(class_name.to_string());
                            } else if ctx.classes_index.contains_key(class_name) {
                                // User-defined class: infer type from new ClassName(...)
                                let type_args: Vec<Type> = new_expr
                                    .type_args
                                    .as_ref()
                                    .map(|ta| {
                                        ta.params.iter().map(|t| extract_ts_type(t)).collect()
                                    })
                                    .unwrap_or_default();
                                if type_args.is_empty() {
                                    ty = Type::Named(class_name.to_string());
                                } else {
                                    ty = Type::Generic {
                                        base: class_name.to_string(),
                                        type_args,
                                    };
                                }
                            }
                        }
                    }
                }
            }

            // #1642/#1643: a `const x = <stream>.getReader(...)` / `.getWriter(...)`
            // / `ReadableStream.from(...)` binding is typed Any by inference, but
            // the result is a Web Streams native instance. Type it as the stream
            // class so codegen `receiver_class_name` resolves value-read method
            // binds (`typeof reader.read === "function"`) for the Any-typed
            // local. Safe: the call path (lower/expr_call/static_and_instance.rs)
            // dispatches via the native-instance registry, not this declared type.
            if matches!(ty, Type::Any) {
                if let Some(init_expr) = &decl.init {
                    if let ast::Expr::Call(call) = init_expr.as_ref() {
                        if let ast::Callee::Expr(callee) = &call.callee {
                            if let ast::Expr::Member(m) = callee.as_ref() {
                                if let ast::MemberProp::Ident(prop) = &m.prop {
                                    // Peel `as T` / `!` / `as const` / parens on
                                    // the receiver (`(rs as any).getReader(...)`).
                                    let mut obj_inner: &ast::Expr = m.obj.as_ref();
                                    loop {
                                        obj_inner = match obj_inner {
                                            ast::Expr::TsAs(x) => &x.expr,
                                            ast::Expr::TsNonNull(x) => &x.expr,
                                            ast::Expr::TsSatisfies(x) => &x.expr,
                                            ast::Expr::TsTypeAssertion(x) => &x.expr,
                                            ast::Expr::TsConstAssertion(x) => &x.expr,
                                            ast::Expr::Paren(x) => &x.expr,
                                            _ => break,
                                        };
                                    }
                                    if let ast::Expr::Ident(obj_id) = obj_inner {
                                        let method = prop.sym.as_ref();
                                        let recv_class = ctx
                                            .lookup_native_instance(obj_id.sym.as_ref())
                                            .map(|(_, c)| c.to_string());
                                        if method == "getReader"
                                            && recv_class.as_deref() == Some("ReadableStream")
                                        {
                                            ty = Type::Named(
                                                "ReadableStreamDefaultReader".to_string(),
                                            );
                                        } else if method == "getWriter"
                                            && recv_class.as_deref() == Some("WritableStream")
                                        {
                                            ty = Type::Named(
                                                "WritableStreamDefaultWriter".to_string(),
                                            );
                                        } else if method == "from"
                                            && obj_id.sym.as_ref() == "ReadableStream"
                                        {
                                            ty = Type::Named("ReadableStream".to_string());
                                        } else if method == "from"
                                            && obj_id.sym.as_ref() == "Readable"
                                        {
                                            ty = Type::Named("Readable".to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a native class instantiation and register it
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                        let class_name = class_ident.sym.as_ref();
                        // A user `class Big {...}` in scope shadows the
                        // hardcoded library-name fallback below. Without
                        // this gate `class Big { f0=0; ... } const b = new
                        // Big()` routed through big.js's handle-based
                        // dispatch so every property read returned 0.
                        let user_class_defined = ctx.classes_index.contains_key(class_name)
                            || ctx.pending_classes.iter().any(|c| c.name == class_name);
                        // First try the general native module lookup (covers all imported native classes)
                        let module_name =
                            if let Some((m, method)) = ctx.lookup_native_module(class_name) {
                                match (m, method) {
                                    ("url", Some("URL" | "URLSearchParams"))
                                    | ("util", Some("TextEncoder" | "TextDecoder")) => None,
                                    _ => Some(m.to_string()),
                                }
                            } else if user_class_defined {
                                None
                            } else {
                                // Fallback to hardcoded map for known classes.
                                // Pool/Client/MongoClient are intentionally NOT
                                // listed here: those names collide with user
                                // classes and TS-source npm packages (e.g.
                                // `@perryts/mysql` exports its own `Pool`), so
                                // an unconditional mapping misclassified them
                                // as `pg`/`mongodb` and routed `.query()` /
                                // `.end()` to `js_pg_*` runtime symbols that
                                // don't exist in user TS code, failing at link
                                // time. The legitimate `import { Pool } from
                                // "pg"` flow is caught by the general lookup
                                // above. (Issue #536.)
                                match class_name {
                                    "EventEmitter" | "EventEmitterAsyncResource" => {
                                        Some("events".to_string())
                                    }
                                    "AsyncLocalStorage" => Some("async_hooks".to_string()),
                                    "AsyncResource" => Some("async_hooks".to_string()),
                                    // #2875: explicit-resource-management stacks.
                                    // Registering the binding as a native instance
                                    // routes `stack.use/.adopt/.defer/.dispose/
                                    // .move/.disposed` through the
                                    // `__disposable__` dispatch rows.
                                    "DisposableStack" | "AsyncDisposableStack" => {
                                        Some("__disposable__".to_string())
                                    }
                                    "WebSocket" | "WebSocketServer" => Some("ws".to_string()),
                                    "Redis" => Some("ioredis".to_string()),
                                    "LRUCache" => Some("lru-cache".to_string()),
                                    "Command" => Some("commander".to_string()),
                                    "Big" => Some("big.js".to_string()),
                                    "Decimal" => Some("decimal.js".to_string()),
                                    "BigNumber" => Some("bignumber.js".to_string()),
                                    _ => None,
                                }
                            };
                        // Handle-backed constructors dispatch through
                        // HANDLE_*_DISPATCH; don't register as typed native
                        // instances (see the mirroring gates in lower.rs).
                        let module_name = match (class_name, module_name.as_deref()) {
                            ("StringDecoder", Some("string_decoder")) => None,
                            (
                                "DiffieHellman" | "DiffieHellmanGroup",
                                Some("crypto" | "node:crypto"),
                            ) => None,
                            _ => module_name,
                        };
                        if let Some(module) = module_name {
                            ctx.register_native_instance(
                                name.clone(),
                                module,
                                class_name.to_string(),
                            );
                        }
                    } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                        if let (
                            ast::Expr::Ident(module_ident),
                            ast::MemberProp::Ident(class_ident),
                        ) = (member.obj.as_ref(), &member.prop)
                        {
                            let module_alias = module_ident.sym.as_ref();
                            if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                                let class_name = class_ident.sym.as_ref();
                                let is_known_native_class = matches!(
                                    (module_name, class_name),
                                    ("async_hooks", "AsyncLocalStorage" | "AsyncResource")
                                        // #2129: `new http.Agent()` /
                                        // `new https.Agent()` share the
                                        // class-filtered ("http", "Agent")
                                        // native table rows.
                                        | ("http" | "https", "Agent")
                                        | ("net" | "node:net", "BlockList" | "SocketAddress")
                                        | ("dns" | "dns/promises", "Resolver")
                                        | ("vm", "SourceTextModule" | "SyntheticModule")
                                        | ("sqlite", "DatabaseSync")
                                ) || (module_name == "stream"
                                    && STREAM_CTOR_NAMES.contains(&class_name));
                                if is_known_native_class {
                                    let (mod_for_class, cls_for_class) =
                                        match (module_name, class_name) {
                                            ("http" | "https", "Agent") => ("http", "Agent"),
                                            ("net" | "node:net", _) => ("net", class_name),
                                            _ => (module_name, class_name),
                                        };
                                    ctx.register_native_instance(
                                        name.clone(),
                                        mod_for_class.to_string(),
                                        cls_for_class.to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // #1645: `const rs = ReadableStream.from(iterable)` — the `.from`
            // Call result is typed Any, so register the binding as a
            // ReadableStream native instance (mirroring `new ReadableStream`'s
            // typing). Without this, `rs.getReader()` / `for await (const c of
            // rs)` fall to generic dispatch on the numeric stream handle and
            // fail. The Call itself is routed to `js_readable_stream_from_iterable`
            // in codegen (expr/calls.rs).
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Call(call) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call.callee {
                        if let ast::Expr::Member(m) = callee.as_ref() {
                            if let ast::MemberProp::Ident(prop) = &m.prop {
                                if prop.sym.as_ref() == "from" {
                                    let mut obj_inner: &ast::Expr = m.obj.as_ref();
                                    loop {
                                        obj_inner = match obj_inner {
                                            ast::Expr::TsAs(x) => &x.expr,
                                            ast::Expr::TsNonNull(x) => &x.expr,
                                            ast::Expr::TsSatisfies(x) => &x.expr,
                                            ast::Expr::TsTypeAssertion(x) => &x.expr,
                                            ast::Expr::TsConstAssertion(x) => &x.expr,
                                            ast::Expr::Paren(x) => &x.expr,
                                            _ => break,
                                        };
                                    }
                                    if matches!(
                                        obj_inner,
                                        ast::Expr::Ident(i) if i.sym.as_ref() == "ReadableStream"
                                    ) {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            "readable_stream".to_string(),
                                            "ReadableStream".to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is an awaited native class instantiation (e.g., await new Redis())
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let ast::Expr::New(new_expr) = await_expr.arg.as_ref() {
                        if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                            let class_name = class_ident.sym.as_ref();
                            // Same user-class shadowing rule as the
                            // non-await new-expr path above.
                            let user_class_defined = ctx.classes_index.contains_key(class_name)
                                || ctx.pending_classes.iter().any(|c| c.name == class_name);
                            // First try the general native module lookup.
                            // Pool/Client/MongoClient are intentionally NOT
                            // in the fallback map — see the sync `new` arm
                            // above for the rationale (issue #536).
                            let module_name =
                                if let Some((m, method)) = ctx.lookup_native_module(class_name) {
                                    match (m, method) {
                                        ("url", Some("URL" | "URLSearchParams"))
                                        | ("util", Some("TextEncoder" | "TextDecoder")) => None,
                                        _ => Some(m.to_string()),
                                    }
                                } else if user_class_defined {
                                    None
                                } else {
                                    match class_name {
                                        "EventEmitter" | "EventEmitterAsyncResource" => {
                                            Some("events".to_string())
                                        }
                                        "AsyncLocalStorage" => Some("async_hooks".to_string()),
                                        "AsyncResource" => Some("async_hooks".to_string()),
                                        "WebSocket" | "WebSocketServer" => Some("ws".to_string()),
                                        "Redis" => Some("ioredis".to_string()),
                                        "LRUCache" => Some("lru-cache".to_string()),
                                        "Command" => Some("commander".to_string()),
                                        "Big" => Some("big.js".to_string()),
                                        "Decimal" => Some("decimal.js".to_string()),
                                        "BigNumber" => Some("bignumber.js".to_string()),
                                        _ => None,
                                    }
                                };
                            let module_name = match (class_name, module_name.as_deref()) {
                                ("StringDecoder", Some("string_decoder")) => None,
                                (
                                    "DiffieHellman" | "DiffieHellmanGroup",
                                    Some("crypto" | "node:crypto"),
                                ) => None,
                                _ => module_name,
                            };
                            if let Some(module) = module_name {
                                ctx.register_native_instance(
                                    name.clone(),
                                    module,
                                    class_name.to_string(),
                                );
                            }
                        }
                    }
                }
            }

            // Check if this is a native module factory function call (e.g., mysql.createPool())
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                let obj_name = obj_ident.sym.as_ref();
                                // Check if it's a known native module
                                if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.as_ref();
                                        // Map factory functions to their class names
                                        let class_name = match (module_name, method_name) {
                                            ("async_hooks", "createHook") => Some("AsyncHook"),
                                            ("dns" | "dns/promises", "Resolver") => {
                                                Some("Resolver")
                                            }
                                            ("mysql2" | "mysql2/promise", "createPool") => {
                                                Some("Pool")
                                            }
                                            ("mysql2" | "mysql2/promise", "createConnection") => {
                                                Some("Connection")
                                            }
                                            ("pg", "connect") => Some("Client"),
                                            ("http" | "https", "request" | "get") => {
                                                Some("ClientRequest")
                                            }
                                            // #2153 — `const server = http.createServer(...)`
                                            // inside a function body (the CJS wrapper closure
                                            // counts: a raw `.js` user file is wrapped in
                                            // `(function(){ ... })()` before lowering). The
                                            // module-level + named-import paths
                                            // (`createServer(...)` after
                                            // `import { createServer } from 'node:http'`) were
                                            // already registering correctly; the member-call
                                            // form `http.createServer(...)` slipped through
                                            // this arm's match because the row didn't exist.
                                            // Without the tag, `server.listen(...)` /
                                            // `server.on(...)` / `server.close()` falls
                                            // through to `js_typed_feedback_native_call_method`
                                            // → generic `js_native_call_method`, which has no
                                            // HttpServer arm → returns NaN.
                                            ("http", "createServer") => Some("HttpServer"),
                                            ("https", "createServer") => Some("HttpsServer"),
                                            ("tls", "createServer" | "Server") => Some("Server"),
                                            ("http2", "createSecureServer") => {
                                                Some("Http2SecureServer")
                                            }
                                            // node-cron's `cron.schedule(expr, cb)` returns a job
                                            // handle whose `start()`/`stop()`/`isRunning()` methods
                                            // dispatch via the ("node-cron", true, METHOD) entries
                                            // in expr.rs's native_module dispatch table. Without
                                            // registering the result as a "CronJob" native instance,
                                            // `job.stop()` falls through to dynamic dispatch and the
                                            // call never reaches js_cron_job_stop.
                                            ("node-cron", "schedule") => Some("CronJob"),
                                            // readline.createInterface() returns a singleton
                                            // handle whose .question/.on/.close methods
                                            // dispatch via the ("readline", true, METHOD)
                                            // entries in lower_call.rs's native_module dispatch
                                            // table. Without registering the result as a
                                            // "Interface" native instance, those calls fall
                                            // through to dynamic dispatch and never reach
                                            // js_readline_question / js_readline_on / etc.
                                            ("readline", "createInterface") => Some("Interface"),
                                            // perry/tui state(initial) returns a handle whose
                                            // .get()/.set() methods dispatch via the
                                            // ("perry/tui", true, "get"/"set", class_filter:
                                            // Some("State")) entries in lower_call.rs's
                                            // NativeModSig table. Without this registration,
                                            // those calls fall through to dynamic dispatch and
                                            // never reach the runtime FFI. (#358 Phase 2.)
                                            ("perry/tui", "state") => Some("State"),
                                            // perry/tui ink-shape hooks (#679 Phase 1): the
                                            // useApp/useStdout/useRef factories each return
                                            // a singleton handle. .exit()/.write()/.get()
                                            // etc. dispatch through the class_filter rows
                                            // in lower_call.rs.
                                            ("perry/tui", "useApp") => Some("TuiApp"),
                                            ("perry/tui", "useStdout") => Some("TuiStdout"),
                                            ("perry/tui", "useRef") => Some("RefBox"),
                                            ("perry/tui", "useFocusManager") => {
                                                Some("FocusManager")
                                            }
                                            _ => None,
                                        };
                                        if let Some(class_name) = class_name {
                                            let class_module = if class_name == "ClientRequest" {
                                                "http"
                                            } else {
                                                module_name
                                            };
                                            ctx.register_native_instance(
                                                name.clone(),
                                                class_module.to_string(),
                                                class_name.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Check if this is a direct call to a default import from a native module
                        // e.g., Fastify() where Fastify is imported from 'fastify'
                        if let ast::Expr::Ident(func_ident) = callee.as_ref() {
                            let func_name = func_ident.sym.as_ref();
                            // Check if this is a default import from a native module
                            if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                                // Register as native instance - the "class" is "App" for default exports
                                ctx.register_native_instance(
                                    name.clone(),
                                    module_name.to_string(),
                                    "App".to_string(),
                                );
                            }
                            // Check if this is a named import that returns a handle (e.g., State from perry/ui)
                            // Clone module_name + method_name to owned String first
                            // so the immutable borrow of ctx ends before we call
                            // register_native_instance (mutable borrow).
                            let mod_method: Option<(String, String)> = ctx
                                .lookup_native_module(func_name)
                                .and_then(|(m, mm)| mm.map(|x| (m.to_string(), x.to_string())));
                            if let Some((module_name, method_name)) = mod_method {
                                if module_name == "perry/ui" {
                                    match method_name.as_str() {
                                        "Canvas" | "State" | "Sheet" | "Toolbar" | "Window"
                                        | "LazyVStack" | "NavigationStack" | "Picker" | "Table"
                                        | "TabBar" => {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name.clone(),
                                                method_name.clone(),
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                                // perry/tui state(initial) — register the receiver as a
                                // "State" native instance so subsequent .get()/.set()
                                // calls dispatch via the perry/tui NativeModSig table
                                // (class_filter: Some("State")). (#358 Phase 2.)
                                if module_name == "perry/tui" && method_name == "state" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_name.clone(),
                                        "State".to_string(),
                                    );
                                }
                                // perry/tui ink-shape hooks (#679 Phase 1).
                                // useApp/useStdout/useRef each return a
                                // singleton handle whose receiver-methods
                                // dispatch through the class_filter rows
                                // ("TuiApp"/"TuiStdout"/"RefBox") added in
                                // lower_call.rs. Without these registrations
                                // a call like `app.exit()` falls back to
                                // dynamic dispatch and the matching FFI
                                // (js_perry_tui_app_exit) is never invoked.
                                if module_name == "perry/tui" {
                                    let class = match method_name.as_str() {
                                        "useApp" => Some("TuiApp"),
                                        "useStdout" => Some("TuiStdout"),
                                        "useRef" => Some("RefBox"),
                                        "useFocusManager" => Some("FocusManager"),
                                        _ => None,
                                    };
                                    if let Some(cn) = class {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module_name.clone(),
                                            cn.to_string(),
                                        );
                                    }
                                }
                                // node:http / node:https / node:http2 — issue #604
                                // followup to #577. The module-level decl path
                                // (lower.rs:5530) already handles `const s =
                                // createServer(...)` at top level; this arm
                                // covers the inside-function case where the
                                // factory call lives in a body. Without this,
                                // `async function main() { const server =
                                // createServer(handler); server.listen(...); }`
                                // had `server` unregistered, so the listen
                                // dispatch fell through the class_filter
                                // gate and never invoked the cb closure.
                                let http_class = match (module_name.as_str(), method_name.as_str())
                                {
                                    ("http", "createServer") => Some("HttpServer"),
                                    ("https", "createServer") => Some("HttpsServer"),
                                    ("http2", "createSecureServer") => Some("Http2SecureServer"),
                                    ("async_hooks", "createHook") => Some("AsyncHook"),
                                    ("dns" | "dns/promises", "Resolver") => Some("Resolver"),
                                    _ => None,
                                };
                                if let Some(cn) = http_class {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        module_name,
                                        cn.to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is an awaited factory call (e.g., const client = await MongoClient.connect(uri))
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                        if let ast::Callee::Expr(callee) = &call_expr.callee {
                            if let ast::Expr::Member(member) = callee.as_ref() {
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    let obj_name = obj_ident.sym.as_ref();
                                    if let Some((module_name, _)) =
                                        ctx.lookup_native_module(obj_name)
                                    {
                                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                            let class_name =
                                                match (module_name, method_ident.sym.as_ref()) {
                                                    ("mongodb", "connect") => Some("MongoClient"),
                                                    ("mysql2" | "mysql2/promise", "createPool") => {
                                                        Some("Pool")
                                                    }
                                                    (
                                                        "mysql2" | "mysql2/promise",
                                                        "createConnection",
                                                    ) => Some("Connection"),
                                                    ("pg", "connect") => Some("Client"),
                                                    // axios.get/post/put/delete/patch/request — mirror
                                                    // the top-level decl arm in lower.rs:4011 so
                                                    // `await axios.get(...)` registers the result as
                                                    // an axios.Response inside async function bodies.
                                                    // Without this, `r.status` / `r.data` fall through
                                                    // to generic property dispatch and read the
                                                    // raw handle pointer as an ObjectHeader. Issue
                                                    // #604 followup — same pattern as the createServer
                                                    // registration above.
                                                    (
                                                        "axios",
                                                        "get" | "post" | "put" | "delete" | "patch"
                                                        | "request",
                                                    ) => Some("Response"),
                                                    _ => None,
                                                };
                                            if let Some(class_name) = class_name {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    module_name.to_string(),
                                                    class_name.to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a method call on a registered native instance (chaining).
            // e.g., const db = client.db(name) where client is a mongodb native instance.
            if let Some(init_expr) = &decl.init {
                // Unwrap await if present
                let actual_init = if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    await_expr.arg.as_ref()
                } else {
                    init_expr.as_ref()
                };
                if let ast::Expr::Call(call_expr) = actual_init {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                let obj_name = obj_ident.sym.to_string();
                                if let Some((module_name, _class)) = ctx
                                    .lookup_native_instance(&obj_name)
                                    .map(|(m, c)| (m.to_string(), c.to_string()))
                                {
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.as_ref();
                                        // Determine if the method returns a handle (another native instance)
                                        let returns_handle =
                                            match (module_name.as_str(), method_name) {
                                                ("mongodb", "db") => Some("Database"),
                                                ("mongodb", "collection") => Some("Collection"),
                                                ("mysql2" | "mysql2/promise", "getConnection") => {
                                                    Some("PoolConnection")
                                                }
                                                ("better-sqlite3", "prepare") => Some("Statement"),
                                                ("sqlite", "prepare") => Some("StatementSync"),
                                                ("sqlite", "createSession") => Some("Session"),
                                                _ => None,
                                            };
                                        if let Some(class_name) = returns_handle {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name,
                                                class_name.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // #5216: `const <name> = require("<spec>")` of a statically
            // resolvable native/Node-builtin module lowers to the same
            // module-namespace binding `import * as <name> from "<spec>"`
            // produces (native module + builtin alias, NO runtime `let` — a
            // namespace import binds nothing observable). Subsumes the old
            // fs/path/crypto-only `is_require_builtin_module` path. Non-literal
            // / unresolvable specifiers fall through to the legacy compile-time
            // refusal in `expr_call::intrinsics::try_require_literal`.
            if let Some(init_expr) = &decl.init {
                if let Some(module_name) = require_resolvable_native_specifier(init_expr) {
                    register_require_namespace_binding(ctx, &name, &module_name);
                    return Ok(result);
                }
            }

            // Check if this is calling toString() on URLSearchParams - returns String
            if matches!(ty, Type::Any) {
                if let Some(init_expr) = &decl.init {
                    if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                        if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                            if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                                if let ast::MemberProp::Ident(method_ident) = &member_expr.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "toString" || method_name == "get" {
                                        // Check if object is a URLSearchParams
                                        if let ast::Expr::Ident(obj_ident) =
                                            member_expr.obj.as_ref()
                                        {
                                            let obj_name = obj_ident.sym.as_ref();
                                            if let Some(obj_ty) = ctx.lookup_local_type(obj_name) {
                                                if matches!(obj_ty, Type::Named(name) if name == "URLSearchParams")
                                                {
                                                    ty = Type::String;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is assigning the result of a native method call that returns the same type
            // e.g., const sum = d1.plus(d2) where d1 is a Decimal -> sum should also be tracked as Decimal
            // Also handles: const r1 = new Big(...).div(...) patterns
            if let Some(init_expr) = &decl.init {
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                        if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                            let mut handled = false;
                            // First try: object is an ident that's a known native instance
                            if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                                let obj_name = obj_ident.sym.as_ref();
                                // Check if object is a native instance
                                if let Some((module, class)) = ctx.lookup_native_instance(obj_name)
                                {
                                    // Check if this method returns the same type (builder pattern)
                                    if let ast::MemberProp::Ident(method_ident) = &member_expr.prop
                                    {
                                        let method_name = method_ident.sym.as_ref();
                                        // Methods that return the same type (Decimal, etc.)
                                        let returns_same_type = match class {
                                            "Decimal" | "Big" | "BigNumber" => matches!(
                                                method_name,
                                                "plus"
                                                    | "minus"
                                                    | "times"
                                                    | "div"
                                                    | "mod"
                                                    | "pow"
                                                    | "sqrt"
                                                    | "abs"
                                                    | "neg"
                                                    | "round"
                                                    | "floor"
                                                    | "ceil"
                                            ),
                                            _ => false,
                                        };
                                        if returns_same_type {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module.to_string(),
                                                class.to_string(),
                                            );
                                            handled = true;
                                        }
                                    }
                                }
                            }
                            // Second try: object is new Big(...) or a chained call like new Big(...).div(...)
                            if !handled {
                                if let Some(module_name) =
                                    detect_native_instance_expr(ctx, &member_expr.obj)
                                {
                                    let class_name = match module_name {
                                        "big.js" => "Big",
                                        "decimal.js" => "Decimal",
                                        "bignumber.js" => "BigNumber",
                                        "lru-cache" => "LRUCache",
                                        "commander" => "Command",
                                        _ => "",
                                    };
                                    if !class_name.is_empty() {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module_name.to_string(),
                                            class_name.to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is assigning from fetch() or await fetch() - register as fetch Response
            if let Some(init_expr) = &decl.init {
                if crate::lower_types::is_node_readable_static_factory_call(ctx, init_expr) {
                    let readable = "Readable".to_string();
                    ty = Type::Named(readable.clone());
                    ctx.register_native_instance(name.clone(), "stream".to_string(), readable);
                }

                // Check for: const response = fetch(url) / fetchWithAuth(url, auth) / fetchPostWithAuth(url, auth, body)
                if let Some(module) = get_fetch_module(init_expr) {
                    ctx.register_native_instance(
                        name.clone(),
                        module.to_string(),
                        "Response".to_string(),
                    );
                }
                // Check for: const response = await fetch(url) / await fetchWithAuth(...) / await fetchPostWithAuth(...)
                else if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let Some(module) = get_fetch_module(&await_expr.arg) {
                        ctx.register_native_instance(
                            name.clone(),
                            module.to_string(),
                            "Response".to_string(),
                        );
                    }
                }

                // #5432: `const res = app.fetch(req)` / `await app.fetch(req)` —
                // a member-call `.fetch(...)` is the Fetch-API server-handler
                // convention (Hono `app.fetch`, itty-router, Cloudflare
                // Workers) and yields a native fetch Response. Record it in a
                // narrow set (NOT `register_native_instance`, which would hijack
                // every method on `res`) so only `res.headers.<m>()` bails the
                // array-method fold. See `fetch_call_response_locals`.
                if is_member_fetch_call(init_expr) {
                    ctx.fetch_call_response_locals.insert(name.clone());
                }

                // Web Fetch API: new Response(...) / new Headers(...) /
                // new Request(...) / new FormData(...)
                // Also handle Response.json(...) and Response.redirect(...) static factories.
                if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                        match class_ident.sym.as_ref() {
                            "Response" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "fetch".to_string(),
                                    "Response".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "Headers" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "Headers".to_string(),
                                    "Headers".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "Request" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "Request".to_string(),
                                    "Request".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "FormData" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "FormData".to_string(),
                                    "FormData".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            // Issue #1211: `new Blob([...])` / `new File([...], name)`.
                            // File shares the Blob runtime registry — the codegen
                            // `module == "blob"` arm dispatches `.name` /
                            // `.lastModified` regardless of class tag, so File
                            // tracks as a Blob instance with the class tag
                            // available for future File-only property checks.
                            "Blob" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "blob".to_string(),
                                    "Blob".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "File" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "blob".to_string(),
                                    "File".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            other
                                if ctx.resolve_class_alias(other).as_deref().is_some_and(
                                    |resolved| matches!(resolved, "Blob" | "File"),
                                ) =>
                            {
                                let resolved = ctx.resolve_class_alias(other).unwrap();
                                ctx.register_native_instance(
                                    name.clone(),
                                    "blob".to_string(),
                                    resolved,
                                );
                                ctx.uses_fetch = true;
                            }
                            // Issue #237: Web Streams API constructors.
                            "ReadableStream" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "readable_stream".to_string(),
                                    "ReadableStream".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            // #4915: `new ReadableStreamBYOBReader(stream)` —
                            // the handle is a reader, same module tag as
                            // `stream.getReader({ mode: "byob" })`.
                            "ReadableStreamBYOBReader" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "readable_stream_reader".to_string(),
                                    "ReadableStreamBYOBReader".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "WritableStream" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "writable_stream".to_string(),
                                    "WritableStream".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            "TransformStream" => {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "transform_stream".to_string(),
                                    "TransformStream".to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                            other => {
                                // Issue #562: `let x = new SubclassOfStream()`
                                // — walk the user class's `native_extends` to
                                // see if it points at a stream module. If so,
                                // register `x` under the same module/class
                                // tag the bare-stream constructor would. The
                                // codegen FFI sites unwrap the
                                // `__perry_stream_handle__` field at dispatch
                                // time, so a subclass instance and a bare
                                // numeric handle are interchangeable.
                                if let Some((module, class)) =
                                    ctx.lookup_class_native_extends(other)
                                {
                                    if matches!(
                                        module,
                                        "readable_stream" | "writable_stream" | "transform_stream"
                                    ) {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module.to_string(),
                                            class.to_string(),
                                        );
                                        ctx.uses_fetch = true;
                                    }
                                }
                            }
                        }
                    } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                        let class_name = match &member.prop {
                            ast::MemberProp::Ident(prop_ident) => Some(prop_ident.sym.as_ref()),
                            ast::MemberProp::Computed(prop) => match prop.expr.as_ref() {
                                ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                                _ => None,
                            },
                            _ => None,
                        };
                        let is_blob_file_ctor = match member.obj.as_ref() {
                            ast::Expr::Ident(obj_ident)
                                if obj_ident.sym.as_ref() == "globalThis" =>
                            {
                                true
                            }
                            ast::Expr::Ident(obj_ident) => ctx
                                .lookup_native_module(obj_ident.sym.as_ref())
                                .is_some_and(|(module, _)| {
                                    module == "buffer" || module == "node:buffer"
                                }),
                            _ => false,
                        };
                        if is_blob_file_ctor {
                            if let Some(class_name @ ("Blob" | "File")) = class_name {
                                ctx.register_native_instance(
                                    name.clone(),
                                    "blob".to_string(),
                                    class_name.to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                        }
                    }
                }
                // Response.json(...) / Response.redirect(...) static factories
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if obj_ident.sym.as_ref() == "Response" {
                                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                        match prop_ident.sym.as_ref() {
                                            "json" | "redirect" | "error" => {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    "fetch".to_string(),
                                                    "Response".to_string(),
                                                );
                                                ctx.uses_fetch = true;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Response.clone() — for: const r5clone = r5.clone();
                // The result is a new Response. Detect by checking if the receiver is already
                // a fetch::Response native instance.
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    if prop_ident.sym.as_ref() == "clone" {
                                        if let Some((m, c)) =
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                        {
                                            if c == "Response" {
                                                let m = m.to_string();
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    m,
                                                    "Response".to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Issue #234 / fetch body helpers: const blob = await <res|req>.blob()
                // registers Blob results; const form = await <res|req>.formData()
                // registers FormData results so follow-up calls dispatch through
                // the typed fetch lowering instead of the generic handle path.
                if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
                    if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                        if let ast::Callee::Expr(callee) = &call_expr.callee {
                            if let ast::Expr::Member(member) = callee.as_ref() {
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                        match prop_ident.sym.as_ref() {
                                            "blob" => {
                                                if let Some((_, c)) = ctx
                                                    .lookup_native_instance(obj_ident.sym.as_ref())
                                                {
                                                    if c == "Response" || c == "Request" {
                                                        ctx.register_native_instance(
                                                            name.clone(),
                                                            "blob".to_string(),
                                                            "Blob".to_string(),
                                                        );
                                                    }
                                                }
                                            }
                                            "formData" => {
                                                if let Some((_, c)) = ctx
                                                    .lookup_native_instance(obj_ident.sym.as_ref())
                                                {
                                                    if c == "Response" || c == "Request" {
                                                        ctx.register_native_instance(
                                                            name.clone(),
                                                            "FormData".to_string(),
                                                            "FormData".to_string(),
                                                        );
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Issue #234: const b2 = blob.slice(...) — chained slicing
                // returns a new Blob. Detect when the receiver is already a
                // blob::Blob native instance.
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    if prop_ident.sym.as_ref() == "slice" {
                                        if let Some((_, c)) =
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                        {
                                            if c == "Blob" {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    "blob".to_string(),
                                                    "Blob".to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Issue #237: Web Streams chained-typed-method bindings.
                // Recognize chained method/property forms that return a new
                // streams native instance so subsequent dispatch routes to
                // the right `module == "..."` arm in lower_call.rs.
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    let m = prop_ident.sym.as_ref().to_string();
                                    let class_owned = ctx
                                        .lookup_native_instance(obj_ident.sym.as_ref())
                                        .map(|(_, c)| c.to_string());
                                    if let Some(c) = class_owned {
                                        if m == "stream" && c == "Blob" {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "readable_stream".to_string(),
                                                "ReadableStream".to_string(),
                                            );
                                        }
                                        if m == "getReader" && c == "ReadableStream" {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "readable_stream_reader".to_string(),
                                                "ReadableStreamDefaultReader".to_string(),
                                            );
                                        }
                                        if m == "getWriter" && c == "WritableStream" {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "writable_stream_writer".to_string(),
                                                "WritableStreamDefaultWriter".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Issue #237: const stream = response.body / const r = ts.readable / .writable
                // Property reads on a native instance — destructured as Member
                // expressions (no Call wrapper).
                if let ast::Expr::Member(member) = init_expr.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            let p = prop_ident.sym.as_ref().to_string();
                            let class_owned = ctx
                                .lookup_native_instance(obj_ident.sym.as_ref())
                                .map(|(_, c)| c.to_string());
                            if let Some(c) = class_owned {
                                if p == "body" && c == "Response" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "readable_stream".to_string(),
                                        "ReadableStream".to_string(),
                                    );
                                }
                                if p == "readable" && c == "TransformStream" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "readable_stream".to_string(),
                                        "ReadableStream".to_string(),
                                    );
                                }
                                if p == "writable" && c == "TransformStream" {
                                    ctx.register_native_instance(
                                        name.clone(),
                                        "writable_stream".to_string(),
                                        "WritableStream".to_string(),
                                    );
                                }
                            }
                        }
                    }
                }

                // Issue #237: const stream = upstream.pipeThrough(transform)
                // returns a ReadableStream (the transform's readable side).
                if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                        if let ast::Expr::Member(member) = callee.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                    if prop_ident.sym.as_ref() == "pipeThrough" {
                                        let class_owned = ctx
                                            .lookup_native_instance(obj_ident.sym.as_ref())
                                            .map(|(_, c)| c.to_string());
                                        if class_owned.as_deref() == Some("ReadableStream") {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                "readable_stream".to_string(),
                                                "ReadableStream".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if calling a function whose return type is a native module type
            // e.g., const dbPool = initializePool() where initializePool(): mysql.Pool
            // Also handles: const dbPool = await initializePool()
            if let Some(init_expr) = &decl.init {
                let call_expr = match init_expr.as_ref() {
                    ast::Expr::Call(c) => Some(c),
                    ast::Expr::Await(await_expr) => {
                        if let ast::Expr::Call(c) = await_expr.arg.as_ref() {
                            Some(c)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                // Variable-to-variable propagation for native instances
                // (`let sock: Socket = plainSock`) is handled by the
                // post-lowering cross-module pass; see
                // `js_transform::scan_for_ident_init_propagation`.
                if let Some(call_expr) = call_expr {
                    if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                        // Check direct function calls: const x = someFunc()
                        if let ast::Expr::Ident(func_ident) = callee_expr.as_ref() {
                            let func_name = func_ident.sym.as_ref();
                            if let Some((module, class)) =
                                ctx.lookup_func_return_native_instance(func_name)
                            {
                                ctx.register_native_instance(
                                    name.clone(),
                                    module.to_string(),
                                    class.to_string(),
                                );
                            }
                        }
                        // Check method calls on native instances: const conn = pool.getConnection()
                        if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                            if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                                let obj_name = obj_ident.sym.as_ref();
                                if let Some((module, class)) = ctx.lookup_native_instance(obj_name)
                                {
                                    if let ast::MemberProp::Ident(method_ident) = &member_expr.prop
                                    {
                                        let method_name = method_ident.sym.as_ref();
                                        // Map method calls to their return types
                                        let return_class = match (module, class, method_name) {
                                            (
                                                "mysql2" | "mysql2/promise",
                                                "Pool",
                                                "getConnection",
                                            ) => Some("PoolConnection"),
                                            ("pg", "Pool", "connect") => Some("Client"),
                                            _ => None,
                                        };
                                        if let Some(ret_class) = return_class {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module.to_string(),
                                                ret_class.to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Issue #461: when the init is an arrow / function expression
            // (`const f = (x) => …` or `const f = function() {}`), pre-define
            // the local BEFORE lowering the init so self-recursive references
            // inside the closure body resolve to `LocalGet(id)` instead of
            // falling through to `lookup_imported_func` and lowering as
            // `ExternFuncRef { name: "f" }` (which then emits a bare unmangled
            // `_f` symbol at link time). Effect's `internal/stream.ts` hits this:
            // `import * as pull from "./stream/pull.js"` (namespace import) +
            // `const pull = (state) => { … pull(...) … }` (local rebinding) —
            // without pre-registration, the inner closure's `pull` reference
            // resolves to the namespace import. Function declarations
            // (`function f() {}`) already have this pre-registration via
            // `lower_decl.rs`'s `Decl::Fn` arm.
            //
            // Gate on function-expr init only: pre-defining for `const x = x + 1`
            // would silently turn a TDZ violation into a self-reference. For
            // closures, the body doesn't execute until call time, so the slot
            // holds the closure value by then.
            // #593: extend the pre-registration to inits that *contain*
            // an Arrow / Fn anywhere in their tree (e.g.
            // `const off = ev.on(() => off())` — Call wrapping Arrow,
            // `const sub = subject.subscribe({ next: () => sub.unsubscribe() })`
            // — Object wrapping Arrow). The closure body is lowered in
            // its own LoweringContext but reuses the parent's `locals`
            // for outer-scope lookups (see `lower_arrow` /
            // `lower_fn_expr`). Without pre-registration, the inner
            // `off` / `sub` reference resolves to GlobalGet(0) and the
            // self-recursive call no-ops at runtime.
            let is_function_expr_init = matches!(
                decl.init.as_deref(),
                Some(ast::Expr::Arrow(_)) | Some(ast::Expr::Fn(_))
            ) || decl
                .init
                .as_deref()
                .is_some_and(ast_expr_contains_function_expr);
            let pre_id = if is_function_expr_init
                && !ctx.pre_registered_module_vars.contains(&name)
                && ctx.lookup_local(&name).is_none()
            {
                Some(ctx.define_local(name.clone(), ty.clone()))
            } else {
                None
            };

            if let Some(init_ast) = decl.init.as_ref() {
                result.extend(predeclare_implicit_assignment_targets(ctx, init_ast));
            }
            let init = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            if matches!(ty, Type::Any) {
                match &init {
                    Some(Expr::NativeMethodCall { module, method, .. })
                        if module == "stream" && method == "from" =>
                    {
                        ty = Type::Named("Readable".to_string());
                    }
                    Some(Expr::NewDynamic { callee, .. }) => {
                        if let Expr::PropertyGet { object, property } = callee.as_ref() {
                            if matches!(object.as_ref(), Expr::NativeModuleRef(module) if module == "net" || module == "node:net")
                                && matches!(property.as_str(), "BlockList" | "SocketAddress")
                            {
                                ty = Type::Named(property.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            // #321: a generator function EXPRESSION bound to a name (`const g =
            // function*(){}`) — register the name so `for (x of g())` / `[...g()]`
            // take the iterator-protocol path, matching named `function* g(){}`
            // declarations. (`.next()`-driving already works via the lifted
            // generator transform; this covers the for-of/spread call sites,
            // whose detection in stmt_loops.rs is name-based.)
            if let Some(Expr::Closure {
                is_generator: true,
                is_async,
                ..
            }) = &init
            {
                ctx.generator_func_names.insert(name.clone());
                if *is_async {
                    ctx.async_generator_func_names.insert(name.clone());
                }
            }
            let id = if let Some(pid) = pre_id {
                pid
            } else if ctx.scope_depth == 0
                && ctx.inside_block_scope == 0
                && ctx.pre_registered_module_vars.remove(&name)
            {
                ctx.pre_registered_module_var_decls.remove(&name);
                // Reuse pre-registered LocalId from module-level forward-declaration pass.
                // #1758: gated on MODULE scope — a nested local of the same name
                // (`function helper() { const zipWith = ... }` where the module also
                // declares `const zipWith`) must NOT consume the module var's
                // pre-registered id, or it conflates the two: the nested local and
                // the module binding share one id, the real module value lands on a
                // fresh id, and a sibling closure that referenced the module name
                // resolves to the wrong (uninitialised) slot → `value is not a
                // function`. effect's `layer.merge` (refs module `zipWith` at L1191)
                // broke this way because a local `zipWith` (L1180) precedes it.
                let id = ctx.lookup_local(&name).unwrap();
                // Update the type now that we have full inference
                if let Some(existing_ty) = ctx.locals.lookup_type_mut(&name) {
                    *existing_ty = ty.clone();
                }
                id
            } else if let Some(fid) = match &decl.name {
                // #4973: the function-body hoist pass pre-registered this
                // exact `let`/`const` declarator (span-keyed) so hoisted
                // sibling functions could forward-reference it. Reuse the
                // pre-registered id here so the init lands in the slot/box
                // those references captured.
                ast::Pat::Ident(ident) => ctx.lexical_forward_decls.remove(&ident.id.span.lo.0),
                _ => None,
            } {
                if let Some((_, _, existing_ty)) =
                    ctx.locals.iter_mut().rev().find(|(_, lid, _)| *lid == fid)
                {
                    *existing_ty = ty.clone();
                }
                fid
            } else if let Some((reuse_pos, id)) = is_var_decl
                .then(|| {
                    // Issue #838 followup (b): when the closure-body hoist
                    // in `lower_fn_expr` / `lower_arrow` pre-registered this
                    // `var` (so forward references like `var O = function(){
                    // … _ … }; var _ = …;` resolve before `_`'s let runs),
                    // reuse that pre-hoisted id here. Otherwise the let
                    // defines a fresh id and the pre-hoisted slot stays
                    // uninitialised — closures created before the let see
                    // value-zero through the capture box and dispatch
                    // misses entirely. dayjs's outer IIFE hits this with
                    // `var O = function(t){ return new _(n); }; var _ = ((
                    // function(){ function M(){…}; … return M; })());`.
                    //
                    // Restrict this path to syntactic `var`. A block-scoped
                    // `let`/`const` with the same name must create a fresh
                    // lexical binding, and using `lookup_local(name)` here
                    // would accidentally grab a shadowing catch parameter.
                    ctx.locals
                        .iter_named(&name)
                        .find(|(_, (_, lid, _))| ctx.var_hoisted_ids.contains(lid))
                        .map(|(pos, (_, lid, _))| (pos, *lid))
                })
                .flatten()
            {
                // Patch the reused binding's type in place (O(1) by position)
                // rather than re-finding it with an O(n) scan (#5267).
                *ctx.locals.type_mut_at(reuse_pos) = ty.clone();
                id
            } else {
                ctx.define_local(name.clone(), ty.clone())
            };
            if !mutable {
                ctx.mark_local_immutable(id);
            }
            // Issue #886: detect `let/const/var <name> = Object.<staticMethod>`
            // from the raw AST so a subsequent indirect call `<name>(args)`
            // can route to the dedicated HIR variant the literal
            // `Object.<staticMethod>(args)` already uses. The detection runs
            // from the AST (rather than the lowered `init`) because the init
            // lowering erases the `Object` qualifier into a generic
            // PropertyGet that resolves to undefined at codegen. esbuild's
            // CJS-bundle prelude emits this pattern verbatim for every
            // bundled package:
            //   var __defProp = Object.defineProperty;
            //   var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
            //   var __getOwnPropNames = Object.getOwnPropertyNames;
            //   var __getProtoOf = Object.getPrototypeOf;
            //   var __defProps = Object.defineProperties;
            // — so anything that imports an esbuild-bundled package threw
            // `TypeError: value is not a function` at module init pre-fix.
            let object_method_alias: Option<String> =
                decl.init.as_deref().and_then(|init_ast| match init_ast {
                    ast::Expr::Member(member) => match (member.obj.as_ref(), &member.prop) {
                        (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                            if obj_ident.sym.as_ref() == "Object" =>
                        {
                            let method_name = method_ident.sym.as_ref();
                            // Whitelist of static methods that already have
                            // a dedicated HIR variant in `lower/expr_call.rs`.
                            // Methods not on this list intentionally fall
                            // through to the generic PropertyGet path so we
                            // don't change behaviour for unsupported ones.
                            let is_supported = matches!(
                                method_name,
                                "defineProperty"
                                    | "defineProperties"
                                    | "setPrototypeOf"
                                    | "getPrototypeOf"
                                    | "getOwnPropertyDescriptor"
                                    | "getOwnPropertyDescriptors"
                                    | "getOwnPropertyNames"
                                    | "getOwnPropertySymbols"
                                    | "keys"
                                    | "values"
                                    | "entries"
                                    | "assign"
                                    | "fromEntries"
                                    | "create"
                                    | "freeze"
                                    | "seal"
                                    | "preventExtensions"
                                    | "isFrozen"
                                    | "isSealed"
                                    | "isExtensible"
                                    | "hasOwn"
                                    | "is"
                            );
                            if is_supported {
                                Some(method_name.to_string())
                            } else {
                                None
                            }
                        }
                        (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                            if obj_ident.sym.as_ref() == "Array"
                                && method_ident.sym.as_ref() == "isArray" =>
                        {
                            Some("Array.isArray".to_string())
                        }
                        (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                            if matches!(
                                method_ident.sym.as_ref(),
                                "json" | "redirect" | "error"
                            ) && {
                                let obj_name = obj_ident.sym.as_ref();
                                (obj_name == "Response" && ctx.lookup_local("Response").is_none())
                                    || ctx
                                        .resolve_class_alias(obj_name)
                                        .as_deref()
                                        .is_some_and(|resolved| resolved == "Response")
                            } =>
                        {
                            let method = match method_ident.sym.as_ref() {
                                "json" => "Response.static_json",
                                "redirect" => "Response.static_redirect",
                                "error" => "Response.static_error",
                                _ => unreachable!(),
                            };
                            Some(method.to_string())
                        }
                        _ => None,
                    },
                    _ => None,
                });
            let array_method_alias: Option<String> =
                decl.init.as_deref().and_then(|init_ast| match init_ast {
                    ast::Expr::Member(member) => match (member.obj.as_ref(), &member.prop) {
                        (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                            if obj_ident.sym.as_ref() == "Array" =>
                        {
                            let method_name = method_ident.sym.as_ref();
                            if method_name == "isArray" {
                                Some(method_name.to_string())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    },
                    _ => None,
                });

            // Issue #886: register the alias once `id` is bound, so the
            // call-side recogniser in `lower/expr_call.rs` can route
            // `LocalGet(id)(args)` to the dedicated HIR variant the literal
            // `Object.<method>(args)` shape already uses.
            if let Some(method_name) = object_method_alias {
                ctx.object_static_method_aliases.insert(id, method_name);
            }
            if let Some(method_name) = array_method_alias {
                ctx.array_static_method_aliases.insert(id, method_name);
            }
            if let Some(Expr::NativeMethodCall { module, method, .. }) = &init {
                if module == "fetch"
                    && matches!(
                        method.as_str(),
                        "static_json" | "static_redirect" | "static_error"
                    )
                {
                    ctx.register_native_instance(
                        name.clone(),
                        "fetch".to_string(),
                        "Response".to_string(),
                    );
                    ctx.uses_fetch = true;
                }
            }

            // Issue #740: track `let/const/var <name> = ClassRef(...)` so
            // `new <name>(...)` can resolve captures via the alias chain.
            // Also follow LocalGet aliases for `const B = A` style chains.
            if let Some(init_expr) = &init {
                // Issue #838 followup (b): tag locals that hold a
                // callable value at runtime. Inside an IIFE the AST
                // pattern `function M(t){…}` hoists to a `Let { name:
                // "M", init: Some(Closure{…}) }`; the matching
                // `M.prototype.x = fn` site needs to resolve `M`'s
                // local id through this set so the
                // prototype-method recogniser routes through the
                // function-classic path. Also covers
                // `var Klass = function(){…}` (anonymous function
                // expression assigned to a local).
                if matches!(init_expr, Expr::Closure { .. } | Expr::FuncRef(_)) {
                    ctx.function_valued_locals.insert(id);
                }
                if is_global_this_value(ctx, init_expr) {
                    ctx.global_this_aliases.insert(id);
                }
                match init_expr {
                    Expr::ClassRef(class_name) => {
                        ctx.register_let_class_alias(name.clone(), class_name.clone());
                    }
                    Expr::LocalGet(src_id) => {
                        if let Some((src_name, _, _)) =
                            ctx.locals.iter().rev().find(|(_, lid, _)| lid == src_id)
                        {
                            let src_name = src_name.clone();
                            if let Some(resolved) = ctx.resolve_class_alias(&src_name) {
                                ctx.register_let_class_alias(name.clone(), resolved);
                            } else if ctx.classes_index.contains_key(&src_name) {
                                ctx.register_let_class_alias(name.clone(), src_name);
                            }
                        }
                        // Issue #838: follow prototype-alias chains too,
                        // so `var m = M.prototype; var n = m; n.foo = …`
                        // still recognises the underlying class.
                        if let Some(class_name) = ctx.prototype_aliases.get(src_id).cloned() {
                            ctx.prototype_aliases.insert(id, class_name);
                        }
                        // Issue #838 followup (b): same chain follow for
                        // function-decl prototype aliases.
                        if let Some(func_id) = ctx.prototype_function_aliases.get(src_id).copied() {
                            ctx.prototype_function_aliases.insert(id, func_id);
                        }
                        if let Some(src_local) = ctx.prototype_function_locals.get(src_id).copied()
                        {
                            ctx.prototype_function_locals.insert(id, src_local);
                        }
                        // Propagate function-valued tag through aliases.
                        if ctx.function_valued_locals.contains(src_id) {
                            ctx.function_valued_locals.insert(id);
                        }
                        // Issue #886: propagate the Object-static-method alias
                        // through `const B = A` chains so re-aliased copies
                        // (`const __defProp2 = __defProp;`) still route to the
                        // dedicated HIR variant at the indirect call site.
                        if let Some(method_name) =
                            ctx.object_static_method_aliases.get(src_id).cloned()
                        {
                            ctx.object_static_method_aliases.insert(id, method_name);
                        }
                        if let Some(method_name) =
                            ctx.array_static_method_aliases.get(src_id).cloned()
                        {
                            ctx.array_static_method_aliases.insert(id, method_name);
                        }
                    }
                    Expr::PropertyGet { object, property }
                        if is_global_this_value(ctx, object.as_ref())
                            && matches!(
                                property.as_str(),
                                "URL"
                                    | "URLSearchParams"
                                    | "TextEncoder"
                                    | "TextDecoder"
                                    | "Blob"
                                    | "File"
                                    | "FormData"
                                    | "Headers"
                                    | "Request"
                                    | "Response"
                                    | "WebSocket"
                            ) =>
                    {
                        ctx.register_let_class_alias(name.clone(), property.clone());
                        if matches!(
                            property.as_str(),
                            "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
                        ) {
                            ctx.uses_fetch = true;
                        }
                    }
                    Expr::PropertyGet { object, property }
                        if matches!(object.as_ref(), Expr::NativeModuleRef(module)
                            if module == "buffer" || module == "node:buffer")
                            && matches!(property.as_str(), "Blob" | "File") =>
                    {
                        ctx.register_let_class_alias(name.clone(), property.clone());
                        ctx.uses_fetch = true;
                    }
                    // Issue #838: `var p = <ClassName>.prototype` records
                    // the alias so a later `p.<method> = <fn>` lowers to
                    // RegisterPrototypeMethod. dayjs's minified shape
                    // (`var m = M.prototype; m.parse = function(){…};
                    //  m.init = function(){…};`) hits this — without
                    // alias-tracking the assignments fell through to a
                    // generic PropertySet on the prototype proxy that
                    // nothing downstream observed.
                    //
                    // Issue #838 followup (b): same shape but the base is
                    // a function declaration (Babel's class-from-function
                    // emit pattern, also what dayjs's minified `function
                    // M(){}; var m = M.prototype` lowers to). Tracked
                    // separately in `prototype_function_aliases` so the
                    // assignment recogniser can route to the
                    // function-flavoured prototype-method registration
                    // path (synthetic class id allocated at runtime).
                    Expr::PropertyGet { object, property } if property == "prototype" => {
                        match object.as_ref() {
                            Expr::ClassRef(class_name) => {
                                ctx.prototype_aliases.insert(id, class_name.clone());
                            }
                            Expr::FuncRef(func_id) => {
                                ctx.prototype_function_aliases.insert(id, *func_id);
                            }
                            // dayjs's minified IIFE shape lowers the inner
                            // `function M(t){…}` to a `Let { name: "M", init:
                            // Some(Closure{…}) }` (function decls inside a
                            // function expression body become hoisted lets in
                            // HIR). The subsequent `var m = M.prototype` then
                            // reads `M` as `LocalGet(M_id)` — match that and
                            // route the alias through the same function-class
                            // bucket, storing the receiver local id so the
                            // recogniser later emits
                            // `RegisterFunctionPrototypeMethod { func:
                            // LocalGet(M_id), … }`.
                            Expr::LocalGet(src_local)
                                if ctx.function_valued_locals.contains(src_local) =>
                            {
                                ctx.prototype_function_locals.insert(id, *src_local);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            // `with (o) { var foo = v; }` — the binding `foo` is hoisted to
            // the enclosing var scope, but the *initialisation* is a normal
            // PutValue under the with environment: when `o` has a `foo`
            // property, the write goes to `o.foo`, not the hoisted local
            // (test262 with/12.10-0-8). Emit the hoisted Let (no init) plus
            // a WithSet for the assignment.
            if is_var_decl && init.is_some() {
                if let Some(env_id) = ctx.active_with_envs_for_ident(&name).into_iter().next() {
                    result.push(Stmt::Let {
                        id,
                        name: name.clone(),
                        ty,
                        mutable,
                        init: None,
                    });
                    let fallback = crate::lower::with_set_fallback_for_ident(ctx, &name);
                    result.push(Stmt::Expr(Expr::WithSet {
                        object: Box::new(Expr::LocalGet(env_id)),
                        property: name,
                        value: Box::new(init.unwrap()),
                        fallback,
                        strict: ctx.current_strict,
                    }));
                    return Ok(result);
                }
            }
            // Next.js / webpack require pattern: `var i = n[e] = {exports:{}}`.
            // A chained member-assignment whose RHS is an object literal
            // miscompiles in the full-bundle context: the constructed object's
            // own field reads back as 0 when the construction flows directly
            // into both the member store and the binding (the nested webpack
            // bundle's `exports` then reads 0 → `exports.Fragment = …` throws).
            // A directly-bound object literal (`var x = {exports:{}}`) is fine,
            // so hoist the construction to its own `Let` and feed the member-set
            // and the binding from that temp — mirroring the working form.
            let init = match init {
                Some(Expr::PutValueSet {
                    target,
                    key,
                    value,
                    receiver,
                    strict,
                }) if matches!(value.as_ref(), Expr::New { .. } | Expr::Object(_)) => {
                    let tmp_id = ctx.define_local("__nx_member_init".to_string(), Type::Any);
                    result.push(Stmt::Let {
                        id: tmp_id,
                        name: "__nx_member_init".to_string(),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(*value),
                    });
                    result.push(Stmt::Expr(Expr::PutValueSet {
                        target,
                        key,
                        value: Box::new(Expr::LocalGet(tmp_id)),
                        receiver,
                        strict,
                    }));
                    result.push(Stmt::Let {
                        id,
                        name,
                        ty,
                        mutable,
                        init: Some(Expr::LocalGet(tmp_id)),
                    });
                    return Ok(result);
                }
                other => other,
            };
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            });
        }
        ast::Pat::Array(_) | ast::Pat::Object(_) => {
            // #3663 / #4905: tag destructured builtin-module members
            // (stream ctors, net factories) as native-module aliases so
            // call sites route through the static native table. Bindings
            // returned in `skip_local_bindings` must not also bind a
            // runtime local — the local (undefined for `net.connect`)
            // would shadow the alias at call sites; ESM named imports
            // never create one (exact parity).
            let skip_local_bindings = register_destructured_stream_ctors(ctx, decl);
            let filtered_pat;
            let pattern: &ast::Pat = if skip_local_bindings.is_empty() {
                &decl.name
            } else if let ast::Pat::Object(obj) = &decl.name {
                let mut obj = obj.clone();
                obj.props.retain(|prop| match prop {
                    ast::ObjectPatProp::Assign(a) => {
                        !skip_local_bindings.contains(&a.key.sym.to_string())
                    }
                    ast::ObjectPatProp::KeyValue(kv) => match kv.value.as_ref() {
                        ast::Pat::Ident(b) => !skip_local_bindings.contains(&b.id.sym.to_string()),
                        _ => true,
                    },
                    _ => true,
                });
                if obj.props.is_empty() {
                    // Every binding became a native alias; nothing left to
                    // bind at runtime (require of a builtin module has no
                    // observable side effects).
                    return Ok(result);
                }
                filtered_pat = ast::Pat::Object(obj);
                &filtered_pat
            } else {
                &decl.name
            };

            // Delegate to the recursive pattern binding helper so that all
            // destructuring features (nested patterns, defaults, rest, computed
            // keys) work consistently across all call sites.

            // ink-shape useState: `const [v, setV] = useState(0)` (#679 Phase 1).
            // Rewrite RHS to call useStateTuple which returns a real
            // [value, setter_closure] 2-element array. Without this, the
            // regular destructure path indexes a scalar return as if it were
            // an array — both elements come out undefined.
            let init_expr =
                if let (ast::Pat::Array(_), Some(init)) = (&decl.name, decl.init.as_ref()) {
                    if let Some(rewritten) = rewrite_use_state_tuple(ctx, init) {
                        rewritten
                    } else {
                        lower_expr(ctx, init)?
                    }
                } else {
                    decl.init
                        .as_ref()
                        .map(|e| lower_expr(ctx, e))
                        .transpose()?
                        .ok_or_else(|| anyhow!("Destructuring requires an initializer"))?
                };
            let stmts = lower_pattern_binding(ctx, pattern, init_expr, mutable)?;
            result.extend(stmts);
        }
        _ => {
            // For other patterns, fall back to existing behavior
            let name = get_binding_name(&decl.name)?;
            let ty = extract_binding_type(&decl.name);
            if let Some(init_ast) = decl.init.as_ref() {
                result.extend(predeclare_implicit_assignment_targets(ctx, init_ast));
            }
            let init = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            // #321: a generator function EXPRESSION bound to a name (`const g =
            // function*(){}`) — register the name so `for (x of g())` / `[...g()]`
            // take the iterator-protocol path, matching named `function* g(){}`
            // declarations. (`.next()`-driving works regardless via the lifted
            // generator transform; this covers the for-of/spread call sites,
            // whose detection in stmt_loops.rs is name-based.)
            if let Some(Expr::Closure {
                is_generator: true,
                is_async,
                ..
            }) = &init
            {
                ctx.generator_func_names.insert(name.clone());
                if *is_async {
                    ctx.async_generator_func_names.insert(name.clone());
                }
            }
            let id = if ctx.scope_depth == 0
                && ctx.inside_block_scope == 0
                && ctx.pre_registered_module_vars.remove(&name)
            {
                ctx.pre_registered_module_var_decls.remove(&name);
                // #1758: module-scope only — see the sibling guard above.
                let id = ctx.lookup_local(&name).unwrap();
                if let Some(existing_ty) = ctx.locals.lookup_type_mut(&name) {
                    *existing_ty = ty.clone();
                }
                id
            } else {
                ctx.define_local(name.clone(), ty.clone())
            };
            if !mutable {
                ctx.mark_local_immutable(id);
            }
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            });
        }
    }

    Ok(result)
}
