//! GENERATED (NM_DEVIRT_PLAN.md): native-module dispatch-install symbol selection.
//! Mirrors perry-runtime `nm_module_index`. `js_create_native_module_namespace`
//! sites emit the returned symbol so the per-module dispatch bucket is registered
//! before any method call; unimported modules are never named → dead-stripped.

/// Map a (possibly `node:`-prefixed) native module name to its dispatch-install
/// symbol, or `None` if the module has no method-dispatch bucket (its methods are
/// field-get callable-exports — dispatch returns undefined either way).
pub(crate) fn nm_install_symbol(name: &str) -> Option<&'static str> {
    let name = name.strip_prefix("node:").unwrap_or(name);
    match name {
        "assert" | "assert/strict" => Some("js_nm_install_assert"),
        "async_hooks" => Some("js_nm_install_async_hooks"),
        "bigint" => Some("js_nm_install_bigint"),
        "buffer" | "buffer.Buffer" => Some("js_nm_install_buffer"),
        "bun" => Some("js_nm_install_bun"),
        "child_process" => Some("js_nm_install_child_process"),
        "cluster" => Some("js_nm_install_cluster"),
        "console" => Some("js_nm_install_console"),
        "crypto" | "crypto.Certificate" | "crypto.KeyObject" | "crypto.subtle"
        | "crypto.webcrypto" => Some("js_nm_install_crypto"),
        "dgram" => Some("js_nm_install_dgram"),
        "dns" | "dns/promises" => Some("js_nm_install_dns"),
        "domain" => Some("js_nm_install_domain"),
        "events" => Some("js_nm_install_events"),
        "fs" => Some("js_nm_install_fs"),
        "http" | "http2" | "https" => Some("js_nm_install_http"),
        "inspector" | "inspector.Network" | "inspector/promises" => Some("js_nm_install_inspector"),
        "module" => Some("js_nm_install_module"),
        "net" => Some("js_nm_install_net"),
        // #6563: node-pty + the API-identical @lydell fork share one bucket.
        "node-pty" | "@lydell/node-pty" => Some("js_nm_install_node_pty"),
        "os" => Some("js_nm_install_os"),
        "path" | "path.posix" | "path.win32" => Some("js_nm_install_path"),
        "perf_histogram" | "perf_hooks" | "perf_observer" | "perf_observer_list" => {
            Some("js_nm_install_perf")
        }
        "process" => Some("js_nm_install_process"),
        "punycode" | "punycode.ucs2" | "punycode.default" => Some("js_nm_install_punycode"),
        "querystring" => Some("js_nm_install_querystring"),
        "readline" => Some("js_nm_install_readline"),
        "repl" => Some("js_nm_install_repl"),
        "sea" => Some("js_nm_install_sea"),
        "sqlite" => Some("js_nm_install_sqlite"),
        "stream" => Some("js_nm_install_stream"),
        "timers" => Some("js_nm_install_timers"),
        "tls" => Some("js_nm_install_tls"),
        "tty" => Some("js_nm_install_tty"),
        "url" => Some("js_nm_install_url"),
        "util" | "util.types" | "util/types" => Some("js_nm_install_util"),
        "v8"
        | "v8.Deserializer"
        | "v8.GCProfiler"
        | "v8.Serializer"
        | "v8.promiseHooks"
        | "v8.startupSnapshot"
        | "v8.DefaultSerializer"
        | "v8.DefaultDeserializer" => Some("js_nm_install_v8"),
        "vm" => Some("js_nm_install_vm"),
        "wasi" => Some("js_nm_install_wasi"),
        "zlib" => Some("js_nm_install_zlib"),
        _ => None,
    }
}

/// All dispatch-install symbols + the dynamic fallback — declared so codegen can
/// emit calls to them.
pub(crate) const NM_INSTALL_SYMBOLS: &[&str] = &[
    "js_nm_install_assert",
    "js_nm_install_async_hooks",
    "js_nm_install_bigint",
    "js_nm_install_buffer",
    "js_nm_install_bun",
    "js_nm_install_child_process",
    "js_nm_install_cluster",
    "js_nm_install_console",
    "js_nm_install_crypto",
    "js_nm_install_dgram",
    "js_nm_install_dns",
    "js_nm_install_domain",
    "js_nm_install_events",
    "js_nm_install_fs",
    "js_nm_install_http",
    "js_nm_install_inspector",
    "js_nm_install_module",
    "js_nm_install_net",
    "js_nm_install_node_pty",
    "js_nm_install_os",
    "js_nm_install_path",
    "js_nm_install_perf",
    "js_nm_install_process",
    "js_nm_install_punycode",
    "js_nm_install_querystring",
    "js_nm_install_readline",
    "js_nm_install_repl",
    "js_nm_install_sea",
    "js_nm_install_sqlite",
    "js_nm_install_stream",
    "js_nm_install_timers",
    "js_nm_install_tls",
    "js_nm_install_tty",
    "js_nm_install_url",
    "js_nm_install_util",
    "js_nm_install_v8",
    "js_nm_install_vm",
    "js_nm_install_wasi",
    "js_nm_install_zlib",
    "js_nm_install_all",
];

/// Submodule (`node:fs/promises`, `node:stream/web`, …) dispatch-install symbol
/// for a sentinel submodule key, or `None` if unknown. Mirrors perry-runtime
/// `submod_index`. Emitted at `js_node_submodule_namespace` sites so a submodule's
/// thunks (fs/promises etc.) are referenced only when it is imported.
pub(crate) fn nm_submod_install_symbol(key: &str) -> Option<&'static str> {
    match key {
        "vm" => Some("js_node_submod_install_vm"),
        "timers" => Some("js_node_submod_install_timers"),
        "timers_promises" => Some("js_node_submod_install_timers_promises"),
        "fs_promises" => Some("js_node_submod_install_fs_promises"),
        "readline_promises" => Some("js_node_submod_install_readline_promises"),
        "stream_promises" => Some("js_node_submod_install_stream_promises"),
        "stream_consumers" => Some("js_node_submod_install_stream_consumers"),
        "stream_web" => Some("js_node_submod_install_stream_web"),
        "hono_jsx_server" => Some("js_node_submod_install_hono_jsx_server"),
        "hono_jsx_streaming" => Some("js_node_submod_install_hono_jsx_streaming"),
        "sys" => Some("js_node_submod_install_sys"),
        "diagnostics_channel" => Some("js_node_submod_install_diagnostics_channel"),
        "trace_events" => Some("js_node_submod_install_trace_events"),
        "test" => Some("js_node_submod_install_test"),
        "test_reporters" => Some("js_node_submod_install_test_reporters"),
        _ => None,
    }
}

pub(crate) const NM_SUBMOD_INSTALL_SYMBOLS: &[&str] = &[
    "js_node_submod_install_vm",
    "js_node_submod_install_timers",
    "js_node_submod_install_timers_promises",
    "js_node_submod_install_fs_promises",
    "js_node_submod_install_readline_promises",
    "js_node_submod_install_stream_promises",
    "js_node_submod_install_stream_consumers",
    "js_node_submod_install_stream_web",
    "js_node_submod_install_hono_jsx_server",
    "js_node_submod_install_hono_jsx_streaming",
    "js_node_submod_install_sys",
    "js_node_submod_install_diagnostics_channel",
    "js_node_submod_install_trace_events",
    "js_node_submod_install_test",
    "js_node_submod_install_test_reporters",
    "js_node_submod_install_all",
    "js_node_submod_enable_install_all",
];
