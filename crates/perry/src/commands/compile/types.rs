//! Public types for the compile pipeline.
//!
//! Extracted from `commands/compile.rs` in v0.5.1019 (file-size CI gate).
//! Re-exported from `compile.rs` so existing `commands::compile::*`
//! paths (used by `commands::sandbox_profile`,
//! `commands::compile::sandbox_buildrs`, and the sibling sub-modules
//! via `super::CompilationContext` etc.) keep resolving.

use clap::Args;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use perry_hir::{Module as HirModule, ModuleKind};

use crate::OutputFormat;

/// Result of a successful compilation
pub struct CompileResult {
    pub output_path: PathBuf,
    pub target: String,
    pub bundle_id: Option<String>,
    pub is_dylib: bool,
    /// V2.2 codegen cache stats from this build, when the cache was enabled.
    /// `None` when disabled (`--no-cache`, `PERRY_NO_CACHE=1`, or bitcode-link mode).
    /// Tuple is `(hits, misses, stores, store_errors)`.
    pub codegen_cache_stats: Option<(usize, usize, usize, usize)>,
}

// Helpers moved to sub-modules:
// - `target_bundle_section`, `toml_string`, `toml_build_number`,
//   `package_bundle_id_from_input`, `read_app_metadata`,
//   `rust_target_triple` → `app_metadata.rs`
// - `package_name_for_path`, `write_audit_manifest`,
//   `allowlist_matches`, `write_audit_manifest_logging_failures`
//   → `audit_manifest.rs`

/// In-memory TypeScript AST cache used by `perry dev` to skip reparsing
/// unchanged files across rebuilds in a single dev session.
///
/// Keyed by canonical path. Staleness check is a full source byte comparison
/// — if the bytes match what we parsed last time, reuse the cached `Module`;
/// otherwise reparse and replace the entry. Content-addressed invalidation
/// means formatter-on-save that rewrites trivia invalidates us correctly,
/// and we never get confused by mtime weirdness (git checkout, touch, etc.).
///
#[derive(Args, Debug)]
pub struct CompileArgs {
    /// Input TypeScript file
    pub input: PathBuf,

    /// Output executable name
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Keep intermediate files (for debugging)
    #[arg(long)]
    pub keep_intermediates: bool,

    /// Print the HIR (for debugging)
    #[arg(long)]
    pub print_hir: bool,

    /// Don't link, just produce object file
    #[arg(long)]
    pub no_link: bool,

    /// Enable WebAssembly host runtime so the produced binary can load .wasm
    /// modules at runtime via `WebAssembly.instantiate(bytes)`. Engine: wasmi
    /// (pure-Rust interpreter). Adds ~1MB to the binary. Issue #76.
    #[arg(long)]
    pub enable_wasm_runtime: bool,

    /// Target platform: ios-simulator, ios, visionos-simulator, visionos,
    /// android, ios-widget, ios-widget-simulator, watchos-widget,
    /// watchos-widget-simulator, android-widget, wearos-tile, web, wasm,
    /// windows, linux (default: native host). See docs/src/cli/flags.md
    /// for the full target table.
    #[arg(long)]
    pub target: Option<String>,

    /// App bundle identifier (required for widget targets)
    #[arg(long)]
    pub app_bundle_id: Option<String>,

    /// Output type: executable (default), dylib (shared library plugin), or
    /// staticlib (`.a` / `.lib` archive for embedding into a Rust/C/C++ host
    /// — see #1088). dylib and staticlib both skip embedded-`main` emission
    /// and expose `perry_module_init`.
    #[arg(long, default_value = "executable")]
    pub output_type: String,

    /// Bundle TypeScript extensions from directory.
    /// Scans subdirectories for package.json with openclaw.extensions entries
    /// and compiles them into the binary as static plugins.
    #[arg(long)]
    pub bundle_extensions: Option<PathBuf>,

    /// Enable type checking via tsgo (Microsoft's native TypeScript checker).
    /// Resolves cross-file types, interfaces, and generics for better optimization.
    /// Requires: npm install -g @typescript/native-preview
    #[arg(long)]
    pub type_check: bool,

    /// Minify and obfuscate JavaScript output (name mangling + whitespace removal).
    /// Automatically enabled for --target web.
    #[arg(long)]
    pub minify: bool,

