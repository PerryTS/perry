//! Native-instance registration driven by `new`/`await new`/factory-call/
//! method-chain initializers for a simple `let/const/var` identifier binding
//! (extracted from `var_decl.rs`'s `Pat::Ident` arm).

use super::*;

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;

use crate::destructuring::var_decl_sources::*;

/// Registers `name` as a native instance based on `new ClassName(...)`,
/// `new mod.Class(...)`, `await new Class(...)`, native-module factory calls,
/// awaited factory calls, and method-chains on existing native instances.
/// Pure side effects on `ctx`; mirrors the original inline blocks verbatim.
pub(crate) fn register_native_from_new_and_calls(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    name: &str,
) {
    // Check if this is a native class instantiation and register it
    if let Some(init_expr) = &decl.init {
        if let ast::Expr::New(new_expr) = init_expr.as_ref() {
            if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                let local_name = class_ident.sym.as_ref();
                // A user `class Big {...}` in scope shadows the
                // hardcoded library-name fallback below. Without
                // this gate `class Big { f0=0; ... } const b = new
                // Big()` routed through big.js's handle-based
                // dispatch so every property read returned 0.
                let user_class_defined = ctx.classes_index.contains_key(local_name)
                    || ctx.pending_classes.iter().any(|c| c.name == local_name);
                // #wall: alias-aware native-instance tagging. An
                // ALIASED import (`import { BlockList as Wj4 } from
                // "net"; const q = new Wj4()`) must register `q` under
                // the IMPORTED class ("BlockList"), not the local alias
                // ("Wj4"), or `q.addSubnet(...)` dispatch (keyed on
                // `("net","BlockList")`) misses and falls to generic
                // property access ("addSubnet is not a function").
                // `lookup_native_module` is alias-aware (the named
                // import registers `local → (module, Some(<imported>))`),
                // so resolve the local to its imported export name and
                // use THAT as the class name for the hardcoded match and
                // the final registration. For the un-aliased case the
                // export equals the local, so this is a no-op.
                let class_name: &str = ctx
                    .lookup_native_module(local_name)
                    .and_then(|(_m, method)| method)
                    .filter(|export| {
                        export
                            .chars()
                            .next()
                            .map(|c| c.is_uppercase())
                            .unwrap_or(false)
                    })
                    .unwrap_or(local_name);
                // First try the general native module lookup (covers all imported native classes)
                let module_name = if let Some((m, method)) = ctx.lookup_native_module(local_name) {
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
                        "EventEmitter" | "EventEmitterAsyncResource" => Some("events".to_string()),
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
                    ("DiffieHellman" | "DiffieHellmanGroup", Some("crypto" | "node:crypto")) => {
                        None
                    }
                    _ => module_name,
                };
                if let Some(module) = module_name {
                    ctx.register_native_instance(name.to_string(), module, class_name.to_string());
                }
            } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                if let (ast::Expr::Ident(module_ident), ast::MemberProp::Ident(class_ident)) =
                    (member.obj.as_ref(), &member.prop)
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
                            let (mod_for_class, cls_for_class) = match (module_name, class_name) {
                                ("http" | "https", "Agent") => ("http", "Agent"),
                                ("net" | "node:net", _) => ("net", class_name),
                                _ => (module_name, class_name),
                            };
                            ctx.register_native_instance(
                                name.to_string(),
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
                                    name.to_string(),
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
                            name.to_string(),
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
                                    ("dns" | "dns/promises", "Resolver") => Some("Resolver"),
                                    ("mysql2" | "mysql2/promise", "createPool") => Some("Pool"),
                                    ("mysql2" | "mysql2/promise", "createConnection") => {
                                        Some("Connection")
                                    }
                                    ("pg", "connect") => Some("Client"),
                                    ("http" | "https", "request" | "get") => Some("ClientRequest"),
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
                                    ("http2", "createSecureServer") => Some("Http2SecureServer"),
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
                                    ("perry/tui", "useFocusManager") => Some("FocusManager"),
                                    _ => None,
                                };
                                if let Some(class_name) = class_name {
                                    let class_module = if class_name == "ClientRequest" {
                                        "http"
                                    } else {
                                        module_name
                                    };
                                    ctx.register_native_instance(
                                        name.to_string(),
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
                            name.to_string(),
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
                                        name.to_string(),
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
                                name.to_string(),
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
                                    name.to_string(),
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
                        let http_class = match (module_name.as_str(), method_name.as_str()) {
                            ("http", "createServer") => Some("HttpServer"),
                            ("https", "createServer") => Some("HttpsServer"),
                            ("http2", "createSecureServer") => Some("Http2SecureServer"),
                            ("async_hooks", "createHook") => Some("AsyncHook"),
                            ("dns" | "dns/promises", "Resolver") => Some("Resolver"),
                            _ => None,
                        };
                        if let Some(cn) = http_class {
                            ctx.register_native_instance(
                                name.to_string(),
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
                            if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let class_name = match (module_name, method_ident.sym.as_ref())
                                    {
                                        ("mongodb", "connect") => Some("MongoClient"),
                                        ("mysql2" | "mysql2/promise", "createPool") => Some("Pool"),
                                        ("mysql2" | "mysql2/promise", "createConnection") => {
                                            Some("Connection")
                                        }
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
                                            "get" | "post" | "put" | "delete" | "patch" | "request",
                                        ) => Some("Response"),
                                        _ => None,
                                    };
                                    if let Some(class_name) = class_name {
                                        ctx.register_native_instance(
                                            name.to_string(),
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
                                let returns_handle = match (module_name.as_str(), method_name) {
                                    ("mongodb", "db") => Some("Database"),
                                    ("mongodb", "collection") => Some("Collection"),
                                    ("mysql2" | "mysql2/promise", "getConnection") => {
                                        Some("PoolConnection")
                                    }
                                    ("better-sqlite3", "prepare") => Some("Statement"),
                                    ("sqlite", "prepare") => Some("StatementSync"),
                                    ("sqlite", "createSession") => Some("Session"),
                                    // dayjs / moment manipulation methods return a
                                    // NEW date handle — without re-registering the
                                    // binding, `const d2 = d.add(7, 'day');
                                    // d2.format(...)` fell to generic dispatch
                                    // (undefined). "App" matches the factory-result
                                    // registration class.
                                    ("dayjs", "add" | "subtract" | "startOf" | "endOf") => {
                                        Some("App")
                                    }
                                    (
                                        "moment",
                                        "add" | "subtract" | "startOf" | "endOf" | "clone",
                                    ) => Some("App"),
                                    _ => None,
                                };
                                if let Some(class_name) = returns_handle {
                                    ctx.register_native_instance(
                                        name.to_string(),
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
}
