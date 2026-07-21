//! ChildProcess execSync/spawnSync/spawn/exec/etc.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    emit_string_literal_global, lower_expr, nanbox_pointer_inline, nanbox_string_inline,
    unbox_to_i64, FnCtx,
};

/// #3079: emit a setup-time `command`/`file` validation call. `cmd_box` is the
/// original NaN-boxed value; `name` is the static argument name (`"command"`
/// for exec/execSync, `"file"` for execFile/execFileSync/spawn/spawnSync). The
/// runtime throws `TypeError [ERR_INVALID_ARG_TYPE]` on a non-string value, so
/// this is emitted before the value is unboxed to a raw pointer.
fn emit_cp_validate_command(ctx: &mut FnCtx<'_>, cmd_box: &str, name: &str) {
    let name_label = emit_string_literal_global(ctx, name);
    let name_len = name.len();
    let blk = ctx.block();
    let _ = blk.call(
        DOUBLE,
        "js_child_process_validate_command",
        &[
            (DOUBLE, cmd_box),
            (PTR, &name_label),
            (I32, &name_len.to_string()),
        ],
    );
}

/// #3079: emit a setup-time `args` validation call. `args_box` is the original
/// NaN-boxed value passed in the args slot. The runtime throws `TypeError
/// [ERR_INVALID_ARG_TYPE]` for a primitive (string/number/boolean/…), accepting
/// `undefined`/`null`/objects. Emitted before the value is unboxed.
fn emit_cp_validate_args(ctx: &mut FnCtx<'_>, args_box: &str) {
    let blk = ctx.block();
    let _ = blk.call(
        DOUBLE,
        "js_child_process_validate_args",
        &[(DOUBLE, args_box)],
    );
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ChildProcessExecSync { command, options } => {
            let cmd_box = lower_expr(ctx, command)?;
            // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string command.
            emit_cp_validate_command(ctx, &cmd_box, "command");
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
            let opts_str = if let Some(opts) = options {
                let o = lower_expr(ctx, opts)?;
                unbox_to_i64(ctx.block(), &o)
            } else {
                "0".to_string()
            };
            // js_child_process_exec_sync(cmd: i64, opts: i64) -> f64.
            // #1937/#1938: the runtime returns an already-NaN-boxed value
            // (Buffer by default, string with `encoding`) and throws on a
            // non-zero exit, so we pass the result straight through.
            let result = ctx.block().call(
                DOUBLE,
                "js_child_process_exec_sync",
                &[(I64, &cmd_str), (I64, &opts_str)],
            );
            Ok(result)
        }

        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        } => {
            let cmd_box = lower_expr(ctx, command)?;
            // #3079: spawnSync's command argument is reported as "file".
            emit_cp_validate_command(ctx, &cmd_box, "file");
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
            let args_str = if let Some(a) = args {
                let v = lower_expr(ctx, a)?;
                emit_cp_validate_args(ctx, &v);
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            let opts_str = if let Some(o) = options {
                let v = lower_expr(ctx, o)?;
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            // js_child_process_spawn_sync(cmd: i64, args: i64, opts: i64) -> i64
            let result = ctx.block().call(
                I64,
                "js_child_process_spawn_sync",
                &[(I64, &cmd_str), (I64, &args_str), (I64, &opts_str)],
            );
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }

        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            let cmd_box = lower_expr(ctx, command)?;
            let _args_box = if let Some(a) = args {
                lower_expr(ctx, a)?
            } else {
                double_literal(0.0)
            };
            let log_box = lower_expr(ctx, log_file)?;
            let blk = ctx.block();
            let log_str = unbox_to_i64(blk, &log_box);
            let log_nanbox = nanbox_string_inline(ctx.block(), &log_str);
            let env_box = if let Some(e) = env_json {
                lower_expr(ctx, e)?
            } else {
                double_literal(0.0)
            };
            // js_child_process_spawn_background(cmd: f64, args_arr: i64, logFile: f64, envJson: f64) -> i64
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
            let result = ctx.block().call(
                I64,
                "js_child_process_spawn_background",
                &[
                    (DOUBLE, &cmd_box),
                    (I64, &cmd_str),
                    (DOUBLE, &log_nanbox),
                    (DOUBLE, &env_box),
                ],
            );
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }

        Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => {
            let cmd_box = lower_expr(ctx, command)?;
            // #3079: spawn's command argument is reported as "file".
            emit_cp_validate_command(ctx, &cmd_box, "file");
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
            let args_str = if let Some(a) = args {
                let v = lower_expr(ctx, a)?;
                emit_cp_validate_args(ctx, &v);
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            let opts_str = if let Some(o) = options {
                let v = lower_expr(ctx, o)?;
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            // #1780: spawn returns a streaming ChildProcess (EventEmitter with
            // Readable stdout/stderr), not the spawnSync result object. The
            // runtime returns an already-NaN-boxed pointer value.
            let result = ctx.block().call(
                DOUBLE,
                "js_child_process_spawn_streams",
                &[(I64, &cmd_str), (I64, &args_str), (I64, &opts_str)],
            );
            Ok(result)
        }

        Expr::ChildProcessFork {
            module,
            args,
            options,
        } => {
            // `fork(modulePath[, args][, options])` — like spawn, but the
            // runtime wires up an IPC channel + send/disconnect/'message'. The
            // runtime returns an already-NaN-boxed ChildProcess pointer. #1933.
            let mod_box = lower_expr(ctx, module)?;
            let blk = ctx.block();
            let mod_str = unbox_to_i64(blk, &mod_box);
            let args_str = if let Some(a) = args {
                let v = lower_expr(ctx, a)?;
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            let opts_str = if let Some(o) = options {
                let v = lower_expr(ctx, o)?;
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_child_process_fork",
                &[(I64, &mod_str), (I64, &args_str), (I64, &opts_str)],
            );
            Ok(result)
        }

        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            // `exec(cmd[, options], callback)` — runs synchronously and fires
            // the callback with `(err, stdout, stderr)` (see
            // `js_child_process_exec`). The callback may sit in the options
            // slot (`exec(cmd, cb)`), so pass both `options` and `callback` as
            // NaN-boxed f64 and let the runtime locate the closure. With no
            // callback the runtime returns the stdout string (legacy shape).
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let cmd_box = lower_expr(ctx, command)?;
            // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string command.
            emit_cp_validate_command(ctx, &cmd_box, "command");
            let cmd_str = unbox_to_i64(ctx.block(), &cmd_box);
            let arg1 = if let Some(o) = options {
                lower_expr(ctx, o)?
            } else {
                undef.clone()
            };
            let arg2 = if let Some(cb) = callback {
                lower_expr(ctx, cb)?
            } else {
                undef.clone()
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_child_process_exec",
                &[(I64, &cmd_str), (DOUBLE, &arg1), (DOUBLE, &arg2)],
            );
            Ok(result)
        }

        Expr::ChildProcessExecFile {
            file,
            args,
            options,
            callback,
        } => {
            // `execFile(file[, args][, options][, callback])` — runs the file
            // directly (no shell) and fires the callback with `(err, stdout,
            // stderr)`. file → i64 string handle; args/options/callback → NaN-
            // boxed f64 (the runtime locates the array + closure). See
            // `js_child_process_exec_file`.
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let file_box = lower_expr(ctx, file)?;
            // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string file.
            emit_cp_validate_command(ctx, &file_box, "file");
            let file_str = unbox_to_i64(ctx.block(), &file_box);
            let args_v = if let Some(a) = args {
                let v = lower_expr(ctx, a)?;
                emit_cp_validate_args(ctx, &v);
                v
            } else {
                undef.clone()
            };
            let opts_v = if let Some(o) = options {
                lower_expr(ctx, o)?
            } else {
                undef.clone()
            };
            let cb_v = if let Some(c) = callback {
                lower_expr(ctx, c)?
            } else {
                undef.clone()
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_child_process_exec_file",
                &[
                    (I64, &file_str),
                    (DOUBLE, &args_v),
                    (DOUBLE, &opts_v),
                    (DOUBLE, &cb_v),
                ],
            );
            Ok(result)
        }

        Expr::ChildProcessExecFileSync {
            file,
            args,
            options,
        } => {
            // `execFileSync(file[, args][, options])` → f64. #1937/#1938: the
            // runtime returns an already-NaN-boxed value (Buffer by default,
            // string with `encoding`) and throws on a non-zero exit, so we pass
            // the result straight through.
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let file_box = lower_expr(ctx, file)?;
            // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string file.
            emit_cp_validate_command(ctx, &file_box, "file");
            let file_str = unbox_to_i64(ctx.block(), &file_box);
            let args_v = if let Some(a) = args {
                let v = lower_expr(ctx, a)?;
                emit_cp_validate_args(ctx, &v);
                v
            } else {
                undef.clone()
            };
            let opts_v = if let Some(o) = options {
                lower_expr(ctx, o)?
            } else {
                undef.clone()
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_child_process_exec_file_sync",
                &[(I64, &file_str), (DOUBLE, &args_v), (DOUBLE, &opts_v)],
            );
            Ok(result)
        }

        Expr::ChildProcessGetProcessStatus(handle) => {
            let h = lower_expr(ctx, handle)?;
            let result =
                ctx.block()
                    .call(I64, "js_child_process_get_process_status", &[(DOUBLE, &h)]);
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }

        Expr::ChildProcessKillProcess(handle) => {
            let h = lower_expr(ctx, handle)?;
            let _ = ctx
                .block()
                .call(I32, "js_child_process_kill_process", &[(DOUBLE, &h)]);
            Ok(double_literal(0.0))
        }

        // -------- URL / URLSearchParams --------
        //
        // Runtime entrypoints live in `crates/perry-runtime/src/url.rs`. The
        // URL object is a plain `*mut ObjectHeader` with 10 string fields;
        // URLSearchParams is a separate `*mut ObjectHeader` holding a
        // `_entries: Array<[key, value]>` field. The HIR emits these nodes
        // only when the local is typed `URL` / `URLSearchParams` (see
        // `crates/perry-hir/src/lower.rs`), so here we assume the receiver
        // NaN-box holds a POINTER_TAG value we can unbox.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
