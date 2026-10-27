//! The optional runtime/stdlib features a program installs from the prebuilt
//! archives, and the compile-time instrument requests.

use std::collections::BTreeSet;

use super::super::CompilationContext;

fn needs_http2_constants(ctx: &CompilationContext) -> bool {
    ctx.native_module_imports.contains("http2") || ctx.uses_get_builtin_module
}

/// `node:test` drags in the whole runner, and with it the JSON serializer its
/// snapshot assertions call. Keep it for a program that imports either entry
/// point, or that can reach any builtin by runtime string via
/// `process.getBuiltinModule`.
fn needs_node_test(ctx: &CompilationContext) -> bool {
    ctx.uses_node_test || ctx.uses_get_builtin_module
}

/// Cache key for the auto-optimize target dir + build stamp. Hashed into the
/// `target/perry-auto-<hash>` dir name so each (features, panic-mode, target,
/// runtime-gate, co-built wrapper set, version) combination gets its own
/// incremental cache. Kept in one place so `build_optimized_libs` and its
/// freshness tests can never drift.
/// Run-time knobs served only by perry-runtime's `gc-instruments` feature.
/// Must match `INSTRUMENT_KNOBS` in `perry-runtime/src/gc/instruments.rs`
/// (pinned by `gc_instrument_knobs_match_the_runtime`).
pub(crate) const GC_INSTRUMENT_KNOBS: &[&str] = &[
    "PERRY_GC_CENSUS",
    "PERRY_GC_PROTECT_FROMSPACE",
    "PERRY_GC_PROTECT_OLD_SWEEP",
    "PERRY_GC_BUDGETED_OLD_RECLAIM",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_FROMSPACE_SCAN",
    "PERRY_GC_FROMSPACE_SCAN_ABORT",
    "PERRY_ALLOC_SITE_SAMPLE",
    "PERRY_GC_VERIFY_MARK",
    "PERRY_GC_VERIFY_CLASSIFIER",
    "PERRY_STACK_SYMBOLS",
    "PERRY_FIELD_REPR_VERIFY",
];

fn all_instruments_requested() -> bool {
    std::env::var("PERRY_GC_INSTRUMENTS")
        .is_ok_and(|v| matches!(v.trim(), "1" | "true" | "on" | "yes"))
}

/// Link the GC instruments into this build: `PERRY_GC_INSTRUMENTS=1`, or any
/// instrument knob set while compiling (so `PERRY_GC_SCHEDULE_SEED=7 perry
/// compile … && PERRY_GC_SCHEDULE_SEED=7 ./a.out` just works).
pub(crate) fn gc_instruments_requested() -> bool {
    all_instruments_requested()
        || GC_INSTRUMENT_KNOBS
            .iter()
            .any(|knob| std::env::var_os(knob).is_some_and(|v| !v.is_empty()))
}

/// Run-time knobs served only by perry-runtime's `hot-diag` feature (#10572).
/// Must match `HOT_DIAG_KNOBS` in `perry-runtime/src/hot_diag.rs` (pinned by
/// `hot_diag_knobs_match_the_runtime`).
pub(crate) const HOT_DIAG_KNOBS: &[&str] = &[
    "PERRY_REGEX_DIAG",
    "PERRY_IC_DIAG",
    "PERRY_ENUM_DIAG",
    "PERRY_RECEIVER_REPR_DIAG",
];

/// Link the hot-path diagnostics into this build: `PERRY_GC_INSTRUMENTS=1`
/// (every runtime instrument), or a hot-diag knob armed while compiling. A
/// knob counts as armed under the runtime's own spelling rules
/// (`hot_diag::sink_from_env`): `0`/`off`/`false`/`no` leave it off.
pub(crate) fn hot_diag_requested() -> bool {
    all_instruments_requested()
        || HOT_DIAG_KNOBS.iter().any(|knob| {
            std::env::var(knob)
                .is_ok_and(|v| !matches!(v.trim(), "" | "0" | "off" | "false" | "no"))
        })
}

