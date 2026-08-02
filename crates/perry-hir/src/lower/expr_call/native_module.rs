//! Native module method calls (process/tty/os/Buffer/Uint8Array/Object/Symbol/Array/net).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use swc_ecma_ast as ast;

use super::reflect_args::{take_reflect_ktp_args, take_reflect_kvtp_args, take_reflect_tp_args};
use crate::ir::*;

use super::super::{is_generator_call_expr, lower_expr, LoweringContext};
use super::os::user_info_expr_for_call;

mod buffer_statics;
mod imported_module_dispatch;
mod object_statics;
mod process_module;
mod reflect_statics;

fn path_submodule_name(module_name: &str) -> Option<&'static str> {
    match module_name.strip_prefix("node:").unwrap_or(module_name) {
        "path/posix" | "path.posix" => Some("posix"),
        "path/win32" | "path.win32" => Some("win32"),
        _ => None,
    }
}

fn is_cluster_default_event_emitter_method(method_name: &str) -> bool {
    matches!(
        method_name,
        "on" | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "emit"
            | "eventNames"
            | "listenerCount"
            | "listeners"
            | "rawListeners"
            | "getMaxListeners"
            | "setMaxListeners"
            | "removeListener"
            | "off"
            | "removeAllListeners"
    )
}

/// Internal process helpers that return arrays through native dispatch.
fn is_process_active_array_helper(method: &str) -> bool {
    matches!(method, "_getActiveHandles" | "_getActiveRequests")
}

/// Peel runtime-transparent TypeScript wrappers (`as`, `as const`, `!`,
/// `satisfies`, angle-bracket assertions, parens) off an expression so a
/// cast receiver like `(Readable as any).toWeb(...)` still matches the
/// bare-identifier module/class shape the dispatch arms below expect.
fn unwrap_ts_wrappers(e: &ast::Expr) -> &ast::Expr {
    let mut cur = e;
    loop {
        match cur {
            ast::Expr::TsAs(x) => cur = &x.expr,
            ast::Expr::TsNonNull(x) => cur = &x.expr,
            ast::Expr::TsSatisfies(x) => cur = &x.expr,
            ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
            ast::Expr::TsConstAssertion(x) => cur = &x.expr,
            ast::Expr::Paren(x) => cur = &x.expr,
            _ => return cur,
        }
    }
}

fn require_literal_native_module(ctx: &LoweringContext, expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Call(call) = unwrap_ts_wrappers(expr) else {
        return None;
    };
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return None;
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return None;
    };
    if ident.sym.as_ref() != "require"
        || ctx.lookup_local("require").is_some()
        || ctx.lookup_func("require").is_some()
        || ctx.lookup_imported_func("require").is_some()
        || call.args.len() != 1
        || call.args[0].spread.is_some()
    {
        return None;
    }
    let ast::Expr::Lit(ast::Lit::Str(s)) = call.args[0].expr.as_ref() else {
        return None;
    };
    let spec = s.value.as_str().unwrap_or("");
    crate::destructuring::resolvable_native_module_for_spec(spec)
}

fn is_node_stream_class_name(name: &str) -> bool {
    matches!(
        name,
        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
    )
}

fn event_emitter_constructor_call(args: Vec<Expr>) -> Expr {
    let Some(receiver) = args.first().cloned() else {
        return Expr::Undefined;
    };
    if !matches!(receiver, Expr::This | Expr::LocalGet(_)) {
        return Expr::Undefined;
    }
    let mut exprs = vec![
        Expr::PropertySet {
            object: Box::new(receiver.clone()),
            property: "_events".to_string(),
            value: Box::new(Expr::Object(Vec::new())),
        },
        Expr::PropertySet {
            object: Box::new(receiver.clone()),
            property: "_eventsCount".to_string(),
            value: Box::new(Expr::Number(0.0)),
        },
        Expr::PropertySet {
            object: Box::new(receiver),
            property: "_maxListeners".to_string(),
            value: Box::new(Expr::Undefined),
        },
    ];
    exprs.extend(args.into_iter().skip(1));
    exprs.push(Expr::Undefined);
    Expr::Sequence(exprs)
}

