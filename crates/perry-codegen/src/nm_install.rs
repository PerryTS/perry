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
        "assert" => Some("js_nm_install_assert"),
        "async_hooks" => Some("js_nm_install_async_hooks"),
        "bigint" => Some("js_nm_install_bigint"),
        "buffer" | "buffer.Buffer" => Some("js_nm_install_buffer"),
        "child_process" => Some("js_nm_install_child_process"),
        "cluster" => Some("js_nm_install_cluster"),
        "console" => Some("js_nm_install_console"),
        "crypto" | "crypto.Certificate" | "crypto.KeyObject" | "crypto.subtle" | "crypto.webcrypto" => Some("js_nm_install_crypto"),
        "dgram" => Some("js_nm_install_dgram"),
        "dns" | "dns/promises" => Some("js_nm_install_dns"),
        "domain" => Some("js_nm_install_domain"),
        "events" => Some("js_nm_install_events"),
        "fs" => Some("js_nm_install_fs"),
        "http" => Some("js_nm_install_http"),
        "inspector" | "inspector.Network" | "inspector/promises" => Some("js_nm_install_inspector"),
        "module" => Some("js_nm_install_module"),
        "net" => Some("js_nm_install_net"),
        "os" => Some("js_nm_install_os"),
        "path" | "path.posix" | "path.win32" => Some("js_nm_install_path"),
        "perf_histogram" | "perf_hooks" | "perf_observer" | "perf_observer_list" => Some("js_nm_install_perf"),
        "process" => Some("js_nm_install_process"),
        "punycode" | "punycode.ucs2" => Some("js_nm_install_punycode"),
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
        "v8" | "v8.Deserializer" | "v8.GCProfiler" | "v8.Serializer" | "v8.promiseHooks" | "v8.startupSnapshot" => Some("js_nm_install_v8"),
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
