//! The existing submodule export declarations, shared with codegen.
//! Runtime callbacks consume the body metadata; compiler callbacks consume
//! only the declaration names. There is no second export inventory.
#[macro_export]
macro_rules! native_submodule_exports {
    ($consumer:ident) => {
        $consumer! {
        (SUBMOD_VM, "vm", "vm", [
            (js_node_export_vm_createContext, "createContext", JsFunctionInfo::of(thunk_vm_create_context as JsBody2<ClosureHeader>)
            .with_declared(2)),
        ]),
        (SUBMOD_TIMERS, "timers", "timers", [
            (js_node_export_timers_setTimeout, "setTimeout", JsFunctionInfo::of(timers_ns_set_timeout as JsBody3<ClosureHeader>).with_rest(2)),
            (js_node_export_timers_setInterval, "setInterval", JsFunctionInfo::of(timers_ns_set_interval as JsBody3<ClosureHeader>).with_rest(2)),
            (js_node_export_timers_setImmediate, "setImmediate", JsFunctionInfo::of(timers_ns_set_immediate as JsBody2<ClosureHeader>)
                .with_rest(1)),
            (js_node_export_timers_clearTimeout, "clearTimeout", JsFunctionInfo::of(timers_ns_clear_timeout as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_timers_clearInterval, "clearInterval", JsFunctionInfo::of(timers_ns_clear_interval as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_timers_clearImmediate, "clearImmediate", JsFunctionInfo::of(timers_ns_clear_immediate as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_TIMERS_PROMISES, "timers/promises", "timers_promises", [
            (js_node_export_timers_promises_setTimeout, "setTimeout", JsFunctionInfo::of(timers_promises_set_timeout as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_timers_promises_setImmediate, "setImmediate", JsFunctionInfo::of(timers_promises_set_immediate as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_timers_promises_setInterval, "setInterval", JsFunctionInfo::of(timers_promises_set_interval as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_timers_promises_scheduler, "scheduler", JsFunctionInfo::of(timers_promises_scheduler as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_FS_PROMISES, "fs/promises", "fs_promises", [
            (js_node_export_fs_promises_readFile, "readFile", JsFunctionInfo::of(thunk_fs_promises_readFile as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_open, "open", JsFunctionInfo::of(thunk_fs_promises_open as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_writeFile, "writeFile", JsFunctionInfo::of(thunk_fs_promises_writeFile as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_appendFile, "appendFile", JsFunctionInfo::of(thunk_fs_promises_appendFile as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_chmod, "chmod", JsFunctionInfo::of(thunk_fs_promises_chmod as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_chown, "chown", JsFunctionInfo::of(thunk_fs_promises_chown as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_lchown, "lchown", JsFunctionInfo::of(thunk_fs_promises_lchown as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_lchmod, "lchmod", JsFunctionInfo::of(thunk_fs_promises_lchmod as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_mkdir, "mkdir", JsFunctionInfo::of(thunk_fs_promises_mkdir as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_readdir, "readdir", JsFunctionInfo::of(thunk_fs_promises_readdir as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_stat, "stat", JsFunctionInfo::of(thunk_fs_promises_stat as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_statfs, "statfs", JsFunctionInfo::of(thunk_fs_promises_statfs as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_lstat, "lstat", JsFunctionInfo::of(thunk_fs_promises_lstat as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_rm, "rm", JsFunctionInfo::of(thunk_fs_promises_rm as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_rmdir, "rmdir", JsFunctionInfo::of(thunk_fs_promises_rmdir as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_unlink, "unlink", JsFunctionInfo::of(thunk_fs_promises_unlink as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_fs_promises_rename, "rename", JsFunctionInfo::of(thunk_fs_promises_rename as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_copyFile, "copyFile", JsFunctionInfo::of(thunk_fs_promises_copyFile as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_cp, "cp", JsFunctionInfo::of(thunk_fs_promises_cp as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_truncate, "truncate", JsFunctionInfo::of(thunk_fs_promises_truncate as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_utimes, "utimes", JsFunctionInfo::of(thunk_fs_promises_utimes as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_lutimes, "lutimes", JsFunctionInfo::of(thunk_fs_promises_lutimes as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_link, "link", JsFunctionInfo::of(thunk_fs_promises_link as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_symlink, "symlink", JsFunctionInfo::of(thunk_fs_promises_symlink as JsBody3<ClosureHeader>)
                .with_declared(3)),
            (js_node_export_fs_promises_readlink, "readlink", JsFunctionInfo::of(thunk_fs_promises_readlink as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_realpath, "realpath", JsFunctionInfo::of(thunk_fs_promises_realpath as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_mkdtemp, "mkdtemp", JsFunctionInfo::of(thunk_fs_promises_mkdtemp as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_mkdtempDisposable, "mkdtempDisposable", JsFunctionInfo::of(thunk_fs_promises_mkdtempDisposable as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_opendir, "opendir", JsFunctionInfo::of(thunk_fs_promises_opendir as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_fs_promises_glob, "glob", JsFunctionInfo::of(thunk_fs_promises_glob as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_watch, "watch", JsFunctionInfo::of(thunk_fs_promises_watch as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_access, "access", JsFunctionInfo::of(thunk_fs_promises_access as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_fs_promises_constants, "constants", JsFunctionInfo::of(thunk_fs_promises_constants as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_READLINE_PROMISES, "readline/promises", "readline_promises", [
            (js_node_export_readline_promises_createInterface, "createInterface", JsFunctionInfo::of(thunk_readline_createInterface as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_readline_promises_Interface, "Interface", JsFunctionInfo::of(thunk_readline_Interface as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_readline_promises_Readline, "Readline", JsFunctionInfo::of(thunk_readline_Readline as JsBody2<ClosureHeader>)
                .with_declared(2)),
        ]),
        (SUBMOD_STREAM_PROMISES, "stream/promises", "stream_promises", [
            (js_node_export_stream_promises_pipeline, "pipeline", JsFunctionInfo::of(thunk_streamP_pipeline as JsBody3<ClosureHeader>).with_rest(2)),
            (js_node_export_stream_promises_finished, "finished", JsFunctionInfo::of(thunk_streamP_finished as JsBody2<ClosureHeader>)
                .with_declared(2)),
        ]),
        (SUBMOD_STREAM_CONSUMERS, "stream/consumers", "stream_consumers", [
            (js_node_export_stream_consumers_text, "text", JsFunctionInfo::of(thunk_consumers_text as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_consumers_json, "json", JsFunctionInfo::of(thunk_consumers_json as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_consumers_buffer, "buffer", JsFunctionInfo::of(thunk_consumers_buffer as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_consumers_arrayBuffer, "arrayBuffer", JsFunctionInfo::of(thunk_consumers_arrayBuffer as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_consumers_bytes, "bytes", JsFunctionInfo::of(thunk_consumers_bytes as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_consumers_blob, "blob", JsFunctionInfo::of(thunk_consumers_blob as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_STREAM_WEB, "stream/web", "stream_web", [
            (js_node_export_stream_web_ReadableStream, "ReadableStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_ReadableStreamDefaultReader, "ReadableStreamDefaultReader", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_ReadableStreamBYOBReader, "ReadableStreamBYOBReader", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_ReadableStreamDefaultController, "ReadableStreamDefaultController", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_ReadableByteStreamController, "ReadableByteStreamController", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_ReadableStreamBYOBRequest, "ReadableStreamBYOBRequest", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_WritableStream, "WritableStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_WritableStreamDefaultWriter, "WritableStreamDefaultWriter", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_WritableStreamDefaultController, "WritableStreamDefaultController", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_TransformStream, "TransformStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_TransformStreamDefaultController, "TransformStreamDefaultController", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_ByteLengthQueuingStrategy, "ByteLengthQueuingStrategy", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_CountQueuingStrategy, "CountQueuingStrategy", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_TextEncoderStream, "TextEncoderStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_TextDecoderStream, "TextDecoderStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_CompressionStream, "CompressionStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_stream_web_DecompressionStream, "DecompressionStream", JsFunctionInfo::of(thunk_stream_web_ctor as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_HONO_JSX_SERVER, "hono/jsx", "hono_jsx_server", [
            (js_node_export_hono_jsx_server_jsx, "jsx", JsFunctionInfo::of(thunk_hono_jsx as JsBody2<ClosureHeader>).with_declared(2)),
            (js_node_export_hono_jsx_server_jsxs, "jsxs", JsFunctionInfo::of(thunk_hono_jsxs as JsBody2<ClosureHeader>).with_declared(2)),
            (js_node_export_hono_jsx_server_Fragment, "Fragment", JsFunctionInfo::of(thunk_hono_fragment as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_hono_jsx_server_JSXNode, "JSXNode", JsFunctionInfo::of(thunk_hono_jsxnode as JsBody1<ClosureHeader>).with_declared(1)),
        ]),
        (SUBMOD_HONO_JSX_STREAMING, "hono/jsx/streaming", "hono_jsx_streaming", [
            (js_node_export_hono_jsx_streaming_renderToReadableStream, "renderToReadableStream", JsFunctionInfo::of(
                thunk_hono_render_to_readable_stream as JsBody2<ClosureHeader>,
            )
            .with_declared(2)),
            (js_node_export_hono_jsx_streaming_Suspense, "Suspense", JsFunctionInfo::of(thunk_hono_suspense as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_SYS, "sys", "sys", [
            (js_node_export_sys_format, "format", JsFunctionInfo::of(thunk_sys_format as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_sys_inspect, "inspect", JsFunctionInfo::of(thunk_sys_inspect as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_sys_debuglog, "debuglog", JsFunctionInfo::of(thunk_sys_debuglog as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_sys_deprecate, "deprecate", JsFunctionInfo::of(thunk_sys_deprecate as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_sys_promisify, "promisify", JsFunctionInfo::of(thunk_sys_promisify as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_sys_callbackify, "callbackify", JsFunctionInfo::of(thunk_sys_callbackify as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_sys_isArray, "isArray", JsFunctionInfo::of(thunk_sys_isArray as JsBody1<ClosureHeader>).with_declared(1)),
        ]),
        (SUBMOD_DIAGNOSTICS_CHANNEL, "diagnostics_channel", "diagnostics_channel", [
            (js_node_export_diagnostics_channel_tracingChannel, "tracingChannel", JsFunctionInfo::of(thunk_diag_tracing_channel as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_diagnostics_channel_boundedChannel, "boundedChannel", JsFunctionInfo::of(thunk_diag_bounded_channel as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_diagnostics_channel_channel, "channel", JsFunctionInfo::of(thunk_diag_channel as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_diagnostics_channel_subscribe, "subscribe", JsFunctionInfo::of(thunk_diag_subscribe as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_diagnostics_channel_unsubscribe, "unsubscribe", JsFunctionInfo::of(thunk_diag_unsubscribe as JsBody2<ClosureHeader>)
                .with_declared(2)),
            (js_node_export_diagnostics_channel_hasSubscribers, "hasSubscribers", JsFunctionInfo::of(thunk_diag_has_subscribers as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_diagnostics_channel_Channel, "Channel", JsFunctionInfo::of(thunk_diag_noop as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_diagnostics_channel_BoundedChannel, "BoundedChannel", JsFunctionInfo::of(thunk_diag_bounded_channel as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        (SUBMOD_TRACE_EVENTS, "trace_events", "trace_events", [
            (js_node_export_trace_events_createTracing, "createTracing", JsFunctionInfo::of(thunk_trace_events_createTracing as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_trace_events_getEnabledCategories, "getEnabledCategories", JsFunctionInfo::of(
                thunk_trace_events_getEnabledCategories as JsBody1<ClosureHeader>,
            )
            .with_rest(0)),
        ]),
        (SUBMOD_TEST, "test", "test", [
            (js_node_export_test_default, "default", JsFunctionInfo::of(thunk_test as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_test, "test", JsFunctionInfo::of(thunk_test as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_skip, "skip", JsFunctionInfo::of(thunk_test_skip as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_todo, "todo", JsFunctionInfo::of(thunk_test_todo as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_only, "only", JsFunctionInfo::of(thunk_test_only as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_suite, "suite", JsFunctionInfo::of(thunk_test_suite as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_describe, "describe", JsFunctionInfo::of(thunk_test_suite as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_it, "it", JsFunctionInfo::of(thunk_test as JsBody3<ClosureHeader>).with_declared(3)),
            (js_node_export_test_before, "before", JsFunctionInfo::of(thunk_test_before as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_test_after, "after", JsFunctionInfo::of(thunk_test_after as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_test_beforeEach, "beforeEach", JsFunctionInfo::of(thunk_test_before_each as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_test_afterEach, "afterEach", JsFunctionInfo::of(thunk_test_after_each as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_test_run, "run", JsFunctionInfo::of(thunk_test_run as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_test_expectFailure, "expectFailure", JsFunctionInfo::of(thunk_test_expect_failure as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_test_mock, "mock", JsFunctionInfo::of(thunk_test_run as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_test_snapshot, "snapshot", JsFunctionInfo::of(thunk_test_run as JsBody1<ClosureHeader>).with_declared(1)),
        ]),
        (SUBMOD_TEST_REPORTERS, "test/reporters", "test_reporters", [
            (js_node_export_test_reporters_spec, "spec", JsFunctionInfo::of(thunk_reporter_spec as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_test_reporters_tap, "tap", JsFunctionInfo::of(thunk_reporter_tap as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_test_reporters_dot, "dot", JsFunctionInfo::of(thunk_reporter_dot as JsBody1<ClosureHeader>).with_declared(1)),
            (js_node_export_test_reporters_junit, "junit", JsFunctionInfo::of(thunk_reporter_junit as JsBody1<ClosureHeader>)
                .with_declared(1)),
            (js_node_export_test_reporters_lcov, "lcov", JsFunctionInfo::of(thunk_reporter_lcov as JsBody1<ClosureHeader>)
                .with_declared(1)),
        ]),
        }
    };
}
macro_rules! declaration_names {
    ($(($spec:ident, $module:literal, $key:literal,
        [$(($getter:ident, $name:literal, $info:expr)),* $(,)?])),* $(,)?) => {
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
