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

fn debug_hir_uses_regex(hir_debug: &str) -> bool {
    hir_debug.contains("RegExp") // RegExp / RegExpDynamic / RegExpTest / RegExpExec / RegExpEscape / RegExpReplaceFn / RegExpExec{Index,Groups}
        || hir_debug.contains("StringMatch") // dedicated .match / .matchAll variants
        // Covers both `PathMatchesGlob` and
        // `PathWin32 { method: MatchesGlob, ... }`.
        || hir_debug.contains("MatchesGlob")
        // Dynamic sub-namespace dispatch keeps the method name in a
        // runtime-dispatch expression, but its Debug representation is not
        // guaranteed to be a plain `method: "..."` field. Match the API name
        // itself: a false positive only links the optional engine, whereas a
        // false negative makes `matchesGlob` silently return false.
        || hir_debug.contains("matchesGlob")
        || hir_debug.contains("property: \"search\"")
        || hir_debug.contains("property: \"match\"")
        || hir_debug.contains("property: \"matchAll\"")
        || hir_debug.contains("property: \"glob\"")
        || hir_debug.contains("property: \"globSync\"")
        || hir_debug.contains("method: \"Glob\"")
}

/// zlib per-codec cherry-pick (stdlib cherry-pick): a `node:zlib` import
/// only selects the gzip/deflate base (`compression-gzip`); the Brotli and
/// zstd backends are linked when a matching API token appears anywhere in
/// the lowered HIR. Method calls surface as `method: "brotliCompressSync"`
/// / `NativeMethodCall { … }` tokens, factory calls as
/// `createBrotliCompress` / `createZstdDecompress`, constants as
/// `BROTLI_*` / `ZSTD_*` property reads. A bare substring match
/// over-includes (a user identifier containing "brotli" links the codec —
/// a size, not a correctness, cost); the rule is zero false negatives for
/// statically-lowered call sites. Fully dynamic access (`zlib[name]`) is
/// covered by the deferred-dynamic-code fallback in
/// `build_optimized_libs`, which enables the full `compression` umbrella.
fn debug_hir_uses_zlib_brotli(hir_debug: &str) -> bool {
    hir_debug.contains("rotli") || hir_debug.contains("BROTLI")
}

/// See [`debug_hir_uses_zlib_brotli`] — same contract for the zstd family.
fn debug_hir_uses_zlib_zstd(hir_debug: &str) -> bool {
    hir_debug.contains("zstd") || hir_debug.contains("Zstd") || hir_debug.contains("ZSTD")
}

/// WHATWG compression streams → the `streams-brotli` codec. Unlike
/// [`debug_hir_uses_zlib_brotli`] this CANNOT key on "brotli": the format is
/// a runtime argument (`new CompressionStream(fmt)`), and a format read from a
/// variable leaves no literal in the HIR. So any reference to either
/// constructor enables the codec. Direct construction lowers to
/// `New { class_name: "CompressionStream", .. }`; an alias
/// (`const C = CompressionStream`) to `PropertyGet { property:
/// "CompressionStream", .. }` — the name survives both. Checked separately
/// because "DecompressionStream" does not contain "CompressionStream" (the
/// `c` is lowercase). A fully computed global name is not seen; that case
/// fails loudly at construction in perry-stdlib rather than silently.
fn debug_hir_uses_web_compression_stream(hir_debug: &str) -> bool {
    hir_debug.contains("CompressionStream") || hir_debug.contains("DecompressionStream")
}

fn debug_hir_uses_get_builtin_module(hir_debug: &str) -> bool {
    hir_debug.contains("property: \"getBuiltinModule\"")
        || (hir_debug.contains("module: \"process\"")
            && hir_debug.contains("method: \"getBuiltinModule\""))
}

fn debug_hir_uses_string_normalization(hir_debug: &str) -> bool {
    // `localeCompare` has several static/dynamic HIR spellings. A bare match
    // deliberately over-includes the tables for a same-named user identifier;
    // feature detection permits size-only false positives, not false negatives.
    // Intl.Collator uses the same normalization tables for canonical
    // equivalence and locale-primary weights.
    hir_debug.contains("property: \"normalize\"")
        || hir_debug.contains("localeCompare")
        || hir_debug.contains("property: \"Collator\"")
}

