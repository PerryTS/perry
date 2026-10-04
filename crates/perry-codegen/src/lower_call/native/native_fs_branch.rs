
{
    if (module == "fs" || module == "node:fs") && object.is_some() {
        let recv = object.unwrap();
        let undefined = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        match method {
            "write" if !args.is_empty() => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let data = arg_values[0].clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_write",
                    &[(DOUBLE, &stream), (DOUBLE, &data)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "flush" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let callback = arg_values.first().cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_flush",
                    &[(DOUBLE, &stream), (DOUBLE, &callback)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "flushSync" => {
                let (recv_value, _arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_flush_sync",
                    &[(DOUBLE, &stream)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "end" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let chunk = arg_values.first().cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_end",
                    &[(DOUBLE, &stream), (DOUBLE, &chunk)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "destroy" | "close" => {
                let (recv_value, _arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_destroy",
                    &[(DOUBLE, &stream)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "reopen" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let file = arg_values.first().cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_reopen",
                    &[(DOUBLE, &stream), (DOUBLE, &file)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "on" | "addListener" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let event = arg_values.first().cloned().unwrap_or_else(undefined);
                let cb = arg_values.get(1).cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_on",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &cb)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "once" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let event = arg_values.first().cloned().unwrap_or_else(undefined);
                let cb = arg_values.get(1).cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_once",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &cb)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "off" | "removeListener" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let event = arg_values.first().cloned().unwrap_or_else(undefined);
                let cb = arg_values.get(1).cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_off",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &cb)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "removeAllListeners" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let event = arg_values.first().cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_remove_all",
                    &[(DOUBLE, &stream), (DOUBLE, &event)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "listenerCount" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let event = arg_values.first().cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_listener_count",
                    &[(DOUBLE, &stream), (DOUBLE, &event)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "emit" => {
                let (recv_value, arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let event = arg_values.first().cloned().unwrap_or_else(undefined);
                let arg = arg_values.get(1).cloned().unwrap_or_else(undefined);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_emit",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &arg)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "append" | "contentMode" | "fd" | "file" | "fsync" | "maxLength" | "minLength"
            | "mkdir" | "mode" | "periodicFlush" | "sync" | "writing" | "destroyed"
                if args.is_empty() =>
            {
                let (recv_value, _arg_values, arg_group) =
                    super::lower_operands_rooted(ctx, recv, args)?;
                let stream = recv_value.clone();
                let key_idx = ctx.strings.intern(method);
                let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let stream_bits = blk.bitcast_double_to_i64(&stream);
                let key_box = blk.load(DOUBLE, &key_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                let result = blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &stream_bits), (I64, &key_raw)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            _ => {}
        }
    }

    // fs module functions: readdirSync, statSync, mkdirSync, etc.
    // These are receiver-less NativeMethodCalls (`import { readdirSync }
    // from 'fs'` → `NativeMethodCall { module: "fs", object: None }`).
    // Dispatch before the catch-all so they call the runtime instead of
    // returning TAG_UNDEFINED.
    if (module == "fs" || module == "node:fs") && object.is_none() {
        match method {
            "Utf8Stream" => {
                let options = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_call_without_new",
                    &[(DOUBLE, &options)],
                ));
            }
            "_toUnixTimestamp" if !args.is_empty() => {
                let time = lower_expr(ctx, &args[0])?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_fs_to_unix_timestamp", &[(DOUBLE, &time)]));
            }
            "readFileSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let path = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_read_file_dispatch",
                    &[(DOUBLE, &path), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "openAsBlob" => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let path = arg_values.first().cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_open_as_blob",
                    &[(DOUBLE, &path), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "readdirSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                // Issue #631: forward the optional `options` arg
                // (e.g. `{withFileTypes:true}`) so the runtime can
                // return Dirent[] instead of string[]. Pre-fix
                // codegen dropped the second arg on the floor and
                // every Node-style `fs.readdirSync(p, {withFileTypes:
                // true}).filter(e => e.isDirectory())` chain crashed
                // with `(string).isDirectory is not a function`.
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
                return Ok(result);
            }
            "statSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
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
                return Ok(result);
            }
            "lstatSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_lstat_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "renameSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let from = arg_values[0].clone();
                let to = arg_values[1].clone();
                ctx.block()
                    .call_void("js_fs_rename_sync", &[(DOUBLE, &from), (DOUBLE, &to)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "unlinkSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_unlink_sync", &[(DOUBLE, &p)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "mkdirSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_mkdir_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "rmSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_rm_recursive_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "rmdirSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_rmdir_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "copyFileSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let src = arg_values[0].clone();
                let dst = arg_values[1].clone();
                let flags = if args.len() >= 3 {
                    arg_values[2].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_copy_file_sync_flags",
                    &[(DOUBLE, &src), (DOUBLE, &dst), (DOUBLE, &flags)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "cpSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let src = arg_values[0].clone();
                let dst = arg_values[1].clone();
                let options = if args.len() >= 3 {
                    arg_values[2].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_cp_sync_options",
                    &[(DOUBLE, &src), (DOUBLE, &dst), (DOUBLE, &options)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "chmodSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let m = arg_values[1].clone();
                ctx.block()
                    .call_void("js_fs_chmod_sync", &[(DOUBLE, &p), (DOUBLE, &m)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "truncateSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let len = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(0.0)
                };
                ctx.block()
                    .call_void("js_fs_truncate_sync", &[(DOUBLE, &p), (DOUBLE, &len)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "ftruncateSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let fd = arg_values[0].clone();
                let len = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(0.0)
                };
                ctx.block()
                    .call_void("js_fs_ftruncate_sync", &[(DOUBLE, &fd), (DOUBLE, &len)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "fsyncSync" if !args.is_empty() => {
                let fd = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_fsync_sync", &[(DOUBLE, &fd)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "fdatasyncSync" if !args.is_empty() => {
                let fd = lower_expr(ctx, &args[0])?;
                ctx.block()
                    .call_void("js_fs_fdatasync_sync", &[(DOUBLE, &fd)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "fchmodSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let fd = arg_values[0].clone();
                let mode = arg_values[1].clone();
                ctx.block()
                    .call_void("js_fs_fchmod_sync", &[(DOUBLE, &fd), (DOUBLE, &mode)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "fstatSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let fd = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_fstat_sync_options",
                    &[(DOUBLE, &fd), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "utimesSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let p = arg_values[0].clone();
                let atime = arg_values[1].clone();
                let mtime = arg_values[2].clone();
                ctx.block().call_void(
                    "js_fs_utimes_sync",
                    &[(DOUBLE, &p), (DOUBLE, &atime), (DOUBLE, &mtime)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "chownSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let p = arg_values[0].clone();
                let uid = arg_values[1].clone();
                let gid = arg_values[2].clone();
                ctx.block().call_void(
                    "js_fs_chown_sync",
                    &[(DOUBLE, &p), (DOUBLE, &uid), (DOUBLE, &gid)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "lchownSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let p = arg_values[0].clone();
                let uid = arg_values[1].clone();
                let gid = arg_values[2].clone();
                ctx.block().call_void(
                    "js_fs_lchown_sync",
                    &[(DOUBLE, &p), (DOUBLE, &uid), (DOUBLE, &gid)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "lchmodSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let m = arg_values[1].clone();
                ctx.block()
                    .call_void("js_fs_lchmod_sync", &[(DOUBLE, &p), (DOUBLE, &m)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "fchownSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let fd = arg_values[0].clone();
                let uid = arg_values[1].clone();
                let gid = arg_values[2].clone();
                ctx.block().call_void(
                    "js_fs_fchown_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &uid), (DOUBLE, &gid)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "lutimesSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let p = arg_values[0].clone();
                let atime = arg_values[1].clone();
                let mtime = arg_values[2].clone();
                ctx.block().call_void(
                    "js_fs_lutimes_sync",
                    &[(DOUBLE, &p), (DOUBLE, &atime), (DOUBLE, &mtime)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "futimesSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let fd = arg_values[0].clone();
                let atime = arg_values[1].clone();
                let mtime = arg_values[2].clone();
                ctx.block().call_void(
                    "js_fs_futimes_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &atime), (DOUBLE, &mtime)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "_toUnixTimestamp" if !args.is_empty() => {
                let time = lower_expr(ctx, &args[0])?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_fs_to_unix_timestamp", &[(DOUBLE, &time)]));
            }
            "readvSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let fd = arg_values[0].clone();
                let bufs = arg_values[1].clone();
                let pos = if args.len() >= 3 {
                    arg_values[2].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_readv_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &bufs), (DOUBLE, &pos)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "writevSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let fd = arg_values[0].clone();
                let bufs = arg_values[1].clone();
                let pos = if args.len() >= 3 {
                    arg_values[2].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_writev_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &bufs), (DOUBLE, &pos)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "statfsSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_statfs_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "opendirSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_fs_opendir_sync", &[(DOUBLE, &p)]));
            }
            "globSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let raw = ctx.block().call(
                    DOUBLE,
                    "js_fs_glob_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                let raw_bits = ctx.block().bitcast_double_to_i64(&raw);
                let result = crate::expr::nanbox_pointer_inline(ctx.block(), &raw_bits);
                arg_group.release(ctx);
                return Ok(result);
            }
            "linkSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let src = arg_values[0].clone();
                let dst = arg_values[1].clone();
                ctx.block()
                    .call_void("js_fs_link_sync", &[(DOUBLE, &src), (DOUBLE, &dst)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "symlinkSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let target = arg_values[0].clone();
                let path = arg_values[1].clone();
                ctx.block()
                    .call_void("js_fs_symlink_sync", &[(DOUBLE, &target), (DOUBLE, &path)]);
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            "readlinkSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let options = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_readlink_dispatch",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "mkdtempDisposableSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
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
                return Ok(result);
            }
            "openSync" if !args.is_empty() => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let p = arg_values[0].clone();
                let flags = if args.len() >= 2 {
                    arg_values[1].clone()
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_open_sync",
                    &[(DOUBLE, &p), (DOUBLE, &flags)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "closeSync" if !args.is_empty() => {
                let fd = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_close_sync", &[(DOUBLE, &fd)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "readSync" if args.len() >= 5 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(5)])?;
                let fd = arg_values[0].clone();
                let buf = arg_values[1].clone();
                let off = arg_values[2].clone();
                let len = arg_values[3].clone();
                let pos = arg_values[4].clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_read_sync",
                    &[
                        (DOUBLE, &fd),
                        (DOUBLE, &buf),
                        (DOUBLE, &off),
                        (DOUBLE, &len),
                        (DOUBLE, &pos),
                    ],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "readSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let fd = arg_values[0].clone();
                let buf = arg_values[1].clone();
                let options = arg_values[2].clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_read_sync_options",
                    &[(DOUBLE, &fd), (DOUBLE, &buf), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "writeSync" if args.len() >= 5 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(5)])?;
                let fd = arg_values[0].clone();
                let buf = arg_values[1].clone();
                let off = arg_values[2].clone();
                let len = arg_values[3].clone();
                let pos = arg_values[4].clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_write_buffer_sync",
                    &[
                        (DOUBLE, &fd),
                        (DOUBLE, &buf),
                        (DOUBLE, &off),
                        (DOUBLE, &len),
                        (DOUBLE, &pos),
                    ],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "writeSync" if args.len() == 4 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..4])?;
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_write_sync_args",
                    &[
                        (DOUBLE, &arg_values[0]),
                        (DOUBLE, &arg_values[1]),
                        (DOUBLE, &arg_values[2]),
                        (DOUBLE, &arg_values[3]),
                    ],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "writeSync" if args.len() >= 3 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let fd = arg_values[0].clone();
                let data = arg_values[1].clone();
                let options = arg_values[2].clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_write_sync_options_dispatch",
                    &[(DOUBLE, &fd), (DOUBLE, &data), (DOUBLE, &options)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            "writeSync" if args.len() >= 2 => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let fd = arg_values[0].clone();
                let data = arg_values[1].clone();
                let result = ctx.block().call(
                    DOUBLE,
                    "js_fs_write_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &data)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            _ => {
                // Fall through — readFileSync/writeFileSync/existsSync/etc.
                // are handled as dedicated HIR Expr variants, not
                // NativeMethodCall. Warn on truly unhandled ones.
                eprintln!(
                    "perry-codegen: unhandled fs.{}() NativeMethodCall ({})",
                    method,
                    args.len()
                );
            }
        }
    }
}