    /// Enable compile-time feature flags (comma-separated).
    /// Each feature becomes a `__feature_NAME__` constant (0 or 1) for dead-code elimination.
    /// Example: --features plugins,experimental
    #[arg(long)]
    pub features: Option<String>,

    /// Enable geisterhand in-process input fuzzer (debug/testing).
    /// Starts an HTTP server for programmatic UI interaction.
    #[arg(long)]
    pub enable_geisterhand: bool,

    /// Port for the geisterhand HTTP server (default: 7676).
    /// Implies --enable-geisterhand.
    #[arg(long)]
    pub geisterhand_port: Option<u16>,

    /// Backward-compat alias — auto-optimization is on by default and
    /// already does what this flag used to do (and more). Setting it has
    /// no effect on the resulting binary; kept so existing scripts don't
    /// break.
    #[arg(long, hide = true)]
    pub minimal_stdlib: bool,

    /// Disable automatic build-profile optimization for the user binary.
    /// By default Perry inspects the project's imports and rebuilds
    /// perry-runtime + perry-stdlib with the smallest matching Cargo
    /// feature set, plus `panic = "abort"` when no `catch_unwind` callers
    /// are reachable (no `perry/ui`, `perry/thread`, `perry/plugin`, or
    /// geisterhand). The result is typically 30%+ smaller. Pass this flag
    /// to fall back to the prebuilt full stdlib + unwind runtime, e.g.
    /// when reproducing an old build or when the workspace source isn't
    /// available.
    #[arg(long)]
    pub no_auto_optimize: bool,

    /// Retain debug symbols in the output binary instead of stripping
    /// them for size. On Windows this adds `/DEBUG` to the lld-link
    /// invocation so a `.pdb` is emitted and `RUST_BACKTRACE` panics in
    /// the compiled app (including perry-runtime frames) symbolize to
    /// `file:line` — essential for diagnosing runtime crashes that are
    /// otherwise an unreadable wall of `<unknown>`. Also skips `/OPT:ICF`
    /// (identical-COMDAT folding) so distinct functions don't collapse
    /// to one symbol in the backtrace. Larger binary; off by default.
    ///
    /// On Linux/macOS (#1663) this now skips the final `strip` and emits
    /// `-g` DWARF, so a SIGSEGV in a compiled service backtraces to real
    /// `js_*`/user function names + `file:line` under lldb/gdb instead of
    /// `??`. Implemented by promoting the flag to the `PERRY_DEBUG_SYMBOLS`
    /// env var in the compile driver, which codegen, the object-cache key,
    /// and the strip step already honor.
    #[arg(long)]
    pub debug_symbols: bool,

    /// Disable the per-module object cache at `.perry-cache/objects/`.
    /// By default Perry caches each module's object bytes keyed by a
    /// hash of the source plus every `CompileOptions` field that can
    /// affect codegen, so unchanged modules skip the LLVM pipeline on
    /// subsequent builds. Pass this flag (or set `PERRY_NO_CACHE=1`)
    /// to force a full recompile, e.g. to reproduce an issue or work
    /// around a suspected stale cache.
    #[arg(long)]
    pub no_cache: bool,

    /// Enable LLVM `reassoc` per-instruction fast-math flags on every
    /// f64 op. Off by default — Perry produces bit-exact f64 output with
    /// Node. With this flag, the optimizer is permitted to reassociate FP
    /// chains (e.g. `(a + b) + c → a + (b + c)`). It also implies
    /// `--fp-contract=fast` unless contraction is explicitly configured.
    /// Also settable via `PERRY_FAST_MATH=1` env var or
    /// `"perry": { "fastMath": true }` in package.json (CLI flag wins).
    /// See `docs/src/cli/fast-math.md` for the full behavior contract.
    #[arg(long)]
    pub fast_math: bool,

    /// Control floating-point contraction separately from broad fast-math.
    /// `off` preserves independent multiply/add rounding. `on` and `fast`
    /// emit LLVM `contract` on f64 ops so FMA-shaped code may fuse without
    /// also enabling reassociation. When omitted, `--fast-math` keeps its
    /// historical behavior by implying `fast`; otherwise the default is
    /// `off`.
    #[arg(long, value_parser = ["off", "on", "fast"])]
    pub fp_contract: Option<String>,

    /// Verify native-representation lowering records after codegen. Also
    /// settable via `PERRY_VERIFY_NATIVE_REGIONS=1`. Enables compiler
    /// invariant checks and disables the per-module object cache for this
    /// build so lowering always runs.
    #[arg(long)]
    pub verify_native_regions: bool,