fn lower_os_module_method_call(
    call: &ast::CallExpr,
    method_name: &str,
    args: &[Expr],
) -> Option<Expr> {
    match method_name {
        "availableParallelism" => Some(Expr::OsAvailableParallelism),
        "platform" => Some(Expr::OsPlatform),
        "arch" => Some(Expr::OsArch),
        "endianness" => Some(Expr::OsEndianness),
        "hostname" => Some(Expr::OsHostname),
        "homedir" => Some(Expr::OsHomedir),
        "tmpdir" => Some(Expr::OsTmpdir),
        "loadavg" => Some(Expr::OsLoadavg),
        "machine" => Some(Expr::OsMachine),
        "totalmem" => Some(Expr::OsTotalmem),
        "freemem" => Some(Expr::OsFreemem),
        "uptime" => Some(Expr::OsUptime),
        "type" => Some(Expr::OsType),
        "release" => Some(Expr::OsRelease),
        "version" => Some(Expr::OsVersion),
        "cpus" => Some(Expr::OsCpus),
        "networkInterfaces" => Some(Expr::OsNetworkInterfaces),
        "userInfo" => Some(user_info_expr_for_call(call, args.to_vec())),
        "getPriority" | "setPriority" => Some(Expr::NativeMethodCall {
            module: "os".to_string(),
            class_name: None,
            object: None,
            method: method_name.to_string(),
            args: args.to_vec(),
        }),
        _ => None,
    }
}

/// Recognize a bundled-mysql2 `createPool` / `createConnection` call by the
/// shape of its config object, so it can be routed to perry-ext-mysql2 even
/// when a bundler inlined mysql2 under a numeric module id (the import-keyed
/// native lowering can't see through that — see the call site).
///
/// The signature is deliberately tight so it cannot hijack an unrelated
/// `createPool`/`createConnection`: the sole config argument must be an object
/// LITERAL that carries BOTH a mysql connection key (`uri`/`host`/`socketPath`)
/// AND at least one mysql2-specific driver/pool option (`connectionLimit`,
/// `waitForConnections`, `queueLimit`, `namedPlaceholders`, …). generic-pool's
/// `createPool(factory, opts)` passes a factory object with `create`/`destroy`
/// (no connection key); pg uses `new Pool()`, not `.createPool({...})`. The
/// older `mysql` package shares mysql2's exact API and wire protocol, so
/// routing it to perry-ext-mysql2 is correct too. A non-literal config
/// (variable / spread) returns `None` and falls through to normal lowering.
///
/// Returns the canonical method name (`"createPool"` / `"createConnection"`).
///
/// Pure signature check over a config object's field names.
fn mysql2_config_signature(method_name: &str, keys: &[&str]) -> Option<&'static str> {
    let canonical = match method_name {
        "createPool" => "createPool",
        "createConnection" => "createConnection",
        _ => return None,
    };
    let mut has_conn_key = false;
    let mut has_mysql2_opt = false;
    for key in keys {
        match *key {
            "uri" | "host" | "socketPath" => has_conn_key = true,
            "connectionLimit" | "waitForConnections" | "queueLimit" | "maxIdle" | "idleTimeout"
            | "namedPlaceholders" | "rowsAsArray" | "enableKeepAlive" | "multipleStatements"
            | "typeCast" => has_mysql2_opt = true,
            _ => {}
        }
    }
    if has_conn_key && has_mysql2_opt {
        Some(canonical)
    } else {
        None
    }
}

/// Recover the config object's keys and run the mysql2 signature check. A closed
/// object literal is lowered to `New { class_name: "__AnonShape_*" }` with the
/// keys stripped into the shape class, so recover them from the lowering
/// context's `anon_shape_fields` map; a literal that stayed `Expr::Object`
/// (small / open shape) is inspected directly.
fn detect_bundled_mysql2_create(
    ctx: &LoweringContext,
    method_name: &str,
    args: &[Expr],
) -> Option<&'static str> {
    let keys: Vec<&str> = match args.first()? {
        Expr::Object(pairs) => pairs.iter().map(|(k, _)| k.as_str()).collect(),
        Expr::New { class_name, .. } => ctx
            .anon_shape_fields
            .get(class_name)?
            .iter()
            .map(|s| s.as_str())
            .collect(),
        _ => return None,
    };
    mysql2_config_signature(method_name, &keys)
}

