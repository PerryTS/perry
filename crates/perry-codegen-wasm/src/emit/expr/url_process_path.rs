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
            | Expr::ProcessPid
            | Expr::ProcessPpid
            | Expr::ProcessVersion
            | Expr::ProcessVersions
            | Expr::ProcessHrtimeBigint
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
            Expr::ProcessNextTick(_)
            | Expr::ProcessChdir(_)
            | Expr::ProcessOn { .. }
            | Expr::ProcessKill { .. }
            | Expr::ProcessExit(_) => {
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
            Expr::PathJoin(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_join", 2);
            }
            Expr::PathWin32Join(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_win32_join", 2);
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
            Expr::PathResolve(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_resolve", 1);
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
                self.emit_memcall(func, "path_sep", 0);
            }
            Expr::PathDelimiter => {
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