/// Whether this compile asks for `PERRY_GC_TRACE` output (the telemetry and
/// typed-feedback JSON, which only the instrumented archives carry).
/// Shared with the build-level no-op cache so the cache uses the same decision
/// as runtime selection.
pub(crate) fn gc_trace_requested() -> bool {
    std::env::var("PERRY_GC_TRACE")
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Whether this compile links the instrumented prebuilt archives
/// (`libperry_runtime_instr.a` and its stdlib / wrapper siblings, built by
/// `scripts/build_instr_runtime.sh`) instead of the default ones. The default
/// archives carry no GC/diagnostic instruments, so the choice is made here,
/// before codegen, from the same knobs the instruments answer to: any
/// instrument or hot-diag knob set while compiling, `PERRY_GC_INSTRUMENTS=1`,
/// `PERRY_GC_TRACE=1`, or a contract build (`PERRY_CHECK_CONTRACTS=1`, whose
/// witnesses and checks the instrumented variant also carries).
pub(crate) fn instrumented_variant_requested() -> bool {
    gc_instruments_requested()
        || hot_diag_requested()
        || gc_trace_requested()
        || crate::commands::compile::program_facts::contracts_requested()
}

/// The archive file name the current compile links for `name` (a default
/// archive name such as `libperry_runtime.a` or `perry_stdlib.lib`): the
/// instrumented variant inserts `_instr` before the extension.
pub(crate) fn variant_archive_name(name: &str) -> String {
    variant_archive_name_for(name, instrumented_variant_requested())
}

/// [`variant_archive_name`] for an explicit choice of variant.
pub(crate) fn variant_archive_name_for(name: &str, instrumented: bool) -> String {
    if !instrumented {
        return name.to_string();
    }
    match name
        .strip_suffix(".a")
        .map(|stem| (stem, ".a"))
        .or_else(|| name.strip_suffix(".lib").map(|stem| (stem, ".lib")))
    {
        Some((stem, ext)) => format!("{stem}_instr{ext}"),
        None => name.to_string(),
    }
}

pub(crate) fn auto_optimized_cross_features(
    ctx: &CompilationContext,
    features: &BTreeSet<&'static str>,
    cli_features: &[String],
) -> Vec<String> {
    let mut cross_features: Vec<String> = vec![
        // perry-runtime's "full" feature gates plugin + os.hostname/homedir.
        // Auto-mode keeps it on so existing behavior is preserved; the
        // panic mode is what shrinks the binary.
        "perry-runtime/full".to_string(),
    ];
    for f in features {
        cross_features.push(format!("perry-stdlib/{}", f));
    }
    // CLI `--features` values that target the runtime (game-loop entry-point
    // shims gated behind `ios-game-loop` / `watchos-game-loop` in
    // `perry-runtime/Cargo.toml`) need `perry-runtime/<f>` passed through, not
    // `perry-stdlib/<f>` — they gate a Rust module, not an npm dep surface.
    for f in cli_features {
        if f == "ios-game-loop" || f == "watchos-game-loop" || f == "ohos-napi" {
            cross_features.push(format!("perry-runtime/{}", f));
        }
    }
    // Issue #76 — install the WebAssembly engine (`js_runtime_install_wasm_host`)
    // when the program references `WebAssembly.*`, and the Node-API host
    // (#8523) for an approved native addon. Both are always compiled into the
    // runtime archive and reached only through their installs.
    if ctx.needs_wasm_runtime {
        cross_features.push("perry-runtime/wasm-host".to_string());
    }
    if !ctx.native_addons.is_empty() {
        cross_features.push("perry-runtime/node-api-host".to_string());
    }
    if ctx.bun_platform
        || ctx.native_module_imports.contains("bun")
        || ctx.uses_dynamic_import_options
    {
        cross_features.push("perry-runtime/bun-cli-utils".to_string());
    }
    // Binary-size feature gating (kept in sync with the inline list on `main`):
    // each engine/table is linked only when the program actually uses it.
    // V8/IPC/worker deserialization can materialize a RegExp supplied by
    // another program, even when this program has no RegExp literal.
    if ctx.uses_regex
        || ctx.uses_proc_ipc
        || ctx.uses_get_builtin_module
        || ctx.native_module_imports.contains("v8")
        || ctx.native_module_imports.contains("worker_threads")
    {
        cross_features.push("perry-runtime/regex-engine".to_string());
    }
    if ctx.uses_temporal {
        cross_features.push("perry-runtime/temporal".to_string());
    }
    if ctx.uses_url || ctx.uses_get_builtin_module || !ctx.native_addons.is_empty() {
        cross_features.push("perry-runtime/url-engine".to_string());
    }
    if ctx.uses_string_normalize {
        cross_features.push("perry-runtime/string-normalize".to_string());
    }
    if ctx.uses_intl_segmenter {
        cross_features.push("perry-runtime/intl-segmenter".to_string());
    }
    // `Intl.*` namespace surface — see perry-runtime's `intl-namespace`.
    // A deferred dynamic-code site can construct `Intl.…` from a runtime
    // string, so force it on there too (mirrors the dyn-eval regex rule).
    let needs_dyn_eval = perry_hir::has_deferred_dynamic_code_sites()
        || ctx.native_module_imports.contains("vm")
        || ctx.uses_data_url_dynamic_import;
    if ctx.uses_intl_namespace || needs_dyn_eval {
        cross_features.push("perry-runtime/intl-namespace".to_string());
    }
    let io_module = ctx.native_module_imports.iter().any(|module| {
        matches!(
            module.as_str(),
            "net"
                | "tls"
                | "http"
                | "https"
                | "http2"
                | "dgram"
                | "dns"
                | "dns/promises"
                | "fs"
                | "fs/promises"
                | "child_process"
                | "worker_threads"
                | "readline"
                | "readline/promises"
                | "node-pty"
                | "zlib"
                | "crypto"
        )
    });
    // Native producers include binary deserialization, bigint-valued OS/fs
    // metadata, FFI, histogram and WebAssembly results. Their returned values
    // must keep the generic BigInt operators and formatting paths installed.
    if features.contains("crypto")
        || features.contains("database-sqlite")
        || features.contains("bundled-ethers")
        || ctx.uses_bigint
        || ctx.uses_proc_ipc
        || ctx.uses_get_builtin_module
        || ctx.uses_temporal
        || ctx.needs_wasm_runtime
        || !ctx.native_addons.is_empty()
        || needs_dyn_eval
        || ctx.native_module_imports.iter().any(|m| {
            matches!(
                m.as_str(),
                "v8" | "worker_threads"
                    | "child_process"
                    | "fs"
                    | "fs/promises"
                    | "os"
                    | "perf_hooks"
                    | "bun"
                    | "bun:ffi"
                    | "crypto"
                    | "ethers"
                    | "sqlite"
                    | "bun:sqlite"
            )
        })
    {
        cross_features.push("perry-runtime/bigint".to_string());
    }
    if ctx.uses_date
        || ctx.uses_get_builtin_module
        || ctx.uses_proc_ipc
        || needs_dyn_eval
        || !ctx.native_addons.is_empty()
        || ctx.native_module_imports.iter().any(|m| {
            matches!(
                m.as_str(),
                "v8" | "worker_threads"
                    | "child_process"
                    | "fs"
                    | "fs/promises"
                    | "crypto"
                    | "bun"
                    | "bun:ffi"
            )
        })
    {
        cross_features.push("perry-runtime/date".to_string());
    }
    // Clone-producing APIs and worker completions share the thread codec.
    if ctx.uses_thread_codec
        || ctx.uses_worker_threads
        || ctx.uses_proc_ipc
        || ctx.uses_global_atomics
        || ctx.uses_get_builtin_module
        || needs_dyn_eval
        || !ctx.native_addons.is_empty()
        || ctx.native_module_imports.iter().any(|m| {
            matches!(
                m.as_str(),
                "worker_threads"
                    | "perry/thread"
                    | "perf_hooks"
                    | "module"
                    | "diagnostics_channel"
                    | "v8"
                    | "child_process"
            )
        })
    {
        cross_features.push("perry-runtime/thread".to_string());
    }
    // Install observation before user scheduling, even for a later require.
    if ctx
        .native_module_imports
        .iter()
        .any(|m| matches!(m.as_str(), "async_hooks" | "events"))
        || ctx.uses_get_builtin_module
        || needs_dyn_eval
        || !ctx.native_addons.is_empty()
    {
        cross_features.push("perry-runtime/async-hooks".to_string());
    }
    if ctx.uses_proxy
        || ctx.uses_get_builtin_module
        || needs_dyn_eval
        || !ctx.native_addons.is_empty()
    {
        cross_features.push("perry-runtime/proxy".to_string());
    }
    if ctx.uses_turnloop
        || io_module
        || ctx.uses_fetch
        || ctx.uses_proc_ipc
        || ctx.uses_get_builtin_module
        || features.contains("async-runtime")
        || needs_dyn_eval
    {
        cross_features.push("perry-runtime/turnloop".to_string());
    }
    // A formatter-only program can reach stringify, without creating a global
    // namespace or constructing parser/tape values. The full JSON installer
    // also fills formatting slots when the JSON API itself is reachable.
    let json_namespace =
        ctx.uses_global_json || (ctx.uses_json_format && !ctx.native_addons.is_empty());
    if ctx.uses_json_engine && !json_namespace && !needs_dyn_eval {
        cross_features.push("perry-runtime/json-engine".to_string());
    }
    if ctx.uses_json_format && !ctx.uses_json_engine && !json_namespace && !needs_dyn_eval {
        cross_features.push("perry-runtime/json-format".to_string());
    }
    // Per-namespace globalThis member tables — see perry-runtime's `global-*`.
    // A deferred dynamic-code site can reach any namespace by runtime string,
    // so force all four on there (mirrors the intl-namespace rule).
    let dynamic_code = needs_dyn_eval;
    for (used, feat) in [
        (ctx.uses_global_math, "global-math"),
        (json_namespace, "global-json"),
        (ctx.uses_global_reflect, "global-reflect"),
        (ctx.uses_global_atomics, "global-atomics"),
        (ctx.uses_global_url, "global-url"),
        (ctx.uses_global_text, "global-text"),
        (ctx.uses_global_websocket, "global-websocket"),
        (ctx.uses_global_webcrypto, "global-webcrypto"),
        (ctx.uses_global_webfetch, "global-webfetch"),
        (ctx.uses_proc_ipc || ctx.uses_get_builtin_module, "proc-ipc"),
    ] {
        if used || dynamic_code {
            cross_features.push(format!("perry-runtime/{feat}"));
        }
    }
    if ctx.uses_intl_locale {
        cross_features.push("perry-runtime/intl-locale".to_string());
    }
    if ctx.uses_intl_datetime {
        cross_features.push("perry-runtime/intl-datetime".to_string());
    }
    // Heap snapshots and `process.report` (~95 KB of JSON serializers plus the
    // `serde_json` pulled only by them): user APIs, always compiled into the
    // archive and installed only for a program that references one. The GC and
    // typed-feedback instruments are not installs at all: they exist only in
    // the instrumented archives (`instrumented_variant_requested`).
    if ctx.uses_diagnostics {
        cross_features.push("perry-runtime/diagnostics".to_string());
    }
    if ctx.uses_dgram {
        cross_features.push("perry-runtime/mod-dgram".to_string());
    }
    // #6468 — keep the `node:http2` constant tables (~20 KB) when source imports
    // `node:http2` or calls `process.getBuiltinModule`, whose target is only
    // known at runtime. Programs using neither path still link none of them.
    if needs_http2_constants(ctx) {
        cross_features.push("perry-runtime/mod-http2-constants".to_string());
    }
    if needs_node_test(ctx) {
        cross_features.push("perry-runtime/mod-node-test".to_string());
    }
    // #6559: a deferred dynamic-code site (`eval(...)` / `new Function(...)`
    // with a runtime body) means the binary may construct functions from
    // runtime strings — the optimized runtime must carry the dyn-eval
    // interpreter. The generated code of the schema-codegen ecosystem (ajv)
    // also leans on regex literals (`key.replace(/~/g, …)`), so the regex
    // engine rides along even when the program's own source never uses one.
    if needs_dyn_eval {
        cross_features.push("perry-runtime/dyn-eval".to_string());
        if !ctx.uses_regex {
            cross_features.push("perry-runtime/regex-engine".to_string());
        }
    }
    // mimalloc global allocator (#62): force-added on EVERY auto-optimize
    // rebuild, including size-optimized ones. Dropping it for size (~140 KB)
    // produces binaries whose console output silently vanishes / that throw
    // spurious TypeErrors: runtime pointer-classification paths assume the
    // allocator's address bands (the macOS mimalloc heap lands in a high
    // window; system-malloc allocations land elsewhere and get misread as
    // handles/non-pointers). Until value/addr_class.rs is audited for
    // system-allocator ranges, the feature stays force-on; the cfg gate in
    // perry-runtime remains for that future audit.
    cross_features.push("perry-runtime/alloc-mimalloc".to_string());
    // `#[used]` keep-alive anchors must be present on EVERY auto-optimize
    // rebuild, not just the bitcode-LTO path. The anchors pin codegen-only
    // `#[no_mangle] pub extern "C" fn` symbols (e.g. `js_box_release`,
    // `js_bool_box_release`, `js_closure_set_box_capture_ptr`,
    // `js_link_path_module_parent`) that are never called from within the
    // perry-runtime crate itself — only from the program's generated object
    // files. Without `#[used]`, rustc dead-code-eliminates these unreferenced
    // symbols during staticlib archive creation, so they are absent from
    // `libperry_runtime.a` and the final link fails with "Undefined symbols
    // for architecture arm64" when the program's codegen emits calls to them.
    //
    // #6917 gated the anchors behind `keepalive-anchors` (bitcode-LTO only)
    // under the assumption that "the classic link keeps every reachable
    // runtime symbol via real undefined references from the program's
    // objects." That assumption is wrong: the program's undefined references
    // can only resolve symbols that are IN the archive, and the archive
    // doesn't contain symbols that were DCE'd during archive creation.
    //
    // In a staticlib archive (`.a`) the linker only pulls in object files
    // that resolve an undefined reference, so `#[used]` anchors only become
    // `-dead_strip` roots when their object file is pulled in — i.e. when
    // the program actually references a symbol from that unit. The size
    // regression is therefore limited to the transitive callees of symbols
    // the program uses (which would be kept anyway), not the entire runtime
    // surface. Re-enable unconditionally; the bitcode-LTO path already did.
    cross_features.push("perry-runtime/keepalive-anchors".to_string());
    // Compile OUT perry-runtime's no-op fetch stubs (`js_fetch_with_options` /
    // `js_headers_new` / `js_request_new`, gated `#[cfg(not(feature =
    // "external-fetch-symbols"))]`) whenever the program uses fetch — perry-stdlib's
    // `web-fetch` then supplies the REAL impls. Without this both the stub
    // (perry-runtime) and the real (perry-stdlib) symbols exist; on the fresh build
    // path the stub has won the link and returned garbage the caller derefs ->
    // SIGSEGV in `js_object_get_class_id`. Enabling the feature drops the stubs from
    // libperry_runtime.a so only the real symbols remain.
    if ctx.uses_fetch {
        cross_features.push("perry-runtime/external-fetch-symbols".to_string());
    }
    cross_features
}
