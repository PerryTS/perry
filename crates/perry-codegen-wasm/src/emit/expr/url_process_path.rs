//! URL accessors, Process/OS, env, FS stubs, Path, WeakRef, FinalizationRegistry.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_url_process_path(
        &mut self,
        func: &mut Function,
        expr: &Expr,
    ) -> bool {
        match expr {
            Expr::UrlNew { url, base } => {
                self.emit_expr(func, url);
                if let Some(b) = base {
                    // URL(url, base) — for now just use url
                    self.emit_expr(func, b);
                    func.instruction(&Instruction::Drop);
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "url_parse", 1);
            }
            Expr::UrlGetHref(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_href", 1);
            }
            Expr::UrlGetPathname(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_pathname", 1);
            }
            Expr::UrlGetProtocol(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_protocol", 1);
            }
            Expr::UrlGetHost(u) | Expr::UrlGetHostname(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_hostname", 1);
            }
            Expr::UrlGetPort(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_port", 1);
            }
            Expr::UrlGetSearch(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_search", 1);
            }
            Expr::UrlGetHash(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_hash", 1);
            }
            Expr::UrlGetOrigin(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_origin", 1);
            }
            Expr::UrlGetSearchParams(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_search_params", 1);
            }

            // --- Process/OS ---
            Expr::ProcessArgv => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "process_argv", 0);
            }
            Expr::ProcessCwd => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "process_cwd", 0);
            }
            Expr::OsPlatform => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "os_platform", 0);
            }
            Expr::ProcessUptime
            | Expr::ProcessMemoryUsage
            | Expr::ProcessThreadCpuUsage(_)
            | Expr::ProcessAvailableMemory
            | Expr::ProcessConstrainedMemory
            | Expr::ProcessPosixCredential(_)
            | Expr::ProcessResourceUsage
            | Expr::ProcessActiveResourcesInfo
            | Expr::ProcessPid
            | Expr::ProcessPpid
            | Expr::ProcessVersion
            | Expr::ProcessVersions
            | Expr::ProcessHrtimeBigint
            | Expr::ProcessHrtime(_)
            | Expr::ProcessTitle
            | Expr::ProcessStdin
            | Expr::ProcessStdout
            | Expr::ProcessStderr
            | Expr::OsArch
            | Expr::OsHostname
            | Expr::OsHomedir
            | Expr::OsTmpdir
            | Expr::OsTotalmem
            | Expr::OsFreemem
            | Expr::OsUptime
            | Expr::OsType
            | Expr::OsRelease
            | Expr::OsCpus
            | Expr::OsNetworkInterfaces
            | Expr::OsUserInfo
            | Expr::OsEOL => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::ProcessNextTick { .. }
            | Expr::ProcessChdir(_)
            | Expr::ProcessOn { .. }
            | Expr::ProcessStdinSetRawMode(_)
            | Expr::ProcessStdinOn { .. }
            | Expr::ProcessStdinRemoveListener { .. }
            | Expr::ProcessStdinLifecycle(_)
            | Expr::ProcessKill { .. }
            | Expr::ProcessExit(_)
            | Expr::ProcessAbort
            | Expr::ProcessUmask(_)
            | Expr::ProcessEmitWarning(_)
            | Expr::ProcessCpuUsage(_)
            | Expr::ProcessSetTitle(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::EnvGet(_) | Expr::EnvGetDynamic(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- FS stubs ---
            Expr::FsReadFileSync(_)
            | Expr::FsWriteFileSync(_, _)
            | Expr::FsExistsSync(_)
            | Expr::FsMkdirSync(_)
            | Expr::FsUnlinkSync(_)
            | Expr::FsAppendFileSync(_, _)
            | Expr::FsReadFileBinary(_)
            | Expr::FsRmRecursive(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Path ---
            Expr::PathJoin(..) | Expr::PathWin32Join(..) | Expr::PathResolve(_)
                if path_operands(expr).is_some() =>
            {
                let (name, operands) = path_operands(expr).unwrap();
                let argc = operands.len() as u32;
                self.emit_frame_begin(func, argc);
                for (i, operand) in operands.into_iter().enumerate() {
                    self.emit_store_arg(func, i as u32, operand);
                }
                self.emit_memcall(func, name, argc);
            }
            Expr::PathWin32 {
                method: perry_hir::PathWin32Method::Resolve,
                ..
            } if path_operands(expr).is_some() => {
                let (name, operands) = path_operands(expr).unwrap();
                let argc = operands.len() as u32;
                self.emit_frame_begin(func, argc);
                for (i, operand) in operands.into_iter().enumerate() {
                    self.emit_store_arg(func, i as u32, operand);
                }
                self.emit_memcall(func, name, argc);
            }
            Expr::PathWin32 { method, args } => {
                let (name, expected_args) = path_win32_bridge(*method);
                self.emit_frame_begin(func, expected_args);
                for (i, a) in args.iter().enumerate().take(expected_args as usize) {
                    self.emit_store_arg(func, i as u32, a);
                }
                self.emit_memcall(func, name, expected_args);
            }
            Expr::PathDirname(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_dirname", 1);
            }
            Expr::PathBasename(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_basename", 1);
            }
            Expr::PathExtname(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_extname", 1);
            }
            Expr::PathIsAbsolute(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall_i32(func, "path_is_absolute", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::FileURLToPath(p) => {
                self.emit_expr(func, p);
                // In WASM, just return the string as-is
            }
            Expr::PathRelative(from, to) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, from);
                self.emit_store_arg(func, 1, to);
                self.emit_memcall(func, "path_relative", 2);
            }
            Expr::PathNormalize(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_normalize", 1);
            }
            Expr::PathParse(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_parse", 1);
            }
            Expr::PathFormat(o) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, o);
                self.emit_memcall(func, "path_format", 1);
            }
            Expr::PathBasenameExt(p, ext) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, p);
                self.emit_store_arg(func, 1, ext);
                self.emit_memcall(func, "path_basename", 2);
            }
            Expr::PathSep => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "path_sep", 0);
            }
            Expr::PathDelimiter => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "path_delimiter", 0);
            }
            Expr::PathToNamespacedPath(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_to_namespaced_path", 1);
            }
            Expr::PathMatchesGlob(p, pat) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, p);
                self.emit_store_arg(func, 1, pat);
                self.emit_memcall(func, "path_matches_glob", 2);
            }
            Expr::PathResolveJoin(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_resolve_join", 2);
            }
            // --- WeakRef and FinalizationRegistry (stub: routes to host runtime) ---
            Expr::WeakRefNew(target) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, target);
                self.emit_memcall(func, "weakref_new", 1);
            }
            Expr::WeakRefDeref(weakref_expr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, weakref_expr);
                self.emit_memcall(func, "weakref_deref", 1);
            }
            Expr::FinalizationRegistryNew(callback) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, callback);
                self.emit_memcall(func, "finreg_new", 1);
            }
            Expr::FinalizationRegistryRegister {
                registry,
                target,
                held,
                token,
            } => {
                self.emit_frame_begin(func, 4);
                self.emit_store_arg(func, 0, registry);
                self.emit_store_arg(func, 1, target);
                self.emit_store_arg(func, 2, held);
                if let Some(t) = token {
                    self.emit_store_arg(func, 3, t);
                } else {
                    self.emit_slot_addr(func, 3);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                self.emit_memcall(func, "finreg_register", 4);
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, registry);
                self.emit_store_arg(func, 1, token);
                self.emit_memcall(func, "finreg_unregister", 2);
            }
            _ => return false,
        }
        true
    }
}