    /// Disable native Buffer/Uint8Array direct-load/store lowering. Also
    /// settable via `PERRY_DISABLE_BUFFER_FAST_PATH=1`; useful for A/B
    /// benchmarking the helper fallback against the native fast path.
    #[arg(long)]
    pub disable_buffer_fast_path: bool,

    /// #504 — emit `<binary>.attest.json` next to the compiled
    /// executable. The sidecar carries SHA-256 of the binary +
    /// provenance (perry version, git commit, build timestamp) so
    /// downstream users can verify a downloaded binary matches the
    /// source. Verify with `perry verify --attest <binary>`.
    /// Also settable via `PERRY_EMIT_ATTEST=1` env or
    /// `"perry": { "emitAttest": true }` in package.json.
    /// See `docs/src/cli/emit-attest.md`.
    #[arg(long)]
    pub emit_attest: bool,
    /// #506 — emit a kernel-enforced sandbox profile next to the
    /// compiled binary. macOS: `<binary>.sandbox` sandbox-exec
    /// profile derived from the build's reachable stdlib surface
    /// (deny `proc:exec` if `child_process` isn't imported, deny
    /// outbound network if `fetch`/`net` isn't, etc.). Linux + other
    /// platforms: documented as a follow-up.
    ///
    /// Also settable via `PERRY_EMIT_SANDBOX=1` env var or
    /// `"perry": { "emitSandbox": true }` in package.json.
    /// See `docs/src/cli/emit-sandbox.md`.
    #[arg(long)]
    pub emit_sandbox: bool,
    /// #496 — fail the build if any of the standard arbitrary-code-
    /// execution surfaces are reachable: `perry-jsruntime` (QuickJS)
    /// is in the graph, any `perry.nativeLibrary` archive is
    /// referenced, or any source module reaches `child_process.*`.
    /// Most apps need none of these; lockdown is a one-line opt-in
    /// to "this app is provably free of arbitrary-code-execution
    /// vectors." Also settable via `PERRY_LOCKDOWN=1` env var or
    /// `"perry": { "lockdown": true }` in package.json.
    /// See `docs/src/cli/lockdown.md`.
    #[arg(long)]
    pub lockdown: bool,

    /// Minimum Windows version the compiled executable must run on.
    /// Accepted values: `7`, `8`, `10` (default `10`). Ignored on every
    /// non-Windows target.
    ///
    /// `10` (default) preserves current behavior — the linker picks its
    /// default subsystem version (Win8+).
    ///
    /// `7` emits the linker subsystem suffix `,5.1` (e.g.
    /// `/SUBSYSTEM:WINDOWS,5.1`) so the PE marks itself Win7-compatible.
    /// Perry's UI runtime falls back through the DPI API tiers
    /// (Win10 → Win8.1 → Vista) at startup via lazy GetProcAddress.
    /// Caveats: programs that import any `.js` module pull V8/deno_core,
    /// which is unconditionally Win10+; cosmetic effects like dark
    /// titlebars and rounded corners silently no-op on Win7. See
    /// `docs/src/platforms/windows-7.md` for the full story (issue #303).
    ///
    /// `8` emits `,6.02` — useful when a deployment baseline is Win8/10
    /// without needing Win7.
    #[arg(long, default_value = "10")]
    pub min_windows_version: String,

    /// HarmonyOS HAP signing: path to the .p12 keystore file. Falls
    /// through CLI flag → `PERRY_HARMONYOS_P12` env var → saved config
    /// (`~/.perry/config.toml`, populated by `perry setup harmonyos`).
    /// Phase 2 v7: lets CI/scripted deploys point at a keystore without
    /// shell config. Only consulted on `--target harmonyos`.
    #[arg(long)]
    pub p12_keystore: Option<PathBuf>,

    /// HarmonyOS HAP signing: keystore password. Same fallback chain as
    /// `--p12-keystore`. Phase 2 v7. Prefer this over the env var when
    /// scripting against multiple keystores in one CI job — env vars are
    /// process-global, the flag is per-invocation.
    #[arg(long)]
    pub p12_password: Option<String>,

    /// HarmonyOS HAP signing: app cert chain (.cer / .pem). Falls through
    /// CLI flag → `PERRY_HARMONYOS_CERT` env var → saved config. Phase 2
    /// v7. Distinct from the provisioning profile (`--harmonyos-profile`).
    #[arg(long)]
    pub harmonyos_cert: Option<PathBuf>,