/// Return the complete static member path whose root identifier resolves to
/// `module_name` as a whole-module reference. Computed properties are not
/// accepted because they are not a statically known namespace path.
fn native_module_member_path(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    module_name: &str,
) -> Option<Vec<String>> {
    match unwrap_ts_wrappers(expr) {
        ast::Expr::Ident(ident) => matches!(
            ctx.lookup_native_module(ident.sym.as_ref()),
            Some((m, None)) if m == module_name
        )
        .then(Vec::new),
        ast::Expr::Member(member) => {
            let ast::MemberProp::Ident(prop) = &member.prop else {
                return None;
            };
            let mut path = native_module_member_path(ctx, member.obj.as_ref(), module_name)?;
            path.push(prop.sym.to_string());
            Some(path)
        }
        _ => None,
    }
}

/// node-forge sub-namespace flattening. Unlike the single-level `ns.method()`
/// shape the other arms match, forge's API is deeply nested:
/// `forge.pki.rsa.generateKeyPair(...)`, `forge.pki.createCertificate()`,
/// `forge.md.sha256.create()`. The call's receiver is therefore a CHAIN of
/// `Member`s (not a bare native-module `Ident`), so none of them fire — and
/// worse, an intermediate read like `forge.pki` otherwise reaches the
/// unimplemented-API gate in `expr_member` (no `node-forge` symbol named
/// `pki`) and defers a throw. Collapse any member chain rooted at the
/// node-forge default import down to its LAST segment, which is exactly the
/// method key the codegen `NATIVE_MODULE_TABLE` rows use (`generateKeyPair`,
/// `createCertificate`, `create`, `privateKeyToPem`, …). Only the exact
/// implemented paths are flattened; in particular, `forge.md.md5.create()`
/// must not accidentally dispatch to the SHA-256 marker just because its final
/// segment is also `create`. `createCertificate` is typed
/// back to a `Certificate` instance by the factory map in
/// `js_transform/local_natives.rs`, so `cert.setSubject(...)` etc. dispatch
/// through the normal single-level instance path.
///
/// Runs BEFORE the generic namespace/`module.Class.staticMethod` dispatch so it
/// wins for the 2-level `forge.pki.createCertificate()` shape that
/// `try_module_class_static` would otherwise claim (reading `forge.pki` as
/// `module.Class` and hitting the gate).
pub(super) fn try_node_forge_namespace(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Expr, Vec<Expr>> {
    let Some(path) = native_module_member_path(ctx, expr, "node-forge") else {
        return Err(args);
    };
    let method = match path
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["pki", "rsa", "generateKeyPair"] => "generateKeyPair",
        ["pki", method @ ("createCertificate" | "certificateFromPem" | "certificateToPem"
        | "privateKeyFromPem" | "privateKeyToPem" | "publicKeyToPem")] => method,
        ["md", "sha256", "create"] => "create",
        _ => return Err(args),
    };
    return Ok(Expr::NativeMethodCall {
        module: "node-forge".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    });
}

