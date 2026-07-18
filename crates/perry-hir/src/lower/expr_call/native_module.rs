//! Native module method calls (process/tty/os/Buffer/Uint8Array/Object/Symbol/Array/net).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use super::super::unimpl_hints;
use super::reflect_args::{take_reflect_ktp_args, take_reflect_kvtp_args, take_reflect_tp_args};
use crate::ir::*;

use super::super::{is_generator_call_expr, lower_expr, LoweringContext};
use super::os::user_info_expr_for_call;

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
            | "removeListener"
            | "off"
            | "removeAllListeners"
    )
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

pub(super) fn try_native_module_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
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

            // Check for process module methods. `import processModule from
            // "node:process"` registers as the native `process` object, while
            // `import * as processNamespace` registers as `process.namespace`;
            // both must use the same strict method gate as the global object.
            let process_name_is_shadowed =
                obj_name == "process" && ctx.shadows_unqualified_global("process");
            let is_process_ref = !process_name_is_shadowed
                && (obj_name == "process"
                    || ctx.lookup_builtin_module_alias(&obj_name) == Some("process")
                    || matches!(
                        ctx.lookup_native_module(&obj_name),
                        Some(("process", _)) | Some(("process.namespace", _))
                    ));
            if is_process_ref {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "uptime" => return Ok(Ok(Expr::ProcessUptime)),
                        "cwd" => return Ok(Ok(Expr::ProcessCwd)),
                        "memoryUsage" => return Ok(Ok(Expr::ProcessMemoryUsage)),
                        "nextTick" => {
                            if !args.is_empty() {
                                let mut iter = args.into_iter();
                                let callback = iter.next().unwrap();
                                let trailing: Vec<Expr> = iter.collect();
                                return Ok(Ok(Expr::ProcessNextTick {
                                    callback: Box::new(callback),
                                    args: trailing,
                                }));
                            }
                        }
                        "on"
                        | "addListener"
                        | "once"
                        | "prependListener"
                        | "prependOnceListener"
                        | "emit"
                        | "listeners"
                        | "rawListeners"
                        | "eventNames"
                        | "listenerCount"
                        | "removeListener"
                        | "off"
                        | "removeAllListeners"
                        | "setMaxListeners"
                        | "getMaxListeners" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            }));
                        }
                        "chdir" => {
                            if !args.is_empty() {
                                return Ok(Ok(Expr::ProcessChdir(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                        }
                        "kill" => {
                            if !args.is_empty() {
                                let mut iter = args.into_iter();
                                let pid = iter.next().unwrap();
                                let signal = iter.next().map(Box::new);
                                return Ok(Ok(Expr::ProcessKill {
                                    pid: Box::new(pid),
                                    signal,
                                }));
                            }
                        }
                        "ref" | "unref" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            }));
                        }
                        "setSourceMapsEnabled" => {
                            // #1400 / #3108: process.setSourceMapsEnabled(bool)
                            // toggles the live source-map flag. Perry compiles
                            // AOT and has no resolver, so the flag drives
                            // nothing observable — but it round-trips through
                            // process.sourceMapsEnabled and validates that the
                            // argument is a boolean (else ERR_INVALID_ARG_TYPE),
                            // matching Node. Lower to the runtime setter,
                            // passing the original argument for validation.
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: "setSourceMapsEnabled".to_string(),
                                args,
                            }));
                        }
                        "getBuiltinModule" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: "getBuiltinModule".to_string(),
                                args,
                            }));
                        }
                        "execve" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: "execve".to_string(),
                                args,
                            }));
                        }
                        "dlopen" => {
                            // #1409: process.dlopen(module, filename, flags?)
                            // is Node's native-addon (.node) loader. Perry
                            // statically links every dependency at compile
                            // time — there's no dynamic loader to call.
                            // Returning undefined is the closest no-op:
                            // call sites that probe for the function before
                            // attempting to load an addon (a common pattern
                            // in optional-dep wrappers) see typeof "function"
                            // and a "loaded" non-error, then fall back to
                            // their pure-JS path. Real addon-loading
                            // attempts will surface as the addon's exports
                            // being undefined downstream.
                            return Ok(Ok(Expr::Undefined));
                        }
                        "hasUncaughtExceptionCaptureCallback" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: "hasUncaughtExceptionCaptureCallback".to_string(),
                                args,
                            }));
                        }
                        "setUncaughtExceptionCaptureCallback"
                        | "addUncaughtExceptionCaptureCallback" => {
                            let method_name = method_ident.sym.as_ref().to_string();
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name,
                                args,
                            }));
                        }
                        "loadEnvFile" => {
                            // #1399 / #2135: process.loadEnvFile(path?)
                            // (Node 20.12+) reads a `.env` file from disk and
                            // merges its KEY=value entries into `process.env`.
                            // Previously a no-op because `process.env.X = v`
                            // didn't persist; #1344 has since wired writes
                            // through `std::env::set_var`, so we lower to a
                            // runtime call that actually reads the file.
                            // Keep the original JS value: the runtime handles
                            // omitted/undefined/null defaulting plus Buffer
                            // and file-URL path objects.
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: "loadEnvFile".to_string(),
                                args,
                            }));
                        }
                        "exit" => {
                            // process.exit() / process.exit(code) — never
                            // returns, terminates the process. Until now this
                            // fell through to generic NativeMethodCall which
                            // silently no-op'd, so scripts that rely on it to
                            // end the event loop (e.g. `main().then(() =>
                            // process.exit(0))` in a net-socket driver) would
                            // hang with the socket still keeping the loop alive.
                            let code = if !args.is_empty() {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            } else {
                                None
                            };
                            return Ok(Ok(Expr::ProcessExit(code)));
                        }
                        "abort" => {
                            // process.abort() — raises SIGABRT, no clean
                            // shutdown. Maps to libc::abort() at runtime.
                            return Ok(Ok(Expr::ProcessAbort));
                        }
                        "umask" => {
                            // process.umask(mask?) — returns the current
                            // file-mode creation mask, optionally setting
                            // a new one first and returning the previous.
                            let mask = if !args.is_empty() {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            } else {
                                None
                            };
                            return Ok(Ok(Expr::ProcessUmask(mask)));
                        }
                        "threadCpuUsage" => {
                            // process.threadCpuUsage(prior?) — CPU time used
                            // by the current thread, as { user, system } in
                            // microseconds. If prior is given, returns the
                            // validated delta.
                            let prior = if !args.is_empty() {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            } else {
                                None
                            };
                            return Ok(Ok(Expr::ProcessThreadCpuUsage(prior)));
                        }
                        "availableMemory" => {
                            // process.availableMemory() — free system memory
                            // available to the process, in bytes.
                            return Ok(Ok(Expr::ProcessAvailableMemory));
                        }
                        "constrainedMemory" => {
                            // process.constrainedMemory() — OS-imposed memory
                            // limit (cgroups/container), in bytes. 0 when no
                            // limit applies.
                            return Ok(Ok(Expr::ProcessConstrainedMemory));
                        }
                        // POSIX credential accessors (#1408). All four delegate
                        // to libc::{getuid,geteuid,getgid,getegid}() at runtime.
                        "getuid" => {
                            return Ok(Ok(Expr::ProcessPosixCredential(
                                crate::ir::PosixCredentialKind::Uid,
                            )));
                        }
                        "geteuid" => {
                            return Ok(Ok(Expr::ProcessPosixCredential(
                                crate::ir::PosixCredentialKind::Euid,
                            )));
                        }
                        "getgid" => {
                            return Ok(Ok(Expr::ProcessPosixCredential(
                                crate::ir::PosixCredentialKind::Gid,
                            )));
                        }
                        "getegid" => {
                            return Ok(Ok(Expr::ProcessPosixCredential(
                                crate::ir::PosixCredentialKind::Egid,
                            )));
                        }
                        "getgroups" => {
                            // #2135: process.getgroups() — supplementary
                            // group IDs as a number array. Dispatch through
                            // the generic NativeMethodCall path; the
                            // node_core table row routes to
                            // `js_process_getgroups`.
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: "getgroups".to_string(),
                                args,
                            }));
                        }
                        // #2135: POSIX credential setters — single numeric
                        // ID arg, return undefined. Implemented as libc
                        // wrappers in the runtime (string-username form is
                        // a no-op today; see js_process_setuid for the
                        // out-of-scope note).
                        "setuid" | "seteuid" | "setgid" | "setegid" => {
                            let method_name = method_ident.sym.as_ref().to_string();
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name,
                                args,
                            }));
                        }
                        // #2135: process.setgroups(groups[]) takes an
                        // array of numeric GIDs; process.initgroups(user,
                        // extra_gid) takes a username string + numeric
                        // GID. The runtime decodes the JSValues itself, so
                        // both pass through the generic NativeMethodCall.
                        "setgroups" | "initgroups" => {
                            let method_name = method_ident.sym.as_ref().to_string();
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "process".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name,
                                args,
                            }));
                        }
                        "emitWarning" => {
                            // process.emitWarning(warning[, type, code, ctor])
                            // — writes a formatted warning to stderr. Perry
                            // collapses the overloads into a positional Vec
                            // and lets the runtime do the formatting.
                            return Ok(Ok(Expr::ProcessEmitWarning(args)));
                        }
                        "cpuUsage" => {
                            // process.cpuUsage(prior?) — { user, system } in
                            // microseconds. If prior is given, returns the
                            // diff (clamped to >= 0).
                            let prior = if !args.is_empty() {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            } else {
                                None
                            };
                            return Ok(Ok(Expr::ProcessCpuUsage(prior)));
                        }
                        "resourceUsage" => {
                            return Ok(Ok(Expr::ProcessResourceUsage));
                        }
                        "getActiveResourcesInfo" => {
                            return Ok(Ok(Expr::ProcessActiveResourcesInfo));
                        }
                        "hrtime" => {
                            // process.hrtime(prior?) — [secs, nanos] from a
                            // monotonic clock. With prior, returns the diff.
                            // `process.hrtime.bigint()` is intercepted earlier.
                            let prior = if !args.is_empty() {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            } else {
                                None
                            };
                            return Ok(Ok(Expr::ProcessHrtime(prior)));
                        }
                        _ => {
                            let hint = unimpl_hints::module_member_hint("process", method_name)
                                .map(|h| format!(" {h}"))
                                .unwrap_or_default();
                            let msg = format!(
                                "`process.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                method_name, hint,
                            );
                            // #5245: default → throw-on-reach + notice; strict
                            // (`perry.strict` / `--strict-unimplemented`) → hard
                            // #463 refusal. Tree-shake deferral handled inside.
                            let api = format!("process.{method_name}");
                            let location = crate::eval_classifier::location_string(
                                &ctx.source_file_path,
                                member.span.lo.0,
                            );
                            match crate::check_unimplemented_api(
                                &msg,
                                &api,
                                &location,
                                member.span.lo.0,
                            ) {
                                crate::UnimplementedDecision::Refuse => {
                                    crate::lower_bail!(member.span, "{}", msg);
                                }
                                crate::UnimplementedDecision::DeferToRuntimeError(runtime_msg) => {
                                    return Ok(Ok(
                                        super::super::const_fold_fn::synth_deferred_throw_value(
                                            ctx,
                                            &runtime_msg,
                                            member.span,
                                        )?,
                                    ));
                                }
                            }
                        }
                    }
                }
            }

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

            // Check for Buffer static methods. Issue #831: aliased
            // imports of Buffer (`import { Buffer as RuntimeBuffer } from
            // "node:buffer"`) must route through the same dedicated
            // BufferFrom/BufferAlloc/etc HIR variants as the global
            // `Buffer`; otherwise the lowering falls through to a
            // receiver-less `NativeMethodCall { module: "buffer", method:
            // "from", object: None }` for which the codegen has no
            // dispatch table entry — it would silently return
            // TAG_UNDEFINED, and any caller that subsequently treats the
            // result as a Buffer (e.g. `b[0]` → Uint8ArrayGet) segfaults
            // on the undefined value.
            let is_buffer_ref = obj_name == "Buffer"
                || matches!(
                    ctx.lookup_native_module(&obj_name),
                    Some(("buffer", Some("Buffer")))
                );
            if is_buffer_ref {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "from" => {
                            let data = args.first().cloned().unwrap_or(Expr::Undefined);
                            // Disambiguate `Buffer.from(data, encoding?)` vs
                            // `Buffer.from(arrayBuffer, byteOffset?, length?)`.
                            // Encoding args are strings, byteOffset/length are
                            // numbers. Issue #1273: previously any non-string
                            // literal second arg routed to BufferFromArrayBuffer,
                            // so `Buffer.from(str, encVar)` produced an empty
                            // buffer. Now: 3+ args, or a Number-literal second
                            // arg, or a string-literal first arg with a Number
                            // second arg → ArrayBuffer form. Otherwise default
                            // to BufferFrom (the runtime helper dispatches on
                            // the actual type of `data`, and routes through
                            // `js_encoding_tag_from_value` for runtime-string
                            // encodings).
                            let is_arraybuffer_form = args.len() >= 3
                                || matches!(args.get(1), Some(Expr::Number(_) | Expr::Integer(_)));
                            if args.len() >= 2 && is_arraybuffer_form {
                                let byte_offset = args.get(1).cloned().unwrap_or(Expr::Number(0.0));
                                let length = args.get(2).cloned().map(Box::new);
                                return Ok(Ok(Expr::BufferFromArrayBuffer {
                                    data: Box::new(data),
                                    byte_offset: Box::new(byte_offset),
                                    length,
                                }));
                            }
                            let encoding = args.get(1).cloned().map(Box::new);
                            return Ok(Ok(Expr::BufferFrom {
                                data: Box::new(data),
                                encoding,
                            }));
                        }
                        "alloc" => {
                            // #2013: a missing `size` must surface Node's
                            // `ERR_INVALID_ARG_TYPE` (Received undefined), so
                            // default to `undefined` — not `0` — and let the
                            // runtime validator throw.
                            let size = args.first().cloned().unwrap_or(Expr::Undefined);
                            let fill = args.get(1).cloned().map(Box::new);
                            let encoding = args.get(2).cloned().map(Box::new);
                            return Ok(Ok(Expr::BufferAlloc {
                                size: Box::new(size),
                                fill,
                                encoding,
                            }));
                        }
                        "allocUnsafe" | "allocUnsafeSlow" => {
                            // #2013: missing `size` → Node ERR_INVALID_ARG_TYPE.
                            let size = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::BufferAllocUnsafe(Box::new(size))));
                        }
                        "concat" => {
                            let list = args.first().cloned().unwrap_or(Expr::Array(vec![]));
                            if let Some(total_length) = args.get(1).cloned() {
                                return Ok(Ok(Expr::BufferConcatWithLength {
                                    list: Box::new(list),
                                    total_length: Box::new(total_length),
                                }));
                            }
                            return Ok(Ok(Expr::BufferConcat(Box::new(list))));
                        }
                        "copyBytesFrom" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "buffer".to_string(),
                                class_name: None,
                                object: None,
                                method: "copyBytesFrom".to_string(),
                                args,
                            }));
                        }
                        "of" => {
                            return Ok(Ok(Expr::BufferFrom {
                                data: Box::new(Expr::Array(args)),
                                encoding: None,
                            }));
                        }
                        "isBuffer" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::BufferIsBuffer(Box::new(obj))));
                        }
                        "isEncoding" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::BufferIsEncoding(Box::new(obj))));
                        }
                        "byteLength" => {
                            let data = args
                                .first()
                                .cloned()
                                .unwrap_or(Expr::String("".to_string()));
                            let encoding = args.get(1).cloned().map(Box::new);
                            return Ok(Ok(Expr::BufferByteLength {
                                data: Box::new(data),
                                encoding,
                            }));
                        }
                        // `Buffer.compare(a, b)` → `a.compare(b)` instance call
                        // (handled by runtime buffer dispatch).
                        "compare" if args.len() >= 2 => {
                            let mut iter = args.into_iter();
                            let a = iter.next().unwrap();
                            let b = iter.next().unwrap();
                            return Ok(Ok(Expr::Call {
                                callee: Box::new(Expr::PropertyGet {
                                    byte_offset: 0,
                                    object: Box::new(a),
                                    property: "compare".to_string(),
                                }),
                                args: vec![b],
                                type_args: vec![],
                                byte_offset: 0,
                            }));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for Uint8Array static methods
            if obj_name == "Uint8Array" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "from" => {
                            let data = args.first().cloned().unwrap_or(Expr::Undefined);
                            // #2774: `Uint8Array.from(src, mapFn, thisArg?)` — run
                            // the mapped materialization first (which validates
                            // mapFn + binds thisArg), then truncate to Uint8.
                            if let Some(map_fn) = args.get(1).cloned() {
                                let this_arg = args.get(2).cloned().map(Box::new);
                                return Ok(Ok(Expr::Uint8ArrayFrom(Box::new(
                                    Expr::ArrayFromMapped {
                                        iterable: Box::new(data),
                                        map_fn: Box::new(map_fn),
                                        this_arg,
                                    },
                                ))));
                            }
                            return Ok(Ok(Expr::Uint8ArrayFrom(Box::new(data))));
                        }
                        // Issue #871 (part 2): `Uint8Array.of(a, b, c, ...)` —
                        // uuid's `sha1.js` calls this with 20 args (the SHA-1
                        // hash output, byte by byte), which hit the
                        // `Call callee shape not supported (PropertyGet) with N args`
                        // bail in `crates/perry-codegen/src/lower_call.rs::~3226`
                        // because the receiver `Uint8Array` lowers to `GlobalGet(0)`
                        // (which `lower_expr` evaluates to `0.0`) so the closure-call
                        // fallback at `~3167` skipped past it, and there's no
                        // `js_closure_call17..20` to dispatch ≥17-arg calls anyway.
                        //
                        // Per ECMAScript: `Uint8Array.of(...items)` is `Uint8Array.from([...items])`
                        // — same shape as the existing `from` arm above, just wrap the
                        // varargs in an array literal first. Routes through `Expr::Array`
                        // → `Expr::Uint8ArrayFrom` which already lowers correctly for any
                        // arity (it's just `js_uint8array_from_array`-or-equivalent on the
                        // packed array). Mirrors the `Array.of` arm at :1618.
                        "of" => {
                            return Ok(Ok(Expr::Uint8ArrayFrom(Box::new(Expr::Array(args)))));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for Object static methods
            if obj_name == "Object" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "keys" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectKeys(Box::new(obj))));
                        }
                        "values" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectValues(Box::new(obj))));
                        }
                        "entries" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectEntries(Box::new(obj))));
                        }
                        // Object.assign(target, ...sources) — per ECMAScript spec, this
                        // MUTATES target with each source's own enumerable string-keyed
                        // and Symbol-keyed properties, and RETURNS target (preserving
                        // identity, class_id, and the SYMBOL_PROPERTIES side-table).
                        // Refs #590: the previous lowering folded the call into
                        // ObjectSpread which allocates a fresh object — that breaks
                        // `result === target` and orphans target's symbol-keyed
                        // properties since the side table is keyed by raw pointer.
                        //
                        "assign" => {
                            let mut iter = args.into_iter();
                            let target = iter.next().unwrap_or(Expr::Undefined);
                            let sources: Vec<Expr> = iter.collect();
                            // Real `Object.assign(target, ...sources)` — mutate target.
                            return Ok(Ok(Expr::ObjectAssign {
                                target: Box::new(target),
                                sources,
                            }));
                        }
                        "fromEntries" => {
                            let entries = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectFromEntries(Box::new(entries))));
                        }
                        "groupBy"
                            // Object.groupBy(items, keyFn) — Node 22+ static method
                            if args.len() >= 2 => {
                                let mut iter = args.into_iter();
                                let items = iter.next().unwrap();
                                let key_fn = iter.next().unwrap();
                                let key_fn = ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                                return Ok(Ok(Expr::ObjectGroupBy {
                                    items: Box::new(items),
                                    key_fn: Box::new(key_fn),
                                }));
                            }
                        "is" => {
                            let mut iter = args.into_iter();
                            let a = iter.next().unwrap_or(Expr::Undefined);
                            let b = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectIs(Box::new(a), Box::new(b))));
                        }
                        "hasOwn" => {
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let key = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectHasOwn(Box::new(obj), Box::new(key))));
                        }
                        "freeze" => {
                            return Ok(Ok(Expr::ObjectFreeze(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "seal" => {
                            return Ok(Ok(Expr::ObjectSeal(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "preventExtensions" => {
                            return Ok(Ok(Expr::ObjectPreventExtensions(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "create" => {
                            let mut it = args.into_iter();
                            let proto = it.next().unwrap_or(Expr::Undefined);
                            let props = it.next().map(Box::new);
                            return Ok(Ok(Expr::ObjectCreate(Box::new(proto), props)));
                        }
                        "isFrozen" => {
                            return Ok(Ok(Expr::ObjectIsFrozen(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "isSealed" => {
                            return Ok(Ok(Expr::ObjectIsSealed(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "isExtensible" => {
                            return Ok(Ok(Expr::ObjectIsExtensible(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "getPrototypeOf" => {
                            return Ok(Ok(Expr::ObjectGetPrototypeOf(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "setPrototypeOf" => {
                            // `Object.setPrototypeOf(obj, proto)` is the foundation
                            // of chalk's "callable + getter-bag" shape (a closure has
                            // its `[[Prototype]]` reset to a Function-derived
                            // accessor-bag). Pre-fix this fell through to a generic
                            // `Object.setPrototypeOf` PropertyGet → Call where
                            // `Object.setPrototypeOf` resolves to undefined and the
                            // call throws `TypeError: value is not a function` —
                            // chalk's `import chalk from "chalk"` died at module init.
                            //
                            // Perry's runtime doesn't track mutable per-instance
                            // prototype chains (class IDs are baked at allocation),
                            // so we model setPrototypeOf as a no-op that still
                            // returns the target — matching the spec's "return obj"
                            // contract. The runtime helper registers (obj, proto)
                            // in a side-table that `Object.getPrototypeOf(obj)` is
                            // free to consult later if a downstream pattern needs it.
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let proto = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectSetPrototypeOf(
                                Box::new(obj),
                                Box::new(proto),
                            )));
                        }
                        "defineProperty" => {
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let key = iter.next().unwrap_or(Expr::Undefined);
                            let descriptor = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectDefineProperty(
                                Box::new(obj),
                                Box::new(key),
                                Box::new(descriptor),
                            )));
                        }
                        "defineProperties" => {
                            // `Object.defineProperties(target, descriptors)` — bulk
                            // form of `defineProperty`. Used by chalk's index.js to
                            // attach the `styles` getter-bag onto
                            // `createChalk.prototype`. Pre-fix this fell through to a
                            // generic `(Object).defineProperties(...)` call which
                            // throws `TypeError: value is not a function` at module
                            // init because `Object` isn't a runtime object with
                            // method dispatch.
                            //
                            // Desugar to a sequence of `ObjectDefineProperty`
                            // applications by reading `descriptors`'s own keys at
                            // compile time when it's an object literal, otherwise
                            // route through a runtime helper that iterates the
                            // descriptor object's keys.
                            let mut iter = args.into_iter();
                            let target = iter.next().unwrap_or(Expr::Undefined);
                            let descs = iter.next().unwrap_or(Expr::Undefined);
                            if let Expr::Object(props) = &descs {
                                // Static descriptor literal — desugar to a Sequence
                                // of `defineProperty(target, key, desc)` calls and
                                // yield `target` as the result value.
                                //
                                // An EMPTY literal must NOT fold to a bare `target`:
                                // `Object.defineProperties(O, {})` still performs the
                                // spec's step-1 `If Type(O) is not Object, throw a
                                // TypeError`, so `Object.defineProperties(undefined,
                                // {})` must throw. With no keys there is no per-key
                                // `defineProperty` to enforce that, so route the
                                // empty case through the runtime helper (which
                                // validates the target).
                                if !props.is_empty() {
                                    let target = target;
                                    let mut exprs: Vec<Expr> = Vec::with_capacity(props.len() + 1);
                                    for (key_name, desc_expr) in props {
                                        exprs.push(Expr::ObjectDefineProperty(
                                            Box::new(target.clone()),
                                            Box::new(Expr::String(key_name.clone())),
                                            Box::new(desc_expr.clone()),
                                        ));
                                    }
                                    exprs.push(target);
                                    return Ok(Ok(Expr::Sequence(exprs)));
                                }
                            }
                            return Ok(Ok(Expr::ObjectDefineProperties(
                                Box::new(target),
                                Box::new(descs),
                            )));
                        }
                        "getOwnPropertyDescriptor" => {
                            // #2144/#3655: built-in function `.name` /
                            // `.length` descriptors.
                            //
                            // `Object.getOwnPropertyDescriptor(<BuiltinCtor>,
                            // "name"|"length")` and
                            // `…(<BuiltinNs>.<staticFn>, "name"|"length")`
                            // need a compile-time fold because those builtin
                            // values are often intrinsic sentinels rather than
                            // first-class closures. Per spec both descriptors
                            // are non-writable, non-enumerable, configurable
                            // data properties. Fold when we can statically
                            // recognize the receiver shape — same gating logic
                            // as the direct `.name` / `.length` folds in
                            // `expr_member.rs`.
                            if call.args.len() >= 2 && args.len() >= 2 {
                                let key_name = match call.args[1].expr.as_ref() {
                                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                                    _ => None,
                                };
                                if matches!(key_name, Some("name" | "length")) {
                                    let lowered_obj_is_global_intrinsic = match &args[0] {
                                        Expr::GlobalGet(0) => true,
                                        Expr::PropertyGet { object: inner, .. } => {
                                            matches!(inner.as_ref(), Expr::GlobalGet(0))
                                        }
                                        _ => false,
                                    };
                                    if lowered_obj_is_global_intrinsic {
                                        match key_name {
                                            Some("name") => {
                                                let folded =
                                                    super::name_fold::builtin_fn_name_for_arg(
                                                        call.args[0].expr.as_ref(),
                                                    );
                                                if let Some(fname) = folded {
                                                    return Ok(Ok(
                                                        super::name_fold::name_data_descriptor(
                                                            fname,
                                                        ),
                                                    ));
                                                }
                                            }
                                            Some("length") => {
                                                let folded =
                                                    super::name_fold::builtin_fn_length_for_arg(
                                                        call.args[0].expr.as_ref(),
                                                    )
                                                    .or_else(|| {
                                                        super::name_fold::builtin_fn_name_for_arg(
                                                            call.args[0].expr.as_ref(),
                                                        )
                                                        .and_then(|name| {
                                                            crate::analysis::builtin_constructor_length(
                                                                &name,
                                                            )
                                                        })
                                                    });
                                                if let Some(len) = folded {
                                                    return Ok(Ok(
                                                        super::name_fold::builtin_data_descriptor(
                                                            Expr::Number(len as f64),
                                                        ),
                                                    ));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let key = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectGetOwnPropertyDescriptor(
                                Box::new(obj),
                                Box::new(key),
                            )));
                        }
                        "getOwnPropertyDescriptors" => {
                            return Ok(Ok(Expr::ObjectGetOwnPropertyDescriptors(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "getOwnPropertyNames" => {
                            return Ok(Ok(Expr::ObjectGetOwnPropertyNames(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "getOwnPropertySymbols" => {
                            return Ok(Ok(Expr::ObjectGetOwnPropertySymbols(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

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

            // Check for RegExp static methods: RegExp.escape (#2899)
            if obj_name == "RegExp" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if method_ident.sym.as_ref() == "escape" {
                        let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                        return Ok(Ok(Expr::RegExpEscape(Box::new(arg))));
                    }
                }
            }

            // Check for Map static methods: Map.groupBy
            if obj_name == "Map" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
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

            if obj_name == "Reflect" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "get" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            // #2766: optional `receiver` (3rd arg) used as the
                            // `this` binding for accessor getters. Default to
                            // `undefined` — the runtime substitutes `target`.
                            let receiver = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectGet {
                                target: Box::new(target),
                                key: Box::new(key),
                                receiver: Box::new(receiver),
                            }));
                        }
                        "set" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            let value = it.next().unwrap_or(Expr::Undefined);
                            // Optional `receiver` (4th arg): default `undefined`
                            // and the runtime substitutes `target`.
                            let receiver = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectSet {
                                target: Box::new(target),
                                key: Box::new(key),
                                value: Box::new(value),
                                receiver: Box::new(receiver),
                            }));
                        }
                        "has" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectHas {
                                target: Box::new(target),
                                key: Box::new(key),
                            }));
                        }
                        "deleteProperty" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectDelete {
                                target: Box::new(target),
                                key: Box::new(key),
                            }));
                        }
                        "ownKeys" => {
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectOwnKeys(Box::new(target))));
                        }
                        "apply" => {
                            let mut it = args.into_iter();
                            let func = it.next().unwrap_or(Expr::Undefined);
                            let this_arg = it.next().unwrap_or(Expr::Undefined);
                            // Spec sec-reflect.apply runs CreateListFromArrayLike
                            // on argumentsList, which throws a TypeError when
                            // Type(argumentsList) is not Object. An OMITTED
                            // argumentsList is `undefined` (not Object), so it
                            // must reach the runtime as `undefined` and throw —
                            // NOT default to an empty array, which would silently
                            // succeed with no args (test262
                            // Reflect/apply/arguments-list-is-not-array-like
                            // `Reflect.apply(fn, null /* empty */)`).
                            let args_arr = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectApply {
                                func: Box::new(func),
                                this_arg: Box::new(this_arg),
                                args: Box::new(args_arr),
                            }));
                        }
                        "construct" => {
                            // Special case: `Reflect.construct(ClassName, [args...])`
                            // where ClassName is a known class — fold to a direct
                            // `new ClassName(...args)` expression.
                            //
                            // #2768: only fold the two-argument form. With an
                            // explicit `newTarget` (3rd arg) the result's prototype
                            // comes from `newTarget` and `newTarget` must be
                            // validated as a constructor — a plain `new ClassName`
                            // would silently drop both. Fall through to
                            // `ReflectConstruct` (runtime `js_reflect_construct`,
                            // which runs `js_new_function_construct_with_new_target`
                            // and the non-constructor `newTarget` TypeError check).
                            if call.args.len() == 2 {
                                if let ast::Expr::Ident(cls_ident) = call.args[0].expr.as_ref() {
                                    let cls_name = cls_ident.sym.to_string();
                                    if ctx.lookup_class(&cls_name).is_some() {
                                        if let ast::Expr::Array(arr_lit) =
                                            call.args[1].expr.as_ref()
                                        {
                                            let new_args: Vec<Expr> = arr_lit
                                                .elems
                                                .iter()
                                                .filter_map(|e| e.as_ref())
                                                .map(|e| lower_expr(ctx, &e.expr))
                                                .collect::<Result<Vec<_>>>()?;
                                            return Ok(Ok(Expr::New {
                                                class_name: cls_name,
                                                args: new_args,
                                                type_args: vec![],
                                                byte_offset: 0,
                                                cap_args_appended: 0,
                                            }));
                                        }
                                    }
                                }
                            }
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                            // 3rd arg = newTarget; defaults to `undefined` so the
                            // runtime falls back to the target/proxy itself.
                            let new_target = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectConstruct {
                                target: Box::new(target),
                                args: Box::new(args_arr),
                                new_target: Box::new(new_target),
                            }));
                        }
                        "defineProperty" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            let descriptor = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectDefineProperty {
                                target: Box::new(target),
                                key: Box::new(key),
                                descriptor: Box::new(descriptor),
                            }));
                        }
                        "getPrototypeOf" => {
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectGetPrototypeOf(Box::new(target))));
                        }
                        "getOwnPropertyDescriptor" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectGetOwnPropertyDescriptor {
                                target: Box::new(target),
                                key: Box::new(key),
                            }));
                        }
                        "defineMetadata" => {
                            let (key, value, target, property_key) = take_reflect_kvtp_args(args);
                            return Ok(Ok(Expr::ReflectDefineMetadata {
                                key: Box::new(key),
                                value: Box::new(value),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectGetMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getOwnMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectGetOwnMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "hasMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectHasMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "hasOwnMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectHasOwnMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getMetadataKeys" => {
                            let (target, property_key) = take_reflect_tp_args(args);
                            return Ok(Ok(Expr::ReflectGetMetadataKeys {
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getOwnMetadataKeys" => {
                            let (target, property_key) = take_reflect_tp_args(args);
                            return Ok(Ok(Expr::ReflectGetOwnMetadataKeys {
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "deleteMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectDeleteMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "setPrototypeOf" => {
                            // #2761: Reflect-specific — boolean result (false on
                            // rejected change) + TypeError on bad args, distinct
                            // from Object.setPrototypeOf (returns the object).
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let proto = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectSetPrototypeOf {
                                target: Box::new(target),
                                proto: Box::new(proto),
                            }));
                        }
                        "isExtensible" => {
                            // #2762: Reflect-specific semantics (boolean +
                            // TypeError on non-object), NOT Object.isExtensible.
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectIsExtensible(Box::new(target))));
                        }
                        "preventExtensions" => {
                            // #2762: Reflect-specific semantics (boolean +
                            // TypeError on non-object), NOT Object.preventExtensions.
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectPreventExtensions(Box::new(target))));
                        }
                        _ => {}
                    }
                }
            }

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

            // Check for Array static methods
            if obj_name == "Array" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
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

            if let Some((module_name, imported_method)) = ctx.lookup_native_module(&obj_name) {
                if module_name == "url" && imported_method == Some("URL") {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        if method_name == "canParse" && !args.is_empty() {
                            let mut iter = args.into_iter();
                            let input = iter.next().unwrap();
                            if let Some(base) = iter.next() {
                                return Ok(Ok(Expr::UrlCanParseWithBase {
                                    input: Box::new(input),
                                    base: Box::new(base),
                                }));
                            }
                            return Ok(Ok(Expr::UrlCanParse(Box::new(input))));
                        }
                        if method_name == "parse" && !args.is_empty() {
                            let mut iter = args.into_iter();
                            let input = iter.next().unwrap();
                            if let Some(base) = iter.next() {
                                return Ok(Ok(Expr::UrlParseWithBase {
                                    input: Box::new(input),
                                    base: Box::new(base),
                                }));
                            }
                            return Ok(Ok(Expr::UrlParse(Box::new(input))));
                        }
                    }
                }

                if let Some(submodule) = path_submodule_name(module_name) {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.to_string();
                        return Ok(
                            match super::nested_namespace::dispatch_path_subnamespace(
                                submodule,
                                &method_name,
                                args,
                            ) {
                                Ok(expr) => Ok(expr),
                                Err(args) => Err(args),
                            },
                        );
                    }
                }

                // Skip modules handled specifically below (path, fs, child_process, etc.)
                // `net` used to be in this list back when its method calls
                // were short-circuited into `Expr::NetCreateConnection` etc.
                // After A1.5 `net` goes through the generic NativeMethodCall
                // path so the LLVM backend's NATIVE_MODULE_TABLE dispatches
                // to `js_net_socket_*` in perry-stdlib.
                let is_handled_module = module_name == "path"
                    || module_name == "node:path"
                    || module_name == "fs"
                    || module_name == "node:fs"
                    || module_name == "child_process"
                    || module_name == "node:child_process"
                    || module_name == "crypto"
                    || module_name == "node:crypto"
                    || module_name == "os"
                    || module_name == "node:os";
                if !is_handled_module {
                    // This is a call on a native module (e.g., mysql.createConnection)
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.to_string();
                        // A destructured/named DATA member of a native module
                        // (`const { METHODS } = require('node:http')` →
                        // `imported_method = Some("METHODS")`) holds a real
                        // Array/Object value. An Array/Object prototype method on it
                        // (`METHODS.map(...)`, `STATUS_CODES.hasOwnProperty(...)`) is
                        // a call on that VALUE — NOT a `module.method` native call.
                        // Bail to generic dynamic dispatch so it runs on the member's
                        // resolved value (express's `router` does
                        // `METHODS.map((m) => m.toLowerCase())`). Without this the
                        // call lowered to `NativeMethodCall { module: "http", method:
                        // "map" }`, which returned undefined / a deferred throw.
                        if imported_method.is_some()
                            && (super::super::array_fold::is_known_array_prototype_method(
                                &method_name,
                            ) || super::super::array_fold::is_known_object_prototype_method(
                                &method_name,
                            ))
                        {
                            return Ok(Err(args));
                        }
                        if module_name == "worker_threads" && method_name == "workerData" {
                            return Ok(Err(args));
                        }
                        if module_name.strip_prefix("node:").unwrap_or(module_name) == "vm"
                            && imported_method.is_none()
                            && method_name == "Module"
                        {
                            let mut exprs = args;
                            exprs.push(Expr::Call {
                                callee: Box::new(Expr::ExternFuncRef {
                                    name: "js_vm_module_call".to_string(),
                                    param_types: Vec::new(),
                                    return_type: Type::Any,
                                }),
                                args: Vec::new(),
                                type_args: Vec::new(),
                                byte_offset: 0,
                            });
                            return Ok(Ok(Expr::Sequence(exprs)));
                        }
                        let normalized_module =
                            module_name.strip_prefix("node:").unwrap_or(module_name);
                        if normalized_module == "cluster"
                            && matches!(imported_method, Some("default"))
                            && is_cluster_default_event_emitter_method(&method_name)
                        {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: method_name,
                                args,
                            }));
                        }
                        if method_name == "call" {
                            if normalized_module == "stream"
                                && matches!(imported_method, None | Some("Stream"))
                            {
                                return Ok(Ok(event_emitter_constructor_call(args)));
                            }
                            if normalized_module == "events"
                                && matches!(imported_method, Some("EventEmitter"))
                            {
                                return Ok(Ok(event_emitter_constructor_call(args)));
                            }
                            // #4973: named-import form of the inherits
                            // pattern — `const { Server } = require('http');
                            // Server.call(this, handler)`. Same extern as
                            // the dotted `http.Server.call(...)` form in
                            // module_class_static.rs.
                            if matches!(normalized_module, "http" | "https")
                                && matches!(imported_method, Some("Server"))
                                && !args.is_empty()
                            {
                                let mut it = args.into_iter();
                                let this_arg = it.next().unwrap();
                                let mut rest: Vec<Expr> = it.collect();
                                rest.resize(2, Expr::Undefined);
                                let mut call_args = vec![this_arg];
                                call_args.extend(rest);
                                let extern_name = if normalized_module == "https" {
                                    "js_https_server_construct_with_this"
                                } else {
                                    "js_http_server_construct_with_this"
                                };
                                return Ok(Ok(Expr::Call {
                                    callee: Box::new(Expr::ExternFuncRef {
                                        name: extern_name.to_string(),
                                        param_types: Vec::new(),
                                        return_type: Type::Any,
                                    }),
                                    args: call_args,
                                    type_args: Vec::new(),
                                    byte_offset: 0,
                                }));
                            }
                        }
                        // Unimplemented-API gate (#463 / #525) for the 2-deep
                        // `mod.method()` call form. Without this, perry/* and
                        // other native-module call sites short-circuited past
                        // the `lower_member` gate that fires for the property-
                        // read form, then bailed in codegen with a per-module
                        // message (`'X' is not a known function`) — different
                        // wording, different escape hatch, harder for users to
                        // recognize as the same class of mistake. Mirrors the
                        // 3-deep gate above for `mod.X.Y()`.
                        let manifest_entry =
                            perry_api_manifest::module_has_symbol(module_name, &method_name);
                        if perry_api_manifest::module_has_any_entries(module_name)
                            && manifest_entry.is_none()
                            // #wall4: an unmistakable `String.prototype` method
                            // (`endsWith`, `slice`, …) called on an identifier that
                            // shares a node-core module name (`url`, `path`) means
                            // the receiver is a runtime string, NOT the module —
                            // don't gate it as an unimplemented module API; fall
                            // through to dynamic dispatch on the real receiver.
                            // Next.js app-page-turbo calls `url.endsWith(...)` on a
                            // URL string bound to a local named `url`.
                            && !super::super::array_fold::is_known_string_prototype_method(
                                &method_name,
                            )
                            // A destructured/named DATA member of a native module
                            // (`const { METHODS } = require('node:http')`,
                            // registered with `imported_method = Some("METHODS")`)
                            // holds a real Array/Object value, not a callable
                            // namespace. An Array/Object prototype method on it
                            // (`METHODS.map(...)`, `STATUS_CODES.hasOwnProperty(...)`)
                            // is a call on that VALUE — gating it as
                            // `http.map`/`http.hasOwnProperty` (#463) compiled it to
                            // a deferred "not implemented" throw, breaking express's
                            // `router` (`METHODS.map((m) => m.toLowerCase())`). Fall
                            // through to dynamic dispatch on the real member value.
                            && !(imported_method.is_some()
                                && (super::super::array_fold::is_known_array_prototype_method(
                                    &method_name,
                                ) || super::super::array_fold::is_known_object_prototype_method(
                                    &method_name,
                                )))
                        {
                            // #925: this is the gate that fires
                            // for `crypto.hmacSha256(data, key)`.
                            let hint = super::super::unimpl_hints::module_member_hint(
                                module_name,
                                &method_name,
                            )
                            .map(|h| format!(" {h}"))
                            .unwrap_or_default();
                            let msg = format!(
                                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                module_name, method_name, hint,
                            );
                            // #5245: default → throw-on-reach + notice; strict →
                            // hard #463 refusal. #2309 tree-shake handled inside.
                            let api = format!("{module_name}.{method_name}");
                            let location = crate::eval_classifier::location_string(
                                &ctx.source_file_path,
                                member.span.lo.0,
                            );
                            match crate::check_unimplemented_api(
                                &msg,
                                &api,
                                &location,
                                member.span.lo.0,
                            ) {
                                crate::UnimplementedDecision::Refuse => {
                                    crate::lower_bail!(member.span, "{}", msg);
                                }
                                crate::UnimplementedDecision::DeferToRuntimeError(runtime_msg) => {
                                    return Ok(Ok(
                                        super::super::const_fold_fn::synth_deferred_throw_value(
                                            ctx,
                                            &runtime_msg,
                                            member.span,
                                        )?,
                                    ));
                                }
                            }
                        }
                        if let Some(entry) = manifest_entry {
                            if !matches!(
                                entry.kind,
                                perry_api_manifest::ApiKind::Method {
                                    has_receiver: false,
                                    class_filter: None
                                }
                            ) {
                                return Ok(Err(args));
                            }
                        }
                        let class_name = if module_name == "stream"
                            && imported_method.is_some_and(is_node_stream_class_name)
                        {
                            imported_method.map(str::to_string)
                        } else {
                            None
                        };
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: module_name.to_string(),
                            class_name,
                            object: None, // Static call on module itself
                            method: method_name,
                            args,
                        }));
                    }
                }
            }
        }
    }

    Ok(Err(args))
}

#[cfg(test)]
mod bundled_mysql2_tests {
    use super::mysql2_config_signature;

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
