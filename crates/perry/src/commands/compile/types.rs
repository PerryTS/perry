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
use serde::{Deserialize, Serialize};

/// Result of a successful compilation
pub struct CompileResult {
    pub output_path: PathBuf,
    pub target: String,
    pub bundle_id: Option<String>,
    // #854: set by every target builder to record library-output shape; not
    // currently read back, but part of the CompileResult contract.
    #[allow(dead_code)]
    pub is_dylib: bool,
    /// V2.2 codegen cache stats from this build, when the cache was enabled.
    /// `None` when disabled (`--no-cache`, `PERRY_NO_CACHE=1`, or bitcode-link mode).
    /// Tuple is `(hits, misses, stores, store_errors)`.
    pub codegen_cache_stats: Option<(usize, usize, usize, usize)>,
    /// Native executable link-cache status for this build. `None` for
    /// non-executable outputs and non-native targets that do not use the
    /// executable linker path.
    #[allow(dead_code)]
    pub link_cache_stats: Option<LinkCacheStats>,
    /// Native executable build-level no-op status. `Some` for native executable
    /// compile attempts; `None` for targets that bypass the native linker path.
    #[allow(dead_code)]
    pub build_cache_stats: Option<BuildCacheStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkCacheStats {
    pub linked: bool,
    pub skipped: bool,
    pub object_fingerprints_used: usize,
    pub object_files_hashed: usize,
    pub external_inputs_hashed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildCacheStats {
    pub hit: bool,
    pub reason: String,
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

    /// #1680: skip the host `package.json` `perry.codegen` build-time
    /// steps (also via `PERRY_SKIP_CODEGEN=1`). Use for reproducible /
    /// sandboxed builds where codegen output is committed and re-running
    /// the generator is unnecessary or undesirable.
    #[arg(long)]
    pub no_codegen: bool,

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

    /// C library / linkage for Linux targets: `glibc` (default, dynamic) or
    /// `musl` (fully static). `--libc musl` upgrades a Linux target
    /// (`linux` / `linux-aarch64`, or the native-host default) to its musl
    /// variant, producing a binary with no glibc loader dependency that runs
    /// on AWS Lambda `provided.al2023`, scratch/distroless containers, Cloud
    /// Run, etc. Equivalent to passing `--target linux-musl`. Ignored for
    /// non-Linux targets. See #4826.
    #[arg(long)]
    pub libc: Option<String>,

    /// CPU baseline for the generated machine code (#6125). Accepts an LLVM
    /// CPU name (`x86-64-v2`, `x86-64-v3`, `znver2`, `apple-m1`, …), `native`
    /// (tune to this machine's full instruction set — the default for host
    /// builds), or `generic`/`off` (the target architecture's portable
    /// baseline — the default for cross builds). Pin this when the binary
    /// will run on other machines: a build box with AVX-512 otherwise bakes
    /// AVX-512 into a host-native build, which SIGILLs on older x86-64 CPUs.
    /// Also settable via the `PERRY_TARGET_CPU` env var or perry.toml
    /// `[build] march`; `[build] native_tuning = false` is shorthand for
    /// `generic`. Applies to app code, and to the auto-optimized
    /// runtime/stdlib rebuild via `-C target-cpu`.
    #[arg(long)]
    pub march: Option<String>,

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

    /// Embed static assets/files into the standalone executable (#5731).
    /// Accepts files, directories, or `**`/`*`-style glob patterns relative to
    /// the project root, e.g. `--embed "./dist/**" --embed ./logo.png`. The
    /// matched bytes are baked into the binary; at runtime they are readable
    /// via `import { embeddedFiles, readEmbedded } from "perry"` and through
    /// `node:fs` at their `$perryfs/<path>` virtual path. Merged with
    /// `perry.embed` (package.json) / `[compile] embed` (perry.toml). Repeatable.
    #[arg(long)]
    pub embed: Vec<String>,

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

    /// Disable the per-module object cache.
    /// By default Perry caches each module's object bytes keyed by a
    /// hash of the source plus every `CompileOptions` field that can
    /// affect codegen, so unchanged modules skip the LLVM pipeline on
    /// subsequent builds. Pass this flag (or set `PERRY_NO_CACHE=1`)
    /// to force a full recompile, e.g. to reproduce an issue or work
    /// around a suspected stale cache.
    #[arg(long)]
    pub no_cache: bool,

    /// Directory for Perry's on-disk caches (objects, build manifests,
    /// link manifests, audit.json). Defaults to
    /// `<project-root>/node_modules/.cache/perry` (the find-cache-dir
    /// convention used by babel-loader / eslint / etc.), so the cache is
    /// auto-ignored along with `node_modules/`. Also settable via the
    /// `PERRY_CACHE_DIR` env var, `[perry] cacheDir` in perry.toml, or
    /// `"perry": { "cacheDir": "<path>" }` in package.json; precedence is
    /// CLI flag > env > perry.toml > package.json. A relative path resolves
    /// against the project root. Useful for read-only project roots and
    /// per-machine CI caches; the object cache is machine-local (native
    /// `.o` bytes keyed by CPU/OS/toolchain), so avoid sharing one
    /// directory across heterogeneous build machines.
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

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

    /// Emit a user-facing type-lowering evidence report for this build.
    /// The report aggregates native-representation artifacts into counts for
    /// boxed/materialized values, coercions, JSValueBits/native reps, direct
    /// field loads, dynamic fallbacks, scalar replacements, bounds evidence,
    /// and typed clone/fallback decisions. Implies native-region verification
    /// and disables build/object cache reuse so the report reflects this run.
    #[arg(long)]
    pub explain_lowering: bool,

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

    /// #5206 — strict-eval mode. Fail the build at compile time if any
    /// `eval(...)` or `new Function(<dynamic body>)` site has a runtime-
    /// unknown body (the historical behavior). By default such a site is
    /// instead compiled to a value that throws a descriptive `Error` only if
    /// reached, and a notice listing the degraded sites is printed at the end
    /// of the build. Also settable via `"perry": { "eval": "error" }` /
    /// `"perry": { "strict": true }` in package.json or perry.toml.
    /// `PERRY_ALLOW_EVAL=1` forces this off.
    #[arg(long)]
    pub strict_eval: bool,

    /// #5230 — strict dynamic-import mode. Fail the build at compile time if a
    /// dynamic `import(...)` has a runtime-computed (non-resolvable) specifier
    /// (the historical behavior). By default such a site is instead compiled to
    /// a rejected `Promise` that throws a descriptive `Error` only if reached,
    /// and is listed in the same end-of-build notice as deferred eval sites.
    /// Also settable via `"perry": { "dynamicImport": "error" }`; the broad
    /// `"perry": { "strict": true }` covers it too. `PERRY_ALLOW_EVAL=1` forces
    /// this off.
    #[arg(long)]
    pub strict_dynamic_import: bool,

    /// #5245 — strict-unimplemented mode. Fail the build at compile time if any
    /// recognized-but-unimplemented node/stdlib API is referenced (the
    /// historical `#463` behavior). By default such a reference is instead
    /// compiled to a value that throws a descriptive `Error` only if reached
    /// (so a `try/catch`-guarded probe — e.g. `safer-buffer`'s
    /// `process.binding('buffer')` — no longer blocks the build), and is listed
    /// in the same end-of-build notice as deferred eval / dynamic-import sites.
    /// Also settable via the broad `"perry": { "strict": true }` in
    /// package.json or perry.toml. `PERRY_ALLOW_UNIMPLEMENTED=1` forces this off.
    #[arg(long)]
    pub strict_unimplemented: bool,

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

    /// Windows PE subsystem for `--target windows` builds. One of `auto`
    /// (default), `console`, `windows`. Ignored on every non-Windows target.
    ///
    /// `auto` keeps the import-driven heuristic: a program that imports
    /// `perry/ui` links as `/SUBSYSTEM:WINDOWS` (GUI), everything else links
    /// as `/SUBSYSTEM:CONSOLE` so `console.log` reaches the terminal (the
    /// issue #120 regression guard).
    ///
    /// `windows` forces `/SUBSYSTEM:WINDOWS` — the right choice for a GUI
    /// app that renders its own window (e.g. a Bloom Engine game) but does
    /// not import `perry/ui`. Without it Windows allocates a console window
    /// alongside the game on launch.
    ///
    /// `console` forces `/SUBSYSTEM:CONSOLE` even for UI programs.
    ///
    /// Precedence: this flag wins when set to `console`/`windows`; left at
    /// `auto` it falls back to `perry.toml [windows] subsystem`, then to the
    /// import heuristic. The perry.toml fallback is what survives the
    /// `perry publish` worker round-trip (the dev shell's flags don't
    /// transfer, but perry.toml is uploaded with `--project`).
    #[arg(long, default_value = "auto")]
    pub windows_subsystem: String,

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

    /// Dump the program's intermediate representation at one or more
    /// pipeline stages, for debugging "compiled to the wrong thing"
    /// bugs. Comma-separated stage list:
    ///
    ///   - `hir`  — post-transform HIR (same data as `--print-hir`,
    ///              but honors `--focus`)
    ///   - `llvm` — per-module LLVM IR `.ll` files (wires up the
    ///              `PERRY_SAVE_LL` / `PERRY_LLVM_KEEP_IR` knobs so you
    ///              don't have to remember the env vars). Written to
    ///              `.perry-trace/llvm/`.
    ///   - `all`  — every stage above
    ///
    /// Example: `perry compile foo.ts --trace hir,llvm --focus parseRow`
    /// localizes which stage corrupted `parseRow` without scrolling a
    /// 10k-line full dump.
    #[arg(long, value_name = "STAGES")]
    pub trace: Option<String>,

    /// Restrict `--trace hir` output to functions, class methods, and
    /// classes whose name contains this substring (case-sensitive).
    /// Suppresses the imports/exports/init noise so a single function's
    /// lowered body is ~40 lines instead of buried in the full module
    /// dump. No effect unless `--trace hir` (or `--print-hir`) is set.
    #[arg(long, value_name = "NAME")]
    pub focus: Option<String>,
}

/// Information about a JavaScript module that will be interpreted at runtime
#[derive(Debug, Clone)]
pub struct JsModule {
    /// Absolute path to the JS file
    pub path: PathBuf,
    /// Source code of the JS module
    pub source: String,
    /// Module specifier used in imports (e.g., "lodash", "./utils.js")
    // #854: descriptive field on the JsModule record; not read on the
    // current V8-free path but kept for the module-graph contract.
    #[allow(dead_code)]
    pub specifier: String,
}

/// #1680 (Phase 2 of #1677): one build-time codegen step from the host
/// `package.json` `perry.codegen` array. Declared as either a bare command
/// string or `{ "command": "...", "label": "..." }`.
#[derive(Debug, Clone)]
pub struct CodegenStep {
    /// Optional human-readable label shown in build output.
    pub label: Option<String>,
    /// The shell command to run before compilation (via `sh -c`).
    pub command: String,
}

/// Compilation context tracking all modules
pub struct CompilationContext {
    /// Native TypeScript modules to compile
    pub native_modules: BTreeMap<PathBuf, HirModule>,
    /// JavaScript modules to interpret via V8
    pub js_modules: BTreeMap<String, JsModule>,
    /// Declaration sidecars discovered for resolved implementation files.
    ///
    /// Keyed by the canonical implementation path (`dist/index.js`); value is
    /// the declaration file advertised by package metadata or an adjacent
    /// sidecar (`dist/index.d.ts`). These are graph metadata, not executable
    /// modules.
    pub declaration_sidecars: BTreeMap<PathBuf, PathBuf>,
    /// Mapping from import specifiers to resolved paths
    // #854: populated import-graph metadata on the compilation context;
    // not read on the current path but part of the context contract.
    #[allow(dead_code)]
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
    /// Root for cache artifacts. Usually the package/config root or current
    /// working directory; kept separate from `project_root` so legacy
    /// module-prefix behavior does not force caches under `src/`. This is the
    /// base the cache directory resolves against, NOT the cache directory
    /// itself — read `cache_dir` for where bytes actually land.
    pub cache_root: PathBuf,
    /// Resolved on-disk cache directory for this build, computed once via
    /// `resolve_cache_dir`. Every on-disk cache (objects, audit.json, build,
    /// link, sandbox profiles) lives directly under this dir; `--trace`
    /// dumps are written separately under `.perry-trace/` (see that flag).
    /// Precedence: `--cache-dir` → `PERRY_CACHE_DIR` → perry.toml
    /// `[perry] cacheDir` → package.json `perry.cacheDir` → default
    /// `<cache_root>/node_modules/.cache/perry`.
    pub cache_dir: PathBuf,
    /// External native libraries discovered from package dependencies
    pub native_libraries: Vec<NativeLibraryManifest>,
    /// Package aliases: maps npm package name → replacement package name (from perry.packageAliases)
    pub package_aliases: HashMap<String, String>,
    /// Packages to compile natively instead of routing to V8 (from perry.compilePackages)
    pub compile_packages: HashSet<String>,
    /// #5731 — assets to embed into the standalone executable, as
    /// `(embed-relative name, absolute source path)` pairs. Populated by
    /// merging the `--embed` flag with `perry.embed` / `[compile] embed` and
    /// expanding globs/directories. The embed-relative name (e.g.
    /// `dist/index.html`) is the runtime registry key and `$perryfs/` virtual
    /// path suffix.
    pub embedded_assets: Vec<(String, PathBuf)>,
    /// #1681 (Phase 3 of #1677): true when this is the build-time capture
    /// stage (the `current_exe` subprocess), so `precompile(EXPR)` sites
    /// emit their build-time value instead of substituting. Re-installed on
    /// the lowering thread before each `lower_module_full` (rayon-safe),
    /// like the `#665`/`#503` thread-locals.
    pub precompile_capture: bool,
    /// #1681: captured build-time `precompile` results, keyed by
    /// `(source_file, span.lo)`, installed by the main compile after the
    /// capture stage and re-installed on the lowering thread per module.
    pub precompile_results: HashMap<(String, u32), String>,
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
    /// Compile package roots already checked for unsupported Node native addon markers.
    pub checked_compile_package_native_addon_roots: HashSet<PathBuf>,
    /// #1680 (Phase 2 of #1677): build-time codegen steps declared in the
    /// host `package.json` `perry.codegen`. Each is a shell command run
    /// (in `codegen_dir`) before module collection, so a codegen library
    /// with an eval-free build-time output (`ajv/standalone`, `prisma
    /// generate`, `drizzle-kit introspect`, …) emits native-compilable
    /// source the normal compile path then picks up — no runtime eval. Read
    /// only from the host package.json (never a dependency's), same trust
    /// boundary as `perry.compilePackages`.
    pub codegen_steps: Vec<CodegenStep>,
    /// Working directory for `codegen_steps` — the directory of the host
    /// `package.json` they were declared in (relative script paths resolve
    /// against it). `None` when no host package.json was found.
    pub codegen_dir: Option<PathBuf>,
    /// Optional tsgo type checker client (when --type-check is enabled)
    pub type_checker: Option<crate::commands::typecheck::TsGoClient>,
    /// Cache for resolve_import results: (import_source, importer_dir) -> Option<(resolved_path, kind)>
    pub resolve_cache: HashMap<(String, PathBuf), Option<(PathBuf, ModuleKind)>>,
    /// Cache for find_node_modules results: start_dir -> Option<node_modules_dir>
    // #854: resolver cache field on the compilation context; not read on the
    // current path but kept as part of the context contract.
    #[allow(dead_code)]
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
    /// Whether any TS module needs the regular-expression engine — a regex
    /// literal / `RegExp`, a regex-coercing string method (`.match` /
    /// `.matchAll` / `.search`), or a glob API (`path.matchesGlob` /
    /// `fs.glob*`, which compile a glob to a regex internally). When false,
    /// the auto-optimize build leaves `perry-runtime/regex-engine` off and the
    /// ~1.2 MB `regex`/`fancy-regex` machinery never links. The RegExp object's
    /// identity/display layer stays compiled, so non-regex programs still
    /// format/compare values correctly.
    pub uses_regex: bool,
    /// Whether any TS module uses the TC39 `Temporal.*` API. Gates
    /// `perry-runtime/temporal` (the `temporal_rs` engine + its transitive
    /// tz/calendar deps, ~580 KB). Independent of JS `Date`, which has its own
    /// implementation — so a program using `Date` but never `Temporal.*` links
    /// none of this.
    pub uses_temporal: bool,
    /// Whether codegen routes any construction to the native `EventEmitter`
    /// (a `new EventEmitter()` / `EventEmitterAsyncResource`, regardless of
    /// where the binding was imported from — e.g. `eventemitter3`'s default
    /// export, whose local name is `EventEmitter`). The `js_event_emitter_*`
    /// helpers live in perry-stdlib's `events` module behind `bundled-events`;
    /// without this flag a program that uses native EventEmitter but never
    /// imports `node:events` fails to link (#5140).
    pub uses_event_emitter: bool,
    /// Whether any TS module uses a WHATWG URL API (`new URL`, the hostname
    /// setter, `url.domainToASCII/Unicode`, legacy `url.resolve`,
    /// `URLSearchParams`, `URLPattern`). Gates `perry-runtime/url-engine` (the
    /// `url` + `idna` crates + transitive `percent_encoding`, ~195 KB). Perry's
    /// URL parsing is otherwise hand-rolled, so a program with no URL API links
    /// none of the host-canonicalization/IDNA machinery.
    pub uses_url: bool,
    /// Whether any TS module calls `String.prototype.normalize`. Gates
    /// `perry-runtime/string-normalize` (`unicode-normalization`, ~113 KB of
    /// NFC/NFD/NFKC/NFKD tables).
    pub uses_string_normalize: bool,
    /// Whether any TS module constructs an `Intl.Segmenter`. Gates
    /// `perry-runtime/intl-segmenter` (`unicode-segmentation`, ~73 KB of UAX #29
    /// grapheme/word/sentence tables). Other `Intl.*` APIs don't need it.
    pub uses_intl_segmenter: bool,
    /// Whether any TS module canonicalizes a locale tag via
    /// `Intl.getCanonicalLocales` or `Intl.*.supportedLocalesOf`. Gates
    /// `perry-runtime/intl-locale` (`icu_locale_core`'s data-free BCP-47 / UTS #35
    /// structural parser). A program that never canonicalizes a locale links a
    /// lighter hand-rolled fallback instead.
    pub uses_intl_locale: bool,
    /// Whether any TS module localizes a date/time — `Intl.DateTimeFormat`, or
    /// `Date.prototype.toLocale{,Date,Time}String`. Gates
    /// `perry-runtime/intl-datetime` (`icu_datetime` + its bundled CLDR
    /// date-time patterns, for byte-parity locale formatting). When absent the
    /// runtime links the lighter hand-rolled numeric fallback instead.
    pub uses_intl_datetime: bool,
    /// Whether any TS module uses a heap-snapshot API (`v8.getHeapSnapshot` /
    /// `v8.writeHeapSnapshot`) or `process.report`. Gates
    /// `perry-runtime/diagnostics` (the cold-path JSON serializers + the
    /// `serde_json` pulled only by them, ~95 KB). The env-driven dev
    /// diagnostics (`PERRY_GC_DIAG` JSON trace, typed-feedback trace dump) ride
    /// the same feature and degrade gracefully when it's off, so they're absent
    /// from size-optimized binaries unless one of these APIs is also used.
    pub uses_diagnostics: bool,
    /// Whether any TS module imports `node:dgram` (UDP sockets). Gates
    /// `perry-runtime/mod-dgram` (`crate::dgram` + `crate::dgram_reactor`,
    /// ~43 KB, incl. the `js_dgram_*` externs codegen emits direct calls to).
    /// Detected from `module: "dgram"` in the HIR (a `dgram` namespace can only
    /// arise from importing it), so a program that never imports `dgram` links
    /// none of it. NB: not via `native_module_imports`, which only tracks
    /// `requires_stdlib` modules — dgram is runtime-only.
    pub uses_dgram: bool,
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
    pub cross_module_class_field_types: HashMap<String, Vec<(String, perry_hir::types::Type)>>,
    /// Cross-module class accessor names collected alongside field types.
    /// HIR lowering uses this to avoid inferring subclass `this.x = ...`
    /// constructor writes as data fields when `x` is an inherited accessor
    /// from an imported superclass. Getter and setter names stay separate so
    /// imported accessors preserve their JavaScript descriptor capabilities.
    pub cross_module_class_accessors: HashMap<String, perry_hir::ClassAccessorNames>,
    /// Minimum Windows version for `--target windows` builds. One of `"7"`,
    /// `"8"`, `"10"`. `"10"` (default) means "no subsystem version suffix";
    /// `"7"` and `"8"` emit `,5.1` / `,6.02` on the linker `/SUBSYSTEM:` flag
    /// so the resulting PE marks itself runnable on the older OS. See issue
    /// #303 + `docs/src/platforms/windows-7.md`. Ignored on non-Windows
    /// targets.
    pub min_windows_version: String,
    /// Resolved Windows PE subsystem override for `--target windows`. One of
    /// `"auto"` (default — use the `needs_ui` import heuristic), `"console"`,
    /// or `"windows"`. Merged from `--windows-subsystem` and perry.toml
    /// `[windows] subsystem` by `validate_windows_subsystem`. Drives the
    /// `/SUBSYSTEM:` linker flag via `windows_subsystem_needs_ui`. Ignored on
    /// non-Windows targets.
    pub windows_subsystem: String,
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
    /// #503/#5263: when true, HIR lowering refuses dynamic-dispatch on known
    /// stdlib namespaces (`process[runtimeVar]()` and similar). Default
    /// **false** (#5263 — allow): dynamic member access over a *linked*
    /// namespace can only reach methods perry already statically linked, so it
    /// is dynamic selection among a known set, not arbitrary code. The refusal
    /// is re-armed by `--lockdown` (the supply-chain gate, #496), or by an
    /// explicit opt-in: `perry.allowDynamicStdlibDispatch: false` in
    /// package.json / env `PERRY_ALLOW_DYNAMIC_STDLIB=0`. `…: true` / `=1`
    /// keeps the default-allow even under those explicit knobs (last wins). An
    /// array value (`["@scope/pkg", ...]`) is captured in
    /// `allow_dynamic_stdlib_packages` and is meaningful only while refusal is
    /// active (i.e. under lockdown / explicit re-enable).
    pub refuse_dynamic_stdlib_dispatch: bool,
    /// #5206: strict-eval mode. When true, a runtime-unknown `eval(...)` /
    /// `new Function(<dynamic body>)` site is a hard compile-time refusal
    /// (the historical behavior). When false (the default), such a site is
    /// compiled to a value that throws a descriptive `Error` only if reached,
    /// and a visible end-of-compile notice lists every degraded site. Sources,
    /// last wins: `perry.eval = "defer" | "error"` / `perry.strict = true` in
    /// package.json or perry.toml → CLI `--strict-eval`. `PERRY_ALLOW_EVAL=1`
    /// always forces this off (back-compat escape hatch).
    pub strict_eval: bool,
    /// #5230: strict mode for non-resolvable (runtime-computed) dynamic
    /// `import(...)` specifiers. When true, such a site is a hard compile error
    /// (the historical behavior). When false (the default), it is deferred to a
    /// rejected `Promise` that throws a descriptive `Error` only if reached, and
    /// recorded in the shared end-of-compile notice. Defaults to `strict_eval`
    /// so the broad `perry.strict = true` covers both; overridden by the
    /// dedicated `perry.dynamicImport = "defer" | "error"` config and the
    /// `--strict-dynamic-import` CLI flag. `PERRY_ALLOW_EVAL=1` forces it off
    /// (shared AOT escape hatch).
    pub strict_dynamic_import: bool,
    /// #5245: strict mode for recognized-but-unimplemented node/stdlib APIs.
    /// When true, a reference to such an API is a hard compile-time `#463`
    /// refusal (the historical behavior). When false (the default), it is
    /// compiled to a value that throws a descriptive `Error` only if reached
    /// (so a `try/catch`-guarded probe doesn't block the build) and is recorded
    /// in the shared end-of-compile notice. Defaults to `strict_eval` so the
    /// broad `perry.strict = true` covers it; the `--strict-unimplemented` CLI
    /// flag forces it on. `PERRY_ALLOW_UNIMPLEMENTED=1` forces it off (back-compat
    /// escape hatch).
    pub strict_unimplemented: bool,
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
    /// #2309: tree-shaking / dead-code elimination enabled for this build.
    /// Off (default) ⇒ behaviour is byte-identical to pre-#2309: no refusal
    /// deferral, no reachability prune, no `process.env` define-folding.
    /// Sources (any enables): `PERRY_TREE_SHAKE=1` env, or
    /// `perry.experiments.treeShake: true` in the host `package.json`.
    pub tree_shake: bool,
    /// #2309: refusals (`new Function` RuntimeUnknown, #463 unimplemented APIs)
    /// recorded while lowering `node_modules` modules under tree-shaking,
    /// instead of hard-erroring. After reachability is computed, any whose
    /// module survived the prune is re-raised; the rest are dropped silently
    /// (the offending code never ships). Tagged with the canonical module path
    /// by the collect driver after each lower.
    pub deferred_refusals: Vec<perry_hir::DeferredRefusal>,
    /// #2309: cache of each package's `sideEffects` field, keyed by the
    /// package directory (the dir containing the owning `package.json`).
    /// Drives whether a bare side-effect import edge of a reachable module may
    /// be dropped during the prune.
    pub side_effects_cache: HashMap<PathBuf, SideEffects>,
    /// #2309 (Stage 2): build-time `process.env.X` substitutions, esbuild
    /// `define`-style. Read from `perry.define` in the host `package.json`
    /// only (same trust boundary as `compilePackages`), plus an implicit
    /// `NODE_ENV → "production"` default applied to `node_modules` code unless
    /// overridden. Keyed by the full `process.env.<NAME>` string.
    pub define: HashMap<String, DefineValue>,
    /// #5247 (CJS-wrap coordinate skew): for each CommonJS module rewritten by
    /// `cjs_wrap::wrap_commonjs_for_target`, the ORIGINAL (pre-wrap) source
    /// text plus the number of newline characters the injected wrapper prefix
    /// prepended before the original module body. Under `--debug-symbols`,
    /// codegen resolves a node's `byte_offset` (which is in WRAPPED
    /// coordinates) to a line by deducting this prefix line count and looking
    /// up the original source — so a throw renders `at <module>:<original-line>`
    /// rather than a line shifted by the preamble. Empty unless
    /// `--debug-symbols` is set (the map is only populated then), keeping the
    /// default build allocation-free.
    pub cjs_wrap_debug_sources: HashMap<PathBuf, CjsWrapDebugSource>,
    /// #5247: mirror of the CLI `--debug-symbols` flag, set after construction.
    /// Gates the CJS-wrap source mapping capture in `collect_modules` so the
    /// default build never records `cjs_wrap_debug_sources`.
    pub debug_symbols: bool,
}

/// #5247: source mapping for a CJS-wrapped module, used only by the
/// `--debug-symbols` source-location path. See `cjs_wrap_debug_sources`.
#[derive(Debug, Clone)]
pub struct CjsWrapDebugSource {
    /// The WRAPPED module source text (the injected-IIFE text perry parsed).
    /// Byte offsets on the HIR are in these coordinates, so codegen counts
    /// newlines against this to get the wrapped line number.
    pub wrapped_source: String,
    /// Newlines in the injected wrapper prefix that precede the original body
    /// in the wrapped text. A wrapped 1-based line `L` maps to original line
    /// `L - prefix_line_count` (clamped; offsets inside the preamble itself —
    /// `L <= prefix_line_count` — map to no location).
    pub prefix_line_count: u32,
}

/// #2309: a package's declared `sideEffects` (package.json). `Unknown` (the
/// default for an absent/unparseable field) is treated as side-effectful —
/// the conservative choice that never drops code.
#[derive(Debug, Clone)]
pub enum SideEffects {
    /// `"sideEffects": false` — no module in the package has observable
    /// top-level side effects; bare side-effect import edges are droppable.
    None,
    /// `"sideEffects": true`, absent, or unparseable — treat every module as
    /// side-effectful (conservative).
    Unknown,
    /// `"sideEffects": ["glob", ...]` — only files matching a glob have side
    /// effects; others are droppable. Globs are relative to the package dir.
    Globs(Vec<String>),
}

/// #2309 (Stage 2): a build-time `define` value. Parsed from `perry.define`
/// (JSON value) into a folded HIR literal.
#[derive(Debug, Clone)]
pub enum DefineValue {
    Str(String),
    Bool(bool),
    Number(f64),
    Null,
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
            declaration_sidecars: BTreeMap::new(),
            import_map: BTreeMap::new(),
            needs_wasm_runtime: false,
            needs_ui: false,
            harmonyos_index_ets: None,
            needs_plugins: false,
            needs_stdlib: false,
            cache_root: project_root.clone(),
            cache_dir: super::object_cache::resolve_cache_dir(&project_root, None),
            project_root,
            native_libraries: Vec::new(),
            package_aliases: HashMap::new(),
            compile_packages: HashSet::new(),
            embedded_assets: Vec::new(),
            precompile_capture: false,
            precompile_results: HashMap::new(),
            fast_math: false,
            fp_contract_mode: perry_codegen::FpContractMode::Off,
            app_metadata: perry_codegen::AppMetadata::default(),
            compile_package_dirs: HashMap::new(),
            checked_compile_package_native_addon_roots: HashSet::new(),
            codegen_steps: Vec::new(),
            codegen_dir: None,
            type_checker: None,
            resolve_cache: HashMap::new(),
            node_modules_cache: HashMap::new(),
            needs_geisterhand: false,
            geisterhand_port: 7676,
            native_module_imports: BTreeSet::new(),
            uses_fetch: false,
            uses_crypto_builtins: false,
            uses_regex: false,
            uses_temporal: false,
            uses_event_emitter: false,
            uses_url: false,
            uses_string_normalize: false,
            uses_intl_segmenter: false,
            uses_intl_locale: false,
            uses_intl_datetime: false,
            uses_diagnostics: false,
            uses_dgram: false,
            needs_thread: false,
            cross_module_class_field_types: HashMap::new(),
            cross_module_class_accessors: HashMap::new(),
            min_windows_version: "10".to_string(),
            windows_subsystem: "auto".to_string(),
            entry_canonical: None,
            extra_stdlib_features: BTreeSet::new(),
            // #5263: default-allow dynamic stdlib member access. Dynamic
            // `fs[name]` over the *linked* namespace can only select among
            // methods perry already statically linked (dynamic selection of a
            // known set, not arbitrary code), so it is safe by default and is
            // needed by legitimate packages (graceful-fs's symbol-keyed retry
            // queue, fs-extra's `fs[method]` wrapping). The refusal is re-armed
            // under `--lockdown` (the supply-chain gate) or by an explicit
            // `perry.allowDynamicStdlibDispatch: false` / `PERRY_ALLOW_DYNAMIC_STDLIB=0`.
            refuse_dynamic_stdlib_dispatch: false,
            strict_eval: false,
            strict_dynamic_import: false,
            strict_unimplemented: false,
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
            tree_shake: false,
            deferred_refusals: Vec::new(),
            side_effects_cache: HashMap::new(),
            define: HashMap::new(),
            cjs_wrap_debug_sources: HashMap::new(),
            debug_symbols: false,
        }
    }
}