    /// HarmonyOS HAP signing: signed provisioning profile (.p7b). Falls
    /// through CLI flag → `PERRY_HARMONYOS_PROFILE` env var → saved
    /// config. Phase 2 v7.
    #[arg(long)]
    pub harmonyos_profile: Option<PathBuf>,

    /// HarmonyOS HAP signing: keystore alias (defaults to `debugKey`,
    /// matching DevEco's auto-generated debug certs). Falls through CLI
    /// flag → `PERRY_HARMONYOS_KEY_ALIAS` env var → saved config →
    /// `debugKey`. Phase 2 v7.
    #[arg(long)]
    pub harmonyos_key_alias: Option<String>,

    /// Widget targets (`ios-widget`, `ios-widget-simulator`, `watchos-widget`,
    /// `watchos-widget-simulator`): skip auto-invoking `swiftc` and emit only
    /// the SwiftUI source + Info.plist. The build instructions are printed so
    /// a downstream pipeline (Xcode project, custom xcodebuild script) can
    /// pick them up. By default Perry drives swiftc itself and produces a
    /// built `WidgetExtension.appex` directory next to the sources.
    #[arg(long)]
    pub skip_swift_build: bool,
}

/// Information about a JavaScript module that will be interpreted at runtime
#[derive(Debug, Clone)]
pub struct JsModule {
    /// Absolute path to the JS file
    pub path: PathBuf,
    /// Source code of the JS module
    pub source: String,
    /// Module specifier used in imports (e.g., "lodash", "./utils.js")
    pub specifier: String,
}

