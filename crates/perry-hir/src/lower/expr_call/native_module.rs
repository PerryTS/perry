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
pub(super) fn unwrap_ts_wrappers(e: &ast::Expr) -> &ast::Expr {
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

fn is_node_core(module: &str) -> bool {
    crate::ir::is_node_builtin_module(module.strip_prefix("node:").unwrap_or(module))
}

/// Sub-namespaces of a node-core module that the runtime by-name dispatcher has
/// a bucket for — the DOTTED tags in `nm_module_index` (perry-runtime), which
/// perry-codegen mirrors in `nm_install_symbol`. This is the third mirror and is
/// deliberately kept to the handful that matter.
///
/// The list is an allowlist, not a derivation from `NODE_BUILTIN_MODULES`:
/// `fs/promises` and `dns/promises` are real node-core module names, but there
/// is no `fs.promises` / `dns.promises` dispatch bucket, so treating the
/// property as a namespace and routing `promises.readFile(...args)` to the
/// dynamic path yields `TypeError: value is not a function` — worse than the
/// positional fold it replaced. Measured, not assumed (#7720 follow-up).
pub(super) fn sub_namespace_has_dispatch_bucket(module: &str, sub: &str) -> bool {
    matches!(
        (module.strip_prefix("node:").unwrap_or(module), sub),
        ("path", "posix" | "win32")
            | ("util", "types")
            | ("crypto", "subtle" | "webcrypto")
            | ("punycode", "ucs2")
    )
}

/// Is the named export `export` of node-core `module` itself a NAMESPACE
/// (`import { posix } from "node:path"`) rather than a class or function value?
///
/// The distinction decides whether `<export>.<method>(...)` is a module call.
/// `Buffer.concat(...)` / `URL.parse(...)` are class statics reached through a
/// different lowering family, and their by-name runtime dispatch does not cover
/// the same surface, so they stay on their existing path.
fn is_submodule_export(module: &str, export: &str) -> bool {
    export == "default" || sub_namespace_has_dispatch_bucket(module, export)
}

/// Does `name` denote a node-core module NAMESPACE in this scope — a
/// namespace/default import (`import path from "node:path"`), a sub-namespace
/// export of one (`import { posix } from "node:path"`), or a `require()` alias?
///
/// Deliberately node-core ONLY: an ext/npm native module (`mongodb`, `redis`,
/// `undici`) is served by codegen-wired `NativeMethodCall` rows with no
/// by-name runtime dispatcher behind them, so declining its fast path would
/// trade a wrong answer for no answer. Every name this returns `true` for
/// resolves through `dispatch_native_module_method` in the runtime.
fn name_is_node_builtin_namespace(ctx: &LoweringContext, name: &str) -> bool {
    if let Some((module, export)) = ctx.lookup_native_module(name) {
        if is_node_core(module)
            && is_top_level_module(module)
            && export.is_none_or(|e| is_submodule_export(module, e))
        {
            return true;
        }
    }
    ctx.lookup_builtin_module_alias(name)
        .is_some_and(|m| is_node_core(m) && is_top_level_module(m))
}

/// Reject the slash sub-module tags (`fs/promises`, `dns/promises`,
/// `assert/strict`).
///
/// A NAMED import of one (`import { promises } from "node:fs"`) registers under
/// the slash tag, but its local does not read back as a dispatchable namespace
/// value — diverting `promises.readFile(...args)` turned a rejected promise into
/// a synchronous `TypeError: value is not a function`, which is worse than the
/// wrong error code it replaced. The DIRECT import
/// (`import fsp from "node:fs/promises"`) needs no help from the bail: it
/// already reaches the generic tail on its own (measured — identical HIR and
/// `ENOENT` output on both arms), so excluding the slash tags costs nothing.
fn is_top_level_module(module: &str) -> bool {
    !module.strip_prefix("node:").unwrap_or(module).contains('/')
}

/// Is `recv` (a call's RECEIVER) a node-core module namespace, or a
/// sub-namespace of one (`path.posix`, `crypto.subtle`, `util.types`,
/// `fs.promises`)?
fn receiver_is_node_builtin_module(ctx: &LoweringContext, recv: &ast::Expr) -> bool {
    match unwrap_ts_wrappers(recv) {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            name_is_node_builtin_namespace(ctx, name)
                // #1750: `const w = path.win32; w.join(...)`.
                || ctx
                    .lookup_subns_path_alias(name)
                    .is_some_and(|(root, _)| name_is_node_builtin_namespace(ctx, root))
        }
        // 3-level sub-namespace (`path.posix.join`, `util.types.isDate`): the
        // ROOT must be a module namespace AND the property must be a
        // bucket-backed sub-namespace. Recursing on the root alone claimed
        // `fs.promises.readFile(...)` / `dns.promises.lookup(...)` too, which
        // have no bucket.
        ast::Expr::Member(inner) => {
            let ast::Expr::Ident(root) = unwrap_ts_wrappers(inner.obj.as_ref()) else {
                return false;
            };
            let Some(sub) = super::static_call_prop_name(&inner.prop) else {
                return false;
            };
            let Some((module, export)) = ctx.lookup_native_module(root.sym.as_ref()) else {
                return false;
            };
            is_node_core(module)
                && matches!(export, None | Some("default"))
                && sub_namespace_has_dispatch_bucket(module, sub)
        }
        // `require("node:path").join(...)` — the inline-require shape.
        other => require_literal_native_module(ctx, other)
            .is_some_and(|m| crate::ir::is_node_builtin_module(&m)),
    }
}