fn debug_hir_uses_global_math_member(hir_debug: &str) -> bool {
    // Value-form reads such as `const cos = Math.cos` lose the `Math`
    // receiver during lowering and reach final HIR as a property read from
    // the shared GlobalGet(0) builtin sentinel. Match every member reified by
    // `install_math_namespace`. A same-named property on another object is a
    // benign false positive (a slightly larger runtime), while a false
    // negative leaves the extracted function undefined.
    const MEMBERS: &[&str] = &[
        "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "ceil", "clz32",
        "cos", "cosh", "exp", "expm1", "f16round", "floor", "fround", "hypot", "imul", "log",
        "log1p", "log2", "log10", "max", "min", "pow", "random", "round", "sign", "sin", "sinh",
        "sqrt", "tan", "tanh", "trunc",
    ];

    MEMBERS
        .iter()
        .any(|member| hir_debug.contains(&format!(r#"property: "{member}""#)))
}

/// `perry-runtime/url-engine` gate. Despite the name it is not only the host
/// parser: `String(url)`, JSON and the setter paths recognize a URL object only
/// under it, so it is needed wherever a URL object can exist.
fn debug_hir_uses_url_engine(hir_debug: &str) -> bool {
    // Dedicated `Url*` HIR variants (static `URL` / `URLSearchParams` /
    // `URLPattern` lowering) and statically lowered `node:url` calls.
    hir_debug.contains("UrlNew")
        || hir_debug.contains("UrlParse")
        || hir_debug.contains("UrlCanParse")
        || hir_debug.contains("UrlPattern")
        || hir_debug.contains("UrlGet")
        || hir_debug.contains("UrlSet")
        || hir_debug.contains("UrlInstance")
        || hir_debug.contains("UrlSearchParams")
        || hir_debug.contains("module: \"url\"")
        // #11121: the URL family used as a VALUE, e.g. `new ns.URL(u)` on a
        // `require("node:url")` namespace, which lowers to a plain
        // `PropertyGet { property: "URL" }` + dynamic construct and carries
        // none of the tokens above. The leading quote keeps `baseUrl` /
        // `imageUrl` identifiers out (same token the `global-url` gate uses).
        || hir_debug.contains("\"URL")
        // A CommonJS `require("node:url")` / `require("url")` resolves the
        // namespace at run time; its members are then reached dynamically.
        || hir_debug.contains("String(\"node:url\")")
        || hir_debug.contains("String(\"url\")")
}

fn imports_fs_promises_glob(hir_module: &perry_hir::Module) -> bool {
    hir_module.imports.iter().any(|import| {
        !import.type_only
            && !import.runtime_erased
            && import
                .source
                .strip_prefix("node:")
                .unwrap_or(&import.source)
                == "fs/promises"
            && import.specifiers.iter().any(|specifier| {
                matches!(
                    specifier,
                    perry_hir::ImportSpecifier::Named { imported, .. } if imported == "glob"
                )
            })
    })
}

/// The Debug text of every piece of lowered code in `hir_module`: module
/// init, top-level functions, AND class bodies (methods, accessors, static
/// blocks, field initializers), which live under `classes`.
fn module_hir_debug(hir_module: &perry_hir::Module) -> String {
    format!(
        "{:?}{:?}{:?}",
        &hir_module.init, &hir_module.functions, &hir_module.classes
    )
}

/// Inspect a lowered module and set the optional-feature gates it needs.
pub(super) fn detect_optional_feature_usage(
    ctx: &mut CompilationContext,
    hir_module: &perry_hir::Module,
) {
    // #11121: ONE token corpus for every text-grep gate below, and it covers
    // all three places lowered code lives. Class methods, accessors, static
    // blocks and field initializers are stored under `classes`, NOT in
    // `functions`; each gate used to build its own `init`+`functions` string
    // and a few had been patched one at a time to add `classes` (fetch, wasm,
    // zlib, regex, Math, diagnostics, …). The rest still missed a use inside a
    // class body — `new node_url_1.URL(url)` in `@redis/client`'s
    // `static parseURL` left `global-url` off, so the dynamic-construct
    // dispatcher's `URL` arm was compiled out and the "URL" it built failed
    // every `URL.prototype` brand check. Building the corpus once removes that
    // false-negative class for every gate at once (the rule is zero false
    // negatives; including more HIR can only over-include a feature).
    let hir_debug = module_hir_debug(hir_module);

    // #11125: these global constructors call stdlib stream FFIs without an
    // import or a NativeMethodCall. Discover them before entry codegen emits
    // dispatcher initialization; link-time symbol discovery is too late.
    // Include value-form constructors and uses nested in class/function bodies.
    if [
        "CompressionStream",
        "DecompressionStream",
        "TextEncoderStream",
        "TextDecoderStream",
    ]
    .iter()
    .any(|name| hir_debug.contains(name))
    {
        ctx.needs_stdlib = true;
        ctx.native_module_imports.insert("stream/web".to_string());
    }

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

    // Robust fallback for WebAssembly detection. The static lowering in
    // `module_static.rs` sets `hir_module.uses_webassembly` for direct
    // `WebAssembly.Module`/`instantiate`/etc. call sites, but a minified
    // bundle can reach the `WebAssembly` global via dynamic property access
    // (`const WA = WebAssembly; WA.Module(bytes)`, `globalThis.WebAssembly`,
    // `globalThis["WebAssembly"]`) that lowers to an ordinary `PropertyGet`
    // or `Ident` without hitting any set-site. The codegen still emits
    // `js_webassembly_*` FFI calls for those paths, so without this fallback
    // the `wasm-host` feature stays off and the link dies with
    // `_js_webassembly_module_new` undefined. Mirror the fetch/crypto
    // fallbacks above: scan the final HIR for the `WebAssembly` token.
    // Over-matching only over-links the wasm host (a size cost); the rule
    // is zero false negatives.
    if !ctx.needs_wasm_runtime {
        if hir_debug.contains("property: \"WebAssembly\"")
            || hir_debug.contains("class_name: \"WebAssembly\"")
            || hir_debug.contains("\"WebAssembly\"")
        {
            ctx.needs_wasm_runtime = true;
        }
    }

    // Detect crypto.* builtin usage (randomBytes/randomUUID/sha256/md5 used
    // without `import crypto`). The runtime symbols live behind the
    // perry-stdlib `crypto` Cargo feature, so we need to flip that on for
    // auto-optimize. Text-grep the serialized Debug form for the established
    // dedicated HIR variants. The global WebCrypto namespace path below uses
    // a structured walk because it is an ordinary `PropertyGet`.
    {
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

    // zlib per-codec cherry-pick: flag Brotli / zstd API usage so
    // `build_optimized_libs` can add `compression-brotli` /
    // `compression-zstd` on top of the `compression-gzip` base that a
    // `node:zlib` import selects. Scan classes too — a codec call inside a
    // static method body must not be stripped from an auto-optimized build.
    {
        if debug_hir_uses_zlib_brotli(&hir_debug) {
            ctx.uses_zlib_brotli = true;
        }
        if debug_hir_uses_zlib_zstd(&hir_debug) {
            ctx.uses_zlib_zstd = true;
        }
        if debug_hir_uses_web_compression_stream(&hir_debug) {
            ctx.uses_web_compression_stream = true;
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
        // Class methods and static initializers live under `classes`, not in
        // `functions`; `hir_debug` includes them so a regex/glob use there
        // cannot be stripped from an auto-optimized build.
        // A named import lowers to an `ExternFuncRef` that carries only its
        // local binding name. Use the structured import record for provenance
        // instead of treating every unrelated external named `glob` as
        // `node:fs/promises.glob`.
        if debug_hir_uses_regex(&hir_debug) || imports_fs_promises_glob(hir_module) {
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
        // Bare `Bun.*` calls have no import declaration. Lowering gives them
        // the same `module: "bun"` marker as imported calls; retain it so the
        // optimized runtime includes #9600's utility backends.
        if hir_debug.contains("module: \"bun\"") || hir_debug.contains("NativeModuleRef(\"bun\")") {
            ctx.native_module_imports.insert("bun".to_string());
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
    if debug_hir_uses_url_engine(&hir_debug) {
        ctx.uses_url = true;
    }

    // Detect `String.prototype.normalize` / `localeCompare` / `Intl.Collator`
    // (all need `unicode-normalization`, ~113 KB) and `Intl.Segmenter` (gates
    // `unicode-segmentation`, ~73 KB).
    // `normalize` and `Segmenter` lower to nodes carrying the name as a
    // `property`, so those use the exact `property: "<name>"` token.
    {
        if debug_hir_uses_string_normalization(&hir_debug) {
            ctx.uses_string_normalize = true;
        }
        if hir_debug.contains("property: \"Segmenter\"") {
            ctx.uses_intl_segmenter = true;
        }
        // `Intl.*` namespace surface (~219 KB). Every `Intl.X` access lowers
        // with `Intl` as a property/identifier token, and the locale-aware
        // prototype methods below can hand back Intl-formatted output, so any
        // of them enables the namespace. Deliberately over-approximate — a
        // missed detection leaves `Intl.NumberFormat` undefined at runtime,
        // so err toward enabling (same contract as `temporal`).
        // `localeCompare` is deliberately NOT a trigger: it returns a number,
        // and its runtime path (`string/compare.rs` + the locale/option
        // validation in `intl::validate_locale_compare`) lives outside the
        // `intl-namespace` gate. Triggering on it linked ~390 KB of unused
        // `Intl.*` constructors into every program that sorts with it.
        if hir_debug.contains("\"Intl\"")
            || hir_debug.contains("property: \"NumberFormat\"")
            || hir_debug.contains("property: \"DateTimeFormat\"")
            || hir_debug.contains("property: \"Collator\"")
            || hir_debug.contains("property: \"RelativeTimeFormat\"")
            || hir_debug.contains("property: \"ListFormat\"")
            || hir_debug.contains("property: \"PluralRules\"")
            || hir_debug.contains("property: \"DisplayNames\"")
            || hir_debug.contains("property: \"DurationFormat\"")
            || hir_debug.contains("property: \"Segmenter\"")
            || hir_debug.contains("property: \"getCanonicalLocales\"")
            || hir_debug.contains("property: \"supportedValuesOf\"")
            || hir_debug.contains("property: \"supportedLocalesOf\"")
            || hir_debug.contains("toLocale")
        {
            ctx.uses_intl_namespace = true;
        }
        // Per-namespace `globalThis` member tables (`Math`/`JSON`/`Reflect`/
        // `Atomics`). Static call sites (`Math.max(x)`, `JSON.stringify(v)`)
        // lower to codegen intrinsics that never touch these tables, so a
        // surviving mention of the name means the namespace may be used as a
        // VALUE (`const m = Math`, `Object.keys(JSON)`) — exactly when the
        // members must exist. Math value reads lose that name entirely, so
        // also scan its supported member names. (`hir_debug` includes class
        // bodies, so an extracted method in a constructor, method, accessor,
        // or field initializer cannot be pruned.) Bare matching stays
        // over-approximate on purpose (a false positive only costs size).
        if hir_debug.contains("\"Math\"") || debug_hir_uses_global_math_member(&hir_debug) {
            ctx.uses_global_math = true;
        }
        if hir_debug.contains("\"JSON\"") {
            ctx.uses_global_json = true;
        }
        if hir_debug.contains("\"Reflect\"") {
            ctx.uses_global_reflect = true;
        }
        if hir_debug.contains("\"Atomics\"") {
            ctx.uses_global_atomics = true;
        }
        // Web-platform member tables. Identifier tokens cover explicit use
        // (`new URL(u)`, `new TextDecoder()`, `crypto.subtle`); the fetch
        // value types additionally ride `uses_fetch`, because a `fetch()`
        // result is a `Response` whose methods the source may reach without
        // ever naming the type. Over-approximate by construction.
        if hir_debug.contains("\"URL")
            // …and the nodes the URL lowering folds a construction into, which
            // carry no quoted type name: the same matcher gap #340/#341 found
            // in the text arm below, and the shape the fetch fallback above
            // already guards against ("the rule is zero false negatives").
            // Latent today, because a URL instance's methods are OWN FIELDS
            // (#10823) rather than prototype members — and load-bearing the
            // moment that family moves onto `URL.prototype`, since
            // `populate_builtin_prototype_methods` builds it only under this
            // feature. Over-matching costs size, never correctness.
            || hir_debug.contains("UrlNew")
            || hir_debug.contains("UrlParse")
            || hir_debug.contains("UrlCanParse")
            || hir_debug.contains("UrlPatternNew")
            || hir_debug.contains("UrlSearchParams")
        {
            ctx.uses_global_url = true;
        }
        // Both spellings, and the second one is load-bearing since #340/#341:
        // a quoted type name (`(globalThis as any).TextDecoder`, `"TextDecoder"`)
        // AND the HIR nodes the text lowering folds a construction into
        // (`TextEncoderNew`, `TextDecoderNew`, `TextDecoderDecode`, …), which
        // carry no quotes at all. A text instance is now an ordinary object
        // linked to `TextEncoder.prototype` / `TextDecoder.prototype`, and those
        // prototypes — carrying the family's methods and accessors — are built
        // by `populate_builtin_prototype_methods` only under this feature. So a
        // program that constructs a decoder without ever naming the type in
        // quotes used to get an instance with no prototype, and every
        // `d.decode` / `d.encoding` read on it answered `undefined`. Measured;
        // `crates/perry/tests/text_decoder_dynamic_surface.rs` is exactly that
        // program shape.
        if hir_debug.contains("\"Text")
            || hir_debug.contains("TextEncoder")
            || hir_debug.contains("TextDecoder")
        {
            ctx.uses_global_text = true;
        }
        if hir_debug.contains("\"WebSocket\"") {
            ctx.uses_global_websocket = true;
        }
        if hir_debug.contains("rypto") || hir_debug.contains("\"subtle\"") {
            ctx.uses_global_webcrypto = true;
        }
        if ctx.uses_fetch
            || hir_debug.contains("\"Headers\"")
            || hir_debug.contains("\"Request\"")
            || hir_debug.contains("\"Response\"")
            || hir_debug.contains("\"Blob\"")
            || hir_debug.contains("\"File\"")
            || hir_debug.contains("\"FormData\"")
            || hir_debug.contains("\"fetch\"")
        {
            ctx.uses_global_webfetch = true;
        }
        // `process` IPC channel properties. Bare-token matching on purpose:
        // the property name reaches the runtime as a string, so any `send` /
        // `disconnect` / `connected` / `channel` mention enables the path. A
        // miss would make `process.send` undefined at runtime, so this errs
        // heavily toward enabling.
        if hir_debug.contains("\"send\"")
            || hir_debug.contains("\"disconnect\"")
            || hir_debug.contains("\"connected\"")
            || hir_debug.contains("\"channel\"")
        {
            ctx.uses_proc_ipc = true;
        }
        // `Intl.Locale`, `Intl.getCanonicalLocales(...)`, and
        // `Intl.*.supportedLocalesOf(...)` gate `perry-runtime/intl-locale`
        // (ICU4X BCP-47 canonicalization + likely-subtag expansion). These lower
        // with the constructor/method name as a `property` token.
        if hir_debug.contains("property: \"getCanonicalLocales\"")
            || hir_debug.contains("property: \"supportedLocalesOf\"")
            || hir_debug.contains("property: \"Locale\"")
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
        if hir_debug.contains("method: \"getHeapSnapshot\"")
            || hir_debug.contains("method: \"writeHeapSnapshot\"")
            || hir_debug.contains("method: \"generateHeapSnapshot\"")
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
        // `node:test` → gates `perry-runtime/mod-node-test` (the runner, and
        // with it the JSON serializer its snapshot assertions call). Like
        // dgram it is runtime-only, so `requires_stdlib` is false for it and
        // `native_module_imports` never learns about it. Match the import
        // list rather than the lowered body: `node:test` reaches the body as
        // `module: "test"`, but `node:test/reporters` lowers its specifiers
        // to bare `ExternFuncRef`s and leaves no module marker there at all,
        // so a body-only scan links a program that calls
        // `js_node_submod_install_test_reporters` against a runtime that no
        // longer defines it.
        if !ctx.uses_node_test {
            ctx.uses_node_test = hir_module.imports.iter().any(|import| {
                let bare = import
                    .source
                    .strip_prefix("node:")
                    .unwrap_or(&import.source);
                bare == "test" || bare == "test/reporters"
            });
        }
        if debug_hir_uses_get_builtin_module(&hir_debug) {
            ctx.uses_get_builtin_module = true;
        }
    }

    // Detect readline usage via process.stdin raw/lifecycle methods. These
    // don't go through an `import 'readline'` statement, so the import-based
    // needs_stdlib detection above misses them.
    {
        if hir_debug.contains("ProcessStdinSetRawMode")
            || hir_debug.contains("ProcessStdinOn")
            || hir_debug.contains("ProcessStdinRemoveListener")
            || hir_debug.contains("ProcessStdinLifecycle")
        {
            ctx.needs_stdlib = true;
            ctx.native_module_imports.insert("readline".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        debug_hir_uses_get_builtin_module, debug_hir_uses_global_math_member, debug_hir_uses_regex,
        debug_hir_uses_string_normalization, debug_hir_uses_web_compression_stream,
        debug_hir_uses_zlib_brotli, debug_hir_uses_zlib_zstd, imports_fs_promises_glob,
    };
    use perry_hir::{Import, ImportSpecifier, Module, ModuleKind};

    #[test]
    fn regex_gate_detects_static_and_dynamic_path_matches_glob() {
        assert!(debug_hir_uses_regex(
            r#"PathWin32 { method: MatchesGlob, args: [] }"#
        ));
        assert!(debug_hir_uses_regex(
            r#"NativeMethodCall { module: String("path.win32"), method: String("matchesGlob"), args: [] }"#
        ));
        assert!(debug_hir_uses_regex(
            r#"NativeMethodCall { module: "bun", method: "Glob", args: [] }"#
        ));
    }

    #[test]
    fn zlib_codec_gates_detect_static_and_dynamic_tokens() {
        // Direct native-table lowering.
        assert!(debug_hir_uses_zlib_brotli(
            r#"NativeMethodCall { module: "zlib", method: "brotliCompressSync", args: [] }"#
        ));
        // Factory + constants spellings.
        assert!(debug_hir_uses_zlib_brotli(
            r#"NativeMethodCall { module: "zlib", method: "createBrotliDecompress" }"#
        ));
        assert!(debug_hir_uses_zlib_brotli(
            r#"PropertyGet { property: "BROTLI_PARAM_QUALITY" }"#
        ));
        assert!(debug_hir_uses_zlib_zstd(
            r#"NativeMethodCall { module: "zlib", method: "zstdCompressSync" }"#
        ));
        assert!(debug_hir_uses_zlib_zstd(
            r#"NativeMethodCall { module: "zlib", method: "createZstdCompress" }"#
        ));
        assert!(debug_hir_uses_zlib_zstd(
            r#"PropertyGet { property: "ZSTD_c_compressionLevel" }"#
        ));
        // A gzip-only program keeps both codec gates off — that's the size win.
        let gzip_only =
            r#"NativeMethodCall { module: "zlib", method: "gzipSync" } method: "gunzipSync""#;
        assert!(!debug_hir_uses_zlib_brotli(gzip_only));
        assert!(!debug_hir_uses_zlib_zstd(gzip_only));
    }

    #[test]
    fn web_compression_stream_gate_keys_on_the_constructor_not_the_format() {
        // Strings taken verbatim from `--print-hir` dumps, not guessed.
        // A runtime format: the literal "brotli" appears nowhere, which is
        // exactly what a zlib-style `contains("rotli")` detector would miss.
        let runtime_format = r#"New { class_name: "CompressionStream", args: [LocalGet(0)], type_args: [], byte_offset: 50, cap_args_appended: 0 }"#;
        assert!(!debug_hir_uses_zlib_brotli(runtime_format));
        assert!(debug_hir_uses_web_compression_stream(runtime_format));
        // DecompressionStream is checked on its own: its `c` is lowercase.
        assert!(debug_hir_uses_web_compression_stream(
            r#"New { class_name: "DecompressionStream", args: [String("deflate")] }"#
        ));
        // An alias lowers to a property read on globalThis.
        assert!(debug_hir_uses_web_compression_stream(
            r#"PropertyGet { object: GlobalGet(0), property: "CompressionStream", byte_offset: 0 }"#
        ));
        // A fetch-only program reads a response body but never names either
        // constructor — that is the ~820 KB encoder this gate keeps out.
        let fetch_only = r#"Call { callee: GlobalGet(3) } PropertyGet { property: "text" } PropertyGet { property: "body" }"#;
        assert!(!debug_hir_uses_web_compression_stream(fetch_only));
    }

    #[test]
    fn get_builtin_module_gate_detects_direct_and_extracted_calls() {
        assert!(debug_hir_uses_get_builtin_module(
            r#"NativeMethodCall { module: "process", method: "getBuiltinModule" }"#
        ));
        assert!(debug_hir_uses_get_builtin_module(
            r#"PropertyGet { property: "getBuiltinModule" }"#
        ));
        assert!(!debug_hir_uses_get_builtin_module(
            r#"NativeMethodCall { module: "process", method: "cwd" }"#
        ));
    }

    #[test]
    fn string_normalization_gate_covers_normalize_and_locale_compare() {
        assert!(debug_hir_uses_string_normalization(
            r#"PropertyGet { property: "normalize" }"#
        ));
        assert!(debug_hir_uses_string_normalization(
            r#"StringMethod { method: "localeCompare" }"#
        ));
        assert!(debug_hir_uses_string_normalization(
            r#"PropertyGet { property: "Collator" }"#
        ));
        assert!(!debug_hir_uses_string_normalization(
            r#"StringMethod { method: "toLowerCase" }"#
        ));
    }

    #[test]
    fn global_math_gate_detects_extracted_members_but_not_direct_intrinsics() {
        assert!(debug_hir_uses_global_math_member(
            r#"PropertyGet { object: GlobalGet(0), property: "cos", optional: false }"#
        ));
        assert!(debug_hir_uses_global_math_member(
            r#"PropertyGet { object: GlobalGet(0), property: "imul", optional: false }"#
        ));
        assert!(debug_hir_uses_global_math_member(
            r#"PropertyGet { object: GlobalGet(0), property: "f16round", optional: false }"#
        ));
        assert!(!debug_hir_uses_global_math_member("MathCos(Number(0.0))"));
        assert!(!debug_hir_uses_global_math_member(
            r#"PropertyGet { object: GlobalGet(0), property: "stringify", optional: false }"#
        ));
    }

    fn detect_for_source(source: &str) -> crate::commands::compile::CompilationContext {
        let ast = perry_parser::parse_typescript(source, "entry.ts").expect("parse");
        let hir =
            perry_hir::lower_module(&ast, "entry", "/tmp/feature-detect/entry.ts").expect("lower");
        let mut ctx =
            crate::commands::compile::CompilationContext::new(std::path::PathBuf::from("/tmp"));
        super::detect_optional_feature_usage(&mut ctx, &hir);
        ctx
    }

    #[test]
    fn web_transform_constructors_enable_dispatch_before_codegen() {
        for constructor in [
            "CompressionStream",
            "DecompressionStream",
            "TextEncoderStream",
            "TextDecoderStream",
        ] {
            let args =
                if constructor.contains("Compression") || constructor.contains("Decompression") {
                    "\"gzip\""
                } else {
                    ""
                };
            for source in [
                format!("const stream = new {constructor}({args});"),
                format!("function make() {{ return new {constructor}({args}); }}"),
                format!("class C {{ make() {{ return new {constructor}({args}); }} }}"),
                format!("const Factory = {constructor}; const stream = new Factory({args});"),
            ] {
                let ctx = detect_for_source(&source);
                assert!(
                    ctx.needs_stdlib,
                    "dispatcher initialization missing: {source}"
                );
                assert!(
                    ctx.native_module_imports.contains("stream/web"),
                    "stream feature missing: {source}"
                );
            }
        }
        let control = detect_for_source("class C { make() { return {}; } }");
        assert!(!control.needs_stdlib);
        assert!(!control.native_module_imports.contains("stream/web"));
    }

    /// #11121: `@redis/client`'s `static parseURL` is the only URL use in its
    /// module. A token that exists only inside a class body must still enable
    /// the gates, or the dynamic `new ns.URL(u)` dispatcher arm is compiled out.
    #[test]
    fn url_use_only_inside_a_class_body_enables_the_url_gates() {
        let ctx = detect_for_source(
            r#"
const node_url_1 = require("node:url");
class C {
  static parseURL(url: string) {
    return new node_url_1.URL(url).hostname;
  }
}
"#,
        );
        assert!(ctx.uses_global_url, "global-url must be enabled");
        assert!(ctx.uses_url, "url-engine must be enabled");

        let control = detect_for_source("class C { static f(x: number) { return x + 1; } }\n");
        assert!(!control.uses_global_url && !control.uses_url);
    }

    #[test]
    fn url_engine_gate_covers_value_form_and_dynamic_require() {
        use super::debug_hir_uses_url_engine as uses;
        assert!(uses(
            r#"PropertyGet { object: LocalGet(25), property: "URL" }"#
        ));
        assert!(uses(r#"PropertyGet { property: "URLSearchParams" }"#));
        assert!(uses(
            r#"Call { callee: LocalGet(6), args: [String("node:url")] }"#
        ));
        assert!(uses(
            r#"Call { callee: LocalGet(6), args: [String("url")] }"#
        ));
        assert!(uses(
            r#"NativeMethodCall { module: "url", method: "fileURLToPath" }"#
        ));
        assert!(!uses(
            r#"Let { name: "baseUrl", init: Some(String("https://x")) }"#
        ));
    }

    /// Same corpus for every gate: a class-body-only use of another token-grep
    /// gate that used to scan init+functions only.
    #[test]
    fn temporal_use_only_inside_a_class_body_enables_the_gate() {
        let ctx = detect_for_source(
            "class C { get now() { return (Temporal as any).Now.instant(); } }\n",
        );
        assert!(ctx.uses_temporal);
    }

    #[test]
    fn fs_promises_glob_gate_uses_import_provenance() {
        let mut module = Module::new("entry.ts");
        module.imports.push(Import {
            source: "node:fs/promises".to_string(),
            specifiers: vec![ImportSpecifier::Named {
                imported: "glob".to_string(),
                local: "findFiles".to_string(),
            }],
            is_native: true,
            module_kind: ModuleKind::NativeCompiled,
            resolved_path: None,
            type_only: false,
            runtime_erased: false,
            is_dynamic: false,
            is_dynamic_target: false,
            is_deferred_require: false,
            is_adopted_require: false,
        });
        assert!(imports_fs_promises_glob(&module));

        module.imports[0].source = "./util".to_string();
        assert!(
            !imports_fs_promises_glob(&module),
            "an unrelated named glob import must not retain the regex engine"
        );
    }
}