/// Compilation context tracking all modules
pub struct CompilationContext {
    /// Native TypeScript modules to compile
    pub native_modules: BTreeMap<PathBuf, HirModule>,
    /// JavaScript modules to interpret via V8
    pub js_modules: BTreeMap<String, JsModule>,
    /// Mapping from import specifiers to resolved paths
    pub import_map: BTreeMap<String, PathBuf>,
    /// Whether the WebAssembly host runtime is needed (codegen detected
    /// `WebAssembly.*` usage OR the user passed `--enable-wasm-runtime`).
    /// Issue #76.
    pub needs_wasm_runtime: bool,
    /// Whether perry/ui module is imported (needs UI library linking).
    /// On the harmonyos target this is forced back to false after the
    /// destructive Phase-2 ArkUI harvest (see `harmonyos_index_ets`) — UI
    /// is rendered declaratively from the emitted `.ets`, not via FFIs.
    pub needs_ui: bool,
    /// HarmonyOS Phase 2: ArkUI source harvested from the entry module's
    /// `App({body: ...})` call by `perry-codegen-arkts::emit_index_ets`.
    /// `Some(...)` means the link path can skip the perry-ui-* lib (UI
    /// is in the .ets, not the .so) and the post-link writer should drop
    /// this content into `<output_dir>/ets/pages/Index.ets`. `None` means
    /// the program uses no UI; falls through to the blank EntryAbility.
    pub harmonyos_index_ets: Option<String>,
    /// Whether perry/plugin module is imported (needs -rdynamic for symbol export)
    pub needs_plugins: bool,
    /// Whether perry-stdlib is needed (heavy native modules like fastify, mysql2, etc.)
    pub needs_stdlib: bool,
    /// Project root (where we start looking for node_modules)
    pub project_root: PathBuf,
    /// External native libraries discovered from package dependencies
    pub native_libraries: Vec<NativeLibraryManifest>,
    /// Package aliases: maps npm package name → replacement package name (from perry.packageAliases)
    pub package_aliases: HashMap<String, String>,
    /// Packages to compile natively instead of routing to V8 (from perry.compilePackages)
    pub compile_packages: HashSet<String>,
    /// Resolved `--fast-math` setting for this build. Default false.
    /// Sources, last wins: `perry.fastMath` in package.json → env var
    /// `PERRY_FAST_MATH=1` → CLI `--fast-math`. Drives the per-instruction
    /// LLVM `reassoc` FMF emission in `perry-codegen` and is hashed into
    /// the per-module object cache key so toggling it invalidates cached
    /// `.o` bytes.
    pub fast_math: bool,
    /// Resolved `--fp-contract=off|on|fast` setting for this build.
    /// Package/env/CLI explicit values win over the `--fast-math` implied
    /// default, allowing `--fast-math --fp-contract=off` to keep
    /// reassociation while disabling FMA contraction.
    pub fp_contract_mode: perry_codegen::FpContractMode,
    /// App metadata backing `perry/system` compile-time introspection APIs
    /// (`getAppVersion`/`getAppBuildNumber`/`getBundleId`). Resolved once
    /// from `perry.toml` + CLI overrides and reused by every codegen
    /// backend so native, JS and arkts agree byte-for-byte.
    pub app_metadata: perry_codegen::AppMetadata,
    /// First-resolved directory for each compile package (deduplication across nested node_modules)
    pub compile_package_dirs: HashMap<String, PathBuf>,
    /// Optional tsgo type checker client (when --type-check is enabled)
    pub type_checker: Option<crate::commands::typecheck::TsGoClient>,
    /// Cache for resolve_import results: (import_source, importer_dir) -> Option<(resolved_path, kind)>
    pub resolve_cache: HashMap<(String, PathBuf), Option<(PathBuf, ModuleKind)>>,
    /// Cache for find_node_modules results: start_dir -> Option<node_modules_dir>
    pub node_modules_cache: HashMap<PathBuf, Option<PathBuf>>,
    /// Whether geisterhand (in-process input fuzzer) is enabled
    pub needs_geisterhand: bool,
    /// Port for geisterhand HTTP server (default 7676)
    pub geisterhand_port: u16,
    /// Set of native module specifiers actually imported by this project
    /// (e.g. "mysql2", "fastify", "ws"). Used by `--minimal-stdlib` to
    /// compute the smallest perry-stdlib feature set that satisfies them.
    pub native_module_imports: BTreeSet<String>,
    /// Whether any TS module calls global `fetch()` (which routes to
    /// reqwest in perry-stdlib's http-client feature).
    pub uses_fetch: bool,
    /// Whether any TS module uses `crypto.randomBytes` / `randomUUID` /
    /// `sha256` / `md5` as Perry builtins (without `import crypto`).
    /// These lower to `Expr::CryptoRandomBytes`/`CryptoRandomUUID`/
    /// `CryptoSha256`/`CryptoMd5` which dispatch to runtime symbols that
    /// live behind the perry-stdlib `crypto` feature.
    pub uses_crypto_builtins: bool,
    /// Whether `perry/thread` is imported. When true, the runtime must
    /// keep `panic = "unwind"` so that worker-thread panics translate to
    /// promise rejections via `catch_unwind` in `perry-runtime/src/thread.rs`
    /// instead of aborting the whole process.
    pub needs_thread: bool,
    /// Cross-module class field types collected post-order in
    /// `collect_modules`. Each parent module's HIR lowering pre-seeds its
    /// `LoweringContext::class_field_types` from this map so type inference
    /// can resolve `someLocal.field` where `someLocal`'s declared type is a
    /// class defined in another module. Without this, `for (const x of
    /// changeset.removes)` where `changeset: ComponentChangeset` (defined
    /// elsewhere) silently iterates 0 times because the iterable's static
    /// type is unknown and the `SetValues`/`MapEntries` wrap is skipped at
    /// `lower_decl.rs:3737-3747`. See ECS demo-simple repro / #412.
    pub cross_module_class_field_types: HashMap<String, Vec<(String, perry_types::Type)>>,
    /// Minimum Windows version for `--target windows` builds. One of `"7"`,
    /// `"8"`, `"10"`. `"10"` (default) means "no subsystem version suffix";
    /// `"7"` and `"8"` emit `,5.1` / `,6.02` on the linker `/SUBSYSTEM:` flag
    /// so the resulting PE marks itself runnable on the older OS. See issue
    /// #303 + `docs/src/platforms/windows-7.md`. Ignored on non-Windows
    /// targets.
    pub min_windows_version: String,
    /// Issue #444: canonical path of the user-supplied entry TypeScript
    /// file. `collect_modules` compares each module's canonical path
    /// against this to set `is_entry_module` on the lowering context,
    /// driving `import.meta.main`. `None` until the first `collect_modules`
    /// call canonicalizes the entry; bundle-extension entries don't update
    /// it, so their `import.meta.main` correctly resolves to false.
    pub entry_canonical: Option<PathBuf>,
    /// Extra perry-stdlib Cargo features the codegen-side FFI registry
    /// (`perry-codegen::ext_registry`) recorded during compilation.
    /// Populated by the drain right before `build_optimized_libs` from
    /// `OwnerKind::Stdlib { feature }` entries; unioned with the
    /// feature set `compute_required_features` derives from
    /// `native_module_imports`.
    ///
    /// Closes the #835/#846 follow-up: compiled-package code (Effect's
    /// `Stream`, …) can emit `js_readable_stream_*` FFIs without any
    /// `import "streams"` in the user TS, which leaves
    /// `native_module_imports` empty and the auto-optimize stdlib
    /// rebuild without the `bundled-streams` feature. This set lets
    /// the registry drain inject the missing feature directly.
    pub extra_stdlib_features: BTreeSet<&'static str>,
    /// #503: when true, HIR lowering refuses dynamic-dispatch on known
    /// stdlib namespaces (`process[runtimeVar]()` and similar). Default
    /// true. Sources, last wins: `perry.allowDynamicStdlibDispatch: true`
    /// in package.json → env `PERRY_ALLOW_DYNAMIC_STDLIB=1` flips this
    /// off. An array value (`["@scope/pkg", ...]`) keeps refusal on but
    /// allows the listed packages — captured in
    /// `allow_dynamic_stdlib_packages`.
    pub refuse_dynamic_stdlib_dispatch: bool,
    /// #503: package names whose modules may legitimately use dynamic
    /// stdlib dispatch (`perry.allowDynamicStdlibDispatch: [...]`).
    /// Consulted per-module during HIR lowering; ignored when
    /// `refuse_dynamic_stdlib_dispatch` is false.
    pub allow_dynamic_stdlib_packages: HashSet<String>,
    /// Source files that imported a JavaScript module the resolver can
    /// only evaluate through a runtime JS engine. This Perry build is
    /// V8-free — the `perry-jsruntime` runtime was removed — so a
    /// non-empty list fails the build with a diagnostic naming the
    /// offending file(s). Records canonical paths; the owning package
    /// name (for `node_modules/<pkg>/...` files) is derived at
    /// diagnostic-emission time. Empty until `collect_modules` runs.
    pub js_runtime_importers: Vec<PathBuf>,
    /// #501: host-controlled per-package capability policy. Map of
    /// `<package_name>` (or `"*"` for the default) → allowed
    /// capability token list (e.g. `["fs:read", "net:fetch"]`).
    /// Parsed from `perry.permissions` in the host package.json.
    /// Empty means the pass is disabled — every dep gets the
    /// implicit "no policy = no enforcement" default for backwards
    /// compatibility.
    pub permissions: std::collections::BTreeMap<String, Vec<String>>,
    /// #501: host application's own npm package name (read from its
    /// own `package.json` `name` field). The capability walker
    /// grants this package `*` unconditionally — host code is what
    /// `--lockdown` mode (#496) is for, not per-package policy.
    pub host_package_name: Option<String>,
    /// #505: per-package exemption list for the build.rs sandbox.
    /// When `PERRY_SANDBOX_BUILDRS=1` is set, cargo invocations
    /// triggered by Perry's compile pipeline for `perry.nativeLibrary`
    /// crate builds are wrapped in `sandbox-exec` on macOS (denying
    /// network, restricting FS writes). Packages listed here run
    /// unsandboxed — escape hatch for build scripts with legitimate
    /// network needs (e.g. `bindgen` calls that fetch headers, or
    /// vendored-rebuild flows that pull crates fresh). Empty by
    /// default; populated from `perry.allowUnsandboxedBuild` in the
    /// host `package.json`.
    pub allow_unsandboxed_build: Vec<String>,
    /// #504 — when true, emit `<binary>.attest.json` next to the
    /// compiled binary. The sidecar holds SHA-256 + size +
    /// provenance metadata so downstream users can verify the
    /// binary matches its declared source via `perry verify --attest`.
    /// Sources, last wins: `perry.emitAttest` in package.json →
    /// `PERRY_EMIT_ATTEST=1` env → `--emit-attest` CLI.
    pub emit_attest: bool,
    /// #506 — when true, emit `<binary>.sandbox` next to the
    /// compiled binary on macOS containing a sandbox-exec profile
    /// derived from the build's reachable stdlib surface. Sources,
    /// last wins: `perry.emitSandbox: true` in package.json → env
    /// `PERRY_EMIT_SANDBOX=1` → CLI `--emit-sandbox`. The host
    /// invokes the binary via `sandbox-exec -f <binary>.sandbox
    /// <binary>` (documented in docs/src/cli/emit-sandbox.md).
    pub emit_sandbox: bool,
    /// #496 — `--lockdown` mode. When true, the build refuses to
    /// link `perry-jsruntime`, refuses any `perry.nativeLibrary`
    /// archive reference, and refuses any source module that
    /// reaches `child_process.*`. Sources, last wins:
    /// `perry.lockdown: true` in package.json → env
    /// `PERRY_LOCKDOWN=1` → CLI `--lockdown`. The build fails with
    /// one combined diagnostic naming every offending site so the
    /// reviewer can fix the whole surface at once.
    pub lockdown: bool,
    /// #502: host-controlled URL/host egress allowlist
    /// (`perry.allowedHosts: [...]` in `package.json`). When
    /// non-empty, the compile-time egress pass walks every
    /// `fetch(url)` / `net.connect(host, …)` call site and refuses
    /// any literal URL/host that doesn't match a pattern in this
    /// list. Empty = pass disabled (opt-in only — see
    /// `perry-hir::egress` for the rationale).
    pub allowed_hosts: Vec<String>,
    /// #502: explicit host opt-in to non-literal egress arguments
    /// (`fetch(someVar)` and friends). Default false: a variable URL
    /// would otherwise defeat the static `grep`-the-binary egress
    /// guarantee. Source: `perry.allowDynamicHosts: true` in host
    /// `package.json`.
    pub allow_dynamic_hosts: bool,
    /// #497: host-controlled allowlist for transitive deps that ship a
    /// `perry.nativeLibrary` manifest. Default empty = nothing allowed.
    /// Read from `perry.allow.nativeLibrary` (array of strings) in the
    /// host's `package.json` only. Patterns: exact match, prefix `*`
    /// (allow-all escape hatch), or scope-style `@scope/*`.
    pub allow_native_library: Vec<String>,
    /// #497: host-controlled allowlist for package names that may be
    /// added to `perry.compilePackages`. Default empty = nothing
    /// allowed. Same pattern syntax as `allow_native_library`. Each
    /// entry the user puts in `perry.compilePackages` must also be
    /// matched by an entry here, otherwise the build refuses. This
    /// makes the dangerous "compile this random npm package into the
    /// binary" surface a two-key opt-in.
    pub allow_compile_packages: Vec<String>,
}