pub(super) fn try_native_module_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    mut args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // `Session` is a class export. A direct call is not construction, even
    // though the native constructor fast path handles `new Session()`.
    if let ast::Expr::Ident(ident) = expr {
        if matches!(
            ctx.lookup_native_module(ident.sym.as_ref()),
            Some(("inspector/promises", Some("Session")))
        ) {
            return Ok(Ok(Expr::NativeMethodCall {
                module: "inspector/promises".to_string(),
                class_name: None,
                object: None,
                method: "SessionCall".to_string(),
                args,
            }));
        }
    }
    // Check for native module method calls (e.g., mysql.createConnection())
    if let ast::Expr::Member(member) = expr {
        // Bundled mysql2 (webpack/turbopack): when a bundler inlines mysql2
        // under a numeric module id, the `createPool(...)` receiver is an
        // opaque `E.i(87205).default`, not a tracked native import — so the
        // identifier-keyed lowering below never fires and the call runs the
        // inlined JS mysql2, which JIT-compiles its row parsers with
        // `new Function` (via `generate-function`). An AOT binary cannot
        // execute a function built from a runtime string, so every query
        // throws. Recognize the call by its mysql2 config-object SIGNATURE
        // (tight enough to be mysql/mysql2-exclusive) and route it to
        // perry-ext-mysql2 regardless of how mysql2 was imported/bundled.
        // The receiver is intentionally dropped: we don't want the JS module
        // loaded at all. Downstream typing (`detect_native_instance_creation`)
        // sees `NativeMethodCall{module:"mysql2/promise", method:"createPool"}`
        // and tags the result `Pool`, so `pool.execute`/`pool.query` dispatch
        // natively too; the emitted `js_mysql2_*` FFIs flip the "mysql2"
        // well-known (see perry-codegen `ext_registry`) to link the staticlib.
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            if let Some(canonical) =
                detect_bundled_mysql2_create(ctx, method_ident.sym.as_ref(), &args)
            {
                return Ok(Ok(Expr::NativeMethodCall {
                    module: "mysql2/promise".to_string(),
                    class_name: None,
                    object: None,
                    method: canonical.to_string(),
                    args,
                }));
            }
        }

        // Inline `require("node:os").platform()` reaches this outer member
        // call before the inner bare `require(...)` lowering can produce a
        // NativeModuleRef. Recognize the same literal-native namespace shape
        // here so it dispatches like `import * as os from "node:os"`.
        if require_literal_native_module(ctx, member.obj.as_ref()).as_deref() == Some("os") {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                if let Some(expr) =
                    lower_os_module_method_call(call, method_ident.sym.as_ref(), &args)
                {
                    return Ok(Ok(expr));
                }
            }
        }

        // #1534/#1540/#1541: the stream acceptance tests deliberately cast
        // the class / namespace before a static call —
        // `(Readable as any).isErrored(r)`, `(Readable as any).toWeb(r)`,
        // `(stream as any).addAbortSignal(sig, r)`. The cast is a runtime
        // no-op, so peel TS-only wrappers off the receiver before matching
        // it as the module/class identifier; otherwise the call falls
        // through to generic dispatch and the static reads as `undefined`.
        if let ast::Expr::Ident(obj_ident) = unwrap_ts_wrappers(member.obj.as_ref()) {
            let obj_name = obj_ident.sym.to_string();

            if matches!(
                ctx.lookup_native_module(&obj_name),
                Some(("stream/web", Some("ReadableStream")))
                    | Some(("node:stream/web", Some("ReadableStream")))
            ) {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if method_ident.sym.as_ref() == "from" {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "readable_stream".to_string(),
                            class_name: Some("ReadableStream".to_string()),
                            object: None,
                            method: "from".to_string(),
                            args,
                        }));
                    }
                }
            }

            args = match process_module::try_process_module_methods(ctx, member, &obj_name, args)? {
                Ok(expr) => return Ok(Ok(expr)),
                Err(rest) => rest,
            };

            // Check for tty module methods (#347 Phase 3)
            let is_tty_module =
                obj_name == "tty" || ctx.lookup_builtin_module_alias(&obj_name) == Some("tty");
            if is_tty_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if method_ident.sym.as_ref() == "isatty" && !args.is_empty() {
                        let arg = args.into_iter().next().unwrap();
                        return Ok(Ok(Expr::TtyIsAtty(Box::new(arg))));
                    }
                }
            }

            // Check for os module methods FIRST (before generic NativeMethodCall)
            let is_os_module =
                obj_name == "os" || ctx.lookup_builtin_module_alias(&obj_name) == Some("os");
            if is_os_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if let Some(expr) =
                        lower_os_module_method_call(call, method_ident.sym.as_ref(), &args)
                    {
                        return Ok(Ok(expr));
                    }
                }
            }

            // node:v8 module methods (#3137/#3138/#3140).
            // serialize/deserialize, heap-stat helpers, and heap-snapshot
            // helpers lower to a receiver-less NativeMethodCall dispatched in
            // codegen to the `js_v8_*` runtime entry points.
            let is_v8_module =
                obj_name == "v8" || ctx.lookup_builtin_module_alias(&obj_name) == Some("v8");
            if is_v8_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "serialize"
                        | "deserialize"
                        | "getHeapStatistics"
                        | "getHeapCodeStatistics"
                        | "getHeapSpaceStatistics"
                        | "cachedDataVersionTag"
                        | "getHeapSnapshot"
                        | "writeHeapSnapshot" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "v8".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            }));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            args = match buffer_statics::try_buffer_uint8array_statics(
                ctx, member, &obj_name, args,
            )? {
                Ok(expr) => return Ok(Ok(expr)),
                Err(rest) => rest,
            };

            args = match object_statics::try_object_statics(ctx, call, member, &obj_name, args)? {
                Ok(expr) => return Ok(Ok(expr)),
                Err(rest) => rest,
            };

            // Check for Symbol static methods: Symbol.for / Symbol.keyFor.
            // Accept BOTH the dot form (`Symbol.for(...)`) and the
            // computed-string form (`Symbol['for'](...)`) — the latter is what
            // the userland `buffer` package writes (`Symbol['for']('nodejs.util.
            // inspect.custom')`). Previously only `MemberProp::Ident` matched, so
            // `Symbol['for'](...)` fell through to generic dispatch, which dropped
            // the `Symbol` receiver and lowered the callee as `globalThis.for`
            // (undefined) → `TypeError: value is not a function` at buffer's
            // module eval (the safer-buffer/iconv-lite/body-parser/express chain).
            if obj_name == "Symbol" {
                let method_name: Option<&str> = match &member.prop {
                    ast::MemberProp::Ident(method_ident) => Some(method_ident.sym.as_ref()),
                    ast::MemberProp::Computed(c) => match c.expr.as_ref() {
                        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                        _ => None,
                    },
                    _ => None,
                };
                match method_name {
                    Some("for") => {
                        let key = args.into_iter().next().unwrap_or(Expr::Undefined);
                        return Ok(Ok(Expr::SymbolFor(Box::new(key))));
                    }
                    Some("keyFor") => {
                        let sym = args.into_iter().next().unwrap_or(Expr::Undefined);
                        return Ok(Ok(Expr::SymbolKeyFor(Box::new(sym))));
                    }
                    _ => {} // Fall through to generic handling
                }
            }

            // Check for RegExp static methods: RegExp.escape (#2899).
            // #6677: accept the string-literal computed form too.
            if obj_name == "RegExp" {
                if let Some(method_name) = super::static_call_prop_name(&member.prop) {
                    if method_name == "escape" {
                        let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                        return Ok(Ok(Expr::RegExpEscape(Box::new(arg))));
                    }
                }
            }

            // Check for Map static methods: Map.groupBy. #6677: computed form too.
            if obj_name == "Map" {
                if let Some(method_name) = super::static_call_prop_name(&member.prop) {
                    if method_name == "groupBy" && args.len() >= 2 {
                        let mut iter = args.into_iter();
                        let items = iter.next().unwrap();
                        let key_fn = iter.next().unwrap();
                        let key_fn = ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                        return Ok(Ok(Expr::MapGroupBy {
                            items: Box::new(items),
                            key_fn: Box::new(key_fn),
                        }));
                    }
                }
            }

            args = match reflect_statics::try_reflect_statics(ctx, call, member, &obj_name, args)? {
                Ok(expr) => return Ok(Ok(expr)),
                Err(rest) => rest,
            };

            if obj_name == "Proxy" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if method_ident.sym.as_ref() == "revocable" {
                        let mut it = args.into_iter();
                        let target = it.next().unwrap_or(Expr::Undefined);
                        let handler = it.next().unwrap_or(Expr::Object(vec![]));
                        return Ok(Ok(Expr::ProxyRevocable {
                            target: Box::new(target),
                            handler: Box::new(handler),
                        }));
                    }
                }
            }

            // Check for Array static methods. #6677: computed form too.
            if obj_name == "Array" {
                if let Some(method_name) = super::static_call_prop_name(&member.prop) {
                    match method_name {
                        "isArray" => {
                            let value = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ArrayIsArray(Box::new(value))));
                        }
                        "from" => {
                            let value = args.first().cloned().unwrap_or(Expr::Undefined);
                            // `Array.from(iterable, mapFn)` uses a dedicated HIR
                            // variant so codegen can handle Map/Set/Array sources
                            // uniformly (materialize + js_array_map).
                            if let Some(map_fn) = args.get(1).cloned() {
                                // #2773: carry the optional thisArg (3rd arg) so
                                // a non-arrow mapFn can bind `this`.
                                let this_arg = args.get(2).cloned().map(Box::new);
                                return Ok(Ok(Expr::ArrayFromMapped {
                                    iterable: Box::new(value),
                                    map_fn: Box::new(map_fn),
                                    this_arg,
                                }));
                            }
                            // Check if the source is a generator call — use iterator protocol
                            let is_gen = is_generator_call_expr(ctx, &value);
                            if is_gen {
                                return Ok(Ok(Expr::IteratorToArray(Box::new(value))));
                            }
                            return Ok(Ok(Expr::ArrayFrom(Box::new(value))));
                        }
                        "of" => {
                            // Array.of(1,2,3) is equivalent to [1,2,3]
                            return Ok(Ok(Expr::Array(args)));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for net module methods
            let is_net_module =
                obj_name == "net" || ctx.lookup_builtin_module_alias(&obj_name) == Some("net");
            if is_net_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "createServer" => {
                            let (options, connection_listener) = match args.as_slice() {
                                [Expr::Closure { .. }] => {
                                    (None, args.first().cloned().map(Box::new))
                                }
                                _ => (
                                    args.first().cloned().map(Box::new),
                                    args.get(1).cloned().map(Box::new),
                                ),
                            };
                            return Ok(Ok(Expr::NetCreateServer {
                                options,
                                connection_listener,
                            }));
                        }
                        // createConnection/connect fall through to generic NativeMethodCall
                        // so they dispatch via NATIVE_MODULE_TABLE to the new
                        // event-driven `js_net_socket_connect` in perry-stdlib (A1/A1.5).
                        // The dedicated `Expr::NetCreateConnection` variant was never
                        // lowered by the LLVM backend and remained as vestigial HIR;
                        // the generic path gives us working codegen for free.
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            args = match imported_module_dispatch::try_imported_module_dispatch(
                ctx, member, &obj_name, args,
            )? {
                Ok(expr) => return Ok(Ok(expr)),
                Err(rest) => rest,
            };
        }
    }

    Ok(Err(args))
}