/// The bridge and operands of a `join` or `resolve` call. HIR folds `join(a, b, c)` into
/// `PathJoin(PathJoin(a, b), c)` and `resolve(a, b, c)` into
/// `PathResolve(PathResolveJoin(PathResolveJoin(a, b), c))`; the web target calls Node's
/// n-ary function once with every operand, in source order, because joining or resolving
/// pairwise normalizes (or resolves) the intermediate result: `join("", "", "/b")` is
/// `"/b"`, and `win32.resolve("C:/base", "D:tmp", "C:leaf")` is `C:\base\leaf`. An explicit
/// `join(join(a, b), c)` has the same HIR, so it is joined in one call too.
pub(in crate::emit) fn path_operands(expr: &Expr) -> Option<(&'static str, Vec<&Expr>)> {
    use perry_hir::PathWin32Method as M;
    fn chain<'e>(
        mut expr: &'e Expr,
        split: impl Fn(&'e Expr) -> Option<(&'e Expr, &'e Expr)>,
    ) -> Vec<&'e Expr> {
        let mut operands = Vec::new();
        while let Some((left, right)) = split(expr) {
            operands.push(right);
            expr = left;
        }
        operands.push(expr);
        operands.reverse();
        operands
    }
    match expr {
        Expr::PathJoin(..) => Some((
            "path_join",
            chain(expr, |e| match e {
                Expr::PathJoin(a, b) => Some((a, b)),
                _ => None,
            }),
        )),
        Expr::PathWin32Join(..) => Some((
            "path_win32_join",
            chain(expr, |e| match e {
                Expr::PathWin32Join(a, b) => Some((a, b)),
                _ => None,
            }),
        )),
        Expr::PathResolve(p) => Some((
            "path_resolve",
            chain(p, |e| match e {
                Expr::PathResolveJoin(a, b) => Some((a, b)),
                _ => None,
            }),
        )),
        Expr::PathWin32 {
            method: M::Resolve,
            args,
        } if args.len() == 1 => Some((
            "path_win32_resolve",
            chain(&args[0], |e| match e {
                Expr::PathWin32 {
                    method: M::ResolveJoin,
                    args,
                } if args.len() == 2 => Some((&args[0], &args[1])),
                _ => None,
            }),
        )),
        _ => None,
    }
}