impl std::fmt::Debug for CompilationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompilationContext")
            .field("native_modules", &self.native_modules.len())
            .field("js_modules", &self.js_modules.len())
            .field("type_checker", &self.type_checker.is_some())
            .finish()
    }
}

impl CompilationContext {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            native_modules: BTreeMap::new(),
            js_modules: BTreeMap::new(),
            import_map: BTreeMap::new(),
            needs_wasm_runtime: false,
            needs_ui: false,
            harmonyos_index_ets: None,
            needs_plugins: false,
            needs_stdlib: false,
            project_root,
            native_libraries: Vec::new(),
            package_aliases: HashMap::new(),
            compile_packages: HashSet::new(),
            fast_math: false,
            fp_contract_mode: perry_codegen::FpContractMode::Off,
            app_metadata: perry_codegen::AppMetadata::default(),
            compile_package_dirs: HashMap::new(),
            type_checker: None,
            resolve_cache: HashMap::new(),
            node_modules_cache: HashMap::new(),
            needs_geisterhand: false,
            geisterhand_port: 7676,
            native_module_imports: BTreeSet::new(),
            uses_fetch: false,
            uses_crypto_builtins: false,
            needs_thread: false,
            cross_module_class_field_types: HashMap::new(),
            min_windows_version: "10".to_string(),
            entry_canonical: None,
            extra_stdlib_features: BTreeSet::new(),
            refuse_dynamic_stdlib_dispatch: true,
            allow_dynamic_stdlib_packages: HashSet::new(),
            js_runtime_importers: Vec::new(),
            permissions: std::collections::BTreeMap::new(),
            host_package_name: None,
            allow_unsandboxed_build: Vec::new(),
            emit_attest: false,
            emit_sandbox: false,
            lockdown: false,
            allowed_hosts: Vec::new(),
            allow_dynamic_hosts: false,
            allow_native_library: Vec::new(),
            allow_compile_packages: Vec::new(),
        }
    }
}