#[cfg(test)]
mod bundled_mysql2_tests {
    use super::{is_process_active_array_helper, mysql2_config_signature};

    #[test]
    fn process_active_array_helper_predicate_matches_supported_methods() {
        assert!(is_process_active_array_helper("_getActiveHandles"));
        assert!(is_process_active_array_helper("_getActiveRequests"));
        assert!(!is_process_active_array_helper("getActiveResourcesInfo"));
    }

    #[test]
    fn matches_pool_with_uri_and_pool_option() {
        // gscmaster's exact config: uri + mysql2 pool options.
        let keys = [
            "uri",
            "waitForConnections",
            "connectionLimit",
            "maxIdle",
            "idleTimeout",
            "queueLimit",
        ];
        assert_eq!(
            mysql2_config_signature("createPool", &keys),
            Some("createPool")
        );
    }

    #[test]
    fn matches_host_credentials_with_pool_option() {
        let keys = ["host", "user", "password", "database", "connectionLimit"];
        assert_eq!(
            mysql2_config_signature("createPool", &keys),
            Some("createPool")
        );
    }

    #[test]
    fn matches_create_connection_with_mysql2_option() {
        let keys = ["host", "user", "password", "namedPlaceholders"];
        assert_eq!(
            mysql2_config_signature("createConnection", &keys),
            Some("createConnection")
        );
    }

    #[test]
    fn rejects_without_connection_key() {
        // Pool options but no connection key — not enough to be sure it's mysql2.
        let keys = ["connectionLimit", "waitForConnections"];
        assert_eq!(mysql2_config_signature("createPool", &keys), None);
    }

    #[test]
    fn rejects_without_mysql2_option() {
        // A bare connection config could be any driver; require a mysql2 option.
        let keys = ["host", "user", "password", "database"];
        assert_eq!(mysql2_config_signature("createPool", &keys), None);
    }

    #[test]
    fn rejects_generic_pool_factory() {
        // generic-pool's `createPool(factory, opts)` — first arg is a factory
        // object with create/destroy, no connection or mysql2 keys.
        let keys = ["create", "destroy", "validate"];
        assert_eq!(mysql2_config_signature("createPool", &keys), None);
    }

    #[test]
    fn rejects_unrelated_method() {
        let keys = ["uri", "connectionLimit"];
        assert_eq!(mysql2_config_signature("connect", &keys), None);
        assert_eq!(mysql2_config_signature("createServer", &keys), None);
    }
}