/// The `__memDispatch` bridge that answers a `node:path` module (or its `posix` / `win32`
/// submodule) used as a value, through a namespace or a default import (`ns.default`);
/// `None` for every other expression.
pub(in crate::emit) fn path_module_bridge(expr: &Expr) -> Option<&'static str> {
    let module = match expr {
        Expr::NativeModuleRef(module) => module,
        Expr::PropertyGet {
            object, property, ..
        } if property == "default" => match object.as_ref() {
            Expr::NativeModuleRef(module) => module,
            _ => return None,
        },
        _ => return None,
    };
    match module.strip_prefix("node:").unwrap_or(module) {
        "path" | "path/posix" => Some("path_module"),
        "path/win32" => Some("path_win32_module"),
        _ => None,
    }
}

/// The operands of a named `node:path` export read as a value (`import { join }` used as a
/// value, or an export of the driver's namespace module): HIR's call to
/// `js_native_module_named_esm_export_value(module, name)`. The web target answers it with
/// the `path_module_export` bridge.
pub(in crate::emit) fn path_export_operands(expr: &Expr) -> Option<[&Expr; 2]> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
        return None;
    };
    match args.as_slice() {
        [module @ Expr::String(m), property]
            if name == "js_native_module_named_esm_export_value"
                && path_module_bridge(&Expr::NativeModuleRef(m.clone())).is_some() =>
        {
            Some([module, property])
        }
        _ => None,
    }
}

/// The `__memDispatch` bridge that answers the ESM namespace of `node:path` (or a
/// submodule) when `module_name` is the driver's `native-esm-<hex>.ts` module for it,
/// `<hex>` spelling the builtin's name byte by byte.
pub(in crate::emit) fn path_namespace_bridge(module_name: &str) -> Option<&'static str> {
    let stem = module_name
        .rsplit('/')
        .next()?
        .strip_prefix("native-esm-")?;
    let hex = stem.strip_suffix(".ts")?;
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    match std::str::from_utf8(&bytes).ok()? {
        "path" | "path/posix" => Some("path_namespace"),
        "path/win32" => Some("path_win32_namespace"),
        _ => None,
    }
}

/// The `__memDispatch` bridge a `path.win32` method calls, and how many arguments it takes.
pub(in crate::emit) fn path_win32_bridge(
    method: perry_hir::PathWin32Method,
) -> (&'static str, u32) {
    use perry_hir::PathWin32Method;
    match method {
        PathWin32Method::Dirname => ("path_win32_dirname", 1),
        PathWin32Method::Basename => ("path_win32_basename", 1),
        PathWin32Method::BasenameExt => ("path_win32_basename_ext", 2),
        PathWin32Method::Extname => ("path_win32_extname", 1),
        PathWin32Method::IsAbsolute => ("path_win32_is_absolute", 1),
        PathWin32Method::Normalize => ("path_win32_normalize", 1),
        PathWin32Method::Parse => ("path_win32_parse", 1),
        PathWin32Method::Format => ("path_win32_format", 1),
        PathWin32Method::Relative => ("path_win32_relative", 2),
        PathWin32Method::Resolve => ("path_win32_resolve", 1),
        PathWin32Method::ResolveJoin => ("path_win32_resolve_join", 2),
        PathWin32Method::ToNamespacedPath => ("path_win32_to_namespaced_path", 1),
        PathWin32Method::MatchesGlob => ("path_win32_matches_glob", 2),
    }
}
