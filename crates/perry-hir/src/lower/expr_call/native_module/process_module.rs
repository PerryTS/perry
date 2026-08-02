//! `process` / `node:process` module-object method calls.
//!
//! Extracted from `expr_call/native_module.rs` as a mechanical move; the
//! block runs in the same position it occupied inline, and `Ok(Err(args))`
//! means "not a process method — keep checking the later receivers".

use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

use super::super::super::unimpl_hints;

pub(super) fn try_process_module_methods(
    ctx: &mut LoweringContext,
    member: &ast::MemberExpr,
    obj_name: &str,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for process module methods. `import processModule from
    // "node:process"` registers as the native `process` object, while
    // `import * as processNamespace` registers as `process.namespace`;
    // both must use the same strict method gate as the global object.
    let process_name_is_shadowed =
        obj_name == "process" && ctx.shadows_unqualified_global("process");
    let is_process_ref = !process_name_is_shadowed
        && (obj_name == "process"
            || ctx.lookup_builtin_module_alias(obj_name) == Some("process")
            || matches!(
                ctx.lookup_native_module(obj_name),
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
                method_name if is_process_active_array_helper(method_name) => {
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
                "setUncaughtExceptionCaptureCallback" | "addUncaughtExceptionCaptureCallback" => {
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
                    match crate::check_unimplemented_api(&msg, &api, &location, member.span.lo.0) {
                        crate::UnimplementedDecision::Refuse => {
                            crate::lower_bail!(member.span, "{}", msg);
                        }
                        crate::UnimplementedDecision::DeferToRuntimeError(runtime_msg) => {
                            return Ok(Ok(
                                super::super::super::const_fold_fn::synth_deferred_throw_value(
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

    Ok(Err(args))
}
