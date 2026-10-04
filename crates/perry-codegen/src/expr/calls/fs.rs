use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::lower_call::lower_call;
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32};

/// Phase H fs: `fs.promises.METHOD(args...)`.
pub(crate) fn arm_fs_promises(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
        property.as_str()
    } else {
        unreachable!()
    };
    match property {
        "readFile" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_promises_read_file",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "writeFile" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
            let path = arg_values[0].clone();
            let content = arg_values[1].clone();
            let options = if args.len() >= 3 {
                arg_values[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_promises_write_file",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "appendFile" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
            let path = arg_values[0].clone();
            let content = arg_values[1].clone();
            let options = if args.len() >= 3 {
                arg_values[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_promises_append_file",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "mkdir" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_promises_mkdir",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "rmdir" => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values
                .first()
                .cloned()
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_promises_rmdir",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        // Every other method (`stat`, `lstat`, `readdir`, ...) is a real
        // export of the `fs.promises` namespace: call it through the generic
        // path, like `arm_fs` does, instead of resolving to `undefined`.
        _ => lower_generic_fs_call(ctx, callee, args),
    }
}

/// An fs method without dedicated lowering: its callee and arguments escape
/// into an unknown call, so downgrade their buffer aliases first.
fn lower_generic_fs_call(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    crate::expr::downgrade_buffer_aliases_in_expr(
        ctx,
        callee,
        crate::native_value::MaterializationReason::UnknownCallEscape,
    );
    for arg in args {
        crate::expr::downgrade_buffer_aliases_in_expr(
            ctx,
            arg,
            crate::native_value::MaterializationReason::UnknownCallEscape,
        );
    }
    lower_call(ctx, callee, args)
}

/// Phase H fs: `fs.METHOD(args...)` — catch-all for sync APIs reaching
/// the generic Call shape.
pub(crate) fn arm_fs(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
        property.as_str()
    } else {
        unreachable!()
    };
    match property {
        "readFileSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_read_file_dispatch",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "openAsBlob" => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values
                .first()
                .cloned()
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_open_as_blob",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "statSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_stat_sync_options",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "readdirSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            // Runtime returns a raw ArrayHeader pointer
            // transmuted to f64 (no NaN-box tag). Unbox as i64
            // and re-NaN-box with POINTER_TAG so downstream
            // length/index paths see a proper array handle.
            // Issue #631: forward optional `options` arg to
            // pick up `withFileTypes:true`.
            let p = arg_values[0].clone();
            let opts = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let raw = blk.call(
                DOUBLE,
                "js_fs_readdir_sync",
                &[(DOUBLE, &p), (DOUBLE, &opts)],
            );
            let raw_bits = blk.bitcast_double_to_i64(&raw);
            let result = nanbox_pointer_inline(blk, &raw_bits);
            arg_group.release(ctx);
            Ok(result)
        }
        "renameSync" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let from = arg_values[0].clone();
            let to = arg_values[1].clone();
            let _ = ctx
                .block()
                .call(I32, "js_fs_rename_sync", &[(DOUBLE, &from), (DOUBLE, &to)]);
            let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            arg_group.release(ctx);
            Ok(result)
        }
        "copyFileSync" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
            let from = arg_values[0].clone();
            let to = arg_values[1].clone();
            let flags = if args.len() >= 3 {
                arg_values[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_copy_file_sync_flags",
                &[(DOUBLE, &from), (DOUBLE, &to), (DOUBLE, &flags)],
            );
            let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            arg_group.release(ctx);
            Ok(result)
        }
        "writeFileSync" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
            let path = arg_values[0].clone();
            let content = arg_values[1].clone();
            let options = if args.len() >= 3 {
                arg_values[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_write_file_sync_options",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            );
            let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            arg_group.release(ctx);
            Ok(result)
        }
        "appendFileSync" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
            let path = arg_values[0].clone();
            let content = arg_values[1].clone();
            let options = if args.len() >= 3 {
                arg_values[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_append_file_sync_options",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            );
            let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            arg_group.release(ctx);
            Ok(result)
        }
        "accessSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            // Node throws on inaccessible paths. We dispatch
            // through `js_fs_access_sync_throw` which calls
            // `js_throw` on failure, longjmping into the
            // nearest enclosing try/catch. Returns NaN-boxed
            // undefined on success.
            let p = arg_values[0].clone();
            let mode = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_access_sync_throw_mode",
                &[(DOUBLE, &p), (DOUBLE, &mode)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "realpathSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_realpath_dispatch",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "mkdtempSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_mkdtemp_dispatch",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "mkdtempDisposableSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_mkdtemp_disposable_sync",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "symlink" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(4)])?;
            let target = arg_values[0].clone();
            let path = arg_values[1].clone();
            let arg2 = if args.len() >= 3 {
                arg_values[2].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let arg3 = if args.len() >= 4 {
                arg_values[3].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_symlink_callback",
                &[
                    (DOUBLE, &target),
                    (DOUBLE, &path),
                    (DOUBLE, &arg2),
                    (DOUBLE, &arg3),
                ],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "rmdirSync" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_rmdir_sync_options",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            arg_group.release(ctx);
            Ok(result)
        }
        "createWriteStream" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_create_write_stream",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "createReadStream" if !args.is_empty() => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            let p = arg_values[0].clone();
            let options = if args.len() >= 2 {
                arg_values[1].clone()
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_create_read_stream",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "_toUnixTimestamp" if !args.is_empty() => {
            let time = lower_expr(ctx, &args[0])?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_fs_to_unix_timestamp", &[(DOUBLE, &time)]))
        }
        "readFile" if args.len() >= 3 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
            // Node `fs.readFile(path, encoding, callback)` —
            // sync read + immediate callback invocation.
            let p = arg_values[0].clone();
            let enc = arg_values[1].clone();
            let cb = arg_values[2].clone();
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_read_file_callback",
                &[(DOUBLE, &p), (DOUBLE, &enc), (DOUBLE, &cb)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        "readFile" if args.len() >= 2 => {
            let (arg_values, arg_group) =
                crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
            // Node `fs.readFile(path, callback)` (no encoding).
            let p = arg_values[0].clone();
            let cb = arg_values[1].clone();
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let result = ctx.block().call(
                DOUBLE,
                "js_fs_read_file_callback",
                &[(DOUBLE, &p), (DOUBLE, &undef), (DOUBLE, &cb)],
            );
            arg_group.release(ctx);
            Ok(result)
        }
        _ => lower_generic_fs_call(ctx, callee, args),
    }
}
