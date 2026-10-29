//! Ordinary process metadata; whole existing value constructors.
use super::*;
use crate::value::JSValue;

pub(crate) fn process_release_value() -> f64 {
    crate::realm_decl! { record obj, RECORD_Q457_0; realm { process.release, kind = namespace, members = { name = "node", attrs = writable | enumerable | configurable; sourceUrl = "", attrs = writable | enumerable | configurable; headersUrl = "", attrs = writable | enumerable | configurable; }; "node:process.release", kind = namespace, members = { name = "node", attrs = writable | enumerable | configurable; sourceUrl = "", attrs = writable | enumerable | configurable; headersUrl = "", attrs = writable | enumerable | configurable; }; } };
    obj.set(
        0,
        JSValue::from_bits((module_string_value("node")).to_bits()),
    );
    obj.set(1, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set(2, JSValue::from_bits((module_string_value("")).to_bits()));
    #[cfg(windows)]
    obj.set(3, JSValue::from_bits((module_string_value("")).to_bits()));
    module_object_value(obj.finish())
}

pub(crate) fn process_features_value() -> f64 {
    crate::realm_decl! { record obj, RECORD_Q458_0; realm { process.features, kind = namespace, members = { inspector = false, attrs = writable | enumerable | configurable; debug = false, attrs = writable | enumerable | configurable; uv = true, attrs = writable | enumerable | configurable; ipv6 = true, attrs = writable | enumerable | configurable; tls_alpn = true, attrs = writable | enumerable | configurable; tls_sni = true, attrs = writable | enumerable | configurable; tls_ocsp = true, attrs = writable | enumerable | configurable; tls = true, attrs = writable | enumerable | configurable; openssl_is_boringssl = false, attrs = writable | enumerable | configurable; cached_builtins = false, attrs = writable | enumerable | configurable; require_module = false, attrs = writable | enumerable | configurable; quic = false, attrs = writable | enumerable | configurable; typescript = "transform", attrs = writable | enumerable | configurable; }; "node:process.features", kind = namespace, members = { inspector = false, attrs = writable | enumerable | configurable; debug = false, attrs = writable | enumerable | configurable; uv = true, attrs = writable | enumerable | configurable; ipv6 = true, attrs = writable | enumerable | configurable; tls_alpn = true, attrs = writable | enumerable | configurable; tls_sni = true, attrs = writable | enumerable | configurable; tls_ocsp = true, attrs = writable | enumerable | configurable; tls = true, attrs = writable | enumerable | configurable; openssl_is_boringssl = false, attrs = writable | enumerable | configurable; cached_builtins = false, attrs = writable | enumerable | configurable; require_module = false, attrs = writable | enumerable | configurable; quic = false, attrs = writable | enumerable | configurable; typescript = "transform", attrs = writable | enumerable | configurable; }; } };
    obj.set(0, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(1, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(2, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(3, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(4, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(5, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(6, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(7, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(8, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(9, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(10, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(11, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(
        12,
        JSValue::from_bits((module_string_value("transform")).to_bits()),
    );
    module_object_value(obj.finish())
}

pub(crate) fn process_config_value() -> f64 {
    crate::realm_decl! { record config, RECORD_Q468_0; realm { process.config, kind = namespace, members = { variables, attrs = writable | enumerable | configurable; target_defaults, attrs = writable | enumerable | configurable; }; "node:process.config", kind = namespace, members = { variables, attrs = writable | enumerable | configurable; target_defaults, attrs = writable | enumerable | configurable; }; } };
    crate::realm_decl! { record variables, RECORD_Q468_1; realm { process.config.variables, kind = namespace, members = { target_arch = "x64", attrs = writable | enumerable | configurable; host_arch = "x64", attrs = writable | enumerable | configurable; node_module_version = 141.0, attrs = writable | enumerable | configurable; node_shared_openssl = false, attrs = writable | enumerable | configurable; node_use_openssl = true, attrs = writable | enumerable | configurable; node_use_node_code_cache = false, attrs = writable | enumerable | configurable; node_use_node_snapshot = false, attrs = writable | enumerable | configurable; v8_enable_i18n_support = 1.0, attrs = writable | enumerable | configurable; v8_enable_pointer_compression = 0.0, attrs = writable | enumerable | configurable; uv_parent_path = "", attrs = writable | enumerable | configurable; }; "node:process.config.variables", kind = namespace, members = { target_arch = "x64", attrs = writable | enumerable | configurable; host_arch = "x64", attrs = writable | enumerable | configurable; node_module_version = 141.0, attrs = writable | enumerable | configurable; node_shared_openssl = false, attrs = writable | enumerable | configurable; node_use_openssl = true, attrs = writable | enumerable | configurable; node_use_node_code_cache = false, attrs = writable | enumerable | configurable; node_use_node_snapshot = false, attrs = writable | enumerable | configurable; v8_enable_i18n_support = 1.0, attrs = writable | enumerable | configurable; v8_enable_pointer_compression = 0.0, attrs = writable | enumerable | configurable; uv_parent_path = "", attrs = writable | enumerable | configurable; }; } };
    crate::realm_decl! { record target_defaults, RECORD_Q468_2; realm { process.config.target_defaults, kind = namespace, members = { cflags, attrs = writable | enumerable | configurable; conditions, attrs = writable | enumerable | configurable; defines, attrs = writable | enumerable | configurable; include_dirs, attrs = writable | enumerable | configurable; libraries, attrs = writable | enumerable | configurable; default_configuration = "Release", attrs = writable | enumerable | configurable; configurations, attrs = writable | enumerable | configurable; }; "node:process.config.target_defaults", kind = namespace, members = { cflags, attrs = writable | enumerable | configurable; conditions, attrs = writable | enumerable | configurable; defines, attrs = writable | enumerable | configurable; include_dirs, attrs = writable | enumerable | configurable; libraries, attrs = writable | enumerable | configurable; default_configuration = "Release", attrs = writable | enumerable | configurable; configurations, attrs = writable | enumerable | configurable; }; } };
    crate::realm_decl! { record configurations, RECORD_Q468_3; realm { process.config.target_defaults.configurations, kind = namespace, members = { Release, attrs = writable | enumerable | configurable; }; "node:process.config.target_defaults.configurations", kind = namespace, members = { Release, attrs = writable | enumerable | configurable; }; } };

    variables.set(
        0,
        JSValue::from_bits((module_string_value(node_arch_name())).to_bits()),
    );
    variables.set(
        1,
        JSValue::from_bits((module_string_value(node_arch_name())).to_bits()),
    );
    variables.set(2, JSValue::from_bits((141.0f64).to_bits()));
    variables.set(3, JSValue::from_bits((bool_value(false)).to_bits()));
    variables.set(4, JSValue::from_bits((bool_value(true)).to_bits()));
    variables.set(5, JSValue::from_bits((bool_value(false)).to_bits()));
    variables.set(6, JSValue::from_bits((bool_value(false)).to_bits()));
    variables.set(7, JSValue::from_bits((1.0f64).to_bits()));
    variables.set(8, JSValue::from_bits((0.0f64).to_bits()));
    variables.set(9, JSValue::from_bits((module_string_value("")).to_bits()));

    target_defaults.set(0, JSValue::from_bits((crate::realm_decl! { birth module_array_value(&[]); realm { process.config.target_defaults.cflags; "node:process.config.target_defaults.cflags"; } }).to_bits()));
    target_defaults.set(1, JSValue::from_bits((crate::realm_decl! { birth module_array_value(&[]); realm { process.config.target_defaults.conditions; "node:process.config.target_defaults.conditions"; } }).to_bits()));
    target_defaults.set(2, JSValue::from_bits((crate::realm_decl! { birth module_array_value(&[]); realm { process.config.target_defaults.defines; "node:process.config.target_defaults.defines"; } }).to_bits()));
    target_defaults.set(3, JSValue::from_bits((crate::realm_decl! { birth module_array_value(&[]); realm { process.config.target_defaults.include_dirs; "node:process.config.target_defaults.include_dirs"; } }).to_bits()));
    target_defaults.set(4, JSValue::from_bits((crate::realm_decl! { birth module_array_value(&[]); realm { process.config.target_defaults.libraries; "node:process.config.target_defaults.libraries"; } }).to_bits()));
    target_defaults.set(
        5,
        JSValue::from_bits((module_string_value("Release")).to_bits()),
    );
    configurations.set(
        0,
        JSValue::from_bits(
            (module_object_value(unsafe {
                crate::realm_decl! { birth crate::object::runtime_layouts::perry_build_runtime_record(
                    std::ptr::addr_of!(crate::object::runtime_layouts::RECORD_PCHECKED031),
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                ); realm { process.config.target_defaults.configurations.Release, kind = namespace, members = { }; "node:process.config.target_defaults.configurations.Release", kind = namespace, members = { }; } }
            }))
            .to_bits(),
        ),
    );
    target_defaults.set(
        6,
        JSValue::from_bits((module_object_value(configurations.finish())).to_bits()),
    );

    config.set(
        0,
        JSValue::from_bits((module_object_value(variables.finish())).to_bits()),
    );
    config.set(
        1,
        JSValue::from_bits((module_object_value(target_defaults.finish())).to_bits()),
    );
    module_object_value(config.finish())
}

pub(crate) fn process_allowed_flags_value() -> f64 {
    const FLAGS: &[&str] = &[
        "--abort-on-uncaught-exception",
        "--addons",
        "--allow-addons",
        "--allow-child-process",
        "--allow-fs-read",
        "--allow-fs-write",
        "--allow-inspector",
        "--allow-net",
        "--allow-wasi",
        "--allow-worker",
        "--async-context-frame",
        "--conditions",
        "--cpu-prof",
        "--cpu-prof-dir",
        "--cpu-prof-interval",
        "--cpu-prof-name",
        "--debug-arraybuffer-allocations",
        "--debug-port",
        "--deprecation",
        "--diagnostic-dir",
        "--disable-proto",
        "--disable-sigusr1",
        "--disable-warning",
        "--disable-wasm-trap-handler",
        "--disallow-code-generation-from-strings",
        "--dns-result-order",
        "--enable-etw-stack-walking",
        "--enable-fips",
        "--enable-network-family-autoselection",
        "--enable-source-maps",
        "--entry-url",
        "--es-module-specifier-resolution",
        "--experimental-abortcontroller",
        "--experimental-addon-modules",
        "--experimental-detect-module",
        "--experimental-eventsource",
        "--experimental-fetch",
        "--experimental-global-customevent",
        "--experimental-global-navigator",
        "--experimental-global-webcrypto",
        "--experimental-import-meta-resolve",
        "--experimental-json-modules",
        "--experimental-loader",
        "--experimental-modules",
        "--experimental-print-required-tla",
        "--experimental-quic",
        "--experimental-repl-await",
        "--experimental-report",
        "--experimental-require-module",
        "--experimental-shadow-realm",
        "--experimental-specifier-resolution",
        "--experimental-sqlite",
        "--experimental-strip-types",
        "--experimental-test-isolation",
        "--experimental-top-level-await",
        "--experimental-transform-types",
        "--experimental-vm-modules",
        "--experimental-wasi-unstable-preview1",
        "--experimental-wasm-modules",
        "--experimental-websocket",
        "--experimental-webstorage",
        "--experimental-worker",
        "--expose-gc",
        "--extra-info-on-fatal-exception",
        "--force-async-hooks-checks",
        "--force-context-aware",
        "--force-fips",
        "--force-node-api-uncaught-exceptions-policy",
        "--frozen-intrinsics",
        "--global-search-paths",
        "--heap-prof",
        "--heap-prof-dir",
        "--heap-prof-interval",
        "--heap-prof-name",
        "--heapsnapshot-near-heap-limit",
        "--heapsnapshot-signal",
        "--http-parser",
        "--icu-data-dir",
        "--import",
        "--input-type",
        "--insecure-http-parser",
        "--inspect",
        "--inspect-brk",
        "--inspect-port",
        "--inspect-publish-uid",
        "--inspect-wait",
        "--interpreted-frames-native-stack",
        "--jitless",
        "--loader",
        "--localstorage-file",
        "--max-http-header-size",
        "--max-old-space-size",
        "--max-old-space-size-percentage",
        "--max-semi-space-size",
        "--napi-modules",
        "--network-family-autoselection",
        "--network-family-autoselection-attempt-timeout",
        "--no-addons",
        "--no-allow-addons",
        "--no-allow-child-process",
        "--no-allow-inspector",
        "--no-allow-net",
        "--no-allow-wasi",
        "--no-allow-worker",
        "--no-async-context-frame",
        "--no-cpu-prof",
        "--no-debug-arraybuffer-allocations",
        "--no-deprecation",
        "--no-disable-sigusr1",
        "--no-disable-wasm-trap-handler",
        "--no-enable-fips",
        "--no-enable-source-maps",
        "--no-entry-url",
        "--no-experimental-addon-modules",
        "--no-experimental-detect-module",
        "--no-experimental-eventsource",
        "--no-experimental-global-navigator",
        "--no-experimental-import-meta-resolve",
        "--no-experimental-print-required-tla",
        "--no-experimental-repl-await",
        "--no-experimental-require-module",
        "--no-experimental-shadow-realm",
        "--no-experimental-sqlite",
        "--no-experimental-transform-types",
        "--no-experimental-vm-modules",
        "--no-experimental-websocket",
        "--no-experimental-webstorage",
        "--no-extra-info-on-fatal-exception",
        "--no-force-async-hooks-checks",
        "--no-force-context-aware",
        "--no-force-fips",
        "--no-force-node-api-uncaught-exceptions-policy",
        "--no-frozen-intrinsics",
        "--no-global-search-paths",
        "--no-heap-prof",
        "--no-insecure-http-parser",
        "--no-inspect",
        "--no-inspect-brk",
        "--no-inspect-wait",
        "--no-network-family-autoselection",
        "--no-node-snapshot",
        "--no-openssl-legacy-provider",
        "--no-openssl-shared-config",
        "--no-pending-deprecation",
        "--no-permission",
        "--no-permission-audit",
        "--no-preserve-symlinks",
        "--no-preserve-symlinks-main",
        "--no-report-compact",
        "--no-report-exclude-env",
        "--no-report-exclude-network",
        "--no-report-on-fatalerror",
        "--no-report-on-signal",
        "--no-report-uncaught-exception",
        "--no-require-module",
        "--no-strip-types",
        "--no-test-only",
        "--no-throw-deprecation",
        "--no-tls-max-v1.2",
        "--no-tls-max-v1.3",
        "--no-tls-min-v1.0",
        "--no-tls-min-v1.1",
        "--no-tls-min-v1.2",
        "--no-tls-min-v1.3",
        "--no-trace-deprecation",
        "--no-trace-env",
        "--no-trace-env-js-stack",
        "--no-trace-env-native-stack",
        "--no-trace-exit",
        "--no-trace-promises",
        "--no-trace-sigint",
        "--no-trace-sync-io",
        "--no-trace-tls",
        "--no-trace-uncaught",
        "--no-trace-warnings",
        "--no-track-heap-objects",
        "--no-use-bundled-ca",
        "--no-use-env-proxy",
        "--no-use-openssl-ca",
        "--no-use-system-ca",
        "--no-verify-base-objects",
        "--no-warnings",
        "--no-watch",
        "--no-watch-preserve-output",
        "--no-zero-fill-buffers",
        "--node-memory-debug",
        "--node-snapshot",
        "--openssl-config",
        "--openssl-legacy-provider",
        "--openssl-shared-config",
        "--pending-deprecation",
        "--perf-basic-prof",
        "--perf-basic-prof-only-functions",
        "--perf-prof",
        "--perf-prof-unwinding-info",
        "--permission",
        "--permission-audit",
        "--preserve-symlinks",
        "--preserve-symlinks-main",
        "--prof-process",
        "--redirect-warnings",
        "--report-compact",
        "--report-dir",
        "--report-directory",
        "--report-exclude-env",
        "--report-exclude-network",
        "--report-filename",
        "--report-on-fatalerror",
        "--report-on-signal",
        "--report-signal",
        "--report-uncaught-exception",
        "--require",
        "--require-module",
        "--secure-heap",
        "--secure-heap-min",
        "--snapshot-blob",
        "--stack-trace-limit",
        "--strip-types",
        "--test-coverage-branches",
        "--test-coverage-exclude",
        "--test-coverage-functions",
        "--test-coverage-include",
        "--test-coverage-lines",
        "--test-global-setup",
        "--test-isolation",
        "--test-name-pattern",
        "--test-only",
        "--test-reporter",
        "--test-reporter-destination",
        "--test-rerun-failures",
        "--test-shard",
        "--test-skip-pattern",
        "--throw-deprecation",
        "--title",
        "--tls-cipher-list",
        "--tls-keylog",
        "--tls-max-v1.2",
        "--tls-max-v1.3",
        "--tls-min-v1.0",
        "--tls-min-v1.1",
        "--tls-min-v1.2",
        "--tls-min-v1.3",
        "--trace-deprecation",
        "--trace-env",
        "--trace-env-js-stack",
        "--trace-env-native-stack",
        "--trace-event-categories",
        "--trace-event-file-pattern",
        "--trace-events-enabled",
        "--trace-exit",
        "--trace-promises",
        "--trace-require-module",
        "--trace-sigint",
        "--trace-sync-io",
        "--trace-tls",
        "--trace-uncaught",
        "--trace-warnings",
        "--track-heap-objects",
        "--unhandled-rejections",
        "--use-bundled-ca",
        "--use-env-proxy",
        "--use-largepages",
        "--use-openssl-ca",
        "--use-system-ca",
        "--v8-pool-size",
        "--verify-base-objects",
        "--warnings",
        "--watch",
        "--watch-kill-signal",
        "--watch-path",
        "--watch-preserve-output",
        "--webstorage",
        "--zero-fill-buffers",
        "-C",
        "-r",
    ];
    crate::realm_decl! { birth module_set_value(FLAGS); realm { process.allowedNodeEnvironmentFlags; "node:process.allowedNodeEnvironmentFlags"; } }
}

#[cfg(test)]
mod realm_storage_tests {
    include!(concat!(env!("OUT_DIR"), "/realm_set_storage_test.rs"));
}
