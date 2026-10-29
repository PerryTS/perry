//! The existing submodule export declarations, shared with codegen.
//! Runtime callbacks consume the body metadata; compiler callbacks consume
//! only the declaration names. There is no second export inventory.
#[macro_export]
macro_rules! native_submodule_exports {
    ($consumer:ident) => {
        $consumer! {
        (SUBMOD_VM, "vm", "vm", [
            (js_node_export_vm_createContext, "createContext", unsafe { &*crate::fn_info!(thunk_vm_create_context, 2;with_declared(2)) }),
        ]),
        (SUBMOD_TIMERS, "timers", "timers", [
            (js_node_export_timers_setTimeout, "setTimeout", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_ns_set_timeout, 3;with_rest(2)); realm { "node:timers.default.setTimeout", provider = closure_alloc; } }),
            (js_node_export_timers_setInterval, "setInterval", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_ns_set_interval, 3;with_rest(2)); realm { "node:timers.default.setInterval", provider = closure_alloc; } }),
            (js_node_export_timers_setImmediate, "setImmediate", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_ns_set_immediate, 2;with_rest(1)); realm { "node:timers.default.setImmediate", provider = closure_alloc; } }),
            (js_node_export_timers_clearTimeout, "clearTimeout", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_ns_clear_timeout, 1;with_declared(1)); realm { "node:timers.default.clearTimeout", provider = closure_alloc; } }),
            (js_node_export_timers_clearInterval, "clearInterval", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_ns_clear_interval, 1;with_declared(1)); realm { "node:timers.default.clearInterval", provider = closure_alloc; } }),
            (js_node_export_timers_clearImmediate, "clearImmediate", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_ns_clear_immediate, 1;with_declared(1)); realm { "node:timers.default.clearImmediate", provider = closure_alloc; } }),
        ]),
        (SUBMOD_TIMERS_PROMISES, "timers/promises", "timers_promises", [
            (js_node_export_timers_promises_setTimeout, "setTimeout", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_promises_set_timeout, 3;with_declared(3)); realm { "node:timers/promises.setTimeout", provider = closure_alloc; } }),
            (js_node_export_timers_promises_setImmediate, "setImmediate", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_promises_set_immediate, 2;with_declared(2)); realm { "node:timers/promises.setImmediate", provider = closure_alloc; } }),
            (js_node_export_timers_promises_setInterval, "setInterval", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::timers::timers_promises_set_interval, 3;with_declared(3)); realm { "node:timers/promises.setInterval", provider = closure_alloc; } }),
            (js_node_export_timers_promises_scheduler, "scheduler", unsafe { &*crate::fn_info!(timers_promises_scheduler, 1;with_declared(1)) }),
        ]),
        (SUBMOD_FS_PROMISES, "fs/promises", "fs_promises", [
            (js_node_export_fs_promises_readFile, "readFile", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_readFile, 2;with_declared(2)); realm { "node:fs/promises.readFile", provider = closure_alloc; } }),
            (js_node_export_fs_promises_open, "open", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_open, 3;with_declared(3)); realm { "node:fs/promises.open", provider = closure_alloc; } }),
            (js_node_export_fs_promises_writeFile, "writeFile", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_writeFile, 3;with_declared(3)); realm { "node:fs/promises.writeFile", provider = closure_alloc; } }),
            (js_node_export_fs_promises_appendFile, "appendFile", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_appendFile, 3;with_declared(3)); realm { "node:fs/promises.appendFile", provider = closure_alloc; } }),
            (js_node_export_fs_promises_chmod, "chmod", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_chmod, 2;with_declared(2)); realm { "node:fs/promises.chmod", provider = closure_alloc; } }),
            (js_node_export_fs_promises_chown, "chown", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_chown, 3;with_declared(3)); realm { "node:fs/promises.chown", provider = closure_alloc; } }),
            (js_node_export_fs_promises_lchown, "lchown", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_lchown, 3;with_declared(3)); realm { "node:fs/promises.lchown", provider = closure_alloc; } }),
            (js_node_export_fs_promises_lchmod, "lchmod", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_lchmod, 2;with_declared(2)); realm { "node:fs/promises.lchmod", provider = closure_alloc; } }),
            (js_node_export_fs_promises_mkdir, "mkdir", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_mkdir, 2;with_declared(2)); realm { "node:fs/promises.mkdir", provider = closure_alloc; } }),
            (js_node_export_fs_promises_readdir, "readdir", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_readdir, 2;with_declared(2)); realm { "node:fs/promises.readdir", provider = closure_alloc; } }),
            (js_node_export_fs_promises_stat, "stat", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_stat, 2;with_declared(2)); realm { "node:fs/promises.stat", provider = closure_alloc; } }),
            (js_node_export_fs_promises_statfs, "statfs", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_statfs, 2;with_declared(2)); realm { "node:fs/promises.statfs", provider = closure_alloc; } }),
            (js_node_export_fs_promises_lstat, "lstat", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_lstat, 2;with_declared(2)); realm { "node:fs/promises.lstat", provider = closure_alloc; } }),
            (js_node_export_fs_promises_rm, "rm", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_rm, 2;with_declared(2)); realm { "node:fs/promises.rm", provider = closure_alloc; } }),
            (js_node_export_fs_promises_rmdir, "rmdir", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_rmdir, 2;with_declared(2)); realm { "node:fs/promises.rmdir", provider = closure_alloc; } }),
            (js_node_export_fs_promises_unlink, "unlink", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_unlink, 1;with_declared(1)); realm { "node:fs/promises.unlink", provider = closure_alloc; } }),
            (js_node_export_fs_promises_rename, "rename", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_rename, 2;with_declared(2)); realm { "node:fs/promises.rename", provider = closure_alloc; } }),
            (js_node_export_fs_promises_copyFile, "copyFile", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_copyFile, 3;with_declared(3)); realm { "node:fs/promises.copyFile", provider = closure_alloc; } }),
            (js_node_export_fs_promises_cp, "cp", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_cp, 3;with_declared(3)); realm { "node:fs/promises.cp", provider = closure_alloc; } }),
            (js_node_export_fs_promises_truncate, "truncate", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_truncate, 2;with_declared(2)); realm { "node:fs/promises.truncate", provider = closure_alloc; } }),
            (js_node_export_fs_promises_utimes, "utimes", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_utimes, 3;with_declared(3)); realm { "node:fs/promises.utimes", provider = closure_alloc; } }),
            (js_node_export_fs_promises_lutimes, "lutimes", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_lutimes, 3;with_declared(3)); realm { "node:fs/promises.lutimes", provider = closure_alloc; } }),
            (js_node_export_fs_promises_link, "link", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_link, 2;with_declared(2)); realm { "node:fs/promises.link", provider = closure_alloc; } }),
            (js_node_export_fs_promises_symlink, "symlink", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_symlink, 3;with_declared(3)); realm { "node:fs/promises.symlink", provider = closure_alloc; } }),
            (js_node_export_fs_promises_readlink, "readlink", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_readlink, 2;with_declared(2)); realm { "node:fs/promises.readlink", provider = closure_alloc; } }),
            (js_node_export_fs_promises_realpath, "realpath", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_realpath, 2;with_declared(2)); realm { "node:fs/promises.realpath", provider = closure_alloc; } }),
            (js_node_export_fs_promises_mkdtemp, "mkdtemp", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_mkdtemp, 2;with_declared(2)); realm { "node:fs/promises.mkdtemp", provider = closure_alloc; } }),
            (js_node_export_fs_promises_mkdtempDisposable, "mkdtempDisposable", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_mkdtempDisposable, 2;with_declared(2)); realm { "node:fs/promises.mkdtempDisposable", provider = closure_alloc; } }),
            (js_node_export_fs_promises_opendir, "opendir", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_opendir, 1;with_declared(1)); realm { "node:fs/promises.opendir", provider = closure_alloc; } }),
            (js_node_export_fs_promises_glob, "glob", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_glob, 2;with_declared(2)); realm { "node:fs/promises.glob", provider = closure_alloc; } }),
            (js_node_export_fs_promises_watch, "watch", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_watch, 2;with_declared(2)); realm { "node:fs/promises.watch", provider = closure_alloc; } }),
            (js_node_export_fs_promises_access, "access", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_fs_promises_access, 2;with_declared(2)); realm { "node:fs/promises.access", provider = closure_alloc; } }),
            (js_node_export_fs_promises_constants, "constants", unsafe { &*crate::fn_info!(thunk_fs_promises_constants, 1;with_declared(1)) }),
        ]),
        (SUBMOD_READLINE_PROMISES, "readline/promises", "readline_promises", [
            (js_node_export_readline_promises_createInterface, "createInterface", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_readline_createInterface, 1;with_declared(1)); realm { "node:readline/promises.createInterface", provider = closure_alloc; } }),
            (js_node_export_readline_promises_Interface, "Interface", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_readline_Interface, 1;with_declared(1)); realm { "node:readline/promises.Interface", kind = constructor, provider = closure, extends = "node:readline/promises.Interface.<prototype>", attrs = {prototype = writable}; } }),
            (js_node_export_readline_promises_Readline, "Readline", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::fs_promises::thunk_readline_Readline, 2;with_declared(2)); realm { "node:readline/promises.Readline", provider = closure_alloc; } }),
        ]),
        (SUBMOD_STREAM_PROMISES, "stream/promises", "stream_promises", [
            (js_node_export_stream_promises_pipeline, "pipeline", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::stream_promises::thunk_streamP_pipeline, 3;with_rest(2)); realm { "node:stream/promises.pipeline", provider = closure_alloc; } }),
            (js_node_export_stream_promises_finished, "finished", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::stream_promises::thunk_streamP_finished, 2;with_declared(2)); realm { "node:stream/promises.finished", provider = closure_alloc; } }),
        ]),
        (SUBMOD_STREAM_CONSUMERS, "stream/consumers", "stream_consumers", [
            (js_node_export_stream_consumers_text, "text", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::consumers::thunk_consumers_text, 1;with_declared(1)); realm { "node:stream/consumers.text", provider = closure_alloc; } }),
            (js_node_export_stream_consumers_json, "json", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::consumers::thunk_consumers_json, 1;with_declared(1)); realm { "node:stream/consumers.json", provider = closure_alloc; } }),
            (js_node_export_stream_consumers_buffer, "buffer", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::consumers::thunk_consumers_buffer, 1;with_declared(1)); realm { "node:stream/consumers.buffer", provider = closure_alloc; } }),
            (js_node_export_stream_consumers_arrayBuffer, "arrayBuffer", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::consumers::thunk_consumers_arrayBuffer, 1;with_declared(1)); realm { "node:stream/consumers.arrayBuffer", provider = closure_alloc; } }),
            (js_node_export_stream_consumers_bytes, "bytes", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::consumers::thunk_consumers_bytes, 1;with_declared(1)); realm { "node:stream/consumers.bytes", provider = closure_alloc; } }),
            (js_node_export_stream_consumers_blob, "blob", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::consumers::thunk_consumers_blob, 1;with_declared(1)); realm { "node:stream/consumers.blob", provider = closure_alloc; } }),
        ]),
        (SUBMOD_STREAM_WEB, "stream/web", "stream_web", [
            (js_node_export_stream_web_ReadableStream, "ReadableStream", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::thunk_stream_web_ctor, 1;with_declared(1)); realm { "node:stream/web.ByteLengthQueuingStrategy", provider = closure_alloc; "node:stream/web.CountQueuingStrategy", provider = closure_alloc; "node:stream/web.ReadableByteStreamController", provider = closure_alloc; "node:stream/web.ReadableStreamBYOBReader", provider = closure_alloc; "node:stream/web.ReadableStreamBYOBRequest", provider = closure_alloc; "node:stream/web.ReadableStreamDefaultController", provider = closure_alloc; "node:stream/web.ReadableStreamDefaultReader", provider = closure_alloc; "node:stream/web.TransformStreamDefaultController", provider = closure_alloc; "node:stream/web.WritableStreamDefaultController", provider = closure_alloc; "node:stream/web.WritableStreamDefaultWriter", provider = closure_alloc; } }),
            (js_node_export_stream_web_ReadableStreamDefaultReader, "ReadableStreamDefaultReader", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_ReadableStreamBYOBReader, "ReadableStreamBYOBReader", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_ReadableStreamDefaultController, "ReadableStreamDefaultController", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_ReadableByteStreamController, "ReadableByteStreamController", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_ReadableStreamBYOBRequest, "ReadableStreamBYOBRequest", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_WritableStream, "WritableStream", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_WritableStreamDefaultWriter, "WritableStreamDefaultWriter", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_WritableStreamDefaultController, "WritableStreamDefaultController", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_TransformStream, "TransformStream", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_TransformStreamDefaultController, "TransformStreamDefaultController", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_ByteLengthQueuingStrategy, "ByteLengthQueuingStrategy", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_CountQueuingStrategy, "CountQueuingStrategy", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_TextEncoderStream, "TextEncoderStream", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_TextDecoderStream, "TextDecoderStream", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_CompressionStream, "CompressionStream", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
            (js_node_export_stream_web_DecompressionStream, "DecompressionStream", crate::fn_info!(reference import crate::node_submodules::thunk_stream_web_ctor, 1; with_declared(1))),
        ]),
        (SUBMOD_HONO_JSX_SERVER, "hono/jsx", "hono_jsx_server", [
            (js_node_export_hono_jsx_server_jsx, "jsx", unsafe { &*crate::fn_info!(thunk_hono_jsx, 2;with_declared(2)) }),
            (js_node_export_hono_jsx_server_jsxs, "jsxs", unsafe { &*crate::fn_info!(thunk_hono_jsxs, 2;with_declared(2)) }),
            (js_node_export_hono_jsx_server_Fragment, "Fragment", unsafe { &*crate::fn_info!(thunk_hono_fragment, 1;with_declared(1)) }),
            (js_node_export_hono_jsx_server_JSXNode, "JSXNode", unsafe { &*crate::fn_info!(thunk_hono_jsxnode, 1;with_declared(1)) }),
        ]),
        (SUBMOD_HONO_JSX_STREAMING, "hono/jsx/streaming", "hono_jsx_streaming", [
            (js_node_export_hono_jsx_streaming_renderToReadableStream, "renderToReadableStream", unsafe {
                &*crate::fn_info!(thunk_hono_render_to_readable_stream, 2;with_declared(2))
            }),
            (js_node_export_hono_jsx_streaming_Suspense, "Suspense", unsafe { &*crate::fn_info!(thunk_hono_suspense, 1;with_declared(1)) }),
        ]),
        (SUBMOD_SYS, "sys", "sys", [
            (js_node_export_sys_format, "format", unsafe { &*crate::fn_info!(thunk_sys_format, 1;with_declared(1)) }),
            (js_node_export_sys_inspect, "inspect", unsafe { &*crate::fn_info!(thunk_sys_inspect, 1;with_declared(1)) }),
            (js_node_export_sys_debuglog, "debuglog", unsafe { &*crate::fn_info!(thunk_sys_debuglog, 1;with_declared(1)) }),
            (js_node_export_sys_deprecate, "deprecate", unsafe { &*crate::fn_info!(thunk_sys_deprecate, 1;with_declared(1)) }),
            (js_node_export_sys_promisify, "promisify", unsafe { &*crate::fn_info!(thunk_sys_promisify, 1;with_declared(1)) }),
            (js_node_export_sys_callbackify, "callbackify", unsafe { &*crate::fn_info!(thunk_sys_callbackify, 1;with_declared(1)) }),
            (js_node_export_sys_isArray, "isArray", unsafe { &*crate::fn_info!(thunk_sys_isArray, 1;with_declared(1)) }),
        ]),
        (SUBMOD_DIAGNOSTICS_CHANNEL, "diagnostics_channel", "diagnostics_channel", [
            (js_node_export_diagnostics_channel_tracingChannel, "tracingChannel", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics::thunk_diag_tracing_channel, 1;with_declared(1)); realm { "node:diagnostics_channel.tracingChannel", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_boundedChannel, "boundedChannel", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics_tail::thunk_diag_bounded_channel, 1;with_declared(1)); realm { "node:diagnostics_channel.BoundedChannel", provider = closure_alloc; "node:diagnostics_channel.boundedChannel", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_channel, "channel", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics::thunk_diag_channel, 1;with_declared(1)); realm { "node:diagnostics_channel.channel", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_subscribe, "subscribe", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics::thunk_diag_subscribe, 2;with_declared(2)); realm { "node:diagnostics_channel.subscribe", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_unsubscribe, "unsubscribe", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics::thunk_diag_unsubscribe, 2;with_declared(2)); realm { "node:diagnostics_channel.unsubscribe", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_hasSubscribers, "hasSubscribers", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics::thunk_diag_has_subscribers, 1;with_declared(1)); realm { "node:diagnostics_channel.hasSubscribers", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_Channel, "Channel", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::diagnostics::thunk_diag_noop, 1;with_declared(1)); realm { "node:diagnostics_channel.Channel", provider = closure_alloc; } }),
            (js_node_export_diagnostics_channel_BoundedChannel, "BoundedChannel", crate::fn_info!(reference import crate::node_submodules::diagnostics_tail::thunk_diag_bounded_channel, 1; with_declared(1))),
        ]),
        (SUBMOD_TRACE_EVENTS, "trace_events", "trace_events", [
            (js_node_export_trace_events_createTracing, "createTracing", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::trace_events::thunk_trace_events_createTracing, 1;with_declared(1)); realm { "node:trace_events.default.createTracing"; } }),
            (js_node_export_trace_events_getEnabledCategories, "getEnabledCategories", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::trace_events::thunk_trace_events_getEnabledCategories, 1;with_rest(0)); realm { "node:trace_events.default.getEnabledCategories"; } }),
        ]),
        (SUBMOD_TEST, "test", "test", [
            (js_node_export_test_default, "default", crate::node_submodules::IMPORT_THUNK_TEST),
            (js_node_export_test_test, "test", crate::node_submodules::IMPORT_THUNK_TEST),
            (js_node_export_test_skip, "skip", crate::node_submodules::IMPORT_THUNK_TEST_SKIP),
            (js_node_export_test_todo, "todo", crate::node_submodules::IMPORT_THUNK_TEST_TODO),
            (js_node_export_test_only, "only", crate::node_submodules::IMPORT_THUNK_TEST_ONLY),
            (js_node_export_test_suite, "suite", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_suite, 3;with_declared(3)); realm { "node:test.describe", provider = closure, members = { skip, attrs = writable | enumerable | configurable; todo, attrs = writable | enumerable | configurable; only, attrs = writable | enumerable | configurable; }; } }),
            (js_node_export_test_describe, "describe", crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test_suite, 3; with_declared(3))),
            (js_node_export_test_it, "it", crate::node_submodules::IMPORT_THUNK_TEST),
            (js_node_export_test_before, "before", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_before, 1;with_declared(1)); realm { "node:test.before", provider = closure_alloc; } }),
            (js_node_export_test_after, "after", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_after, 1;with_declared(1)); realm { "node:test.after", provider = closure_alloc; } }),
            (js_node_export_test_beforeEach, "beforeEach", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_before_each, 1;with_declared(1)); realm { "node:test.beforeEach", provider = closure_alloc; } }),
            (js_node_export_test_afterEach, "afterEach", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_after_each, 1;with_declared(1)); realm { "node:test.afterEach", provider = closure_alloc; } }),
            (js_node_export_test_run, "run", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_run, 1;with_declared(1)); realm { "node:test.run", provider = closure_alloc; } }),
            (js_node_export_test_expectFailure, "expectFailure", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::runner::thunk_test_expect_failure, 1;with_declared(1)); realm { "node:test.expectFailure", provider = closure_alloc; } }),
            (js_node_export_test_mock, "mock", crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test_run, 1; with_declared(1))),
            (js_node_export_test_snapshot, "snapshot", crate::fn_info!(reference import crate::node_submodules::test::runner::thunk_test_run, 1; with_declared(1))),
        ]),
        (SUBMOD_TEST_REPORTERS, "test/reporters", "test_reporters", [
            (js_node_export_test_reporters_spec, "spec", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::reporters::thunk_reporter_spec, 1;with_declared(1)); realm { "node:test/reporters.default.spec", provider = closure_alloc; } }),
            (js_node_export_test_reporters_tap, "tap", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::reporters::thunk_reporter_tap, 1;with_declared(1)); realm { "node:test/reporters.default.tap", provider = closure_alloc; } }),
            (js_node_export_test_reporters_dot, "dot", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::reporters::thunk_reporter_dot, 1;with_declared(1)); realm { "node:test/reporters.default.dot", provider = closure_alloc; } }),
            (js_node_export_test_reporters_junit, "junit", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::reporters::thunk_reporter_junit, 1;with_declared(1)); realm { "node:test/reporters.default.junit", provider = closure_alloc; } }),
            (js_node_export_test_reporters_lcov, "lcov", crate::realm_decl! { info_expr crate::fn_info!(reference export crate::node_submodules::test::reporters::thunk_reporter_lcov, 1;with_declared(1)); realm { "node:test/reporters.default.lcov", provider = closure_alloc; } }),
        ]),
        }
    };
}
macro_rules! declaration_names {
    ($(($spec:ident, $module:literal, $key:literal,
        [$(($getter:ident, $name:literal, $($info:tt)*)),* $(,)?])),* $(,)?) => {
        /// Public and internal export declarations with their linker symbols.
        pub const SUBMODULE_EXPORTS: &[(&str, &str, &[(&str, &str)])] = &[
            $(($module, $key, &[$(($name, stringify!($getter))),*])),*
        ];
    };
}
native_submodule_exports!(declaration_names);

pub fn export_value_symbol(module: &str, property: &str) -> Option<&'static str> {
    let module = module.strip_prefix("node:").unwrap_or(module);
    let (_, _, exports) = SUBMODULE_EXPORTS
        .iter()
        .find(|(name, _, _)| *name == module)?;
    exports
        .iter()
        .find(|(name, _)| *name == property)
        .map(|(_, symbol)| *symbol)
}