/// #7720: is this call a node-core native-module call — `ns.method(...)`,
/// `ns.sub.method(...)`, or a named import `method(...)`?
///
/// Every native fast path below consumes its arguments POSITIONALLY, so a
/// spread operand (`path.join(...parts)`) is folded in as one argument holding
/// the whole array: `path.join` saw a single non-string and threw
/// `ERR_INVALID_ARG_TYPE`, `util.format` inspected the array instead of
/// formatting it, `fs.existsSync` tested an array for existence. `lower_call`
/// uses this to decline the entire fast-path chain for a spread call, leaving
/// the generic tail to build an `Expr::CallSpread` over the namespace member —
/// the same lowering the value-read form (`const j = path.join; j(...parts)`)
/// already takes, which materializes the args array and dispatches through
/// `js_native_call_method` → `dispatch_native_module_method`. That dispatcher
/// is variadic by construction, so it gets both the valid case and Node's
/// `ERR_INVALID_ARG_TYPE` for an invalid one right.
///
/// Generalizes the per-module bails #6668 added for `crypto` (those stay: they
/// also cover the bare `crypto` GLOBAL receiver, which is not an import and so
/// is invisible here). Native CLASS statics (`Buffer.concat(...list)`,
/// `URL.parse(...)`) are deliberately NOT included — see `is_submodule_export`.
pub(super) fn is_node_builtin_module_call(ctx: &LoweringContext, callee: &ast::Expr) -> bool {
    match unwrap_ts_wrappers(callee) {
        // `ns.method(...)` / `ns.sub.method(...)`.
        ast::Expr::Member(member) => receiver_is_node_builtin_module(ctx, member.obj.as_ref()),
        // A named export of a node-core module called directly:
        // `import { join } from "node:path"; join(...parts)`.
        ast::Expr::Ident(ident) => ctx
            .lookup_native_module(ident.sym.as_ref())
            .is_some_and(|(module, export)| is_node_core(module) && export.is_some()),
        _ => false,
    }
}

/// A module that can call `syncBuiltinESMExports()` must invoke named Node
/// imports through their ESM export cells. The ordinary native fast path calls
/// the built-in implementation directly and would therefore ignore a CommonJS
/// replacement copied into the cell by the sync operation.
pub(super) fn named_import_call_needs_esm_binding(
    ctx: &LoweringContext,
    callee: &ast::Expr,
) -> bool {
    let ast::Expr::Ident(ident) = unwrap_ts_wrappers(callee) else {
        return false;
    };
    let Some((module, Some(export))) = ctx.lookup_native_module(ident.sym.as_ref()) else {
        return false;
    };
    if !is_node_core(module)
        || export == "default"
        || (module.strip_prefix("node:").unwrap_or(module) == "module"
            && export == "syncBuiltinESMExports")
    {
        return false;
    }
    ctx.native_modules.iter().any(|(_, module, method)| {
        module.strip_prefix("node:").unwrap_or(module) == "module"
            && method.as_deref() == Some("syncBuiltinESMExports")
    })
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
mod native_module_helper_tests {
    use super::is_process_active_array_helper;

    #[test]
    fn process_active_array_helper_predicate_matches_supported_methods() {
        assert!(is_process_active_array_helper("_getActiveHandles"));
        assert!(is_process_active_array_helper("_getActiveRequests"));
        assert!(!is_process_active_array_helper("getActiveResourcesInfo"));
    }
}
