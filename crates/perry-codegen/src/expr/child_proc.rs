//! ChildProcess execSync/spawnSync/spawn/exec/etc.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ChildProcessExecSync { command, options } => {
            let cmd_box = lower_expr(ctx, command)?;
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
            let opts_str = if let Some(opts) = options {
                let o = lower_expr(ctx, opts)?;
                unbox_to_i64(ctx.block(), &o)
            } else {
                "0".to_string()
            };
            // js_child_process_exec_sync(cmd: i64, opts: i64) -> i64 (string handle)
            // Runtime returns null on error; guard against it by
            // replacing null with an empty string so `.length` reads 0
            // instead of crashing.
            let raw = ctx.block().call(
                I64,
                "js_child_process_exec_sync",
                &[(I64, &cmd_str), (I64, &opts_str)],
            );
            let is_null = ctx.block().icmp_eq(I64, &raw, "0");
            let empty = ctx
                .block()
                .call(I64, "js_string_from_bytes", &[(PTR, "null"), (I32, "0")]);
            let blk = ctx.block();
            let result = blk.select(crate::types::I1, &is_null, I64, &empty, &raw);
            Ok(nanbox_string_inline(ctx.block(), &result))
        }

        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        } => {
            let cmd_box = lower_expr(ctx, command)?;
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
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
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
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
                I64,
                "js_child_process_spawn_sync",
                &[(I64, &cmd_str), (I64, &args_str), (I64, &opts_str)],
            );
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }

        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            let cmd_box = lower_expr(ctx, command)?;
            let blk = ctx.block();
            let cmd_str = unbox_to_i64(blk, &cmd_box);
            let opts_str = if let Some(o) = options {
                let v = lower_expr(ctx, o)?;
                unbox_to_i64(ctx.block(), &v)
            } else {
                "0".to_string()
            };
            if let Some(cb) = callback {
                let _ = lower_expr(ctx, cb)?;
            }
            let result = ctx.block().call(
                I64,
                "js_child_process_exec_sync",
                &[(I64, &cmd_str), (I64, &opts_str)],
            );
            Ok(nanbox_string_inline(ctx.block(), &result))
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
