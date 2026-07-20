//! Optional-feature usage detection (#5140 / size-optimize).
//!
//! Extracted from `collect_module_finish` to keep `collect_modules.rs`
//! under the 2000-line cap. Each block text-greps a module's lowered HIR
//! (or inspects structured fields) to flip a `ctx.uses_*` / `needs_*` gate
//! so auto-optimize links only the runtime subsystems the program can
//! actually reach. Over-matching only over-includes a subsystem (a size,
//! not a correctness, cost); the rule throughout is zero false negatives.

use super::crypto_ns::module_uses_global_crypto_namespace;
use crate::commands::compile::CompilationContext;

/// Inspect a lowered module and set the optional-feature gates it needs.
pub(super) fn detect_optional_feature_usage(
    ctx: &mut CompilationContext,
    hir_module: &perry_hir::Module,
) {
    // Detect fetch() usage — js_fetch_with_options lives in perry-stdlib
    if hir_module.uses_fetch {
        ctx.needs_stdlib = true;
        ctx.uses_fetch = true;
    }

    // Robust fallback for fetch detection. The ~30 `ctx.uses_fetch` set-sites in
    // perry-hir lowering are shape-specific; a minified bundle's `new Headers()`
    // / `new Request()` / `fetch(...)` can reach codegen as `Expr::New { class_name:
    // "Headers" }` / a `Fetch*` variant (codegen dispatches those to
    // `js_headers_new` / `js_request_new` / `js_fetch_with_options`) WITHOUT having
    // hit any set-site, leaving `hir_module.uses_fetch` false. The perry-stdlib
    // `web-fetch` feature is then stripped, only the no-op runtime stub remains, and
    // it returns garbage the caller derefs -> SIGSEGV in `js_object_get_class_id`.
    // Mirror the EventEmitter / URL token-grep below: scan the final HIR for the
    // fetch web-platform constructors + the dedicated fetch call variants. Over-
    // matching only over-links `web-fetch` (a size cost); the rule is zero false
    // negatives.
    if !ctx.uses_fetch {
        let hir_debug: String = format!(
            "{:?}{:?}{:?}",
            &hir_module.init, &hir_module.functions, &hir_module.classes
        );
        if hir_debug.contains("class_name: \"Headers\"")
            || hir_debug.contains("class_name: \"Request\"")
            || hir_debug.contains("class_name: \"Response\"")
            || hir_debug.contains("class_name: \"FormData\"")
            || hir_debug.contains("class_name: \"Blob\"")
            || hir_debug.contains("class_name: \"File\"")
            || hir_debug.contains("FetchWithOptions")
            || hir_debug.contains("FetchGetWithAuth")
            || hir_debug.contains("FetchPostWithAuth")
        {
            ctx.needs_stdlib = true;
            ctx.uses_fetch = true;
        }
    }
    if std::env::var_os("PERRY_FETCH_DIAG").is_some() {
        eprintln!(
            "[perry-fetch-diag] module hir.uses_fetch={} -> ctx.uses_fetch={}",
            hir_module.uses_fetch, ctx.uses_fetch
        );
    }

    // Issue #76 — auto-link the wasmi host runtime when any module
    // references `WebAssembly.*`. Without this the user has to remember
    // `--enable-wasm-runtime`; with it the flag is only needed when they
    // want to override the auto-detection (e.g. force-link for plugins
    // they'll dlopen later).
    if hir_module.uses_webassembly {
        ctx.needs_wasm_runtime = true;
    }

    // Detect crypto.* builtin usage (randomBytes/randomUUID/sha256/md5 used
    // without `import crypto`). The runtime symbols live behind the
    // perry-stdlib `crypto` Cargo feature, so we need to flip that on for
    // auto-optimize. Text-grep the serialized Debug form for the established
    // dedicated HIR variants. The global WebCrypto namespace path below uses
    // a structured walk because it is an ordinary `PropertyGet`.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        let uses_global_crypto_namespace = module_uses_global_crypto_namespace(hir_module);
        if hir_debug.contains("CryptoRandomBytes")
            || hir_debug.contains("CryptoRandomUUID")
            || hir_debug.contains("CryptoSha256")
            || hir_debug.contains("CryptoMd5")
            // Web Crypto API (issue #561). The four WebCrypto* HIR
            // variants lower to extern calls into perry-stdlib's
            // webcrypto module, gated behind the `crypto` feature.
            // Without flipping the gate, auto-optimize would build
            // perry-stdlib without `crypto` and link would fail with
            // "_js_webcrypto_digest" undefined.
            || hir_debug.contains("WebCryptoDigest")
            || hir_debug.contains("WebCryptoImportKey")
            || hir_debug.contains("WebCryptoSign")
            || hir_debug.contains("WebCryptoVerify")
            || hir_debug.contains("WebCryptoEncrypt")
            || hir_debug.contains("WebCryptoDecrypt")
            || hir_debug.contains("WebCryptoGenerateKey")
            || hir_debug.contains("WebCryptoWrapKey")
            || hir_debug.contains("WebCryptoUnwrapKey")
            // `globalThis.crypto` / bare `crypto` now materializes the
            // WebCrypto singleton. Its `randomUUID` property dispatches
            // through perry-stdlib's crypto bridge when called via a
            // runtime property read rather than the direct HIR variant.
            || uses_global_crypto_namespace
        {
            ctx.needs_stdlib = true;
            ctx.uses_crypto_builtins = true;
        }
    }

    // Detect whether this module needs the regex engine. The engine
    // (`regex`/`fancy-regex`, ~1.2 MB) is gated behind `perry-runtime/
    // regex-engine` and the RegExp object's identity/display layer stays
    // always-compiled, so a program that can never produce a RegExp at
    // runtime links none of the matching machinery. A regex value can only
    // exist if a regex literal / `RegExp` was evaluated, OR a regex-coercing
    // string method (`.match`/`.matchAll`/`.search`, which build a RegExp from
    // even a string arg per spec) ran, OR a glob API was used (the runtime
    // compiles globs to regexes internally). We grep the serialized Debug form
    // for the unambiguous HIR variant tokens and the generic-dispatch method
    // names. Over-matching only over-includes the engine (a size, not a
    // correctness, cost); the goal is zero false negatives. `eval` is
    // non-functional in Perry so it can't create a regex at runtime.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("RegExp")            // RegExp / RegExpDynamic / RegExpTest / RegExpExec / RegExpEscape / RegExpReplaceFn / RegExpExec{Index,Groups}
            || hir_debug.contains("StringMatch")   // dedicated .match / .matchAll variants
            || hir_debug.contains("PathMatchesGlob")
            || hir_debug.contains("property: \"search\"")
            || hir_debug.contains("property: \"match\"")
            || hir_debug.contains("property: \"matchAll\"")
            || hir_debug.contains("property: \"glob\"")
            || hir_debug.contains("property: \"globSync\"")
        {
            ctx.uses_regex = true;
        }
    }

    // Detect TC39 `Temporal.*` usage. The engine (`temporal_rs` + transitive
    // tz/calendar deps, ~580 KB) is gated behind `perry-runtime/temporal`;
    // the Temporal cell's identity layer stays always-compiled, so a program
    // that never touches `Temporal` links none of the date-math machinery.
    // `Temporal` is a global namespace (like `Intl`/`Math`): accessing it (even
    // when aliased, e.g. `const now = Temporal.Now`) materializes a
    // `PropertyGet { property: "Temporal" }`, so we match that exact token
    // rather than a bare `"Temporal"` substring — the latter also fires on
    // user identifiers like `myTemporal` / `temporalLog`, spuriously enabling
    // the engine and undercutting the size win. JS `Date` is a separate impl.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("property: \"Temporal\"") {
            ctx.uses_temporal = true;
        }
    }

    // #5140 — detect native `EventEmitter` construction. The `EventEmitter`
    // builtin-new path (`new EventEmitter()` / `EventEmitterAsyncResource`,
    // routed by the local binding NAME — so it fires for `eventemitter3`'s
    // default export too, not only `node:events`) emits `js_event_emitter_*`
    // calls. Those helpers live in perry-stdlib's `events` module behind
    // `bundled-events`; a program that uses native EventEmitter without
    // importing `node:events` otherwise fails to link with undefined
    // `_js_event_emitter_*` symbols. Match the lowered `Expr::New` token.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("class_name: \"EventEmitter\"")
            || hir_debug.contains("class_name: \"EventEmitterAsyncResource\"")
        {
            ctx.uses_event_emitter = true;
            // Treat native EventEmitter use exactly like a `node:events` import
            // so the full events wiring fires: the perry-ext-events well-known
            // archive (which defines `js_event_emitter_*`) is linked, the
            // `bundled-events` feature is enabled, and the construct dispatcher
            // is registered (`external-events-construct`). Idempotent — a set.
            ctx.native_module_imports.insert("events".to_string());
        }
    }

    // #6593 (pi bundle) — detect name-heuristic native package lowering.
    // `detect_native_instance_expr` routes `new LRUCache(...)` / `new
    // Decimal(...)` / `new Command(...)` etc. to a native module by BINDING
    // NAME, with no import statement required. An esbuild bundle that
    // inlines such a package (pi's hosted-git-info inlines `lru-cache`)
    // therefore emits `NativeMethodCall { module: "lru-cache", … }` calls
    // while `native_module_imports` never learns about the module — the
    // per-binding perry-stdlib feature stays off and the link dies with
    // undefined `_js_lru_cache_*` symbols (from `GitHost.fromUrl`). Same
    // failure mode and fix as the EventEmitter block above. Scan classes
    // too: the pi call sites live in a static method body, which the
    // init+functions-only scans miss.
    {
        let hir_debug: String = format!(
            "{:?}{:?}{:?}",
            &hir_module.init, &hir_module.functions, &hir_module.classes
        );
        for native_module in [
            "lru-cache",
            "big.js",
            "decimal.js",
            "bignumber.js",
            "commander",
        ] {
            if hir_debug.contains(&format!("module: \"{native_module}\"")) {
                ctx.needs_stdlib = true;
                ctx.native_module_imports.insert(native_module.to_string());
            }
        }
    }

    // Detect WHATWG URL API usage. The `url`+`idna` host-canonicalization
    // engine (~195 KB) is gated behind `perry-runtime/url-engine`; Perry's URL
    // parsing is otherwise hand-rolled, so a program with no URL API links none
    // of it. Web `URL`/`URLPattern`/`URLSearchParams` lower to dedicated `Url*`
    // HIR variants (always `Url` + an uppercase letter, e.g. `UrlNew`,
    // `UrlSet…`, `UrlSearchParams…`); `node:url` lowers to a
    // `NativeMethodCall { module: "url", … }`. We match those exact tokens
    // instead of a bare `"Url"`/`"URL"` substring, which would also fire on
    // common camelCase identifiers like `baseUrl` / `imageUrl` and spuriously
    // link the engine. Over-matching within the URL family (e.g. enabling for a
    // URLSearchParams-only program that doesn't strictly need the host parser)
    // is a benign size cost; the rule is zero false negatives.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("UrlNew")
            || hir_debug.contains("UrlParse")
            || hir_debug.contains("UrlCanParse")
            || hir_debug.contains("UrlPattern")
            || hir_debug.contains("UrlGet")
            || hir_debug.contains("UrlSet")
            || hir_debug.contains("UrlInstance")
            || hir_debug.contains("UrlSearchParams")
            || hir_debug.contains("module: \"url\"")
        {
            ctx.uses_url = true;
        }
    }

    // Detect `String.prototype.normalize` (gates `unicode-normalization`,
    // ~113 KB) and `Intl.Segmenter` (gates `unicode-segmentation`, ~73 KB).
    // Both lower to method/namespace nodes carrying the name as a `property`,
    // so we match the exact `property: "<name>"` token. (A bare `"Segmenter"`
    // substring would also fire on a user identifier named `Segmenter`.)
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("property: \"normalize\"") {
            ctx.uses_string_normalize = true;
        }
        if hir_debug.contains("property: \"Segmenter\"") {
            ctx.uses_intl_segmenter = true;
        }
        // `Intl.getCanonicalLocales(...)` / `Intl.*.supportedLocalesOf(...)` gate
        // `perry-runtime/intl-locale` (`icu_locale_core` BCP-47 canonicalization).
        // Both lower with the method name as a `property` token.
        if hir_debug.contains("property: \"getCanonicalLocales\"")
            || hir_debug.contains("property: \"supportedLocalesOf\"")
        {
            ctx.uses_intl_locale = true;
        }
        // `Intl.DateTimeFormat` / `Date.prototype.toLocale{,Date,Time}String`
        // gate `perry-runtime/intl-datetime` (icu4x `icu_datetime` + CLDR
        // date-time patterns). `toLocaleString` is ambiguous (Number also has
        // one) but including the feature for a number-only program only costs a
        // little size, whereas MISSING it on a date-formatting program drops
        // byte-parity — so we err toward enabling.
        if hir_debug.contains("property: \"DateTimeFormat\"")
            || hir_debug.contains("property: \"toLocaleString\"")
            || hir_debug.contains("property: \"toLocaleDateString\"")
            || hir_debug.contains("property: \"toLocaleTimeString\"")
            || hir_debug.contains("method: \"toLocaleString\"")
            || hir_debug.contains("method: \"toLocaleDateString\"")
            || hir_debug.contains("method: \"toLocaleTimeString\"")
        {
            ctx.uses_intl_datetime = true;
        }
    }

    // Detect heap-snapshot / `process.report` usage, the only user-facing APIs
    // behind the `diagnostics` feature (~95 KB of cold-path JSON serializers +
    // the `serde_json` pulled only by them). `v8.getHeapSnapshot` /
    // `v8.writeHeapSnapshot` lower to `NativeMethodCall { method: "…" }`;
    // `process.report.*` surfaces as `property: "report"`. The env-driven dev
    // diagnostics (GC-diag / typed-feedback JSON) ride the same feature and
    // degrade gracefully when off, so they need no detection.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("method: \"getHeapSnapshot\"")
            || hir_debug.contains("method: \"writeHeapSnapshot\"")
            || hir_debug.contains("property: \"report\"")
        {
            ctx.uses_diagnostics = true;
        }
        // `node:dgram` (UDP) → gates `perry-runtime/mod-dgram` (~43 KB; dgram
        // lowers to `NativeMethodCall { module: "dgram" }`, runtime-only so not
        // in `native_module_imports`).
        if hir_debug.contains("module: \"dgram\"") {
            ctx.uses_dgram = true;
        }
    }

    // Detect readline usage via process.stdin raw/lifecycle methods. These
    // don't go through an `import 'readline'` statement, so the import-based
    // needs_stdlib detection above misses them.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("ProcessStdinSetRawMode")
            || hir_debug.contains("ProcessStdinOn")
            || hir_debug.contains("ProcessStdinRemoveListener")
            || hir_debug.contains("ProcessStdinLifecycle")
        {
            ctx.needs_stdlib = true;
            ctx.native_module_imports.insert("readline".to_string());
        }
    }

    // Detect ioredis usage (detected by class name, not import path)
    let mut found_ioredis = false;
    for (_, module_name, _) in &hir_module.exported_native_instances {
        if module_name == "ioredis" {
            found_ioredis = true;
            break;
        }
    }
    if !found_ioredis {
        for (_, module_name, _) in &hir_module.exported_func_return_native_instances {
            if module_name == "ioredis" {
                found_ioredis = true;
                break;
            }
        }
    }
    if found_ioredis {
        ctx.needs_stdlib = true;
        ctx.native_module_imports.insert("ioredis".to_string());
    }
}