/// External native library manifest parsed from package.json `perry.nativeLibrary` field
#[derive(Debug, Clone)]
pub struct NativeLibraryManifest {
    /// Package module name (e.g., "@honeide/editor")
    pub module: String,
    /// Resolved package directory path
    pub package_dir: PathBuf,
    /// `perry.nativeLibrary.abiVersion` — semver range the wrapper
    /// declares it was built against. Validated against the bundled
    /// `perry-ffi`'s version (#466 Phase 2). `None` is permitted
    /// during the v0.5.x cycle but emits a warning; from v0.6.0 it
    /// becomes a hard resolution error. See
    /// `docs/src/native-libraries/manifest-v1.md`.
    pub abi_version: Option<String>,
    /// FFI function declarations
    pub functions: Vec<NativeFunctionDecl>,
    /// Target-specific build configuration
    pub target_config: Option<TargetNativeConfig>,
}

/// An FFI function declaration from a native library manifest
#[derive(Debug, Clone)]
pub struct NativeFunctionDecl {
    pub name: String,
    pub params: Vec<String>,
    pub returns: String,
}

/// Target-specific native library build configuration
#[derive(Debug, Clone)]
pub struct TargetNativeConfig {
    pub crate_path: PathBuf,
    pub lib_name: String,
    /// If set, the absolute path to a prebuilt static library that the
    /// linker should consume directly. Skips the cargo build step
    /// entirely — `crate_path` is not used when this is `Some`.
    ///
    /// Resolved from the manifest's `prebuilt:` string (issue #860): a
    /// node-style module path like
    /// `@bloomengine/engine-darwin-arm64/lib/libbloom_macos.a` that
    /// points into a sibling package installed via npm
    /// `optionalDependencies` (the esbuild / sharp / swc / lightningcss
    /// distribution pattern).
    pub prebuilt: Option<PathBuf>,
    pub frameworks: Vec<String>,
    /// Vendored Apple frameworks linked *only* when an opt-in env var
    /// resolves to a directory holding them (issue #1304). Unlike
    /// `frameworks` (system frameworks, always linked from the SDK's
    /// `System/Library/Frameworks`), these come from a third-party SDK
    /// the app dev builds/downloads locally — e.g. GoogleSignIn for
    /// `@perryts/google-auth`. Each entry is passed as `-framework
    /// <name>`, gated on `frameworks_env`.
    ///
    /// Contract is **static frameworks only**: `-framework` links the
    /// archive directly with no `.app/Frameworks/` embed or rpath, so
    /// the vendored `.framework` must contain a static Mach-O. Dynamic
    /// frameworks (CocoaPods-built GoogleSignIn) would also need
    /// embedding + an `@executable_path/Frameworks` rpath, which is not
    /// yet implemented (see #1304's open question).
    pub optional_frameworks: Vec<String>,
    /// Name of the environment variable that points at the directory
    /// holding the `optional_frameworks` (issue #1304). When set and the
    /// referenced path is an existing directory, the link line gains
    /// `-F <dir>` plus one `-framework` per `optional_frameworks` entry.
    /// When unset (or the path is missing), the optional frameworks are
    /// skipped silently — the wrapper's Swift bridge `#if
    /// canImport(...)` fallback already compiles a no-SDK code path, so
    /// the build still links and returns a runtime "framework not
    /// linked" result instead of failing.
    pub frameworks_env: Option<String>,
    pub libs: Vec<String>,
    /// Extra `-L`/`/LIBPATH:` search paths to hand the linker before the
    /// `libs` entries are resolved. Anchored to the manifest's
    /// `package_dir`, so relative entries in `package.json` resolve
    /// against the package, not the user's cwd.
    pub lib_dirs: Vec<PathBuf>,
    pub pkg_config: Vec<String>,
    /// Swift sources (absolute paths) to compile via swiftc and link into the
    /// final binary. Used by `--features watchos-swift-app` so a native lib
    /// can ship its own `@main struct App: App` SwiftUI root.
    pub swift_sources: Vec<PathBuf>,
    /// Metal shader sources (absolute paths) to compile via `xcrun metal` and
    /// pack into `<app>.app/default.metallib`. Consumed at runtime by SwiftUI's
    /// `ShaderLibrary.default` / Metal's dynamic loader — not linked. iOS /
    /// tvOS / watchOS only.
    pub metal_sources: Vec<PathBuf>,
}
