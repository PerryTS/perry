//! ChildProcess execSync/spawnSync/spawn/exec/etc.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//!
//! # Layer 1 rooting (#7615 slice 7)
//!
//! Every entry point here has the same skeleton — an ordered argument list with
//! optional slots, a run of setup-time validators, a run of
//! [`unbox_to_i64`] strips, and one consuming `js_child_process_*` call — and
//! before this slice each arm re-implemented that skeleton by hand. Three of the
//! eight rooted unconditionally through `expr::temp_root`; the other five rooted
//! nothing at all while holding **raw heap pointers** across arbitrary user
//! lowerings. [`lower_cp_args`] is the skeleton, and every arm is now an
//! argument list plus a consuming call.
//!
//! Two properties are worth stating because they are decisions rather than
//! mechanics:
//!
//! * **Validators run below the whole argument list.** They used to be
//!   interleaved — lower `command`, validate it, lower `options` — which throws
//!   `ERR_INVALID_ARG_TYPE` *before* evaluating the later arguments. JS
//!   evaluates a call's whole argument list before the callee is entered, so
//!   node runs those side effects and only then throws. The relative order of
//!   the validators among themselves is unchanged (`file`, then `args`, then
//!   `options`), which is node's own order in `normalizeSpawnArguments`.
//! * **The validators get their own re-read.** Each arm re-reads its operands
//!   once for the validators and again for the unbox + consuming call.
//!   `js_child_process_validate_options` reads a dozen own properties off a user
//!   object, allocating a key string per read; rather than depend on that being
//!   a non-collecting window (#7198's position), the second re-read simply
//!   removes the question. It is free where nothing is protected —
//!   [`crate::rooting::RootedGroup::reread`] on an unprotected operand emits no
//!   IR at all — and one `load` where something is.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::rooting::{any_operand_may_collect, with_rooted_group, RootedGroup};
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    emit_string_literal_global, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx,
};

/// Flatten an argument list's optional slots into one ordered operand list.
///
/// Returns the present operands in evaluation order, plus a per-slot index into
/// that list — `None` for an absent slot. The flattening is what lets the
/// positional protection rule ("operand `i` is live across every operand after
/// it") be answered by [`any_operand_may_collect`], the same predicate every
/// other lowering consults, instead of by an `if let Some(..)` ladder per arm.
fn operand_slots<'a>(slots: &[Option<&'a Expr>]) -> (Vec<&'a Expr>, Vec<Option<usize>>) {
    let mut exprs = Vec::with_capacity(slots.len());
    let mut at = Vec::with_capacity(slots.len());
    for slot in slots {
        match slot {
            Some(expr) => {
                exprs.push(*expr);
                at.push(Some(exprs.len() - 1));
            }
            None => at.push(None),
        }
    }
    (exprs, at)
}

/// Lower one `child_process` argument list into `group`, rooting each operand
/// across the lowering of the operands that follow it.
///
/// `discarded` names a slot whose lowered value has no consumer — only
/// `spawnBackground`'s `args`, which is evaluated for its side effects and then
/// dropped. A value with no consumer has no window, so it is lowered in place
/// (evaluation order is observable) and protected not at all.
///
/// `across_call` is the caller's statement that an **emitted** step below the
/// list can reach a collection point. Only `fork` sets it: that arm emits
/// `js_jsvalue_to_string_coerce`, which runs a user `toString` on an object
/// module path. `any_operand_may_collect` reads expressions and cannot see an
/// emitted call, which is why the answer is stated rather than derived — the
/// precedent is `crate::rooting::with_operands_rooted_across_call`.
fn lower_cp_args<'a>(
    ctx: &mut FnCtx<'_>,
    group: &mut RootedGroup<'a>,
    exprs: &[&'a Expr],
    discarded: Option<usize>,
    across_call: bool,
) -> Result<()> {
    for (i, expr) in exprs.iter().enumerate() {
        let consumed = Some(i) != discarded;
        let collects = consumed
            && (across_call || any_operand_may_collect(ctx, exprs[i + 1..].iter().copied()));
        group.lower(ctx, expr, collects)?;
    }
    Ok(())
}