/// External native library manifest parsed from package.json `perry.nativeLibrary` field
#[derive(Debug, Clone)]
pub struct NativeLibraryManifest {
    /// Package module name (e.g., "@honeide/editor")
    pub module: String,
    /// Resolved package directory path
    // #854: set when parsing a native-library manifest; not read back yet
    // but part of the NativeLibraryManifest record.
    #[allow(dead_code)]
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
    pub params: Vec<perry_api_manifest::NativeAbiType>,
    pub returns: perry_api_manifest::NativeAbiType,
}

/// Backend package metadata a native library can attach to a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeBackend {
    Metal,
    Vulkan,
    D3d12,
}

impl NativeBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            NativeBackend::Metal => "metal",
            NativeBackend::Vulkan => "vulkan",
            NativeBackend::D3d12 => "d3d12",
        }
    }
}

/// Optional package metadata for backend-owned artifacts. This is
/// intentionally descriptive: Perry packages and copies artifacts but
/// does not create an app-level graphics API surface from this block.
#[derive(Debug, Clone, Default)]
pub struct NativeBackendPackageMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub kind: Option<String>,
}

/// Backend-specific packaging metadata nested under
/// `perry.nativeLibrary.targets.<target>.backends.<backend>`.
#[derive(Debug, Clone)]
pub struct NativeBackendConfig {
    pub backend: NativeBackend,
    /// False means the backend is intentionally unavailable for this
    /// target and should not add link/resource metadata.
    pub available: bool,
    pub unavailable_reason: Option<String>,
    /// Optional backend-specific archive to link in addition to the
    /// target-level `prebuilt` / `crate` output.
    pub prebuilt: Option<PathBuf>,
    pub frameworks: Vec<String>,
    pub libs: Vec<String>,
    pub lib_dirs: Vec<PathBuf>,
    pub pkg_config: Vec<String>,
    /// Source shaders that may require backend tools (`xcrun metal`,
    /// `glslc`, `dxc`) to build during packaging.
    pub shader_sources: Vec<PathBuf>,
    /// Precompiled shader/resource outputs (`.metallib`, `.spv`,
    /// `.cso`/`.dxil`, etc.) that Perry should package directly.
    pub shader_outputs: Vec<PathBuf>,
    /// Backend-owned resource files or directories copied into bundle
    /// resource output when the target has one.
    pub resources: Vec<PathBuf>,
    pub package: NativeBackendPackageMetadata,
}

/// Target-specific native library build configuration
#[derive(Debug, Clone)]
pub struct TargetNativeConfig {
    /// False means the package explicitly does not ship this target.
    /// Compile/link should skip it without treating missing crate/lib
    /// metadata as an error.
    pub available: bool,
    pub unavailable_reason: Option<String>,
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
    /// Target-level resource files or directories copied into bundle
    /// resource output when the target has one.
    pub resources: Vec<PathBuf>,
    /// Target-level precompiled shader/resource outputs copied into
    /// bundle resource output when the target has one.
    pub shader_outputs: Vec<PathBuf>,
    /// Backend-specific packaging metadata for Metal, Vulkan, and D3D12.
    pub backends: Vec<NativeBackendConfig>,
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
