use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::lower_call::lower_call;
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

/// Phase H fs: `fs.promises.METHOD(args...)`.
pub(crate) fn arm_fs_promises(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
        property.as_str()
    } else {
        unreachable!()
    };
    match property {
        "readFile" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_promises_read_file",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "writeFile" if args.len() >= 2 => {
            let path = lower_expr(ctx, &args[0])?;
            let content = lower_expr(ctx, &args[1])?;
            let options = if args.len() >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_promises_write_file",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            ))
        }
        "appendFile" if args.len() >= 2 => {
            let path = lower_expr(ctx, &args[0])?;
            let content = lower_expr(ctx, &args[1])?;
            let options = if args.len() >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_promises_append_file",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            ))
        }
        "mkdir" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_promises_mkdir",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        _ => {
            // Unsupported — return a resolved promise holding
            // undefined so `await` sees a real pending→settled
            // transition instead of a null pointer.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            Ok(nanbox_pointer_inline(blk, &promise_handle))
        }
    }
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
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_read_file_dispatch",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "openAsBlob" => {
            let p = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_open_as_blob",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "statSync" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_stat_sync_options",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "readdirSync" if !args.is_empty() => {
            // Runtime returns a raw ArrayHeader pointer
            // transmuted to f64 (no NaN-box tag). Unbox as i64
            // and re-NaN-box with POINTER_TAG so downstream
            // length/index paths see a proper array handle.
            // Issue #631: forward optional `options` arg to
            // pick up `withFileTypes:true`.
            let p = lower_expr(ctx, &args[0])?;
            let opts = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
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
            Ok(nanbox_pointer_inline(blk, &raw_bits))
        }
        "renameSync" if args.len() >= 2 => {
            let from = lower_expr(ctx, &args[0])?;
            let to = lower_expr(ctx, &args[1])?;
            let _ = ctx
                .block()
                .call(I32, "js_fs_rename_sync", &[(DOUBLE, &from), (DOUBLE, &to)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "copyFileSync" if args.len() >= 2 => {
            let from = lower_expr(ctx, &args[0])?;
            let to = lower_expr(ctx, &args[1])?;
            let flags = if args.len() >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_copy_file_sync_flags",
                &[(DOUBLE, &from), (DOUBLE, &to), (DOUBLE, &flags)],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "writeFileSync" if args.len() >= 2 => {
            let path = lower_expr(ctx, &args[0])?;
            let content = lower_expr(ctx, &args[1])?;
            let options = if args.len() >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_write_file_sync_options",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "appendFileSync" if args.len() >= 2 => {
            let path = lower_expr(ctx, &args[0])?;
            let content = lower_expr(ctx, &args[1])?;
            let options = if args.len() >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_append_file_sync_options",
                &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "accessSync" if !args.is_empty() => {
            // Node throws on inaccessible paths. We dispatch
            // through `js_fs_access_sync_throw` which calls
            // `js_throw` on failure, longjmping into the
            // nearest enclosing try/catch. Returns NaN-boxed
            // undefined on success.
            let p = lower_expr(ctx, &args[0])?;
            let mode = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_access_sync_throw_mode",
                &[(DOUBLE, &p), (DOUBLE, &mode)],
            ))
        }
        "realpathSync" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_realpath_dispatch",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "mkdtempSync" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_mkdtemp_dispatch",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "mkdtempDisposableSync" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_mkdtemp_disposable_sync",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "symlink" if args.len() >= 2 => {
            let target = lower_expr(ctx, &args[0])?;
            let path = lower_expr(ctx, &args[1])?;
            let arg2 = if args.len() >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let arg3 = if args.len() >= 4 {
                lower_expr(ctx, &args[3])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_symlink_callback",
                &[
                    (DOUBLE, &target),
                    (DOUBLE, &path),
                    (DOUBLE, &arg2),
                    (DOUBLE, &arg3),
                ],
            ))
        }
        "rmdirSync" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let _ = ctx.block().call(
                I32,
                "js_fs_rmdir_sync_options",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "createWriteStream" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_create_write_stream",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "createReadStream" if !args.is_empty() => {
            let p = lower_expr(ctx, &args[0])?;
            let options = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_create_read_stream",
                &[(DOUBLE, &p), (DOUBLE, &options)],
            ))
        }
        "_toUnixTimestamp" if !args.is_empty() => {
            let time = lower_expr(ctx, &args[0])?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_fs_to_unix_timestamp", &[(DOUBLE, &time)]))
        }
        "readFile" if args.len() >= 3 => {
            // Node `fs.readFile(path, encoding, callback)` —
            // sync read + immediate callback invocation.
            let p = lower_expr(ctx, &args[0])?;
            let enc = lower_expr(ctx, &args[1])?;
            let cb = lower_expr(ctx, &args[2])?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_read_file_callback",
                &[(DOUBLE, &p), (DOUBLE, &enc), (DOUBLE, &cb)],
            ))
        }
        "readFile" if args.len() >= 2 => {
            // Node `fs.readFile(path, callback)` (no encoding).
            let p = lower_expr(ctx, &args[0])?;
            let cb = lower_expr(ctx, &args[1])?;
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_fs_read_file_callback",
                &[(DOUBLE, &p), (DOUBLE, &undef), (DOUBLE, &cb)],
            ))
        }
        _ => {
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
    }
}