/// Re-read the operand in `slot`, or the `undefined` literal when the slot is
/// absent — the NaN-boxed argument form (`exec`'s `options`/`callback`,
/// `execFile`'s `args`/`options`/`callback`).
fn slot_box(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
    slot: Option<usize>,
    undef: &str,
) -> Result<String> {
    match slot {
        Some(i) => group.reread(ctx, i),
        None => Ok(undef.to_string()),
    }
}

/// [`slot_box`] followed by the raw-pointer strip, or the `0` sentinel the
/// `i64`-argument entry points use for an absent slot.
///
/// The strip is deliberately fused to the re-read: `unbox_to_i64` produces a
/// bare heap pointer, and #7280's taxonomy (a) is precisely a pointer that has
/// already left the NaN-boxed representation a rooted slot can be re-read into.
/// Holding one across anything is unrepairable, so it is produced immediately
/// above its use and never crosses another operand.
fn slot_ptr(ctx: &mut FnCtx<'_>, group: &RootedGroup<'_>, slot: Option<usize>) -> Result<String> {
    match slot {
        Some(i) => {
            let boxed = group.reread(ctx, i)?;
            Ok(unbox_to_i64(ctx.block(), &boxed))
        }
        None => Ok("0".to_string()),
    }
}

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

/// `fork()` accepts a module path string, Buffer, or WHATWG URL. Validate the
/// original tagged value before coercing it to the raw string pointer.
fn emit_cp_validate_fork_module(ctx: &mut FnCtx<'_>, module_box: &str) {
    let blk = ctx.block();
    let _ = blk.call(
        DOUBLE,
        "js_child_process_validate_fork_module",
        &[(DOUBLE, module_box)],
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

/// Validate a spawn/fork option bag while it is still NaN-boxed. `sync`
/// selects spawnSync's stdio rules and `allow_null` models fork's overload.
fn emit_cp_validate_options(ctx: &mut FnCtx<'_>, opts_box: &str, sync: bool, allow_null: bool) {
    let blk = ctx.block();
    let _ = blk.call(
        DOUBLE,
        "js_child_process_validate_options",
        &[
            (DOUBLE, opts_box),
            (I32, if sync { "1" } else { "0" }),
            (I32, if allow_null { "1" } else { "0" }),
        ],
    );
}

fn emit_cp_validate_spawn_args(ctx: &mut FnCtx<'_>, args_box: &str, sync: bool, allow_null: bool) {
    let blk = ctx.block();
    let _ = blk.call(
        DOUBLE,
        "js_child_process_validate_spawn_args",
        &[
            (DOUBLE, args_box),
            (I32, if sync { "1" } else { "0" }),
            (I32, if allow_null { "1" } else { "0" }),
        ],
    );
}

/// The `spawn` / `spawnSync` validator run: command, then args, then options.
///
/// That order is node's own (`normalizeSpawnArguments` validates `file` first),
/// and it is now the order the emissions appear in rather than an accident of
/// where each operand happened to be lowered. Each validator reads a freshly
/// re-read operand, so none of them observes a register produced above the one
/// before it.
fn emit_cp_validators(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
    at: &[Option<usize>],
    command_name: &str,
    sync: bool,
    allow_null: bool,
) -> Result<()> {
    let cmd_box = group.reread(ctx, 0)?;
    emit_cp_validate_command(ctx, &cmd_box, command_name);
    if let Some(i) = at[1] {
        let v = group.reread(ctx, i)?;
        emit_cp_validate_spawn_args(ctx, &v, sync, allow_null);
    }
    if let Some(i) = at[2] {
        let v = group.reread(ctx, i)?;
        emit_cp_validate_options(ctx, &v, sync, allow_null);
    }
    Ok(())
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ChildProcessExecSync { command, options } => {
            let (exprs, at) = operand_slots(&[Some(command), options.as_deref()]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, None, false)?;
                // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string
                // command -- below the whole argument list, which is where node
                // throws it (the callee is not entered until every argument has
                // been evaluated).
                let cmd_box = g.reread(ctx, 0)?;
                emit_cp_validate_command(ctx, &cmd_box, "command");
                let cmd_str = slot_ptr(ctx, g, at[0])?;
                let opts_str = slot_ptr(ctx, g, at[1])?;
                // js_child_process_exec_sync(cmd: i64, opts: i64) -> f64.
                // #1937/#1938: the runtime returns an already-NaN-boxed value
                // (Buffer by default, string with `encoding`) and throws on a
                // non-zero exit, so we pass the result straight through.
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_child_process_exec_sync",
                    &[(I64, &cmd_str), (I64, &opts_str)],
                ))
            })
        }

        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        } => {
            let (exprs, at) = operand_slots(&[Some(command), args.as_deref(), options.as_deref()]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, None, false)?;
                emit_cp_validators(ctx, g, &at, "file", true, false)?;
                let cmd_str = slot_ptr(ctx, g, at[0])?;
                let args_str = slot_ptr(ctx, g, at[1])?;
                let opts_str = slot_ptr(ctx, g, at[2])?;
                // js_child_process_spawn_sync(cmd: i64, args: i64, opts: i64) -> i64
                let result = ctx.block().call(
                    I64,
                    "js_child_process_spawn_sync",
                    &[(I64, &cmd_str), (I64, &args_str), (I64, &opts_str)],
                );
                Ok(nanbox_pointer_inline(ctx.block(), &result))
            })
        }

        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            let (exprs, at) = operand_slots(&[
                Some(command),
                args.as_deref(),
                Some(log_file),
                env_json.as_deref(),
            ]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, at[1], false)?;
                let log_box = g.reread(ctx, at[2].expect("log_file is not optional"))?;
                let log_str = unbox_to_i64(ctx.block(), &log_box);
                let log_nanbox = nanbox_string_inline(ctx.block(), &log_str);
                let env_box = match at[3] {
                    Some(i) => g.reread(ctx, i)?,
                    None => double_literal(0.0),
                };
                // js_child_process_spawn_background(cmd: f64, args_arr: i64, logFile: f64, envJson: f64) -> i64
                let cmd_box = g.reread(ctx, 0)?;
                let cmd_str = unbox_to_i64(ctx.block(), &cmd_box);
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
            })
        }

        Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => {
            let (exprs, at) = operand_slots(&[Some(command), args.as_deref(), options.as_deref()]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, None, false)?;
                emit_cp_validators(ctx, g, &at, "file", false, false)?;
                let cmd_str = slot_ptr(ctx, g, at[0])?;
                let args_str = slot_ptr(ctx, g, at[1])?;
                let opts_str = slot_ptr(ctx, g, at[2])?;
                // #1780: spawn returns a streaming ChildProcess (EventEmitter with
                // Readable stdout/stderr), not the spawnSync result object. The
                // runtime returns an already-NaN-boxed pointer value.
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_child_process_spawn_streams",
                    &[(I64, &cmd_str), (I64, &args_str), (I64, &opts_str)],
                ))
            })
        }

        Expr::ChildProcessFork {
            module,
            args,
            options,
        } => {
            // `fork(modulePath[, args][, options])` — like spawn, but the
            // runtime wires up an IPC channel + send/disconnect/'message'. The
            // runtime returns an already-NaN-boxed ChildProcess pointer. #1933.
            let (exprs, at) = operand_slots(&[Some(module), args.as_deref(), options.as_deref()]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                // `across_call`: `js_jsvalue_to_string_coerce` below runs a user
                // `toString` on an object module path (`fork(new URL(..))` is the
                // documented overload), so every operand's window collects
                // whatever the argument expressions themselves do.
                lower_cp_args(ctx, g, &exprs, None, true)?;
                let mod_box = g.reread(ctx, 0)?;
                emit_cp_validate_fork_module(ctx, &mod_box);
                if let Some(i) = at[1] {
                    let v = g.reread(ctx, i)?;
                    emit_cp_validate_spawn_args(ctx, &v, false, true);
                }
                if let Some(i) = at[2] {
                    let v = g.reread(ctx, i)?;
                    emit_cp_validate_options(ctx, &v, false, true);
                }
                // The coercion is the collection point the roots exist for, so
                // `module` is re-read immediately above it and `args`/`options`
                // strictly below it. Before this slice `mod_str` — a RAW string
                // pointer — was produced here and then held across both of those
                // lowerings, which is #7280 taxonomy (a): no re-read of a
                // NaN-boxed slot can repair an already-stripped pointer.
                let mod_box = g.reread(ctx, 0)?;
                let mod_str =
                    ctx.block()
                        .call(I64, "js_jsvalue_to_string_coerce", &[(DOUBLE, &mod_box)]);
                let args_str = slot_ptr(ctx, g, at[1])?;
                let opts_str = slot_ptr(ctx, g, at[2])?;
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_child_process_fork",
                    &[(I64, &mod_str), (I64, &args_str), (I64, &opts_str)],
                ))
            })
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
            let (exprs, at) =
                operand_slots(&[Some(command), options.as_deref(), callback.as_deref()]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, None, false)?;
                // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string command.
                let cmd_box = g.reread(ctx, 0)?;
                emit_cp_validate_command(ctx, &cmd_box, "command");
                let arg1 = slot_box(ctx, g, at[1], &undef)?;
                let arg2 = slot_box(ctx, g, at[2], &undef)?;
                let cmd_str = slot_ptr(ctx, g, at[0])?;
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_child_process_exec",
                    &[(I64, &cmd_str), (DOUBLE, &arg1), (DOUBLE, &arg2)],
                ))
            })
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
            let (exprs, at) = operand_slots(&[
                Some(file),
                args.as_deref(),
                options.as_deref(),
                callback.as_deref(),
            ]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, None, false)?;
                // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string file.
                let file_box = g.reread(ctx, 0)?;
                emit_cp_validate_command(ctx, &file_box, "file");
                if let Some(i) = at[1] {
                    let v = g.reread(ctx, i)?;
                    emit_cp_validate_args(ctx, &v);
                }
                let args_v = slot_box(ctx, g, at[1], &undef)?;
                let opts_v = slot_box(ctx, g, at[2], &undef)?;
                let cb_v = slot_box(ctx, g, at[3], &undef)?;
                let file_str = slot_ptr(ctx, g, at[0])?;
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_child_process_exec_file",
                    &[
                        (I64, &file_str),
                        (DOUBLE, &args_v),
                        (DOUBLE, &opts_v),
                        (DOUBLE, &cb_v),
                    ],
                ))
            })
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
            let (exprs, at) = operand_slots(&[Some(file), args.as_deref(), options.as_deref()]);
            with_rooted_group(ctx, exprs.len(), |ctx, g| {
                lower_cp_args(ctx, g, &exprs, None, false)?;
                // #3079: throw `ERR_INVALID_ARG_TYPE` for a missing/non-string file.
                let file_box = g.reread(ctx, 0)?;
                emit_cp_validate_command(ctx, &file_box, "file");
                if let Some(i) = at[1] {
                    let v = g.reread(ctx, i)?;
                    emit_cp_validate_args(ctx, &v);
                }
                let args_v = slot_box(ctx, g, at[1], &undef)?;
                let opts_v = slot_box(ctx, g, at[2], &undef)?;
                let file_str = slot_ptr(ctx, g, at[0])?;
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_child_process_exec_file_sync",
                    &[(I64, &file_str), (DOUBLE, &args_v), (DOUBLE, &opts_v)],
                ))
            })
        }

        Expr::ChildProcessGetProcessStatus(handle) => {
            // One operand, consumed by the very next emission: no window.
            let h = super::lower_expr(ctx, handle)?;
            let result =
                ctx.block()
                    .call(I64, "js_child_process_get_process_status", &[(DOUBLE, &h)]);
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }

        Expr::ChildProcessKillProcess(handle) => {
            // One operand, consumed by the very next emission: no window.
            let h = super::lower_expr(ctx, handle)?;
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
