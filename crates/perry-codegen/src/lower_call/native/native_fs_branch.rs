
{
    if (module == "fs" || module == "node:fs") && object.is_some() {
        let recv = object.unwrap();
        let undefined = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        match method {
            "write" if !args.is_empty() => {
                let stream = lower_expr(ctx, recv)?;
                let data = lower_expr(ctx, &args[0])?;
                for arg in args.iter().skip(1) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_write",
                    &[(DOUBLE, &stream), (DOUBLE, &data)],
                ));
            }
            "flush" => {
                let stream = lower_expr(ctx, recv)?;
                let callback = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(1) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_flush",
                    &[(DOUBLE, &stream), (DOUBLE, &callback)],
                ));
            }
            "flushSync" => {
                let stream = lower_expr(ctx, recv)?;
                for arg in args {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_flush_sync",
                    &[(DOUBLE, &stream)],
                ));
            }
            "end" => {
                let stream = lower_expr(ctx, recv)?;
                let chunk = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(1) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_end",
                    &[(DOUBLE, &stream), (DOUBLE, &chunk)],
                ));
            }
            "destroy" | "close" => {
                let stream = lower_expr(ctx, recv)?;
                for arg in args {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_destroy",
                    &[(DOUBLE, &stream)],
                ));
            }
            "reopen" => {
                let stream = lower_expr(ctx, recv)?;
                let file = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(1) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_reopen",
                    &[(DOUBLE, &stream), (DOUBLE, &file)],
                ));
            }
            "on" | "addListener" => {
                let stream = lower_expr(ctx, recv)?;
                let event = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                let cb = if let Some(arg) = args.get(1) {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(2) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_on",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &cb)],
                ));
            }
            "once" => {
                let stream = lower_expr(ctx, recv)?;
                let event = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                let cb = if let Some(arg) = args.get(1) {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(2) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_once",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &cb)],
                ));
            }
            "off" | "removeListener" => {
                let stream = lower_expr(ctx, recv)?;
                let event = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                let cb = if let Some(arg) = args.get(1) {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(2) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_off",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &cb)],
                ));
            }
            "removeAllListeners" => {
                let stream = lower_expr(ctx, recv)?;
                let event = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(1) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_remove_all",
                    &[(DOUBLE, &stream), (DOUBLE, &event)],
                ));
            }
            "listenerCount" => {
                let stream = lower_expr(ctx, recv)?;
                let event = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for arg in args.iter().skip(1) {
                    let _ = lower_expr(ctx, arg)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_listener_count",
                    &[(DOUBLE, &stream), (DOUBLE, &event)],
                ));
            }
            "emit" => {
                let stream = lower_expr(ctx, recv)?;
                let event = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                let arg = if let Some(arg) = args.get(1) {
                    lower_expr(ctx, arg)?
                } else {
                    undefined()
                };
                for extra in args.iter().skip(2) {
                    let _ = lower_expr(ctx, extra)?;
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_utf8_stream_emit",
                    &[(DOUBLE, &stream), (DOUBLE, &event), (DOUBLE, &arg)],
                ));
            }
            "append" | "contentMode" | "fd" | "file" | "fsync" | "maxLength" | "minLength"
            | "mkdir" | "mode" | "periodicFlush" | "sync" | "writing" | "destroyed"
                if args.is_empty() =>
            {
                let stream = lower_expr(ctx, recv)?;
                let key_idx = ctx.strings.intern(method);
                let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let stream_bits = blk.bitcast_double_to_i64(&stream);
                let key_box = blk.load(DOUBLE, &key_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                return Ok(blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &stream_bits), (I64, &key_raw)],
                ));
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
                let path = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_read_file_dispatch",
                    &[(DOUBLE, &path), (DOUBLE, &options)],
                ));
            }
            "openAsBlob" => {
                let path = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_open_as_blob",
                    &[(DOUBLE, &path), (DOUBLE, &options)],
                ));
            }
            "readdirSync" if !args.is_empty() => {
                // Issue #631: forward the optional `options` arg
                // (e.g. `{withFileTypes:true}`) so the runtime can
                // return Dirent[] instead of string[]. Pre-fix
                // codegen dropped the second arg on the floor and
                // every Node-style `fs.readdirSync(p, {withFileTypes:
                // true}).filter(e => e.isDirectory())` chain crashed
                // with `(string).isDirectory is not a function`.
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
                return Ok(nanbox_pointer_inline(blk, &raw_bits));
            }
            "statSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_stat_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                ));
            }
            "lstatSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_lstat_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                ));
            }
            "renameSync" if args.len() >= 2 => {
                let from = lower_expr(ctx, &args[0])?;
                let to = lower_expr(ctx, &args[1])?;
                ctx.block()
                    .call_void("js_fs_rename_sync", &[(DOUBLE, &from), (DOUBLE, &to)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "unlinkSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_unlink_sync", &[(DOUBLE, &p)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "mkdirSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_mkdir_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "rmSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_rm_recursive_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "rmdirSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_rmdir_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "copyFileSync" if args.len() >= 2 => {
                let src = lower_expr(ctx, &args[0])?;
                let dst = lower_expr(ctx, &args[1])?;
                let flags = if args.len() >= 3 {
                    lower_expr(ctx, &args[2])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_copy_file_sync_flags",
                    &[(DOUBLE, &src), (DOUBLE, &dst), (DOUBLE, &flags)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "cpSync" if args.len() >= 2 => {
                let src = lower_expr(ctx, &args[0])?;
                let dst = lower_expr(ctx, &args[1])?;
                let options = if args.len() >= 3 {
                    lower_expr(ctx, &args[2])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_fs_cp_sync_options",
                    &[(DOUBLE, &src), (DOUBLE, &dst), (DOUBLE, &options)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "chmodSync" if args.len() >= 2 => {
                let p = lower_expr(ctx, &args[0])?;
                let m = lower_expr(ctx, &args[1])?;
                ctx.block()
                    .call_void("js_fs_chmod_sync", &[(DOUBLE, &p), (DOUBLE, &m)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "truncateSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let len = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(0.0)
                };
                ctx.block()
                    .call_void("js_fs_truncate_sync", &[(DOUBLE, &p), (DOUBLE, &len)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "ftruncateSync" if !args.is_empty() => {
                let fd = lower_expr(ctx, &args[0])?;
                let len = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(0.0)
                };
                ctx.block()
                    .call_void("js_fs_ftruncate_sync", &[(DOUBLE, &fd), (DOUBLE, &len)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
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
                let fd = lower_expr(ctx, &args[0])?;
                let mode = lower_expr(ctx, &args[1])?;
                ctx.block()
                    .call_void("js_fs_fchmod_sync", &[(DOUBLE, &fd), (DOUBLE, &mode)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "fstatSync" if !args.is_empty() => {
                let fd = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_fstat_sync_options",
                    &[(DOUBLE, &fd), (DOUBLE, &options)],
                ));
            }
            "utimesSync" if args.len() >= 3 => {
                let p = lower_expr(ctx, &args[0])?;
                let atime = lower_expr(ctx, &args[1])?;
                let mtime = lower_expr(ctx, &args[2])?;
                ctx.block().call_void(
                    "js_fs_utimes_sync",
                    &[(DOUBLE, &p), (DOUBLE, &atime), (DOUBLE, &mtime)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "chownSync" if args.len() >= 3 => {
                let p = lower_expr(ctx, &args[0])?;
                let uid = lower_expr(ctx, &args[1])?;
                let gid = lower_expr(ctx, &args[2])?;
                ctx.block().call_void(
                    "js_fs_chown_sync",
                    &[(DOUBLE, &p), (DOUBLE, &uid), (DOUBLE, &gid)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "lchownSync" if args.len() >= 3 => {
                let p = lower_expr(ctx, &args[0])?;
                let uid = lower_expr(ctx, &args[1])?;
                let gid = lower_expr(ctx, &args[2])?;
                ctx.block().call_void(
                    "js_fs_lchown_sync",
                    &[(DOUBLE, &p), (DOUBLE, &uid), (DOUBLE, &gid)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "lchmodSync" if args.len() >= 2 => {
                let p = lower_expr(ctx, &args[0])?;
                let m = lower_expr(ctx, &args[1])?;
                ctx.block()
                    .call_void("js_fs_lchmod_sync", &[(DOUBLE, &p), (DOUBLE, &m)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "fchownSync" if args.len() >= 3 => {
                let fd = lower_expr(ctx, &args[0])?;
                let uid = lower_expr(ctx, &args[1])?;
                let gid = lower_expr(ctx, &args[2])?;
                ctx.block().call_void(
                    "js_fs_fchown_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &uid), (DOUBLE, &gid)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "lutimesSync" if args.len() >= 3 => {
                let p = lower_expr(ctx, &args[0])?;
                let atime = lower_expr(ctx, &args[1])?;
                let mtime = lower_expr(ctx, &args[2])?;
                ctx.block().call_void(
                    "js_fs_lutimes_sync",
                    &[(DOUBLE, &p), (DOUBLE, &atime), (DOUBLE, &mtime)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "futimesSync" if args.len() >= 3 => {
                let fd = lower_expr(ctx, &args[0])?;
                let atime = lower_expr(ctx, &args[1])?;
                let mtime = lower_expr(ctx, &args[2])?;
                ctx.block().call_void(
                    "js_fs_futimes_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &atime), (DOUBLE, &mtime)],
                );
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "_toUnixTimestamp" if !args.is_empty() => {
                let time = lower_expr(ctx, &args[0])?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_fs_to_unix_timestamp", &[(DOUBLE, &time)]));
            }
            "readvSync" if args.len() >= 2 => {
                let fd = lower_expr(ctx, &args[0])?;
                let bufs = lower_expr(ctx, &args[1])?;
                let pos = if args.len() >= 3 {
                    lower_expr(ctx, &args[2])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_readv_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &bufs), (DOUBLE, &pos)],
                ));
            }
            "writevSync" if args.len() >= 2 => {
                let fd = lower_expr(ctx, &args[0])?;
                let bufs = lower_expr(ctx, &args[1])?;
                let pos = if args.len() >= 3 {
                    lower_expr(ctx, &args[2])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_writev_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &bufs), (DOUBLE, &pos)],
                ));
            }
            "statfsSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_statfs_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                ));
            }
            "opendirSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_fs_opendir_sync", &[(DOUBLE, &p)]));
            }
            "globSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let raw = ctx.block().call(
                    DOUBLE,
                    "js_fs_glob_sync_options",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                );
                let raw_bits = ctx.block().bitcast_double_to_i64(&raw);
                return Ok(crate::expr::nanbox_pointer_inline(ctx.block(), &raw_bits));
            }
            "linkSync" if args.len() >= 2 => {
                let src = lower_expr(ctx, &args[0])?;
                let dst = lower_expr(ctx, &args[1])?;
                ctx.block()
                    .call_void("js_fs_link_sync", &[(DOUBLE, &src), (DOUBLE, &dst)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "symlinkSync" if args.len() >= 2 => {
                let target = lower_expr(ctx, &args[0])?;
                let path = lower_expr(ctx, &args[1])?;
                ctx.block()
                    .call_void("js_fs_symlink_sync", &[(DOUBLE, &target), (DOUBLE, &path)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "readlinkSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_readlink_dispatch",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                ));
            }
            "mkdtempDisposableSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let options = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_mkdtemp_disposable_sync",
                    &[(DOUBLE, &p), (DOUBLE, &options)],
                ));
            }
            "openSync" if !args.is_empty() => {
                let p = lower_expr(ctx, &args[0])?;
                let flags = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_open_sync",
                    &[(DOUBLE, &p), (DOUBLE, &flags)],
                ));
            }
            "closeSync" if !args.is_empty() => {
                let fd = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_fs_close_sync", &[(DOUBLE, &fd)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "readSync" if args.len() >= 5 => {
                let fd = lower_expr(ctx, &args[0])?;
                let buf = lower_expr(ctx, &args[1])?;
                let off = lower_expr(ctx, &args[2])?;
                let len = lower_expr(ctx, &args[3])?;
                let pos = lower_expr(ctx, &args[4])?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_read_sync",
                    &[
                        (DOUBLE, &fd),
                        (DOUBLE, &buf),
                        (DOUBLE, &off),
                        (DOUBLE, &len),
                        (DOUBLE, &pos),
                    ],
                ));
            }
            "readSync" if args.len() >= 3 => {
                let fd = lower_expr(ctx, &args[0])?;
                let buf = lower_expr(ctx, &args[1])?;
                let options = lower_expr(ctx, &args[2])?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_read_sync_options",
                    &[(DOUBLE, &fd), (DOUBLE, &buf), (DOUBLE, &options)],
                ));
            }
            "writeSync" if args.len() >= 5 => {
                let fd = lower_expr(ctx, &args[0])?;
                let buf = lower_expr(ctx, &args[1])?;
                let off = lower_expr(ctx, &args[2])?;
                let len = lower_expr(ctx, &args[3])?;
                let pos = lower_expr(ctx, &args[4])?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_write_buffer_sync",
                    &[
                        (DOUBLE, &fd),
                        (DOUBLE, &buf),
                        (DOUBLE, &off),
                        (DOUBLE, &len),
                        (DOUBLE, &pos),
                    ],
                ));
            }
            "writeSync" if args.len() >= 3 => {
                let fd = lower_expr(ctx, &args[0])?;
                let data = lower_expr(ctx, &args[1])?;
                let options = lower_expr(ctx, &args[2])?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_write_sync_options_dispatch",
                    &[(DOUBLE, &fd), (DOUBLE, &data), (DOUBLE, &options)],
                ));
            }
            "writeSync" if args.len() >= 2 => {
                let fd = lower_expr(ctx, &args[0])?;
                let data = lower_expr(ctx, &args[1])?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_fs_write_sync",
                    &[(DOUBLE, &fd), (DOUBLE, &data)],
                ));
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
