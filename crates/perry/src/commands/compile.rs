//! Compile command - compiles TypeScript to native executable

use anyhow::{anyhow, Result};
use clap::Args;
use perry_hir::{Module as HirModule, ModuleKind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

// Tier 2.1 (v0.5.333): split out self-contained sub-concerns into the
// `compile/` directory. The `compile.rs` orchestrator stays as the
// public API surface; helpers move to focused modules so unrelated
// changes don't churn this file.
mod parse_cache;
mod strip_dedup;
mod library_search;
mod targets;
mod object_cache;
mod resolve;
mod optimized_libs;
mod collect_modules;
mod link;
pub use parse_cache::ParseCache;
use parse_cache::parse_cached;
use strip_dedup::strip_duplicate_objects_from_lib;
use library_search::{
    build_geisterhand_libs,
    find_geisterhand_library, find_geisterhand_runtime, find_geisterhand_ui,
    find_harmonyos_sdk, find_jsruntime_library, find_lld_link, find_library,
    find_llvm_tool, find_msvc_lib_paths, find_msvc_link_exe, find_perry_windows_sdk,
    find_runtime_library, find_stdlib_library, find_ui_library,
    windows_pe_subsystem_flag,
};
use targets::{
    apple_sdk_version, compile_for_android_widget, compile_for_ios_widget,
    compile_for_wasm, compile_for_watchos_widget, compile_for_wearos_tile,
    compile_metallib_for_bundle, find_visionos_swift_runtime, find_watchos_swift_runtime,
    generate_js_bundle, lookup_bundle_id_from_toml,
};
pub use object_cache::{djb2_hash, ObjectCache};
use object_cache::compute_object_cache_key;
use optimized_libs::{build_optimized_libs, OptimizedLibs};
use collect_modules::collect_modules;
use link::build_and_run_link;
pub use resolve::find_perry_workspace_root;
use resolve::{
    is_declaration_file, is_js_file,
    cached_resolve_import, compute_module_prefix, discover_extension_entries,
    extract_compile_package_dir,
    has_perry_native_library, is_in_compile_package,
    is_in_perry_native_package, parse_native_library_manifest, parse_package_specifier, resolve_import,
};

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

    /// Enable V8 JavaScript runtime for importing pure JS modules from node_modules.
    /// This is a fallback option when native compilation is not possible.
    /// WARNING: This significantly increases binary size (~10-15MB).
    #[arg(long)]
    pub enable_js_runtime: bool,

    /// Target platform: ios-simulator, ios, android, ios-widget, ios-widget-simulator (default: native host)
    #[arg(long)]
    pub target: Option<String>,

    /// App bundle identifier (required for widget targets)
    #[arg(long)]
    pub app_bundle_id: Option<String>,

    /// Output type: executable (default) or dylib (shared library plugin)
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

    /// Disable the per-module object cache at `.perry-cache/objects/`.
    /// By default Perry caches each module's object bytes keyed by a
    /// hash of the source plus every `CompileOptions` field that can
    /// affect codegen, so unchanged modules skip the LLVM pipeline on
    /// subsequent builds. Pass this flag (or set `PERRY_NO_CACHE=1`)
    /// to force a full recompile, e.g. to reproduce an issue or work
    /// around a suspected stale cache.
    #[arg(long)]
    pub no_cache: bool,
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
    /// Whether JS runtime is needed
    pub needs_js_runtime: bool,
    /// Whether perry/ui module is imported (needs UI library linking)
    pub needs_ui: bool,
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
    /// First-resolved directory for each compile package (deduplication across nested node_modules)
    pub compile_package_dirs: HashMap<String, PathBuf>,
    /// Optional tsgo type checker client (when --type-check is enabled)
    pub type_checker: Option<super::typecheck::TsGoClient>,
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
    /// Per-module source hash (djb2 over the canonical source bytes).
    /// Populated during `collect_modules` as each native TS module is read
    /// and used by V2.2's object cache to key `.perry-cache/objects/<target>/<hash>.o`
    /// entries without re-reading the source in the codegen loop. Mirrors the
    /// djb2 scheme already used by `build_optimized_libs` (see prior art).
    pub module_source_hashes: HashMap<PathBuf, u64>,
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
            needs_js_runtime: false,
            needs_ui: false,
            needs_plugins: false,
            needs_stdlib: false,
            project_root,
            native_libraries: Vec::new(),
            package_aliases: HashMap::new(),
            compile_packages: HashSet::new(),
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
            module_source_hashes: HashMap::new(),
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
    pub frameworks: Vec<String>,
    pub libs: Vec<String>,
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

/// Get the Rust target triple for a given perry target string
fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("ios-simulator") | Some("ios-widget-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") | Some("ios-widget") => Some("aarch64-apple-ios"),
        Some("visionos-simulator") => Some("aarch64-apple-visionos-sim"),
        Some("visionos") => Some("aarch64-apple-visionos"),
        Some("watchos-simulator") => Some("aarch64-apple-watchos-sim"),
        Some("watchos") => Some("arm64_32-apple-watchos"),
        Some("tvos-simulator") => Some("aarch64-apple-tvos-sim"),
        Some("tvos") => Some("aarch64-apple-tvos"),
        Some("harmonyos") => Some("aarch64-unknown-linux-ohos"),
        Some("harmonyos-simulator") => Some("x86_64-unknown-linux-ohos"),
        Some("android") => Some("aarch64-linux-android"),
        Some("linux") => Some("x86_64-unknown-linux-gnu"),
        Some("windows") => Some("x86_64-pc-windows-msvc"),
        Some("macos") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// Emit the ArkTS shim next to the compiled `.so` for HarmonyOS targets.
///
/// Writes two files:
///
/// * `ets/entryability/EntryAbility.ets` — UIAbility subclass that runs
///   the Perry TS entry once in `onCreate`. Further ArkTS lifecycle hooks
///   (background, destroy) are left as no-ops; a future version can
///   forward them into TS callbacks.
///
/// * `ets/pages/Index.ets` — the landing page referenced by
///   `windowStage.loadContent`. Minimal: a single centered `Text` that
///   confirms the TS code ran. The HAP bundler (PR B.3) will be the first
///   consumer of this file; for now we emit a valid ArkUI component so
///   DevEco Studio or `hvigor check` won't reject the shim.
///
/// `so_filename` is templated into Index.ets's `import` so the `dlopen`
/// name matches whatever the user passed to `-o` (defaults to
/// `lib<stem>.so`).
fn emit_harmonyos_arkts_stubs(output_dir: &Path, so_filename: &str) -> Result<()> {
    let ets_dir = output_dir.join("ets");
    let entryability_dir = ets_dir.join("entryability");
    fs::create_dir_all(&entryability_dir)?;

    // Phase 1 shim: one ArkTS file, no UI. `es2abc --extension ts` accepts
    // plain-TypeScript content even in a `.ets` file, so we stay compatible
    // with `pages/` conventions without needing ets-loader to desugar
    // ArkUI decorators (`@Entry @Component struct`). A real UI page would
    // pull in those decorators + `struct` syntax which are ArkTS-only;
    // that's PR C's job (TS→ArkTS emitter).
    //
    // No `windowStage.loadContent(...)` call — we don't ship a page, so
    // loading one would fail at runtime. The UIAbility still launches from
    // the home screen; the window is blank but `onCreate` runs and
    // `perryEntry.run()` executes Perry's compiled `main()`. Output goes
    // to hilog, which `hdc hilog` captures. Good enough to validate the
    // cross-compile → NAPI → TS-execution loop.
    let entry_ability = format!(
        "// Auto-generated by Perry — do not edit.\n\
         // Regenerated every `perry compile --target harmonyos`.\n\
         import UIAbility from '@ohos.app.ability.UIAbility';\n\
         import perryEntry from '{so}';\n\
         \n\
         export default class EntryAbility extends UIAbility {{\n\
         \x20\x20\x20\x20onCreate(want, launchParam) {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Run the compiled Perry program once per ability instance.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Returns the process-style exit code; ignored here.\n\
         \x20\x20\x20\x20\x20\x20\x20\x20perryEntry.run();\n\
         \x20\x20\x20\x20}}\n\
         \x20\x20\x20\x20onDestroy() {{}}\n\
         \x20\x20\x20\x20onForeground() {{}}\n\
         \x20\x20\x20\x20onBackground() {{}}\n\
         }}\n",
        so = so_filename
    );
    fs::write(entryability_dir.join("EntryAbility.ets"), entry_ability)?;

    Ok(())
}

pub fn run(args: CompileArgs, format: OutputFormat, use_color: bool, verbose: u8) -> Result<CompileResult> {
    run_with_parse_cache(args, None, format, use_color, verbose)
}

/// Same as [`run`] but accepts an optional in-memory [`ParseCache`] that
/// `perry dev` uses to reuse parsed ASTs across rebuilds in a single session.
/// Pass `None` for the batch-compile path.
pub fn run_with_parse_cache(
    args: CompileArgs,
    mut parse_cache: Option<&mut ParseCache>,
    format: OutputFormat,
    use_color: bool,
    verbose: u8,
) -> Result<CompileResult> {
    match format {
        OutputFormat::Text => println!("Collecting modules..."),
        OutputFormat::Json => {}
    }

    let project_root = args.input
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));

    let mut ctx = CompilationContext::new(project_root.clone());

    // Read perry.packageAliases from the project's package.json (if present)
    // This allows mapping npm package imports to native Perry packages at compile time.
    // Example: { "@parse/node-apn": "perry-push", "@prisma/client": "perry-prisma" }
    // Walk up from project_root (which is the parent of the entry file) to find package.json.
    let pkg_json_path = {
        let mut dir = project_root.clone();
        let mut found = None;
        loop {
            let candidate = dir.join("package.json");
            if candidate.exists() {
                found = Some(candidate);
                break;
            }
            if !dir.pop() {
                break;
            }
        }
        found
    };
    if let Some(pkg_json_path) = pkg_json_path {
        if let Ok(content) = fs::read_to_string(&pkg_json_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(aliases) = pkg.get("perry").and_then(|p| p.get("packageAliases")).and_then(|a| a.as_object()) {
                    for (from, to) in aliases {
                        if let Some(to_str) = to.as_str() {
                            match format {
                                OutputFormat::Text => println!("  Package alias: {} → {}", from, to_str),
                                OutputFormat::Json => {}
                            }
                            ctx.package_aliases.insert(from.clone(), to_str.to_string());
                        }
                    }
                }
                if let Some(compile_pkgs) = pkg.get("perry").and_then(|p| p.get("compilePackages")).and_then(|a| a.as_array()) {
                    for pkg_name in compile_pkgs {
                        if let Some(name) = pkg_name.as_str() {
                            match format {
                                OutputFormat::Text => println!("  Compile package: {}", name),
                                OutputFormat::Json => {}
                            }
                            ctx.compile_packages.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // --- i18n: parse [i18n] config from perry.toml and load locale files ---
    let mut i18n_config: Option<perry_transform::i18n::I18nConfig> = None;
    let mut i18n_translations: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

    // Walk up from project_root to find perry.toml (it may be in parent of src/)
    let toml_root = {
        let mut dir = project_root.clone();
        loop {
            if dir.join("perry.toml").exists() {
                break Some(dir);
            }
            if !dir.pop() {
                break None;
            }
        }
    };
    if let Some(ref toml_dir) = toml_root {
        let toml_path = toml_dir.join("perry.toml");
        if toml_path.exists() {
            if let Ok(content) = fs::read_to_string(&toml_path) {
                if let Ok(doc) = content.parse::<toml::Table>() {
                    if let Some(i18n) = doc.get("i18n").and_then(|v| v.as_table()) {
                        let locales: Vec<String> = i18n.get("locales")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                            .unwrap_or_default();
                        let default_locale = i18n.get("default_locale")
                            .and_then(|v| v.as_str())
                            .unwrap_or("en")
                            .to_string();
                        let dynamic = i18n.get("dynamic")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        // Parse [i18n.currencies] — locale → currency code
                        let mut currencies = HashMap::new();
                        if let Some(curr_table) = i18n.get("currencies").and_then(|v| v.as_table()) {
                            for (locale, code) in curr_table {
                                if let Some(code_str) = code.as_str() {
                                    currencies.insert(locale.clone(), code_str.to_string());
                                }
                            }
                        }

                        if !locales.is_empty() {
                            match format {
                                OutputFormat::Text => println!("  i18n: {} locale(s) [{}], default: {}",
                                    locales.len(), locales.join(", "), default_locale),
                                OutputFormat::Json => {}
                            }

                            // Load locale files
                            let locales_dir = toml_dir.join("locales");
                            for locale in &locales {
                                let locale_file = locales_dir.join(format!("{}.json", locale));
                                if locale_file.exists() {
                                    if let Ok(json_content) = fs::read_to_string(&locale_file) {
                                        match serde_json::from_str::<BTreeMap<String, String>>(&json_content) {
                                            Ok(translations) => {
                                                match format {
                                                    OutputFormat::Text => println!("    Loaded locales/{}.json ({} keys)", locale, translations.len()),
                                                    OutputFormat::Json => {}
                                                }
                                                i18n_translations.insert(locale.clone(), translations);
                                            }
                                            Err(e) => {
                                                eprintln!("  Warning: Failed to parse locales/{}.json: {}", locale, e);
                                            }
                                        }
                                    }
                                } else {
                                    eprintln!("  Warning: Locale file locales/{}.json not found", locale);
                                }
                            }

                            i18n_config = Some(perry_transform::i18n::I18nConfig {
                                locales,
                                default_locale,
                                dynamic,
                                currencies,
                            });
                        }
                    }
                }
            }
        }
    }

    // Initialize tsgo type checker if --type-check is enabled
    if args.type_check {
        match super::typecheck::TsGoClient::spawn(&project_root) {
            Ok(mut client) => {
                // Try to load the project's tsconfig.json
                if let Some(tsconfig) = super::typecheck::find_tsconfig(&project_root) {
                    match format {
                        OutputFormat::Text => println!("  Type checking enabled (tsgo)"),
                        OutputFormat::Json => {}
                    }
                    if let Err(e) = client.load_project(&tsconfig) {
                        match format {
                            OutputFormat::Text => eprintln!("  Warning: tsgo project load failed: {}. Continuing without type checking.", e),
                            OutputFormat::Json => {}
                        }
                    } else {
                        ctx.type_checker = Some(client);
                    }
                } else {
                    match format {
                        OutputFormat::Text => eprintln!("  Warning: No tsconfig.json found. Type checking disabled."),
                        OutputFormat::Json => {}
                    }
                }
            }
            Err(e) => {
                match format {
                    OutputFormat::Text => eprintln!("  Warning: {}", e),
                    OutputFormat::Json => {}
                }
            }
        }
    }

    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1; // Start at 1, 0 is reserved for "no parent"
    let skip_transforms = matches!(args.target.as_deref(), Some("web") | Some("wasm"));

    collect_modules(&args.input, &mut ctx, &mut visited, args.enable_js_runtime, format, args.target.as_deref(), &mut next_class_id, skip_transforms, parse_cache.as_deref_mut())?;

    // Bundle extensions if --bundle-extensions specified
    let mut bundled_extensions: Vec<(PathBuf, String)> = Vec::new();
    if let Some(ext_dir) = &args.bundle_extensions {
        let ext_entries = discover_extension_entries(ext_dir)?;
        match format {
            OutputFormat::Text => println!("Bundling {} extension(s)...", ext_entries.len()),
            OutputFormat::Json => {}
        }
        for (entry_path, plugin_id) in &ext_entries {
            match format {
                OutputFormat::Text => println!("  Extension: {} ({})", plugin_id, entry_path.display()),
                OutputFormat::Json => {}
            }
            collect_modules(entry_path, &mut ctx, &mut visited,
                           args.enable_js_runtime, format, args.target.as_deref(), &mut next_class_id, skip_transforms, parse_cache.as_deref_mut())?;
            bundled_extensions.push((entry_path.canonicalize()?, plugin_id.clone()));
        }
    }

    // Recompute project_root as the common ancestor of all module paths.
    // The initial project_root is the parent of the entry file, but modules may be in sibling
    // directories (e.g., entry in workers/, modules in lib/). This ensures unique module names.
    if ctx.native_modules.len() > 1 {
        let mut common: Option<PathBuf> = None;
        for path in ctx.native_modules.keys() {
            if let Some(parent) = path.parent() {
                match &common {
                    None => common = Some(parent.to_path_buf()),
                    Some(prev) => {
                        // Find common prefix of prev and parent
                        let mut new_common = PathBuf::new();
                        for (a, b) in prev.components().zip(parent.components()) {
                            if a == b {
                                new_common.push(a);
                            } else {
                                break;
                            }
                        }
                        common = Some(new_common);
                    }
                }
            }
        }
        if let Some(new_root) = common {
            if !new_root.as_os_str().is_empty() {
                ctx.project_root = new_root;
                // Re-set module names based on the new project root
                let paths: Vec<PathBuf> = ctx.native_modules.keys().cloned().collect();
                for path in paths {
                    if let Some(module) = ctx.native_modules.get_mut(&path) {
                        let filename = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("module.ts");
                        module.name = path
                            .strip_prefix(&ctx.project_root)
                            .ok()
                            .and_then(|p| p.to_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| filename.to_string());
                    }
                }
            }
        }
    }

    let total_modules = ctx.native_modules.len() + ctx.js_modules.len();
    match format {
        OutputFormat::Text => {
            println!("Found {} module(s): {} native, {} JavaScript",
                total_modules,
                ctx.native_modules.len(),
                ctx.js_modules.len()
            );
        }
        OutputFormat::Json => {}
    }

    if args.enable_geisterhand || args.geisterhand_port.is_some() {
        ctx.needs_geisterhand = true;
        if let Some(port) = args.geisterhand_port {
            ctx.geisterhand_port = port;
        }
    }

    // --- Web/WASM target: emit WASM binary + JS runtime bridge ---
    if matches!(args.target.as_deref(), Some("web") | Some("wasm")) {
        return compile_for_wasm(&ctx, &args, format);
    }

    // --- Widget targets: emit platform-specific source + optional native provider ---
    if matches!(args.target.as_deref(), Some("ios-widget") | Some("ios-widget-simulator")) {
        return compile_for_ios_widget(&ctx, &args, format);
    }
    if matches!(args.target.as_deref(), Some("watchos-widget") | Some("watchos-widget-simulator")) {
        return compile_for_watchos_widget(&ctx, &args, format);
    }
    if args.target.as_deref() == Some("android-widget") {
        return compile_for_android_widget(&ctx, &args, format);
    }
    if args.target.as_deref() == Some("wearos-tile") {
        return compile_for_wearos_tile(&ctx, &args, format);
    }

    // Transform JS imports + fix local native instances (parallel,
    // fused per-module). Tier 4.2 (v0.5.335): pre-fix this was two
    // separate `par_iter_mut().for_each(...)` passes back-to-back.
    // The two operations are independent within a single module, so
    // running them inside one rayon job per module amortizes the
    // scheduler cost. The js_imports step is gated on
    // `needs_js_runtime`; modules that don't need it pay only the
    // cheap branch.
    use rayon::prelude::*;
    let needs_js_runtime = ctx.needs_js_runtime;
    ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
        if needs_js_runtime {
            perry_hir::transform_js_imports(hir_module);
        }
        perry_hir::fix_local_native_instances(hir_module);
    });

    // Build map of exported native instances from all modules. Must
    // run AFTER fix_local_native_instances above so the exports list
    // reflects post-rewrite state.
    let mut exported_instances: BTreeMap<(String, String), perry_hir::ExportedNativeInstance> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for (export_name, native_module, native_class) in &hir_module.exported_native_instances {
            exported_instances.insert(
                (path_str.clone(), export_name.clone()),
                perry_hir::ExportedNativeInstance {
                    native_module: native_module.clone(),
                    native_class: native_class.clone(),
                },
            );
        }
    }

    // Build map of exported functions that return native instances.
    let mut exported_func_return_instances: BTreeMap<(String, String), perry_hir::ExportedNativeInstance> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for (func_name, native_module, native_class) in &hir_module.exported_func_return_native_instances {
            exported_func_return_instances.insert(
                (path_str.clone(), func_name.clone()),
                perry_hir::ExportedNativeInstance {
                    native_module: native_module.clone(),
                    native_class: native_class.clone(),
                },
            );
        }
    }

    // Cross-module fix → local-fix re-run → monomorphize (parallel,
    // fused per-module). Tier 4.2: pre-fix this was three separate
    // `par_iter_mut().for_each(...)` passes. The local-fix re-run
    // depends on `fix_cross_module_native_instances` having
    // populated cross-module type info on this module, and
    // monomorphize depends on the post-local-fix module shape — but
    // both dependencies are intra-module, so running all three in
    // one rayon job per module is safe and saves two scheduler
    // round-trips. The cross-module step is gated on at least one
    // export existing (skip the call entirely otherwise).
    let has_native_exports =
        !exported_instances.is_empty() || !exported_func_return_instances.is_empty();
    ctx.native_modules.par_iter_mut().for_each(|(_, hir_module)| {
        if has_native_exports {
            perry_hir::fix_cross_module_native_instances(
                hir_module,
                &exported_instances,
                &exported_func_return_instances,
            );
        }
        // Always re-run local fix (matches pre-Tier-4.2 behaviour —
        // the prior code unconditionally ran a second local-fix pass
        // after the cross-module branch). When `has_native_exports`
        // is false this is effectively a no-op since nothing changed
        // since the first local-fix in Pass A above.
        perry_hir::fix_local_native_instances(hir_module);
        perry_hir::monomorphize_module(hir_module);
    });

    // --- i18n: apply i18n transform pass ---
    let i18n_table = if let Some(ref config) = i18n_config {
        let table = perry_transform::i18n::apply_i18n(
            &mut ctx.native_modules, config, &i18n_translations
        );
        // Report diagnostics
        for diag in &table.diagnostics {
            match diag.severity {
                perry_transform::i18n::I18nSeverity::Warning => {
                    match format {
                        OutputFormat::Text => eprintln!("  i18n warning: {}", diag.message),
                        OutputFormat::Json => {}
                    }
                }
                perry_transform::i18n::I18nSeverity::Error => {
                    match format {
                        OutputFormat::Text => eprintln!("  i18n error: {}", diag.message),
                        OutputFormat::Json => {}
                    }
                }
            }
        }
        match format {
            OutputFormat::Text => if !table.keys.is_empty() {
                println!("  i18n: {} localizable string(s) detected", table.keys.len());
            },
            OutputFormat::Json => {}
        }
        // The LLVM backend threads i18n through `CompileOptions::i18n_table`
        // (set per-job at the dispatch site below). No thread-local needed.
        Some(table)
    } else {
        None
    };

    if args.print_hir {
        for (path, hir_module) in &ctx.native_modules {
            println!("\n=== HIR (after monomorphization): {} ===", path.display());
            println!("Module: {}", hir_module.name);
            println!("Imports: {}", hir_module.imports.len());
            for import in &hir_module.imports {
                println!(
                    "  - {} ({} specifiers, kind: {:?})",
                    import.source,
                    import.specifiers.len(),
                    import.module_kind
                );
            }
            println!("Exports: {}", hir_module.exports.len());
            println!("Functions: {}", hir_module.functions.len());
            for func in &hir_module.functions {
                println!(
                    "  - {} (params: {}, type_params: {}, async: {}, exported: {})",
                    func.name,
                    func.params.len(),
                    func.type_params.len(),
                    func.is_async,
                    func.is_exported
                );
                for p in &func.params {
                    println!("      param {} (id={}): {:?}", p.name, p.id, p.ty);
                }
                for (i, stmt) in func.body.iter().enumerate() {
                    println!("      [{}] {:?}", i, stmt);
                }
            }
            println!("Init statements: {}", hir_module.init.len());
            for (i, stmt) in hir_module.init.iter().enumerate() {
                println!("  [{}] {:?}", i, stmt);
            }
            println!("===========\n");
        }

        if !ctx.js_modules.is_empty() {
            println!("\n=== JavaScript Modules (interpreted) ===");
            for (specifier, module) in &ctx.js_modules {
                println!("  {} -> {}", specifier, module.path.display());
            }
            println!("===========\n");
        }
    }

    // --- i18n: write key registry ---
    if let Some(ref table) = i18n_table {
        if !table.keys.is_empty() {
            let perry_dir = ctx.project_root.join(".perry");
            let _ = fs::create_dir_all(&perry_dir);
            let registry: Vec<serde_json::Value> = table.keys.iter().enumerate().map(|(i, key)| {
                serde_json::json!({
                    "key": key,
                    "string_idx": i,
                })
            }).collect();
            let registry_json = serde_json::json!({ "keys": registry });
            let _ = fs::write(perry_dir.join("i18n-keys.json"),
                serde_json::to_string_pretty(&registry_json).unwrap_or_default());
        }
    }

    match format {
        OutputFormat::Text => println!("Generating code..."),
        OutputFormat::Json => {}
    }

    let mut obj_paths = Vec::new();

    // Get canonical path of entry module
    let entry_path = args.input.canonicalize().unwrap_or_else(|_| args.input.clone());

    // Collect non-entry module names for init function calls
    // Topologically sort by import dependencies so that if module A imports from module B,
    // module B is initialized first. This ensures module-level variables (e.g., Maps) are
    // allocated before other modules try to use them via imported functions.
    let non_entry_module_names: Vec<String> = {
        // Build path->name mapping and dependency graph
        let mut path_to_name: HashMap<PathBuf, String> = HashMap::new();
        let mut name_to_path: HashMap<String, PathBuf> = HashMap::new();
        let mut deps: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

        for (path, hir_module) in &ctx.native_modules {
            if *path == entry_path {
                continue;
            }
            path_to_name.insert(path.clone(), hir_module.name.clone());
            name_to_path.insert(hir_module.name.clone(), path.clone());

            let mut module_deps = Vec::new();
            for import in &hir_module.imports {
                if let Some(ref resolved) = import.resolved_path {
                    let resolved_path = PathBuf::from(resolved);
                    if resolved_path != entry_path && ctx.native_modules.contains_key(&resolved_path) {
                        module_deps.push(resolved_path);
                    }
                }
            }
            // Also treat ExportAll/ReExport sources as dependencies.
            // If module A does `export * from './B'`, then B must be initialized before A
            // so that B's export globals are set before any consumer of A reads them.
            for export in &hir_module.exports {
                let source = match export {
                    perry_hir::Export::ExportAll { source } => Some(source),
                    perry_hir::Export::ReExport { source, .. } => Some(source),
                    perry_hir::Export::Named { .. } => None,
                };
                if let Some(src) = source {
                    if let Some((resolved_path, _)) = resolve_import(src, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                        if resolved_path != entry_path && ctx.native_modules.contains_key(&resolved_path) {
                            module_deps.push(resolved_path);
                        }
                    }
                }
            }
            deps.insert(path.clone(), module_deps);
        }

        // DFS-based topological sort (handles circular dependencies gracefully)
        // Dependencies are visited before the module itself. Cycles are broken
        // at the back-edge (module already being visited), ensuring the best
        // possible ordering even with circular imports.
        let mut sorted = Vec::new();
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut visiting: HashSet<PathBuf> = HashSet::new(); // cycle detection

        fn dfs_visit(
            path: &PathBuf,
            deps: &HashMap<PathBuf, Vec<PathBuf>>,
            path_to_name: &HashMap<PathBuf, String>,
            visited: &mut HashSet<PathBuf>,
            visiting: &mut HashSet<PathBuf>,
            sorted: &mut Vec<String>,
        ) {
            if visited.contains(path) || visiting.contains(path) {
                return; // already done or cycle back-edge
            }
            visiting.insert(path.clone());

            // Visit dependencies first (so they get initialized before us)
            if let Some(module_deps) = deps.get(path) {
                // Sort deps for deterministic order
                let mut sorted_deps = module_deps.clone();
                sorted_deps.sort();
                for dep in &sorted_deps {
                    dfs_visit(dep, deps, path_to_name, visited, visiting, sorted);
                }
            }

            visiting.remove(path);
            visited.insert(path.clone());
            if let Some(name) = path_to_name.get(path) {
                sorted.push(name.clone());
            }
        }

        // Sort starting nodes for deterministic iteration order
        let mut all_paths: Vec<PathBuf> = path_to_name.keys().cloned().collect();
        all_paths.sort();

        for path in &all_paths {
            dfs_visit(path, &deps, &path_to_name, &mut visited, &mut visiting, &mut sorted);
        }

        sorted
    };

    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!("\nModule init order ({} modules):", non_entry_module_names.len());
        for (i, name) in non_entry_module_names.iter().enumerate() {
            eprintln!("  [{}] {}", i, name);
        }
        eprintln!();
    }

    // Build a map of all exported enums from all modules (owned data, no borrows)
    // Key: (resolved_path, enum_name) -> Vec<(member_name, EnumValue)>
    let mut exported_enums: BTreeMap<(String, String), Vec<(String, perry_hir::EnumValue)>> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for en in &hir_module.enums {
            if en.is_exported {
                let members: Vec<(String, perry_hir::EnumValue)> = en.members.iter()
                    .map(|m| (m.name.clone(), m.value.clone()))
                    .collect();
                exported_enums.insert((path_str.clone(), en.name.clone()), members);
            }
        }
    }

    // Propagate enum re-exports: when module A has `export * from "./B"`,
    // all enums exported from B should also be accessible via A's path.
    loop {
        let mut new_enum_entries: Vec<((String, String), Vec<(String, perry_hir::EnumValue)>)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                let source_str = match export {
                    perry_hir::Export::ExportAll { source } => Some((source.as_str(), None)),
                    perry_hir::Export::ReExport { source, imported, exported } => Some((source.as_str(), Some((imported.as_str(), exported.as_str())))),
                    _ => None,
                };
                if let Some((source, re_export_names)) = source_str {
                    if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                        let source_path_str = resolved_source.to_string_lossy().to_string();
                        for ((src_path, enum_name), members) in &exported_enums {
                            if src_path == &source_path_str {
                                let (propagate, exported_name) = match re_export_names {
                                    Some((imported, exported)) => (enum_name == imported, exported.to_string()),
                                    None => (true, enum_name.clone()),
                                };
                                if propagate {
                                    let key = (path_str.clone(), exported_name);
                                    if !exported_enums.contains_key(&key) {
                                        new_enum_entries.push((key, members.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if new_enum_entries.is_empty() { break; }
        for (key, members) in new_enum_entries {
            exported_enums.insert(key, members);
        }
    }

    // Fix imported enum references in all modules BEFORE building exported_classes
    // (exported_classes holds references into ctx.native_modules, so we need to do
    // the mutable fixup pass first)
    {
        let mut module_enums: BTreeMap<PathBuf, BTreeMap<String, Vec<(String, perry_hir::EnumValue)>>> = BTreeMap::new();
        for (path, hir_module) in &ctx.native_modules {
            let mut imported_enums_for_module: BTreeMap<String, Vec<(String, perry_hir::EnumValue)>> = BTreeMap::new();
            for import in &hir_module.imports {
                if import.module_kind != perry_hir::ModuleKind::NativeCompiled { continue; }
                let resolved_path = match &import.resolved_path {
                    Some(p) => p.clone(),
                    None => continue,
                };
                for spec in &import.specifiers {
                    let (local_name, exported_name) = match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => (local.clone(), imported.clone()),
                        perry_hir::ImportSpecifier::Default { local } => (local.clone(), local.clone()),
                        perry_hir::ImportSpecifier::Namespace { .. } => continue,
                    };
                    let key = (resolved_path.clone(), exported_name.clone());
                    if let Some(members) = exported_enums.get(&key) {
                        imported_enums_for_module.insert(local_name, members.clone());
                    }
                }
            }
            if !imported_enums_for_module.is_empty() {
                module_enums.insert(path.clone(), imported_enums_for_module);
            }
        }
        for (path, imported_enums_for_module) in &module_enums {
            if let Some(hir_module) = ctx.native_modules.get_mut(path) {
                perry_hir::fix_imported_enums(hir_module, imported_enums_for_module);
            }
        }
    }

    // Collect all non-generic type aliases from all modules.
    // These are passed to each module's compiler so type_to_abi can resolve
    // Named("BlockTag") -> Union([...]) for correct ABI types in function signatures.
    let mut all_type_aliases: std::collections::BTreeMap<String, perry_types::Type> = std::collections::BTreeMap::new();
    for (_path, hir_module) in &ctx.native_modules {
        for ta in &hir_module.type_aliases {
            if ta.type_params.is_empty() {
                all_type_aliases.insert(ta.name.clone(), ta.ty.clone());
            }
        }
    }

    // Set of every type name (class, interface, enum, type alias) that
    // exists *anywhere* in the program's HIR — across every native
    // module. The per-module polymorphic-receiver augmentation pass
    // (issue #240) consults this when scanning function/class type
    // annotations: any `Named(X)` reference whose X is NOT in this set
    // and NOT a builtin TS/runtime type name signals an interface that
    // came from a type-only import (i.e. `import type { Driver } from
    // "./driver"` — the source module never enters `native_modules` at
    // all because it has no value-side exports). When such an
    // unresolved reference appears, the consumer module needs full
    // visibility into every program-wide class so the dispatch tower
    // at `crates/perry-codegen/src/lower_call.rs::needs_dynamic_dispatch`
    // can resolve `obj.method()` against any implementer at runtime.
    //
    // Without this, `function consume(d: Driver) { d.findOne(...) }`
    // compiled in a module that only type-imports `Driver` produces a
    // dispatch-tower implementor list of size 0, and the call falls
    // through to a generic property-get closure call that resolves to
    // `undefined` — silently dropping every method invocation through
    // the interface. Type-only imports are stripped at HIR lowering
    // (`crates/perry-hir/src/lower.rs:2777`), so the consumer's
    // `hir_module.imports` doesn't even mention the source module.
    let mut all_program_type_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_path, hir_module) in &ctx.native_modules {
        for class in &hir_module.classes {
            all_program_type_names.insert(class.name.clone());
        }
        for iface in &hir_module.interfaces {
            all_program_type_names.insert(iface.name.clone());
        }
        for en in &hir_module.enums {
            all_program_type_names.insert(en.name.clone());
        }
        for ta in &hir_module.type_aliases {
            all_program_type_names.insert(ta.name.clone());
        }
    }

    // Build a map of all exported classes from all modules
    // Key: (resolved_path, class_name) -> Class reference
    let mut exported_classes: BTreeMap<(String, String), &perry_hir::Class> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for class in &hir_module.classes {
            if class.is_exported {
                exported_classes.insert((path_str.clone(), class.name.clone()), class);
            }
        }
    }

    // Set of exported VARIABLES (not functions) — keyed by (module_path, name).
    // Used to distinguish variable getters from function references when an
    // ExternFuncRef appears as a value in an importing module.
    let mut exported_var_names: BTreeSet<(String, String)> = BTreeSet::new();
    // Build a map of all exported functions with their param counts from all modules
    let mut exported_func_param_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    // Build a map of all exported functions with their return types from all modules
    let mut exported_func_return_types: BTreeMap<(String, String), perry_types::Type> = BTreeMap::new();
    // Set of exported functions that were declared `async` in their source module.
    // We track this separately because users routinely write `async function f() { ... }`
    // without an explicit `Promise<T>` annotation, in which case `func.return_type` is the
    // inner type or `Type::Any` and importers can't infer async-ness from the return type alone.
    let mut exported_async_funcs: BTreeSet<(String, String)> = BTreeSet::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for func in &hir_module.functions {
            if func.is_exported {
                exported_func_param_counts.insert((path_str.clone(), func.name.clone()), func.params.len());
                exported_func_return_types.insert((path_str.clone(), func.name.clone()), func.return_type.clone());
                if func.is_async {
                    exported_async_funcs.insert((path_str.clone(), func.name.clone()));
                }
            }
        }
        // Also register exported_functions aliases (e.g., "default" → actual function)
        // This handles `export default funcName` where the export name differs from the function name
        for (export_name, func_id) in &hir_module.exported_functions {
            if let Some(func) = hir_module.functions.iter().find(|f| f.id == *func_id) {
                let key = (path_str.clone(), export_name.clone());
                exported_func_param_counts.entry(key.clone()).or_insert(func.params.len());
                exported_func_return_types.entry(key.clone()).or_insert_with(|| func.return_type.clone());
                if func.is_async {
                    exported_async_funcs.insert(key);
                }
            }
        }
        // Debug: print superstruct exports
        if path_str.contains("superstruct") {
            eprintln!("[DEBUG] superstruct: {} functions ({} exported), {} exported_functions entries",
                hir_module.functions.len(),
                hir_module.functions.iter().filter(|f| f.is_exported).count(),
                hir_module.exported_functions.len());
            for (name, _fid) in &hir_module.exported_functions {
                eprintln!("[DEBUG]   exported_function: {}", name);
            }
        }

        // Also scan init statements for exported closures (arrow functions assigned to const)
        // These are in exported_objects but not in functions, so they need param counts too
        let exported_set: std::collections::HashSet<&String> = hir_module.exported_objects.iter().collect();
        for stmt in &hir_module.init {
            if let perry_hir::ir::Stmt::Let { name, init: Some(expr), .. } = stmt {
                if exported_set.contains(name) {
                    if let perry_hir::ir::Expr::Closure { params, return_type, is_async, .. } = expr {
                        exported_func_param_counts.insert((path_str.clone(), name.clone()), params.len());
                        exported_func_return_types.insert((path_str.clone(), name.clone()), return_type.clone());
                        if *is_async {
                            exported_async_funcs.insert((path_str.clone(), name.clone()));
                        }
                    }
                }
            }
        }
    }

    // Populate exported_var_names: names that are in exported_objects but NOT
    // in exported_func_param_counts (closures assigned to const are in both).
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for obj_name in &hir_module.exported_objects {
            let key = (path_str.clone(), obj_name.clone());
            if !exported_func_param_counts.contains_key(&key) {
                exported_var_names.insert(key);
            }
        }
    }

    // Build a map of all exports from all modules: module_path -> HashMap<export_name, origin_module_path>
    // This is used for namespace imports (`import * as X from './module'`) to resolve all exports
    let mut all_module_exports: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        let exports = all_module_exports.entry(path_str.clone()).or_insert_with(BTreeMap::new);
        // Exported functions
        for func in &hir_module.functions {
            if func.is_exported {
                exports.insert(func.name.clone(), path_str.clone());
            }
        }
        // Exported objects (export const x = { ... })
        for obj_name in &hir_module.exported_objects {
            exports.insert(obj_name.clone(), path_str.clone());
        }
        // Exported classes
        for class in &hir_module.classes {
            if class.is_exported {
                exports.insert(class.name.clone(), path_str.clone());
            }
        }
        // Exported enums
        for en in &hir_module.enums {
            if en.is_exported {
                exports.insert(en.name.clone(), path_str.clone());
            }
        }
        // Named exports (export { foo, bar as baz })
        for export in &hir_module.exports {
            if let perry_hir::Export::Named { exported, .. } = export {
                exports.insert(exported.clone(), path_str.clone());
            }
            // ReExport is handled in the propagation loop below (avoids borrow issues)
        }
    }

    // Propagate exports through ExportAll and ReExport chains
    loop {
        let mut new_export_entries: Vec<(String, String, String)> = Vec::new(); // (module_path, export_name, origin_path)
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                let current_exports = all_module_exports.get(&path_str);
                                for (name, origin) in source_exports {
                                    let already_exists = current_exports
                                        .map(|e| e.contains_key(name))
                                        .unwrap_or(false);
                                    if !already_exists {
                                        new_export_entries.push((path_str.clone(), name.clone(), origin.clone()));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                if let Some(origin) = source_exports.get(imported) {
                                    let current_exports = all_module_exports.get(&path_str);
                                    let already_correct = current_exports
                                        .and_then(|e| e.get(exported.as_str()))
                                        .map(|v| v == origin)
                                        .unwrap_or(false);
                                    if !already_correct {
                                        new_export_entries.push((path_str.clone(), exported.clone(), origin.clone()));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        // Check if this local was imported from another module
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                            if let Some(origin) = source_exports.get(&imported_name) {
                                                let current_exports = all_module_exports.get(&path_str);
                                                let already_correct = current_exports
                                                    .and_then(|e| e.get(exported.as_str()))
                                                    .map(|v| v == origin)
                                                    .unwrap_or(false);
                                                if !already_correct {
                                                    new_export_entries.push((path_str.clone(), exported.clone(), origin.clone()));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_export_entries.is_empty() { break; }
        for (module_path, name, origin) in new_export_entries {
            all_module_exports.entry(module_path).or_insert_with(BTreeMap::new).insert(name, origin);
        }
    }

    // Also propagate exported_func_param_counts through ExportAll/ReExport/Named chains
    loop {
        let mut new_func_entries: Vec<((String, String), usize)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), &param_count) in &exported_func_param_counts {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), func_name.clone());
                                    if !exported_func_param_counts.contains_key(&key) {
                                        new_func_entries.push((key, param_count));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), &param_count) in &exported_func_param_counts {
                                if src_path == &source_path_str && func_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_func_param_counts.contains_key(&key) {
                                        new_func_entries.push((key, param_count));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(&param_count) = exported_func_param_counts.get(&key_src) {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_func_param_counts.contains_key(&key) {
                                                new_func_entries.push((key, param_count));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_func_entries.is_empty() { break; }
        for (key, param_count) in new_func_entries {
            exported_func_param_counts.insert(key, param_count);
        }
    }

    // Propagate exported_func_return_types through ExportAll/ReExport/Named chains.
    // exported_async_funcs is propagated in the same loop so that re-exported async
    // functions remain marked async at every step in the chain.
    loop {
        let mut new_func_entries: Vec<((String, String), perry_types::Type)> = Vec::new();
        let mut new_async_entries: Vec<(String, String)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), return_type) in &exported_func_return_types {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), func_name.clone());
                                    if !exported_func_return_types.contains_key(&key) {
                                        new_func_entries.push((key.clone(), return_type.clone()));
                                    }
                                    let async_key = (source_path_str.clone(), func_name.clone());
                                    let propagated_async_key = (path_str.clone(), func_name.clone());
                                    if exported_async_funcs.contains(&async_key)
                                        && !exported_async_funcs.contains(&propagated_async_key)
                                    {
                                        new_async_entries.push(propagated_async_key);
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), return_type) in &exported_func_return_types {
                                if src_path == &source_path_str && func_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_func_return_types.contains_key(&key) {
                                        new_func_entries.push((key.clone(), return_type.clone()));
                                    }
                                    let async_key = (source_path_str.clone(), func_name.clone());
                                    let propagated_async_key = (path_str.clone(), exported.clone());
                                    if exported_async_funcs.contains(&async_key)
                                        && !exported_async_funcs.contains(&propagated_async_key)
                                    {
                                        new_async_entries.push(propagated_async_key);
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(return_type) = exported_func_return_types.get(&key_src) {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_func_return_types.contains_key(&key) {
                                                new_func_entries.push((key.clone(), return_type.clone()));
                                            }
                                            let propagated_async_key = (path_str.clone(), exported.clone());
                                            if exported_async_funcs.contains(&key_src)
                                                && !exported_async_funcs.contains(&propagated_async_key)
                                            {
                                                new_async_entries.push(propagated_async_key);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_func_entries.is_empty() && new_async_entries.is_empty() { break; }
        for (key, return_type) in new_func_entries {
            exported_func_return_types.insert(key, return_type);
        }
        for key in new_async_entries {
            exported_async_funcs.insert(key);
        }
    }

    // Propagate class re-exports through ExportAll/ReExport/Named chains
    loop {
        let mut new_entries: Vec<((String, String), &perry_hir::Class)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, class_name), class) in &exported_classes {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), class_name.clone());
                                    if !exported_classes.contains_key(&key) {
                                        new_entries.push((key, *class));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport { source, imported, exported } => {
                        if let Some((resolved_source, _)) = resolve_import(source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, class_name), class) in &exported_classes {
                                if src_path == &source_path_str && class_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_classes.contains_key(&key) {
                                        new_entries.push((key, *class));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } =>
                                        (l == local, imported.clone()),
                                    perry_hir::ImportSpecifier::Default { local: l } =>
                                        (l == local, "default".to_string()),
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(&import.source, path, &ctx.project_root, &ctx.compile_packages, &ctx.compile_package_dirs) {
                                        let source_path_str = resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(class) = exported_classes.get(&key_src) {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_classes.contains_key(&key) {
                                                new_entries.push((key, *class));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if new_entries.is_empty() { break; }
        for (key, class) in new_entries {
            exported_classes.insert(key, class);
        }
    }

    let target = args.target.clone();

    // Fail-fast for HarmonyOS: without the OHOS SDK we can't cross-compile the
    // runtime or invoke the link, and the downstream error chain is two
    // confusing messages instead of one. Check up front unless a prebuilt
    // harmonyos runtime is already on disk (the npm-distribution case, once
    // that ships). `find_runtime_library` is a borrowed-result, so we inspect
    // without propagating errors.
    if matches!(target.as_deref(), Some("harmonyos") | Some("harmonyos-simulator"))
        && find_harmonyos_sdk().is_none()
        && find_runtime_library(target.as_deref()).is_err()
    {
        anyhow::bail!(
            "OHOS SDK not found. --target {} needs the OpenHarmony native SDK \
             (clang + musl sysroot) to cross-compile perry-runtime.\n\n\
             Install DevEco Studio from https://developer.huawei.com/consumer/en/develop \
             (the SDK ships under Preferences → SDK Platforms → OpenHarmony), or \
             download the standalone \"OpenHarmony SDK\" bundle.\n\n\
             Then export OHOS_SDK_HOME pointing at the SDK root — the directory \
             that contains `native/llvm/bin/clang` and `native/sysroot/`.\n\n\
             Common defaults already probed:\n  \
             - $HOME/Library/Huawei/Sdk  (macOS DevEco default)\n  \
             - $HOME/Huawei/Sdk          (Linux DevEco default)",
            target.as_deref().unwrap()
        );
    }

    // Pre-compute feature flags (moved out of parallel loop to avoid ctx mutation)
    let compiled_features: Vec<String> = if let Some(ref features_str) = args.features {
        let mut features: Vec<String> = features_str.split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        let is_mobile = matches!(target.as_deref(),
            Some("ios") | Some("ios-simulator") |
            Some("visionos") | Some("visionos-simulator") |
            Some("android") |
            Some("watchos") | Some("watchos-simulator") |
            Some("tvos") | Some("tvos-simulator") |
            Some("harmonyos") | Some("harmonyos-simulator")
        );
        if is_mobile {
            features.retain(|f| f != "plugins");
        }
        if features.iter().any(|f| f == "plugins") {
            ctx.needs_plugins = true;
        }
        // Auto-enable the HarmonyOS NAPI entry wrapper. Without this the
        // linked .so has no `napi_module_register` call and the ArkTS shim
        // fails at import time with "module entry not found".
        if matches!(target.as_deref(), Some("harmonyos") | Some("harmonyos-simulator"))
            && !features.iter().any(|f| f == "ohos-napi")
        {
            features.push("ohos-napi".to_string());
        }
        features
    } else if matches!(target.as_deref(), Some("harmonyos") | Some("harmonyos-simulator")) {
        // User didn't pass --features at all; still auto-enable ohos-napi.
        vec!["ohos-napi".to_string()]
    } else {
        Vec::new()
    };

    // Pre-compute native library FFI functions
    let ffi_functions: Vec<(String, Vec<String>, String)> = ctx.native_libraries.iter()
        .flat_map(|lib| lib.functions.iter().map(|f| {
            (f.name.clone(), f.params.clone(), f.returns.clone())
        }))
        .collect();

    // Pre-compute JS module specifiers
    let js_module_specifiers: Vec<String> = ctx.js_modules.keys().cloned().collect();
    let needs_js_runtime = ctx.needs_js_runtime || args.enable_js_runtime;

    // Compile native modules in parallel using rayon
    

    // Snapshot i18n data from main thread so rayon workers can access it.
    // The `default_locale_idx` is required by the LLVM backend to resolve
    // `Expr::I18nString` against the right translation row at compile time
    // — without it the lowering would either fall back to the verbatim key
    // or guess locale 0.
    //
    // Tier 4.6 (v0.5.336): wrapped in `Arc` so the per-module clone in
    // the par_iter() worker below is a cheap reference bump instead of
    // duplicating the (potentially large) `Vec<String>` of every
    // translated string. Pre-fix, a project with N modules cloned the
    // full translations Vec N times during codegen.
    let i18n_snapshot: Option<std::sync::Arc<(Vec<String>, usize, usize, Vec<String>, usize)>> =
        i18n_table.as_ref().map(|table| {
            std::sync::Arc::new((
                table.translations.clone(),
                table.keys.len(),
                table.locale_count,
                table.locale_codes.clone(),
                table.default_locale_idx,
            ))
        });

    // Phase J: detect bitcode-link mode. The actual .bc paths aren't known
    // yet (build_optimized_libs runs after compilation), but we decide the
    // mode here so the per-module codegen can emit .ll instead of .o.
    let bitcode_link =
        std::env::var("PERRY_LLVM_BITCODE_LINK").ok().as_deref() == Some("1");

    // V2.2: Per-module object cache at `.perry-cache/objects/<target>/<key>.o`.
    // Disabled when the user passed `--no-cache`, when `PERRY_NO_CACHE=1`, or
    // when we're in bitcode-link mode (the artifacts aren't object files).
    // Key derivation: `compute_object_cache_key(opts, source_hash, perry_version)`.
    let cache_env_disabled =
        std::env::var("PERRY_NO_CACHE").ok().as_deref() == Some("1");
    let cache_enabled = !args.no_cache && !cache_env_disabled && !bitcode_link;
    // Target dir name for the cache layout. Using the resolved LLVM triple
    // keeps cross-compile caches from colliding with native-host caches.
    let cache_target_dir = target.as_deref().unwrap_or("host");
    let object_cache = ObjectCache::new(&ctx.project_root, cache_target_dir, cache_enabled);
    let perry_version = env!("CARGO_PKG_VERSION");

    let compile_results: Vec<Result<(PathBuf, Vec<u8>), String>> = ctx.native_modules.par_iter()
        .map(|(path, hir_module)| {
            // Compile this module to LLVM IR (or .ll text in bitcode-link mode)
            // and return the object bytes for the linker to consume.
            let is_entry = path == &entry_path;
            // Compute the prefix list of non-entry modules so the
            // entry main can call each `<prefix>__init` in order.
            // The prefix derivation must match what
            // `perry_codegen::compile_module` does internally
            // (sanitize(hir.name)) so the symbols match. LLVM IR
            // identifiers cannot start with a digit, so prefix with
            // `_` if the first character would be one (handles module
            // names like `05_fibonacci.ts`).
            let sanitize_name = |s: &str| -> String {
                let mut out: String = s
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
                    .collect();
                if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    out.insert(0, '_');
                }
                out
            };
            // CRITICAL: iterate `non_entry_module_names` (topologically
            // sorted above) rather than `ctx.native_modules` — the latter
            // is a `BTreeMap<PathBuf, _>` and iterates in alphabetical
            // path order, which silently reverses the dependency order
            // for any project whose leaf modules sort after their
            // dependents (e.g. `types/registry.ts` sorting after
            // `connection.ts`). When that happens, a top-level
            // `registerDefaultCodecs()` call in register-defaults.ts
            // runs BEFORE types/registry.ts's init has set up the
            // `REGISTRY_OIDS` global — the push-site writes to a stale
            // (0.0-initialized) global while the read-site later loads
            // from the real one. Symptom: registry appears empty to
            // every later consumer even though primitives like
            // `let registered = false` look shared (they only need
            // storage, not init-order). Fixes GH #32.
            let non_entry_module_prefixes: Vec<String> = if is_entry {
                non_entry_module_names
                    .iter()
                    .map(|name| sanitize_name(name))
                    .collect()
            } else {
                Vec::new()
            };
            // Build import → source-prefix table for cross-module
            // ExternFuncRef calls. For each Named import in this
            // module, look up the source module's HIR by resolved
            // path and capture its name. The LLVM codegen uses this
            // to generate `perry_fn_<source_prefix>__<name>`.
            let mut import_function_prefixes: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            let mut namespace_imports: Vec<String> = Vec::new();
            let mut imported_classes: Vec<perry_codegen::ImportedClass> = Vec::new();
            let mut imported_enums: Vec<(String, Vec<(String, perry_hir::EnumValue)>)> = Vec::new();
            let mut imported_async_set: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut imported_param_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut imported_return_types: std::collections::HashMap<String, perry_types::Type> = std::collections::HashMap::new();
            let mut imported_vars: std::collections::HashSet<String> = std::collections::HashSet::new();

            for import in &hir_module.imports {
                if import.module_kind != perry_hir::ModuleKind::NativeCompiled {
                    continue;
                }
                let resolved_path = match &import.resolved_path {
                    Some(p) => p,
                    None => continue,
                };
                let resolved_path_str = resolved_path.clone();
                let source_module = ctx
                    .native_modules
                    .iter()
                    .find(|(p, _)| p.to_string_lossy() == *resolved_path)
                    .map(|(_, m)| m);
                let source_prefix = match &source_module {
                    Some(m) => sanitize_name(&m.name),
                    None => continue,
                };

                for spec in &import.specifiers {
                    // Handle namespace imports (import * as X)
                    if let perry_hir::ImportSpecifier::Namespace { local } = spec {
                        namespace_imports.push(local.clone());
                        // Register all exports from the source module
                        if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                            for (export_name, origin_path) in exports {
                                let origin_prefix = compute_module_prefix(origin_path, &ctx.project_root);
                                import_function_prefixes.insert(export_name.clone(), origin_prefix.clone());

                                let key = (origin_path.clone(), export_name.clone());
                                if let Some(&param_count) = exported_func_param_counts.get(&key) {
                                    imported_param_counts.insert(export_name.clone(), param_count);
                                }
                                if let Some(class) = exported_classes.get(&key) {
                                    imported_classes.push(perry_codegen::ImportedClass {
                                        name: class.name.clone(),
                                        local_alias: None,
                                        source_prefix: origin_prefix.clone(),
                                        constructor_param_count: class.constructor.as_ref().map(|c| c.params.len()).unwrap_or(0),
                                        method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                                        method_param_counts: class.methods.iter().map(|m| m.params.len()).collect(),
                                        static_method_names: class.static_methods.iter().map(|m| m.name.clone()).collect(),
                                        getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                                        setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                                        parent_name: class.extends_name.clone(),
                                        field_names: class.fields.iter().map(|f| f.name.clone()).collect(),
                                        field_types: class.fields.iter().map(|f| f.ty.clone()).collect(),
                                        source_class_id: Some(class.id),
                                    });
                                }
                                if let Some(members) = exported_enums.get(&key) {
                                    imported_enums.push((export_name.clone(), members.clone()));
                                }
                            }
                        }
                        continue;
                    }

                    let (local_name, exported_name) = match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => (local.clone(), imported.clone()),
                        perry_hir::ImportSpecifier::Default { local } => (local.clone(), "default".to_string()),
                        perry_hir::ImportSpecifier::Namespace { .. } => unreachable!(),
                    };

                    let key = (resolved_path_str.clone(), exported_name.clone());

                    // Resolve effective prefix (follow re-exports)
                    let effective_prefix = if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                        if let Some(origin_path) = exports.get(&exported_name) {
                            if origin_path != &resolved_path_str {
                                compute_module_prefix(origin_path, &ctx.project_root)
                            } else {
                                source_prefix.clone()
                            }
                        } else {
                            source_prefix.clone()
                        }
                    } else {
                        source_prefix.clone()
                    };

                    import_function_prefixes.insert(exported_name.clone(), effective_prefix.clone());
                    if local_name != exported_name {
                        import_function_prefixes.insert(local_name.clone(), effective_prefix.clone());
                    }

                    // Imported variables (not functions) — ExternFuncRef-as-value
                    // should call the getter, not wrap as closure.
                    if exported_var_names.contains(&key) {
                        imported_vars.insert(exported_name.clone());
                        if local_name != exported_name {
                            imported_vars.insert(local_name.clone());
                        }
                    }

                    // Imported classes
                    if let Some(class) = exported_classes.get(&key) {
                        imported_classes.push(perry_codegen::ImportedClass {
                            name: class.name.clone(),
                            local_alias: if local_name != class.name { Some(local_name.clone()) } else { None },
                            source_prefix: effective_prefix.clone(),
                            constructor_param_count: class.constructor.as_ref().map(|c| c.params.len()).unwrap_or(0),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class.methods.iter().map(|m| m.params.len()).collect(),
                            static_method_names: class.static_methods.iter().map(|m| m.name.clone()).collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class.fields.iter().map(|f| f.name.clone()).collect(),
                            field_types: class.fields.iter().map(|f| f.ty.clone()).collect(),
                            source_class_id: Some(class.id),
                        });
                    }

                    // Imported param counts
                    if let Some(&param_count) = exported_func_param_counts.get(&key) {
                        imported_param_counts.insert(exported_name.clone(), param_count);
                        if local_name != exported_name {
                            imported_param_counts.insert(local_name.clone(), param_count);
                        }
                    }

                    // Imported return types
                    if let Some(return_type) = exported_func_return_types.get(&key) {
                        imported_return_types.insert(local_name.clone(), return_type.clone());
                    }

                    // Imported async functions
                    if exported_async_funcs.contains(&key) {
                        imported_async_set.insert(local_name.clone());
                        if local_name != exported_name {
                            imported_async_set.insert(exported_name.clone());
                        }
                    }

                    // Imported enums
                    if let Some(members) = exported_enums.get(&key) {
                        imported_enums.push((local_name.clone(), members.clone()));
                    }
                }

                // Named imports only bring in explicitly-imported symbols, so
                // a class that leaks out of the source module as the return
                // type of an imported *function* (e.g. `import { makeThing }`
                // where `makeThing(): Promise<Thing>`) leaves `Thing` invisible
                // to this module's dispatch tables. `t.doWork(...)` then can't
                // find `("Thing", "doWork")` in `ctx.methods` and falls through
                // to `js_native_call_method`, which returns the receiver's
                // ObjectHeader as a stub. Closes #83.
                //
                // Mirror the namespace-import behavior: for every
                // native-compiled module we import from (and every module that
                // module transitively re-exports from), enumerate every class
                // defined in that module and register it for dispatch, even
                // when the class name wasn't in the specifier list. Local
                // classes with the same name take precedence in
                // `compile_module` (the `class_table.contains_key` check), so
                // this doesn't clobber anything.
                //
                // We iterate `ctx.native_modules` directly — NOT the
                // `exported_classes` BTreeMap. `exported_classes` gets alias
                // entries stamped under every re-exporter's path (the
                // `Export::ReExport` / `Export::ExportAll` propagation loop
                // above), so iterating it would hand us the class keyed by
                // `index.ts` when it was actually compiled under
                // `pool.ts`. Using each module's own `hir.classes` Vec guarantees
                // `src_path` is the TRUE defining module, so the mangled
                // `perry_method_<source_prefix>__<Class>__<method>` symbol
                // matches what that module actually emitted (otherwise the
                // linker fails with "undefined symbol
                // _perry_method_src_index_ts__Pool__query" when Pool was
                // compiled under src_pool_ts).
                let mut origin_paths: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                origin_paths.insert(resolved_path_str.clone());
                if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                    for origin_path in exports.values() {
                        origin_paths.insert(origin_path.clone());
                    }
                }
                for (src_pathbuf, src_hir) in &ctx.native_modules {
                    let src_path = src_pathbuf.to_string_lossy().to_string();
                    if !origin_paths.contains(&src_path) {
                        continue;
                    }
                    for class in &src_hir.classes {
                        if !class.is_exported {
                            continue;
                        }
                        // Dedup across multiple import statements: the same class
                        // may be transitively reachable from several imports, and
                        // the same-class-twice case would produce duplicate
                        // `@perry_class_keys_<modprefix>__<Class>` globals in IR.
                        // Same-name local classes win via `compile_module`'s
                        // class_table check, so this filter is strictly about
                        // cross-module twinning.
                        if imported_classes.iter().any(|c| c.name == class.name) {
                            continue;
                        }
                        let class_prefix = compute_module_prefix(&src_path, &ctx.project_root);
                        imported_classes.push(perry_codegen::ImportedClass {
                            name: class.name.clone(),
                            local_alias: None,
                            source_prefix: class_prefix,
                            constructor_param_count: class.constructor.as_ref().map(|c| c.params.len()).unwrap_or(0),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class.methods.iter().map(|m| m.params.len()).collect(),
                            static_method_names: class.static_methods.iter().map(|m| m.name.clone()).collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class.fields.iter().map(|f| f.name.clone()).collect(),
                            field_types: class.fields.iter().map(|f| f.ty.clone()).collect(),
                            source_class_id: Some(class.id),
                        });
                    }
                }
            }

            // Polymorphic-receiver augmentation (issue #240): when this
            // module references a type name that doesn't resolve to any
            // class, interface, enum, or type alias in the program's
            // HIR — and isn't a TS/runtime builtin — the most likely
            // explanation is that the name names an interface in a
            // module that was reached only via a type-only import.
            // `import type { Driver } from "./driver.ts"` is stripped
            // at HIR lowering (`crates/perry-hir/src/lower.rs:2777`),
            // so `driver.ts` never enters `ctx.native_modules`, and
            // `Driver` becomes invisible to the rest of the program.
            // The consumer's HIR still has `Named("Driver")` on the
            // function param — it just doesn't resolve.
            //
            // When such an unresolved reference appears, this module's
            // dispatch tower (`crates/perry-codegen/src/lower_call.rs`)
            // would otherwise see an empty `implementors` list at
            // `obj.method()` call sites and the call would fall through
            // to a generic property-get closure call that resolves to
            // `undefined` — silently dropping the call. The fix is to
            // pull every program-wide exported class into
            // `imported_classes` so the dispatch tower can resolve the
            // call against any class that has the called method. The
            // dispatch tower at the call site filters per-method-name,
            // so IR size is bounded by the number of implementing
            // classes, not the total class count.
            //
            // Without `implements`-clause tracking we can't be more
            // surgical (e.g. pull only classes that satisfy a specific
            // interface). The conservative "pull everything" matches
            // the existing precedent for namespace imports (line ~1810
            // above), which already pulls every class in the source
            // module on `import * as ns`.
            fn is_builtin_type_name(name: &str) -> bool {
                matches!(
                    name,
                    // Primitive aliases sometimes carried as Named
                    "Number" | "String" | "Boolean" | "BigInt" | "Symbol"
                    | "Object" | "Function"
                    // Built-in JS objects
                    | "Array" | "ReadonlyArray" | "Tuple"
                    | "Map" | "Set" | "WeakMap" | "WeakSet" | "WeakRef"
                    | "Date" | "RegExp" | "Promise"
                    | "Error" | "TypeError" | "RangeError" | "SyntaxError"
                    | "ReferenceError" | "EvalError" | "URIError"
                    | "AggregateError" | "InternalError" | "SuppressedError"
                    // TypedArrays / buffers
                    | "Buffer" | "ArrayBuffer" | "SharedArrayBuffer" | "DataView"
                    | "Uint8Array" | "Uint8ClampedArray"
                    | "Int8Array" | "Int16Array" | "Uint16Array"
                    | "Int32Array" | "Uint32Array"
                    | "Float32Array" | "Float64Array"
                    | "BigInt64Array" | "BigUint64Array"
                    // Iterables / generators
                    | "Iterable" | "Iterator" | "IteratorResult"
                    | "AsyncIterable" | "AsyncIterator" | "AsyncIteratorResult"
                    | "Generator" | "AsyncGenerator"
                    | "GeneratorFunction" | "AsyncGeneratorFunction"
                    // Common stdlib utility types
                    | "Partial" | "Required" | "Readonly" | "Record" | "Pick"
                    | "Omit" | "Exclude" | "Extract" | "NonNullable"
                    | "ReturnType" | "InstanceType" | "Awaited"
                    | "Parameters" | "ConstructorParameters"
                    | "ThisParameterType" | "OmitThisParameter"
                    | "ThisType" | "Capitalize" | "Uncapitalize"
                    | "Uppercase" | "Lowercase"
                    // Globals sometimes referenced as types
                    | "console" | "JSON" | "Math" | "Reflect" | "Proxy"
                    | "globalThis" | "this"
                    // Perry runtime / UI / system primitives
                    | "Widget" | "Color" | "Font" | "Image"
                )
            }
            let mut local_known: std::collections::HashSet<String> = std::collections::HashSet::new();
            for class in &hir_module.classes {
                local_known.insert(class.name.clone());
            }
            for iface in &hir_module.interfaces {
                local_known.insert(iface.name.clone());
            }
            for en in &hir_module.enums {
                local_known.insert(en.name.clone());
            }
            for ta in &hir_module.type_aliases {
                local_known.insert(ta.name.clone());
            }
            for ic in &imported_classes {
                local_known.insert(ic.name.clone());
                if let Some(alias) = &ic.local_alias {
                    local_known.insert(alias.clone());
                }
            }
            for (n, _) in &imported_enums {
                local_known.insert(n.clone());
            }
            let is_unresolved_name = |name: &str| -> bool {
                !local_known.contains(name)
                    && !all_program_type_names.contains(name)
                    && !is_builtin_type_name(name)
            };
            fn type_has_unresolved<F: Fn(&str) -> bool>(
                ty: &perry_types::Type,
                check: &F,
            ) -> bool {
                use perry_types::Type;
                match ty {
                    Type::Named(name) => check(name),
                    Type::Generic { base, type_args } => {
                        check(base)
                            || type_args.iter().any(|t| type_has_unresolved(t, check))
                    }
                    Type::Array(elem) => type_has_unresolved(elem, check),
                    Type::Promise(inner) => type_has_unresolved(inner, check),
                    Type::Union(variants) => variants.iter().any(|v| type_has_unresolved(v, check)),
                    Type::Tuple(items) => items.iter().any(|v| type_has_unresolved(v, check)),
                    Type::Function(ft) => {
                        ft.params.iter().any(|(_, t, _)| type_has_unresolved(t, check))
                            || type_has_unresolved(&ft.return_type, check)
                    }
                    _ => false,
                }
            }
            fn stmts_have_unresolved<F: Fn(&str) -> bool>(
                stmts: &[perry_hir::Stmt],
                check: &F,
            ) -> bool {
                stmts.iter().any(|s| stmt_has_unresolved(s, check))
            }
            fn stmt_has_unresolved<F: Fn(&str) -> bool>(
                stmt: &perry_hir::Stmt,
                check: &F,
            ) -> bool {
                match stmt {
                    perry_hir::Stmt::Let { ty, .. } => type_has_unresolved(ty, check),
                    perry_hir::Stmt::If { then_branch, else_branch, .. } => {
                        stmts_have_unresolved(then_branch, check)
                            || else_branch
                                .as_ref()
                                .map(|a| stmts_have_unresolved(a, check))
                                .unwrap_or(false)
                    }
                    perry_hir::Stmt::While { body, .. }
                    | perry_hir::Stmt::DoWhile { body, .. } => {
                        stmts_have_unresolved(body, check)
                    }
                    perry_hir::Stmt::For { init, body, .. } => {
                        let init_hit = init
                            .as_ref()
                            .map(|s| stmt_has_unresolved(s.as_ref(), check))
                            .unwrap_or(false);
                        init_hit || stmts_have_unresolved(body, check)
                    }
                    perry_hir::Stmt::Labeled { body, .. } => {
                        stmt_has_unresolved(body.as_ref(), check)
                    }
                    perry_hir::Stmt::Try { body, catch, finally } => {
                        if stmts_have_unresolved(body, check) {
                            return true;
                        }
                        if let Some(c) = catch {
                            if stmts_have_unresolved(&c.body, check) {
                                return true;
                            }
                        }
                        if let Some(f) = finally {
                            if stmts_have_unresolved(f, check) {
                                return true;
                            }
                        }
                        false
                    }
                    perry_hir::Stmt::Switch { cases, .. } => cases
                        .iter()
                        .any(|case| stmts_have_unresolved(&case.body, check)),
                    _ => false,
                }
            }
            fn fn_has_unresolved<F: Fn(&str) -> bool>(
                f: &perry_hir::Function,
                check: &F,
            ) -> bool {
                f.params.iter().any(|p| type_has_unresolved(&p.ty, check))
                    || type_has_unresolved(&f.return_type, check)
                    || stmts_have_unresolved(&f.body, check)
            }
            let mut references_interface = false;
            'outer: for func in &hir_module.functions {
                if fn_has_unresolved(func, &is_unresolved_name) {
                    references_interface = true;
                    break 'outer;
                }
            }
            if !references_interface {
                'outer: for class in &hir_module.classes {
                    for field in &class.fields {
                        if type_has_unresolved(&field.ty, &is_unresolved_name) {
                            references_interface = true;
                            break 'outer;
                        }
                    }
                    if let Some(ctor) = &class.constructor {
                        if fn_has_unresolved(ctor, &is_unresolved_name) {
                            references_interface = true;
                            break 'outer;
                        }
                    }
                    for m in class
                        .methods
                        .iter()
                        .chain(class.static_methods.iter())
                        .chain(class.getters.iter().map(|(_, g)| g))
                        .chain(class.setters.iter().map(|(_, s)| s))
                    {
                        if fn_has_unresolved(m, &is_unresolved_name) {
                            references_interface = true;
                            break 'outer;
                        }
                    }
                }
            }
            if !references_interface
                && stmts_have_unresolved(&hir_module.init, &is_unresolved_name)
            {
                references_interface = true;
            }
            if references_interface {
                for (src_pathbuf, src_hir) in &ctx.native_modules {
                    let src_path = src_pathbuf.to_string_lossy().to_string();
                    for class in &src_hir.classes {
                        if !class.is_exported {
                            continue;
                        }
                        if imported_classes.iter().any(|c| c.name == class.name) {
                            continue;
                        }
                        let class_prefix = compute_module_prefix(&src_path, &ctx.project_root);
                        imported_classes.push(perry_codegen::ImportedClass {
                            name: class.name.clone(),
                            local_alias: None,
                            source_prefix: class_prefix,
                            constructor_param_count: class.constructor.as_ref().map(|c| c.params.len()).unwrap_or(0),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class.methods.iter().map(|m| m.params.len()).collect(),
                            static_method_names: class.static_methods.iter().map(|m| m.name.clone()).collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class.fields.iter().map(|f| f.name.clone()).collect(),
                            field_types: class.fields.iter().map(|f| f.ty.clone()).collect(),
                            source_class_id: Some(class.id),
                        });
                    }
                }
            }

            // Transitive class closure: pull in classes referenced by
            // field types of already-imported classes. Without this, a
            // chain like `vm.viewport.scroll.scrollTop` (where vm is
            // `EditorViewModel`, `viewport: ViewportManager`, `scroll:
            // ScrollController`) breaks at the first hop because only
            // `EditorViewModel` lives in `imported_classes` for this
            // module — `receiver_class_name` can't walk through
            // `viewport.scroll` because `ViewportManager` isn't in
            // `class_table` and its field types are unknown. Closing
            // over field types lets `PropertyGet` recursion resolve
            // the receiver class at every step of the chain.
            let mut visited_imports: std::collections::HashSet<String> = imported_classes
                .iter()
                .map(|ic| ic.name.clone())
                .collect();
            let mut closure_worklist: Vec<String> =
                visited_imports.iter().cloned().collect();
            while let Some(name) = closure_worklist.pop() {
                let ic_idx = imported_classes
                    .iter()
                    .position(|ic| ic.name == name);
                let Some(idx) = ic_idx else { continue };
                let field_types_clone = imported_classes[idx].field_types.clone();
                for ty in &field_types_clone {
                    let ref_name = match ty {
                        perry_types::Type::Named(n) => n.clone(),
                        perry_types::Type::Generic { base, .. } => base.clone(),
                        _ => continue,
                    };
                    if visited_imports.contains(&ref_name) {
                        continue;
                    }
                    let found = exported_classes
                        .iter()
                        .find(|((_, cname), _)| cname == &ref_name)
                        .map(|((path, _), class)| (path.clone(), *class));
                    if let Some((src_path, class)) = found {
                        let class_prefix = compute_module_prefix(&src_path, &ctx.project_root);
                        imported_classes.push(perry_codegen::ImportedClass {
                            name: class.name.clone(),
                            local_alias: None,
                            source_prefix: class_prefix,
                            constructor_param_count: class.constructor.as_ref().map(|c| c.params.len()).unwrap_or(0),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class.methods.iter().map(|m| m.params.len()).collect(),
                            static_method_names: class.static_methods.iter().map(|m| m.name.clone()).collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class.fields.iter().map(|f| f.name.clone()).collect(),
                            field_types: class.fields.iter().map(|f| f.ty.clone()).collect(),
                            source_class_id: Some(class.id),
                        });
                        visited_imports.insert(ref_name.clone());
                        closure_worklist.push(ref_name);
                    }
                }
            }

            // Type aliases from all modules
            let type_alias_map: std::collections::HashMap<String, perry_types::Type> =
                all_type_aliases.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

            // Resolve the CLI's short target name (ios/android/etc.) to
            // an LLVM triple. `None` falls through to the host default
            // inside `compile_module`.
            let resolved_triple = target
                .as_deref()
                .and_then(perry_codegen::resolve_target_triple);
            // ── Feature plumbing ──
            // Set all compile options so the codegen honors
            // the same project configuration. Without this, the
            // auto-optimize feature detection + linker flag
            // construction can't see which modules the program
            // actually uses and strips too much from libperry_stdlib.a.
            let bundled_ext_vec: Vec<(String, String)> = if is_entry {
                bundled_extensions
                    .iter()
                    .map(|(ext_path, _plugin_id)| {
                        let ext_prefix = compute_module_prefix(
                            &ext_path.to_string_lossy(),
                            &ctx.project_root,
                        );
                        (ext_path.to_string_lossy().to_string(), ext_prefix)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let native_module_init_names_vec: Vec<String> = if is_entry {
                non_entry_module_names.clone()
            } else {
                Vec::new()
            };
            let js_module_specifiers_vec: Vec<String> = if needs_js_runtime {
                js_module_specifiers.clone()
            } else {
                Vec::new()
            };

            let opts = perry_codegen::CompileOptions {
                target: resolved_triple,
                is_entry_module: is_entry,
                non_entry_module_prefixes,
                import_function_prefixes,
                emit_ir_only: bitcode_link,
                namespace_imports,
                imported_classes,
                imported_enums,
                imported_async_funcs: imported_async_set,
                type_aliases: type_alias_map,
                imported_func_param_counts: imported_param_counts,
                imported_func_return_types: imported_return_types,
                imported_vars,

                // Feature plumbing
                output_type: args.output_type.clone(),
                needs_stdlib: ctx.needs_stdlib,
                needs_ui: ctx.needs_ui,
                needs_geisterhand: ctx.needs_geisterhand,
                geisterhand_port: ctx.geisterhand_port,
                needs_js_runtime,
                enabled_features: compiled_features.clone(),
                native_module_init_names: native_module_init_names_vec,
                js_module_specifiers: js_module_specifiers_vec,
                bundled_extensions: bundled_ext_vec,
                native_library_functions: ffi_functions.clone(),
                i18n_table: i18n_snapshot.clone(),
            };
            // V2.2 object cache lookup. The key hashes every codegen-affecting
            // field of `opts` together with this module's source hash and the
            // perry version. A hit returns the exact `.o` bytes we emitted
            // the last time opts + source were identical — cross-run bit
            // identity, not just semantic equivalence. A miss (no entry,
            // disabled cache, or IO error) falls through to `compile_module`.
            let cache_key = if object_cache.is_enabled() {
                let source_hash = ctx
                    .module_source_hashes
                    .get(path)
                    .copied()
                    .unwrap_or(0);
                Some(compute_object_cache_key(&opts, source_hash, perry_version))
            } else {
                None
            };
            let object_code = match cache_key.and_then(|k| object_cache.lookup(k)) {
                Some(bytes) => bytes,
                None => {
                    let bytes = perry_codegen::compile_module(hir_module, opts)
                        .map_err(|e| format!(
                            "Error compiling module '{}' ({}) with --backend llvm: {:#}",
                            hir_module.name, path.display(), e
                        ))?;
                    if let Some(k) = cache_key {
                        object_cache.store(k, &bytes);
                    }
                    bytes
                }
            };
            let obj_name = hir_module.name
                .replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
                .trim_matches('_')
                .to_string();
            // In bitcode mode the bytes are .ll text; use .ll extension.
            let ext = if bitcode_link { "ll" } else { "o" };
            let obj_path = PathBuf::from(format!("{}.{}", obj_name, ext));
            return Ok((obj_path, object_code));
        })
        .collect();

    // Tier 4.4 (v0.5.336): partition compile results, then write object
    // files in parallel via rayon. The OS handles concurrent writes to
    // distinct paths, and codegen typically finishes producing bytes
    // faster than a single thread can drain them to disk for projects
    // with many modules. Pre-fix this was a single sequential
    // `for ... fs::write(...)`. Errors from compilation print in source
    // order (preserved); successful writes' "Wrote ..." messages print
    // after all writes complete.
    let mut failed_modules: Vec<String> = Vec::new();
    let mut to_write: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for result in compile_results {
        match result {
            Ok(pair) => to_write.push(pair),
            Err(msg) => {
                eprintln!("{}", msg);
                // Extract module name from error message for
                // failed_modules. Error format is
                // `Error compiling module '<name>' (<path>) ...`.
                if let Some(name) = msg.split('\'').nth(1) {
                    failed_modules.push(name.to_string());
                }
            }
        }
    }

    // Parallel write phase. Returns one Result per write so we can
    // bail on the first I/O error after the par_iter finishes.
    
    let write_results: Vec<Result<(), std::io::Error>> = to_write
        .par_iter()
        .map(|(obj_path, object_code)| fs::write(obj_path, object_code))
        .collect();

    // Bail on first write failure (I/O errors are usually disk-full /
    // permission, not per-file recoverable).
    for r in write_results {
        if let Err(e) = r {
            return Err(e.into());
        }
    }

    // Sequential print + obj_paths collection (output grouped, source
    // order preserved).
    for (obj_path, _) in to_write {
        match format {
            OutputFormat::Text => {
                let label = if obj_path.extension().and_then(|e| e.to_str()) == Some("ll") {
                    "Wrote LLVM IR"
                } else {
                    "Wrote object file"
                };
                println!("{}: {}", label, obj_path.display());
            }
            OutputFormat::Json => {}
        }
        obj_paths.push(obj_path);
    }

    // Verbose codegen-cache stats. We print here (rather than in dev.rs
    // alongside the parse-cache line) only when `parse_cache` is `None`
    // — i.e. batch `perry compile` / `perry run` invocations. In the
    // `perry dev` hot path, `run_with_parse_cache` is called with a
    // `Some(cache)` and `dev.rs` prints both `parse cache:` and
    // `codegen cache:` lines together after we return, so printing here
    // would duplicate the codegen line. The env var matches the one
    // `perry dev` uses so a single `PERRY_DEV_VERBOSE=1` turns on cache
    // diagnostics everywhere.
    if parse_cache.is_none()
        && object_cache.is_enabled()
        && std::env::var("PERRY_DEV_VERBOSE").ok().as_deref() == Some("1")
    {
        let h = object_cache.hits();
        let m = object_cache.misses();
        let total = h + m;
        if total > 0 {
            eprintln!("  • codegen cache: {}/{} hit ({} miss)", h, total, m);
        }
    }

    // ── Loud failure summary ─────────────────────────────────────────
    //
    // Render the per-module compile errors prominently *here*, before
    // `build_optimized_libs` runs cargo and floods stdout/stderr with
    // hundreds of lines of warnings. The individual `eprintln!("{}", msg)`
    // calls above produced one line per failure that gets buried in the
    // cargo noise; this block re-surfaces them in a box-drawn header so
    // it's the last thing the user sees before the linking step.
    //
    // Critically: if the *entry* module is in the failed list, the
    // linker can't possibly produce a working executable — `main` is
    // emitted by the entry module's `compile_module_entry` path, and a
    // stub `_perry_init_*` doesn't satisfy that. The original 0.5.0
    // mango bug was exactly this: 13 modules failed (including
    // `mango/src/app.ts` itself), the driver replaced them all with
    // empty inits, and the link step exploded with `Undefined symbols
    // for architecture arm64: "_main"` — which is a downstream symptom
    // that took a lot of digging to trace back to the real codegen
    // errors hidden in the build noise. Hard-fail here instead.
    let entry_module_name: Option<String> = ctx
        .native_modules
        .get(&entry_path)
        .map(|h| h.name.clone());
    if !failed_modules.is_empty() {
        let entry_failed = entry_module_name
            .as_deref()
            .map(|name| failed_modules.iter().any(|m| m == name))
            .unwrap_or(false);

        let bar = "═".repeat(72);
        let (red_on, red_off, bold_on, bold_off) = if use_color {
            ("\x1b[1;31m", "\x1b[0m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "")
        };
        eprintln!();
        if entry_failed {
            eprintln!("{}{}{}", red_on, bar, red_off);
            eprintln!(
                "{}✗ ENTRY MODULE FAILED TO COMPILE — REFUSING TO LINK{}",
                red_on, red_off
            );
            eprintln!("{}{}{}", red_on, bar, red_off);
        } else {
            eprintln!("{}{}{}", red_on, bar, red_off);
            eprintln!(
                "{}⚠ {} module(s) failed to compile — linking with empty stubs{}",
                red_on,
                failed_modules.len(),
                red_off
            );
            eprintln!("{}{}{}", red_on, bar, red_off);
        }
        eprintln!();
        for m in &failed_modules {
            let is_entry = Some(m.as_str()) == entry_module_name.as_deref();
            let marker = if is_entry { " (entry)" } else { "" };
            eprintln!("  - {}{}{}{}", bold_on, m, marker, bold_off);
        }
        eprintln!();
        if entry_failed {
            eprintln!(
                "Aborting: the entry module's `main` symbol is required by the linker."
            );
            eprintln!("Fix the codegen errors above (search for `Error compiling module`)");
            eprintln!("and re-run. The driver previously emitted an empty `_perry_init_*`");
            eprintln!("stub here and continued to link, which produced the misleading");
            eprintln!("`Undefined symbols: \"_main\"` error far downstream.");
            eprintln!();
            return Err(anyhow!(
                "entry module '{}' failed to compile (see errors above)",
                entry_module_name.as_deref().unwrap_or("?")
            ));
        } else {
            eprintln!("Continuing with linking. Empty `_perry_init_*` stubs will be");
            eprintln!("emitted for the failed modules so the binary still links, but");
            eprintln!("any code in those modules will be inert at runtime.");
            eprintln!();
        }
    }

    // Auto-mode: pick the smallest matching (features, panic) profile
    // for this binary and rebuild perry-runtime + perry-stdlib in a
    // hash-keyed target dir. Both halves fall back to the prebuilt full
    // libraries if the rebuild fails or the workspace source isn't on
    // disk. `--no-auto-optimize` disables the rebuild path entirely.
    //
    // The legacy `--minimal-stdlib` flag is now a no-op alias for
    // backward compat — auto-mode already does what it used to and more.
    let optimized_libs: OptimizedLibs = if args.no_auto_optimize {
        OptimizedLibs::empty()
    } else {
        build_optimized_libs(&ctx, target.as_deref(), &compiled_features, format, verbose)
    };
    let stdlib_lib_resolved: Option<PathBuf> = optimized_libs.stdlib.clone()
        .or_else(|| find_stdlib_library(target.as_deref()));

    // Generate stubs for missing symbols from unresolved imports (npm packages etc.)
    {
        use std::collections::HashSet;
        let mut undefined_syms: HashSet<String> = HashSet::new();
        let mut defined_syms: HashSet<String> = HashSet::new();
        // Prefer the auto-built runtime so the symbol-stub scan and the
        // final link see the same artifact (panic mode + feature set).
        let runtime_lib_path = optimized_libs.runtime.clone()
            .or_else(|| find_runtime_library(target.as_deref()).ok());
        let stdlib_lib_path = stdlib_lib_resolved.clone();
        // Check if jsruntime will be used - if so, don't generate stubs for its symbols
        let use_jsruntime = ctx.needs_js_runtime || args.enable_js_runtime;
        // Check if stdlib will be linked - if so, it provides perry_runtime symbols (no stubs needed)
        let target_is_windows = matches!(target.as_deref(), Some("windows")) || (cfg!(target_os = "windows") && target.is_none());
        let will_link_stdlib = (ctx.needs_stdlib || target_is_windows) && stdlib_lib_path.is_some();
        let jsruntime_lib_path = if use_jsruntime {
            find_jsruntime_library(target.as_deref())
        } else {
            None
        };
        let mut all_scan_paths: Vec<PathBuf> = obj_paths.clone();
        if let Some(ref p) = runtime_lib_path { all_scan_paths.push(p.clone()); }
        if ctx.needs_stdlib {
            if let Some(ref p) = stdlib_lib_path { all_scan_paths.push(p.clone()); }
        }
        if let Some(ref p) = jsruntime_lib_path { all_scan_paths.push(p.clone()); }
        // Scan UI library for defined symbols so we don't generate stubs for
        // functions that exist in the platform UI library (e.g. screen detection FFI)
        if ctx.needs_ui {
            if let Some(ui_lib) = find_ui_library(target.as_deref()) {
                all_scan_paths.push(ui_lib);
            }
        }
        // Mark native library FFI functions as defined so we don't generate stubs
        // that would shadow the real implementations in the native library .a/.so
        for native_lib in &ctx.native_libraries {
            for func in &native_lib.functions {
                defined_syms.insert(func.name.clone());
            }
        }
        // Platform detection for nm tool and symbol prefix
        let _is_ios = matches!(target.as_deref(), Some("ios-simulator") | Some("ios"));
        let is_android = matches!(target.as_deref(), Some("android"));
        let is_harmonyos = matches!(target.as_deref(), Some("harmonyos") | Some("harmonyos-simulator"));
        let is_linux = matches!(target.as_deref(), Some("linux")) || (!cfg!(target_os = "macos") && !cfg!(target_os = "windows") && target.is_none());
        let is_windows = matches!(target.as_deref(), Some("windows")) || (cfg!(target_os = "windows") && target.is_none());
        // Symbol prefix depends on object format:
        // Mach-O targets (macOS, iOS, watchOS, tvOS): nm shows `_` prefix
        // COFF (Windows targets): no prefix
        // ELF (Linux/Android/HarmonyOS targets): no prefix
        // Use TARGET (what we're compiling to), not HOST (what we're running on)
        let is_macho = matches!(target.as_deref(),
            Some("ios") | Some("ios-simulator") | Some("ios-widget") | Some("ios-widget-simulator") |
            Some("visionos") | Some("visionos-simulator") |
            Some("macos") | Some("watchos") | Some("watchos-simulator") |
            Some("tvos") | Some("tvos-simulator")
        ) || (!is_windows && !is_linux && !is_android && !is_harmonyos && cfg!(target_os = "macos"));
        // Find the nm tool: use llvm-nm when cross-compiling (host nm can't read foreign object formats)
        let needs_llvm_nm = is_windows || (is_macho && !cfg!(target_os = "macos"));
        let nm_cmd = if needs_llvm_nm {
            find_llvm_tool("llvm-nm")
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "nm".to_string())
        } else {
            "nm".to_string()
        };
        // Scan object files in parallel for symbol resolution
        let scan_results: Vec<(HashSet<String>, HashSet<String>)> = all_scan_paths.par_iter()
            .map(|scan_path| {
                let mut local_undef = HashSet::new();
                let mut local_def = HashSet::new();
                if let Ok(output) = std::process::Command::new(&nm_cmd).arg("-g").arg(scan_path).output() {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let (st, sn) = if parts.len() == 3 { (parts[1], parts[2]) } else { (parts[0], parts[1]) };
                            let cn = if is_macho {
                                sn.strip_prefix('_').unwrap_or(sn)
                            } else {
                                sn
                            };
                            if st == "U" {
                                if cn.starts_with("__export_") || cn.starts_with("__wrapper_") {
                                    local_undef.insert(cn.to_string());
                                } else if !use_jsruntime && !will_link_stdlib && (cn == "js_call_function" || cn == "js_load_module" || cn == "js_new_from_handle"
                                    || cn == "js_new_instance" || cn == "js_create_callback" || cn == "js_runtime_init"
                                    || cn == "js_set_property" || cn == "js_get_export" || cn == "js_await_js_promise") {
                                    local_undef.insert(cn.to_string());
                                } else if is_windows && (
                                    cn.starts_with("perry_ui_") || cn.starts_with("perry_system_") ||
                                    cn.starts_with("perry_plugin_") || cn.starts_with("perry_get_")
                                ) {
                                    local_undef.insert(cn.to_string());
                                }
                            } else if matches!(st, "T" | "t" | "D" | "d" | "S" | "s" | "B" | "b") {
                                local_def.insert(cn.to_string());
                            }
                        }
                    }
                }
                (local_undef, local_def)
            })
            .collect();

        // Merge parallel scan results
        for (local_undef, local_def) in scan_results {
            undefined_syms.extend(local_undef);
            defined_syms.extend(local_def);
        }
        let missing: Vec<String> = undefined_syms.difference(&defined_syms).cloned().collect();
        if !missing.is_empty() {
            let (mut md, mut mf, mut mi) = (Vec::new(), Vec::new(), Vec::new());
            for s in &missing {
                if s.starts_with("__export_") {
                    md.push(s.clone());
                } else if s == "js_await_any_promise" {
                    // Identity stub: takes f64, returns it as-is (pass-through for standalone builds)
                    mi.push(s.clone());
                } else {
                    mf.push(s.clone());
                }
            }
            if let OutputFormat::Text = format { eprintln!("  Generating stubs for {} missing symbols ({} data, {} functions, {} identity)", missing.len(), md.len(), mf.len(), mi.len()); for s in &missing { eprintln!("    - {}", s); } }
            let stub_bytes = perry_codegen::stubs::generate_stub_object(&md, &mf, &mi, target.as_deref())?;
            let stub_path = PathBuf::from("_perry_stubs.o");
            fs::write(&stub_path, &stub_bytes)?;
            obj_paths.push(stub_path);
        }
    }

    // Phase J: bitcode link — merge user .ll + runtime/stdlib .bc into one
    // optimized object via llvm-link → opt → llc. This replaces both the
    // per-module clang -c step AND the archive linking.
    let _bitcode_linked = if bitcode_link && optimized_libs.runtime_bc.is_some() {
        if matches!(format, OutputFormat::Text) {
            println!("Using LLVM bitcode link (whole-program LTO)");
        }
        // Separate .ll files (user modules) from .o files (stubs)
        let ll_files: Vec<PathBuf> = obj_paths.iter()
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ll"))
            .cloned()
            .collect();
        let stub_objs: Vec<PathBuf> = obj_paths.iter()
            .filter(|p| p.extension().and_then(|e| e.to_str()) != Some("ll"))
            .cloned()
            .collect();

        if ll_files.is_empty() {
            eprintln!("  bitcode-link: no .ll files produced, falling back to normal link");
            false
        } else {
            let runtime_bc = optimized_libs.runtime_bc.as_ref().unwrap();
            let stdlib_bc = optimized_libs.stdlib_bc.as_deref();

            match perry_codegen::linker::bitcode_link_pipeline(
                &ll_files,
                runtime_bc,
                stdlib_bc,
                &optimized_libs.extra_bc,
                target.as_deref(),
            ) {
                Ok(linked_obj) => {
                    match format {
                        OutputFormat::Text => {
                            if let Ok(meta) = std::fs::metadata(&linked_obj) {
                                println!(
                                    "  bitcode-link: merged {} modules → {} ({:.1} MB)",
                                    ll_files.len(),
                                    linked_obj.display(),
                                    meta.len() as f64 / (1024.0 * 1024.0)
                                );
                            }
                        }
                        OutputFormat::Json => {}
                    }
                    // Clean up intermediate .ll files
                    for ll in &ll_files {
                        let _ = fs::remove_file(ll);
                    }
                    // Replace obj_paths with the merged .o + any stubs
                    obj_paths = vec![linked_obj];
                    obj_paths.extend(stub_objs);
                    true
                }
                Err(e) => {
                    eprintln!("  bitcode-link: pipeline failed ({}), falling back to normal link", e);
                    false
                }
            }
        }
    } else if bitcode_link {
        // bitcode_link was requested but runtime .bc wasn't produced.
        // Fall back: compile any .ll files to .o via clang -c.
        eprintln!("  bitcode-link: runtime .bc not available, falling back to normal link");
        let mut new_obj_paths: Vec<PathBuf> = Vec::new();
        for p in &obj_paths {
            if p.extension().and_then(|e| e.to_str()) == Some("ll") {
                let ll_text = fs::read_to_string(p)?;
                let obj_bytes = perry_codegen::linker::compile_ll_to_object(
                    &ll_text,
                    target.as_deref(),
                )?;
                let obj_path = p.with_extension("o");
                fs::write(&obj_path, &obj_bytes)?;
                let _ = fs::remove_file(p);
                new_obj_paths.push(obj_path);
            } else {
                new_obj_paths.push(p.clone());
            }
        }
        obj_paths = new_obj_paths;
        false
    } else {
        false
    };

    // Generate JS bundle if needed
    let _js_bundle_path = if ctx.needs_js_runtime && !ctx.js_modules.is_empty() {
        let bundle_path = generate_js_bundle(&ctx, Path::new("."))?;
        match format {
            OutputFormat::Text => println!("Generated JS bundle: {}", bundle_path.display()),
            OutputFormat::Json => {}
        }
        Some(bundle_path)
    } else {
        None
    };

    let stem = args
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let is_dylib = args.output_type == "dylib";
    let exe_path = args.output.unwrap_or_else(|| {
        if is_dylib {
            #[cfg(target_os = "macos")]
            { PathBuf::from(format!("{}.dylib", stem)) }
            #[cfg(not(target_os = "macos"))]
            { PathBuf::from(format!("{}.so", stem)) }
        } else if matches!(target.as_deref(), Some("harmonyos") | Some("harmonyos-simulator")) {
            // HarmonyOS apps ship as .so loaded by the ArkTS runtime via
            // napi_module_register — there is no standalone executable
            // shipping shape. `lib` prefix matches the dlopen name used by
            // the generated ArkTS shim (`import entry from 'libapp.so'`).
            PathBuf::from(format!("lib{}.so", stem))
        } else if matches!(target.as_deref(), Some("windows"))
            || (target.is_none() && cfg!(target_os = "windows"))
        {
            PathBuf::from(format!("{}.exe", stem))
        } else {
            PathBuf::from(stem)
        }
    });

    if !failed_modules.is_empty() {
        // The loud failure summary + entry-module abort already ran
        // earlier (right after the parallel compile loop), so by the
        // time we get here we know the entry module compiled OK and
        // every entry in `failed_modules` is a non-entry module that
        // we're consciously stubbing out so the binary can still link.
        // Generate one empty `_perry_init_*` per failed module — the
        // entry main calls each non-entry init in order, so the symbols
        // need to exist or the linker will fail.
        let stub_init_names: Vec<String> = failed_modules
            .iter()
            .map(|m| {
                let sanitized = m.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
                format!("_perry_init_{}", sanitized)
            })
            .collect();
        if !stub_init_names.is_empty() {
            let stub_bytes = perry_codegen::stubs::generate_stub_object(
                &[],
                &stub_init_names,
                &[],
                target.as_deref(),
            )?;
            let stub_path = PathBuf::from("_perry_failed_stubs.o");
            fs::write(&stub_path, &stub_bytes)?;
            obj_paths.push(stub_path);
        }
    }

    if args.no_link {
        let codegen_cache_stats = if object_cache.is_enabled() {
            Some((object_cache.hits(), object_cache.misses(), object_cache.stores(), object_cache.store_errors()))
        } else { None };
        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            is_dylib,
            codegen_cache_stats,
        });
    }

    match format {
        OutputFormat::Text => {
            if ctx.needs_stdlib {
                println!("Linking (with stdlib)...");
            } else {
                println!("Linking (runtime-only)...");
            }
        }
        OutputFormat::Json => {}
    }

    let is_ios = matches!(target.as_deref(), Some("ios-simulator") | Some("ios"));
    let is_visionos = matches!(target.as_deref(), Some("visionos-simulator") | Some("visionos"));
    let is_android = matches!(target.as_deref(), Some("android"));
    let is_harmonyos = matches!(target.as_deref(), Some("harmonyos") | Some("harmonyos-simulator"));
    let is_linux = matches!(target.as_deref(), Some("linux"))
        || (target.is_none() && cfg!(target_os = "linux"));
    let _is_windows = matches!(target.as_deref(), Some("windows"))
        || (target.is_none() && cfg!(target_os = "windows"));
    // is_watchos / is_tvos are defined below (near jsruntime_lib).
    // The is_cross_* bindings used to live here, but they're now derived
    // inside `link::build_and_run_link` which is the only consumer.

    // For dylib output, skip runtime/stdlib linking — symbols resolve from host at dlopen time
    if is_dylib {
        let mut cmd = if is_linux {
            let mut c = Command::new("cc");
            c.arg("-shared");
            c
        } else {
            // macOS — use flat_namespace so plugins can resolve symbols from the host
            let mut c = Command::new("cc");
            c.arg("-dynamiclib")
             .arg("-flat_namespace")
             .arg("-undefined").arg("dynamic_lookup");
            c
        };

        for obj_path in &obj_paths {
            cmd.arg(obj_path);
        }

        cmd.arg("-o").arg(&exe_path);

        let status = cmd.status()?;
        if !status.success() {
            return Err(anyhow!("Linking dylib failed"));
        }

        match format {
            OutputFormat::Text => println!("Wrote shared library: {}", exe_path.display()),
            OutputFormat::Json => {
                println!("{{\"output\": \"{}\"}}", exe_path.display());
            }
        }

        // Clean up intermediate files
        if !args.keep_intermediates {
            for obj_path in &obj_paths {
                let _ = fs::remove_file(obj_path);
            }
        }

        let codegen_cache_stats = if object_cache.is_enabled() {
            Some((object_cache.hits(), object_cache.misses(), object_cache.stores(), object_cache.store_errors()))
        } else { None };
        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            is_dylib: true,
            codegen_cache_stats,
        });
    }

    // When geisterhand is enabled, prefer the geisterhand-enabled runtime
    // (has the registry, dispatch queue, and pump functions). Otherwise
    // prefer the auto-mode rebuild (which may be panic=abort) over the
    // prebuilt one. Auto-mode never enables panic=abort when geisterhand
    // is on, so the geisterhand path always uses the prebuilt variant.
    let runtime_lib = if ctx.needs_geisterhand {
        if let Some(gh_rt) = find_geisterhand_runtime(target.as_deref()) {
            gh_rt
        } else {
            find_runtime_library(target.as_deref())?
        }
    } else if let Some(auto_rt) = optimized_libs.runtime.clone() {
        auto_rt
    } else {
        find_runtime_library(target.as_deref())?
    };
    let stdlib_lib = stdlib_lib_resolved.clone();
    let is_watchos = matches!(target.as_deref(), Some("watchos") | Some("watchos-simulator"));
    let is_tvos = matches!(target.as_deref(), Some("tvos") | Some("tvos-simulator"));
    let jsruntime_lib = if !is_ios && !is_visionos && !is_android && !is_harmonyos && !is_watchos && !is_tvos && (ctx.needs_js_runtime || args.enable_js_runtime) {
        match find_jsruntime_library(target.as_deref()) {
            Some(lib) => {
                match format {
                    OutputFormat::Text => println!("Using V8 JavaScript runtime for JS module support"),
                    OutputFormat::Json => {}
                }
                Some(lib)
            }
            None => {
                if ctx.needs_js_runtime {
                    return Err(anyhow!(
                        "JavaScript modules found but libperry_jsruntime.a not found. Build it with: cargo build --release -p perry-jsruntime"
                    ));
                }
                None
            }
        }
    } else {
        None
    };

    // Build & run the per-platform link command. Tier 2.1 final extraction
    // (v0.5.342) — see crates/perry/src/commands/compile/link.rs.
    build_and_run_link(
        &args.input,
        &ctx,
        target.as_deref(),
        &obj_paths,
        &compiled_features,
        &runtime_lib,
        &stdlib_lib,
        &jsruntime_lib,
        &exe_path,
        format,
    )?;

    // HarmonyOS: emit the ArkTS EntryAbility + Index page next to the .so,
    // then bundle everything into a .hap. The ArkTS shim's import name is
    // templated off the actual .so filename so it matches at dlopen time.
    if is_harmonyos {
        if let Some(output_dir) = exe_path.parent() {
            let so_filename = exe_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("libperry_app.so");
            let stem = exe_path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("app")
                .trim_start_matches("lib");
            match emit_harmonyos_arkts_stubs(output_dir, so_filename) {
                Err(e) => eprintln!("Warning: failed to emit ArkTS shim: {}", e),
                Ok(()) => {
                    if matches!(format, OutputFormat::Text) {
                        println!("Wrote ArkTS shim: {}/ets/", output_dir.display());
                    }
                    let sdk = find_harmonyos_sdk();
                    let hap_args = crate::commands::harmonyos_hap::HapBuildArgs {
                        so_path: &exe_path,
                        ets_dir: &output_dir.join("ets"),
                        stem,
                        sdk_native: sdk.as_deref(),
                        quiet: !matches!(format, OutputFormat::Text),
                    };
                    match crate::commands::harmonyos_hap::build_hap(&hap_args) {
                        Ok(res) => {
                            if matches!(format, OutputFormat::Text) {
                                println!(
                                    "Wrote HAP: {} ({}, ets: {})",
                                    res.hap_path.display(),
                                    if res.signed { "signed" } else { "unsigned" },
                                    if res.abc_compiled { "bytecode" } else { "source" },
                                );
                            }
                        }
                        Err(e) => eprintln!("Warning: HAP assembly failed: {}", e),
                    }
                }
            }
        }
    }

    // For Android and HarmonyOS, copy companion shared libraries (.so) next to
    // the output binary so the downstream bundler (APK/AAB for Android, HAP for
    // HarmonyOS in PR B.3) can pick them up from the staging dir.
    if is_android || is_harmonyos {
        if let Some(output_dir) = exe_path.parent() {
            for native_lib in &ctx.native_libraries {
                if let Some(ref target_config) = native_lib.target_config {
                    let lib_name = &target_config.lib_name;
                    if lib_name.ends_with(".so") {
                        let crate_target_dir = target_config.crate_path.join("target");
                        let candidate = if let Some(triple) = rust_target_triple(target.as_deref()) {
                            crate_target_dir.join(triple).join("release").join(lib_name)
                        } else {
                            crate_target_dir.join("release").join(lib_name)
                        };
                        if candidate.exists() {
                            let dest = output_dir.join(lib_name);
                            if let Err(e) = fs::copy(&candidate, &dest) {
                                eprintln!("Warning: failed to copy companion library {}: {}", lib_name, e);
                            } else {
                                match format {
                                    OutputFormat::Text => println!("Copied companion library: {}", lib_name),
                                    OutputFormat::Json => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Track iOS bundle info for CompileResult
    let mut result_bundle_id: Option<String> = None;
    let mut result_app_dir: Option<PathBuf> = None;

    // For iOS targets, create a .app bundle
    if is_ios {
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        // Precedence: --app-bundle-id CLI flag > perry.toml [ios].bundle_id / [app]
        // / [project] / top-level > package.json "bundleId" > com.perry.{name}.
        // CLI wins so callers (doc-tests harness, CI, scripts) can override the
        // embedded ID without editing manifests; without this the app installs
        // under its fallback CFBundleIdentifier and a later `simctl launch
        // <custom-id>` fails with FBSOpenApplicationServiceErrorDomain code=4.
        let bundle_id = args.app_bundle_id.clone().or_else(|| {
            (|| -> Option<String> {
                let mut dir = args.input.canonicalize().ok()?;
                for _ in 0..5 {
                    dir = dir.parent()?.to_path_buf();
                    // Check perry.toml first: [ios].bundle_id, then top-level bundle_id
                    let toml_path = dir.join("perry.toml");
                    if toml_path.exists() {
                        if let Ok(data) = fs::read_to_string(&toml_path) {
                            if let Ok(doc) = data.parse::<toml::Table>() {
                                let toml_bid = doc.get("ios")
                                    .and_then(|i| i.get("bundle_id"))
                                    .or_else(|| doc.get("app").and_then(|a| a.get("bundle_id")))
                                    .or_else(|| doc.get("project").and_then(|p| p.get("bundle_id")))
                                    .or_else(|| doc.get("bundle_id"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                if toml_bid.is_some() {
                                    return toml_bid;
                                }
                            }
                        }
                    }
                    // Then check package.json
                    let pkg = dir.join("package.json");
                    if pkg.exists() {
                        let data = fs::read_to_string(pkg).ok()?;
                        let idx = data.find("\"bundleId\"")?;
                        let colon = data[idx..].find(':')?;
                        let q1 = data[idx + colon..].find('"')? + idx + colon + 1;
                        let q2 = data[q1..].find('"')? + q1;
                        return Some(data[q1..q2].to_string());
                    }
                }
                None
            })()
        }).unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        // Read perry.toml for version, build_number, name
        let (toml_version, toml_build_number, _toml_name) = (|| -> Option<(Option<String>, Option<String>, Option<String>)> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    let data = fs::read_to_string(&toml_path).ok()?;
                    let doc: toml::Table = data.parse().ok()?;
                    let project = doc.get("project")?.as_table()?;
                    let version = project.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let build_number = project.get("build_number").and_then(|v| {
                        v.as_integer().map(|n| n.to_string()).or_else(|| v.as_str().map(|s| s.to_string()))
                    });
                    let name = project.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                    return Some((version, build_number, name));
                }
            }
            None
        })().unwrap_or((None, None, None));
        let app_version = toml_version.as_deref().unwrap_or("1.0.0");
        let app_build_number = toml_build_number.as_deref().unwrap_or("1");

        let encryption_exempt_plist = (|| -> Option<String> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    let data = fs::read_to_string(toml_path).ok()?;
                    let doc: toml::Table = data.parse().ok()?;
                    let ios = doc.get("ios")?.as_table()?;
                    let exempt = ios.get("encryption_exempt")?.as_bool()?;
                    if exempt {
                        return Some(
                            "    <key>ITSAppUsesNonExemptEncryption</key>\n    <false/>".into()
                        );
                    } else {
                        return Some(
                            "    <key>ITSAppUsesNonExemptEncryption</key>\n    <true/>".into()
                        );
                    }
                }
            }
            None
        })().unwrap_or_default();

        // Game-loop apps use traditional UIApplicationMain lifecycle, not SceneDelegate.
        // Including UIApplicationSceneManifest causes a black screen with game-loop.
        let scene_manifest = if compiled_features.iter().any(|f| f == "ios-game-loop") {
            String::new()
        } else {
            r#"    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UIApplicationSupportsMultipleScenes</key>
        <false/>
        <key>UISceneConfigurations</key>
        <dict>
            <key>UIWindowSceneSessionRoleApplication</key>
            <array>
                <dict>
                    <key>UISceneConfigurationName</key>
                    <string>Default Configuration</string>
                    <key>UISceneDelegateClassName</key>
                    <string>PerrySceneDelegate</string>
                </dict>
            </array>
        </dict>
    </dict>
"#.to_string()
        };

        // Simulator bundles must declare iPhoneSimulator / iphonesimulator in
        // Info.plist. Mismatch against the Mach-O LC_BUILD_VERSION (which is
        // "iphonesimulator" when the binary was built for -target
        // aarch64-apple-ios-sim) causes simctl to refuse launch with
        // `FBSOpenApplicationServiceErrorDomain code=4`.
        let is_sim = matches!(target.as_deref(), Some("ios-simulator"));
        let plist_supported_platform = if is_sim { "iPhoneSimulator" } else { "iPhoneOS" };
        let plist_platform_name = if is_sim { "iphonesimulator" } else { "iphoneos" };
        let plist_sdk_name = if is_sim { "iphonesimulator" } else { "iphoneos" };
        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>{app_build_number}</string>
    <key>CFBundleShortVersionString</key>
    <string>{app_version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleIconName</key>
    <string>AppIcon</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>CFBundleSupportedPlatforms</key>
    <array><string>{plist_supported_platform}</string></array>
    <key>DTPlatformName</key>
    <string>{plist_platform_name}</string>
    <key>DTPlatformVersion</key>
    <string>26.4</string>
    <key>DTSDKName</key>
    <string>{plist_sdk_name}26.4</string>
    <key>DTPlatformBuild</key>
    <string>23E237</string>
    <key>DTSDKBuild</key>
    <string>23E237</string>
    <key>DTXcode</key>
    <string>2640</string>
    <key>DTXcodeBuild</key>
    <string>17E192</string>
    <key>DTCompiler</key>
    <string>com.apple.compilers.llvm.clang.1_0</string>
    <key>UIRequiredDeviceCapabilities</key>
    <array><string>arm64</string></array>
    <key>CFBundleIcons</key>
    <dict>
        <key>CFBundlePrimaryIcon</key>
        <dict>
            <key>CFBundleIconFiles</key>
            <array>
                <string>AppIcon60x60</string>
            </array>
        </dict>
    </dict>
    <key>CFBundleIcons~ipad</key>
    <dict>
        <key>CFBundlePrimaryIcon</key>
        <dict>
            <key>CFBundleIconFiles</key>
            <array>
                <string>AppIcon76x76</string>
            </array>
        </dict>
    </dict>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationPortraitUpsideDown</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    {scene_manifest}</dict>
</plist>"#,
        );

        // Apply orientations from perry.toml [ios].orientations
        let info_plist = (|| -> Option<String> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    let data = fs::read_to_string(&toml_path).ok()?;
                    let doc: toml::Table = data.parse().ok()?;
                    let ios = doc.get("ios")?.as_table()?;
                    let orientations = ios.get("orientations")?.as_array()?;
                    let mut entries = Vec::new();
                    for o in orientations {
                        let s = o.as_str()?;
                        match s {
                            "landscape" => {
                                entries.push("UIInterfaceOrientationLandscapeLeft");
                                entries.push("UIInterfaceOrientationLandscapeRight");
                            }
                            "portrait" => {
                                entries.push("UIInterfaceOrientationPortrait");
                                entries.push("UIInterfaceOrientationPortraitUpsideDown");
                            }
                            other => {
                                // Allow raw UIInterfaceOrientation* values
                                if other.starts_with("UIInterfaceOrientation") {
                                    entries.push(other);
                                }
                            }
                        }
                    }
                    if !entries.is_empty() {
                        let xml: String = entries.iter()
                            .map(|e| format!("        <string>{}</string>", e))
                            .collect::<Vec<_>>().join("\n");
                        let all_orientations = format!(
                            "    <key>UISupportedInterfaceOrientations</key>\n    <array>\n{}\n    </array>",
                            xml
                        );
                        // Replace both iPhone and iPad orientation blocks
                        let mut plist = info_plist.clone();
                        // Replace iPhone orientations
                        if let (Some(start), Some(_)) = (
                            plist.find("<key>UISupportedInterfaceOrientations</key>"),
                            plist.find("<key>UISupportedInterfaceOrientations~ipad</key>"),
                        ) {
                            let ipad_start = plist.find("<key>UISupportedInterfaceOrientations~ipad</key>").unwrap();
                            // Find end of iPhone array
                            let _iphone_section = &plist[start..ipad_start];
                            plist = format!(
                                "{}{}\n    {}",
                                &plist[..start],
                                all_orientations,
                                &plist[ipad_start..]
                            );
                            // iPad must always have all 4 orientations for App Store validation
                            // (the app can still lock to landscape at runtime)
                        }
                        return Some(plist);
                    }
                }
            }
            None
        })().unwrap_or(info_plist);

        // Append usage descriptions for camera and microphone
        let usage_descriptions = concat!(
            "    <key>NSCameraUsageDescription</key>\n",
            "    <string>This app uses the camera to identify colors.</string>\n",
            "    <key>NSMicrophoneUsageDescription</key>\n",
            "    <string>This app uses the microphone to measure sound levels.</string>",
        );
        let info_plist = info_plist.replace(
            "</dict>\n</plist>",
            &format!("{}\n</dict>\n</plist>", usage_descriptions),
        );

        // Append ITSAppUsesNonExemptEncryption if configured in perry.toml
        let info_plist = if !encryption_exempt_plist.is_empty() {
            info_plist.replace(
                "</dict>\n</plist>",
                &format!("{}\n</dict>\n</plist>", encryption_exempt_plist),
            )
        } else {
            info_plist
        };

        // Append custom Info.plist entries from [ios.info_plist] in perry.toml
        let custom_plist_entries = (|| -> Option<String> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if toml_path.exists() {
                    let data = fs::read_to_string(&toml_path).ok()?;
                    let doc: toml::Table = data.parse().ok()?;
                    let ios = doc.get("ios")?.as_table()?;
                    let info_plist_table = ios.get("info_plist")?.as_table()?;
                    let mut entries = String::new();
                    for (key, value) in info_plist_table {
                        if let Some(s) = value.as_str() {
                            entries.push_str(&format!(
                                "    <key>{}</key>\n    <string>{}</string>\n",
                                key, s
                            ));
                        } else if let Some(b) = value.as_bool() {
                            entries.push_str(&format!(
                                "    <key>{}</key>\n    <{}/>",
                                key, if b { "true" } else { "false" }
                            ));
                        }
                    }
                    if !entries.is_empty() {
                        return Some(entries);
                    }
                }
            }
            None
        })().unwrap_or_default();
        let info_plist = if !custom_plist_entries.is_empty() {
            info_plist.replace(
                "</dict>\n</plist>",
                &format!("{}</dict>\n</plist>", custom_plist_entries),
            )
        } else {
            info_plist
        };

        fs::write(app_dir.join("Info.plist"), info_plist)?;

        // Read splash screen config from package.json perry.splash section
        let splash_config: Option<(Option<std::path::PathBuf>, String, Option<std::path::PathBuf>)> = (|| -> Option<(Option<std::path::PathBuf>, String, Option<std::path::PathBuf>)> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let pkg = dir.join("package.json");
                if pkg.exists() {
                    let data = fs::read_to_string(&pkg).ok()?;
                    let pkg_val: serde_json::Value = serde_json::from_str(&data).ok()?;
                    let splash = pkg_val.get("perry")?.get("splash")?;

                    // Check for custom storyboard override first
                    if let Some(sb_path) = splash.get("ios").and_then(|i| i.get("storyboard")).and_then(|v| v.as_str()) {
                        let abs = dir.join(sb_path);
                        if abs.exists() {
                            return Some((None, "#FFFFFF".into(), Some(abs)));
                        }
                    }

                    // Resolve image: splash.ios.image -> splash.image
                    let image_path = splash.get("ios").and_then(|i| i.get("image")).and_then(|v| v.as_str())
                        .or_else(|| splash.get("image").and_then(|v| v.as_str()))
                        .map(|p| dir.join(p))
                        .filter(|p| p.exists());

                    // Resolve background: splash.ios.background -> splash.background -> "#FFFFFF"
                    let background = splash.get("ios").and_then(|i| i.get("background")).and_then(|v| v.as_str())
                        .or_else(|| splash.get("background").and_then(|v| v.as_str()))
                        .unwrap_or("#FFFFFF")
                        .to_string();

                    if image_path.is_some() || background != "#FFFFFF" {
                        return Some((image_path, background, None));
                    }
                    return None;
                }
            }
            None
        })();

        // Write a compiled LaunchScreen storyboard — with splash image if configured,
        // otherwise a minimal blank storyboard so iPadOS treats the app as native iPad.
        let launch_sb_xml = if let Some((ref image_path, ref bg_hex, ref custom_sb)) = splash_config {
            if let Some(custom) = custom_sb {
                // Custom storyboard: copy as-is
                fs::read_to_string(custom).unwrap_or_default()
            } else {
                // Copy splash image into bundle
                if let Some(img) = image_path {
                    let _ = fs::copy(img, app_dir.join("splash_image.png"));
                }

                // Parse hex color to RGB floats
                let hex = bg_hex.trim_start_matches('#');
                let (r, g, b) = if hex.len() == 6 {
                    let rv = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255) as f64 / 255.0;
                    let gv = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255) as f64 / 255.0;
                    let bv = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255) as f64 / 255.0;
                    (rv, gv, bv)
                } else {
                    (1.0, 1.0, 1.0)
                };

                let image_views = if image_path.is_some() {
                    format!(r#"
                        <subviews>
                            <imageView clipsSubviews="YES" userInteractionEnabled="NO" contentMode="scaleAspectFit" image="splash_image" translatesAutoresizingMaskIntoConstraints="NO" id="img-splash-1">
                                <rect key="frame" x="132.5" y="362" width="128" height="128"/>
                                <constraints>
                                    <constraint firstAttribute="width" constant="128" id="img-w-1"/>
                                    <constraint firstAttribute="height" constant="128" id="img-h-1"/>
                                </constraints>
                            </imageView>
                        </subviews>
                        <constraints>
                            <constraint firstItem="img-splash-1" firstAttribute="centerX" secondItem="Ze5-6b-2t3" secondAttribute="centerX" id="cx-1"/>
                            <constraint firstItem="img-splash-1" firstAttribute="centerY" secondItem="Ze5-6b-2t3" secondAttribute="centerY" id="cy-1"/>
                        </constraints>"#)
                } else {
                    String::new()
                };

                let resources = if image_path.is_some() {
                    r#"
    <resources>
        <image name="splash_image" width="128" height="128"/>
    </resources>"#.to_string()
                } else {
                    String::new()
                };

                format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" toolsVersion="21701" targetRuntime="iOS.CocoaTouch" propertyAccessControl="none" useAutolayout="YES" launchScreen="YES" useTraitCollections="YES" useSafeAreas="YES" colorMatched="YES" initialViewController="01J-lp-oVM">
    <scenes>
        <scene sceneID="EHf-IW-A2E">
            <objects>
                <viewController id="01J-lp-oVM" sceneMemberID="viewController">
                    <view key="view" contentMode="scaleToFill" id="Ze5-6b-2t3">
                        <rect key="frame" x="0.0" y="0.0" width="393" height="852"/>
                        <autoresizingMask key="autoresizingMask" widthSizable="YES" heightSizable="YES"/>
                        <color key="backgroundColor" red="{r}" green="{g}" blue="{b}" alpha="1" colorSpace="custom" customColorSpace="sRGB"/>{image_views}
                    </view>
                </viewController>
                <placeholder placeholderIdentifier="IBFirstResponder" id="iYj-Kq-Ea1" userLabel="First Responder" sceneMemberID="firstResponder"/>
            </objects>
            <point key="canvasLocation" x="0" y="0"/>
        </scene>
    </scenes>{resources}
</document>"#)
            }
        } else {
            // No splash config — minimal blank storyboard for iPadOS compatibility
            r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" toolsVersion="21701" targetRuntime="iOS.CocoaTouch" propertyAccessControl="none" useAutolayout="YES" launchScreen="YES" useTraitCollections="YES" useSafeAreas="YES" colorMatched="YES" initialViewController="01J-lp-oVM">
    <scenes>
        <scene sceneID="EHf-IW-A2E">
            <objects>
                <viewController id="01J-lp-oVM" sceneMemberID="viewController">
                    <view key="view" contentMode="scaleToFill" id="Ze5-6b-2t3">
                        <rect key="frame" x="0.0" y="0.0" width="393" height="852"/>
                        <autoresizingMask key="autoresizingMask" widthSizable="YES" heightSizable="YES"/>
                        <color key="backgroundColor" systemColor="systemBackgroundColor"/>
                    </view>
                </viewController>
                <placeholder placeholderIdentifier="IBFirstResponder" id="iYj-Kq-Ea1" userLabel="First Responder" sceneMemberID="firstResponder"/>
            </objects>
            <point key="canvasLocation" x="0" y="0"/>
        </scene>
    </scenes>
</document>"#.to_string()
        };

        let sb_source = app_dir.join("_LaunchScreen.storyboard");
        fs::write(&sb_source, launch_sb_xml)?;
        let storyboardc = app_dir.join("Base.lproj").join("LaunchScreen.storyboardc");
        let _ = fs::create_dir_all(app_dir.join("Base.lproj"));
        let _ = fs::remove_dir_all(&storyboardc);
        let ibt_result = std::process::Command::new("ibtool")
            .arg("--compile")
            .arg(storyboardc.as_os_str())
            .arg(sb_source.as_os_str())
            .output();
        let _ = fs::remove_file(&sb_source);
        if ibt_result.is_err() || !ibt_result.as_ref().unwrap().status.success() {
            eprintln!("Warning: ibtool failed to compile LaunchScreen.storyboard");
        }

        // Bundle resource files: scan source for ImageFile('...') calls and copy referenced files
        // Also copy any directories named 'logo', 'assets', 'resources', 'images' from the project root
        let source_dir = args.input.canonicalize().ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        if let Some(src_dir) = &source_dir {
            // Walk up to find project root (where package.json is)
            let mut project_root = src_dir.clone();
            for _ in 0..5 {
                if project_root.join("package.json").exists() { break; }
                if let Some(parent) = project_root.parent() {
                    project_root = parent.to_path_buf();
                } else { break; }
            }
            // Copy common resource directories into the bundle
            for dir_name in &["logo", "assets", "resources", "images"] {
                let resource_dir = project_root.join(dir_name);
                if resource_dir.is_dir() {
                    let dest = app_dir.join(dir_name);
                    eprintln!("[perry] iOS asset copy: src={} -> dst={}", resource_dir.display(), dest.display());
                    fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                        fs::create_dir_all(dst)?;
                        for entry in fs::read_dir(src)? {
                            let entry = entry?;
                            let ty = entry.file_type()?;
                            let dest_path = dst.join(entry.file_name());
                            if ty.is_dir() {
                                copy_dir_recursive(&entry.path(), &dest_path)?;
                            } else {
                                fs::copy(entry.path(), &dest_path)?;
                            }
                        }
                        Ok(())
                    }
                    let _ = copy_dir_recursive(&resource_dir, &dest);
                }
            }
        }

        // --- i18n: generate .lproj bundles for iOS/macOS ---
        if let (Some(ref table), Some(ref config)) = (&i18n_table, &i18n_config) {
            if !table.keys.is_empty() {
                for (locale_idx, locale) in config.locales.iter().enumerate() {
                    let lproj_dir = app_dir.join(format!("{}.lproj", locale));
                    let _ = fs::create_dir_all(&lproj_dir);
                    let mut strings_content = String::new();
                    for (key_idx, key) in table.keys.iter().enumerate() {
                        let flat_idx = locale_idx * table.keys.len() + key_idx;
                        let value = table.translations.get(flat_idx).cloned().unwrap_or_else(|| key.clone());
                        // Escape for .strings format
                        let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
                        let escaped_val = value.replace('\\', "\\\\").replace('"', "\\\"");
                        strings_content.push_str(&format!("\"{}\" = \"{}\";\n", escaped_key, escaped_val));
                    }
                    let _ = fs::write(lproj_dir.join("Localizable.strings"), &strings_content);
                }
                match format {
                    OutputFormat::Text => println!("  Generated {}.lproj bundles for {} locale(s)",
                        config.locales.join(", "), config.locales.len()),
                    OutputFormat::Json => {}
                }
            }
        }

        compile_metallib_for_bundle(&ctx, target.as_deref(), &app_dir, format)?;

        match format {
            OutputFormat::Text => {
                println!("Wrote iOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on iOS Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else if is_visionos {
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        let bundle_id = lookup_bundle_id_from_toml(&args.input, "visionos")
            .or_else(|| lookup_bundle_id_from_toml(&args.input, "app"))
            .or_else(|| lookup_bundle_id_from_toml(&args.input, "ios"))
            .unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        let (app_version, app_build_number, deployment_target, encryption_exempt, custom_plist_entries) = (|| -> Option<(String, String, String, Option<bool>, String)> {
            let mut dir = args.input.canonicalize().ok()?;
            for _ in 0..5 {
                dir = dir.parent()?.to_path_buf();
                let toml_path = dir.join("perry.toml");
                if !toml_path.exists() {
                    continue;
                }
                let data = fs::read_to_string(&toml_path).ok()?;
                let doc: toml::Table = data.parse().ok()?;
                let project = doc.get("project").and_then(|v| v.as_table());
                let visionos = doc.get("visionos").and_then(|v| v.as_table());
                let version = project
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("1.0.0")
                    .to_string();
                let build_number = project
                    .and_then(|p| p.get("build_number"))
                    .and_then(|v| v.as_integer().map(|n| n.to_string()).or_else(|| v.as_str().map(|s| s.to_string())))
                    .unwrap_or_else(|| "1".to_string());
                let deployment_target = visionos
                    .and_then(|v| v.get("deployment_target").or_else(|| v.get("minimum_version")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("1.0")
                    .to_string();
                let encryption_exempt = visionos
                    .and_then(|v| v.get("encryption_exempt"))
                    .and_then(|v| v.as_bool());
                let mut entries = String::new();
                if let Some(info_plist) = visionos
                    .and_then(|v| v.get("info_plist"))
                    .and_then(|v| v.as_table())
                {
                    for (key, value) in info_plist {
                        if let Some(s) = value.as_str() {
                            entries.push_str(&format!("    <key>{}</key>\n    <string>{}</string>\n", key, s));
                        } else if let Some(b) = value.as_bool() {
                            entries.push_str(&format!("    <key>{}</key>\n    <{}/>\n", key, if b { "true" } else { "false" }));
                        } else if let Some(i) = value.as_integer() {
                            entries.push_str(&format!("    <key>{}</key>\n    <integer>{}</integer>\n", key, i));
                        }
                    }
                }
                return Some((version, build_number, deployment_target, encryption_exempt, entries));
            }
            Some(("1.0.0".to_string(), "1".to_string(), "1.0".to_string(), None, String::new()))
        })().unwrap();

        let platform_name = if target.as_deref() == Some("visionos-simulator") {
            "XRSimulator"
        } else {
            "XROS"
        };

        let mut info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>{app_build_number}</string>
    <key>CFBundleShortVersionString</key>
    <string>{app_version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>MinimumOSVersion</key>
    <string>{deployment_target}</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>{platform_name}</string>
    </array>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
    </array>
    <key>UIDeviceFamily</key>
    <array>
        <integer>7</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UIApplicationSupportsMultipleScenes</key>
        <true/>
        <key>UIApplicationPreferredDefaultSceneSessionRole</key>
        <string>UIWindowSceneSessionRoleApplication</string>
        <key>UISceneConfigurations</key>
        <dict/>
    </dict>
</dict>
</plist>"#
        );

        let usage_descriptions = concat!(
            "    <key>NSCameraUsageDescription</key>\n",
            "    <string>This app uses the camera to identify colors.</string>\n",
            "    <key>NSMicrophoneUsageDescription</key>\n",
            "    <string>This app uses the microphone to measure sound levels.</string>\n",
        );
        info_plist = info_plist.replace("</dict>\n</plist>", &format!("{}</dict>\n</plist>", usage_descriptions));

        if let Some(exempt) = encryption_exempt {
            let encryption_entry = format!(
                "    <key>ITSAppUsesNonExemptEncryption</key>\n    <{}/>\n",
                if exempt { "false" } else { "true" }
            );
            info_plist = info_plist.replace("</dict>\n</plist>", &format!("{}</dict>\n</plist>", encryption_entry));
        }

        if !custom_plist_entries.is_empty() {
            info_plist = info_plist.replace("</dict>\n</plist>", &format!("{}</dict>\n</plist>", custom_plist_entries));
        }

        fs::write(app_dir.join("Info.plist"), info_plist)?;

        let source_dir = args.input.canonicalize().ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        if let Some(src_dir) = &source_dir {
            let mut project_root = src_dir.clone();
            for _ in 0..5 {
                if project_root.join("package.json").exists() || project_root.join("perry.toml").exists() { break; }
                if let Some(parent) = project_root.parent() {
                    project_root = parent.to_path_buf();
                } else { break; }
            }
            fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                fs::create_dir_all(dst)?;
                for entry in fs::read_dir(src)? {
                    let entry = entry?;
                    let ty = entry.file_type()?;
                    let dest_path = dst.join(entry.file_name());
                    if ty.is_dir() {
                        copy_dir_recursive(&entry.path(), &dest_path)?;
                    } else {
                        fs::copy(entry.path(), &dest_path)?;
                    }
                }
                Ok(())
            }
            for dir_name in &["logo", "assets", "resources", "images"] {
                let resource_dir = project_root.join(dir_name);
                if resource_dir.is_dir() {
                    let dest = app_dir.join(dir_name);
                    let _ = copy_dir_recursive(&resource_dir, &dest);
                }
            }
        }

        if let (Some(ref table), Some(ref config)) = (&i18n_table, &i18n_config) {
            if !table.keys.is_empty() {
                for (locale_idx, locale) in config.locales.iter().enumerate() {
                    let lproj_dir = app_dir.join(format!("{}.lproj", locale));
                    let _ = fs::create_dir_all(&lproj_dir);
                    let mut strings_content = String::new();
                    for (key_idx, key) in table.keys.iter().enumerate() {
                        let flat_idx = locale_idx * table.keys.len() + key_idx;
                        let value = table.translations.get(flat_idx).cloned().unwrap_or_else(|| key.clone());
                        let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
                        let escaped_val = value.replace('\\', "\\\\").replace('"', "\\\"");
                        strings_content.push_str(&format!("\"{}\" = \"{}\";\n", escaped_key, escaped_val));
                    }
                    let _ = fs::write(lproj_dir.join("Localizable.strings"), &strings_content);
                }
            }
        }

        match format {
            OutputFormat::Text => {
                println!("Wrote visionOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on Apple Vision Pro Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else if is_watchos {
        // Create watchOS .app bundle
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        let bundle_id = lookup_bundle_id_from_toml(&args.input, "watchos")
            .or_else(|| lookup_bundle_id_from_toml(&args.input, "app"))
            .unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>10.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>4</integer>
    </array>
    <key>WKApplication</key>
    <true/>
    <key>WKWatchOnly</key>
    <true/>
</dict>
</plist>"#
        );
        fs::write(app_dir.join("Info.plist"), info_plist)?;

        // Copy project resource directories into the bundle so
        // bloom_load_texture / load_sound / read_file can resolve relative
        // asset paths via [[NSBundle mainBundle] resourcePath].
        let source_dir = args.input.canonicalize().ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        if let Some(src_dir) = &source_dir {
            let mut project_root = src_dir.clone();
            for _ in 0..5 {
                if project_root.join("package.json").exists() || project_root.join("perry.toml").exists() { break; }
                if let Some(parent) = project_root.parent() {
                    project_root = parent.to_path_buf();
                } else { break; }
            }
            fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                fs::create_dir_all(dst)?;
                for entry in fs::read_dir(src)? {
                    let entry = entry?;
                    let ty = entry.file_type()?;
                    let dest_path = dst.join(entry.file_name());
                    if ty.is_dir() {
                        copy_dir_recursive(&entry.path(), &dest_path)?;
                    } else {
                        fs::copy(entry.path(), &dest_path)?;
                    }
                }
                Ok(())
            }
            for dir_name in &["logo", "assets", "resources", "images"] {
                let resource_dir = project_root.join(dir_name);
                if resource_dir.is_dir() {
                    let dest = app_dir.join(dir_name);
                    let _ = copy_dir_recursive(&resource_dir, &dest);
                }
            }
        }

        compile_metallib_for_bundle(&ctx, target.as_deref(), &app_dir, format)?;

        match format {
            OutputFormat::Text => {
                println!("Wrote watchOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on Apple Watch Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else if is_tvos {
        // Create tvOS .app bundle
        let app_dir = exe_path.with_extension("app");
        let _ = fs::create_dir_all(&app_dir);
        let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
        fs::copy(&exe_path, &bundle_exe)?;
        let _ = fs::remove_file(&exe_path);

        let exe_stem = exe_path.file_stem().and_then(|s| s.to_str()).unwrap_or(stem);
        let bundle_id = lookup_bundle_id_from_toml(&args.input, "tvos")
            .or_else(|| lookup_bundle_id_from_toml(&args.input, "app"))
            .unwrap_or_else(|| format!("com.perry.{}", exe_stem));
        result_bundle_id = Some(bundle_id.clone());
        result_app_dir = Some(app_dir.clone());

        let info_plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>3</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIRequiresFullScreen</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>BloomApplication</string>
</dict>
</plist>"#
        );
        fs::write(app_dir.join("Info.plist"), info_plist)?;

        compile_metallib_for_bundle(&ctx, target.as_deref(), &app_dir, format)?;

        match format {
            OutputFormat::Text => {
                println!("Wrote tvOS app bundle: {}", app_dir.display());
                println!();
                println!("To run on Apple TV Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": app_dir.to_string_lossy(),
                    "bundle_id": bundle_id,
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    } else {
        // For Windows/Linux (non-bundle targets), copy asset directories next to the exe
        // so that resolve_asset_path can find them relative to the executable.
        if let Some(output_dir) = exe_path.parent() {
            let source_dir = args.input.canonicalize().ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            if let Some(src_dir) = source_dir {
                let mut project_root = src_dir.clone();
                for _ in 0..5 {
                    if project_root.join("package.json").exists() { break; }
                    if let Some(parent) = project_root.parent() {
                        project_root = parent.to_path_buf();
                    } else { break; }
                }
                fn copy_dir_recursive_standalone(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                    fs::create_dir_all(dst)?;
                    for entry in fs::read_dir(src)? {
                        let entry = entry?;
                        let ty = entry.file_type()?;
                        let dest_path = dst.join(entry.file_name());
                        if ty.is_dir() {
                            copy_dir_recursive_standalone(&entry.path(), &dest_path)?;
                        } else {
                            fs::copy(entry.path(), &dest_path)?;
                        }
                    }
                    Ok(())
                }
                // Resolve output_dir: exe_path.parent() returns "" for bare filenames like "Mango"
                let output_resolved = if output_dir.as_os_str().is_empty() {
                    std::path::PathBuf::from(".")
                } else {
                    output_dir.to_path_buf()
                };
                let output_canon = output_resolved.canonicalize().unwrap_or_else(|_| output_resolved.clone());
                let project_canon = project_root.canonicalize().unwrap_or_else(|_| project_root.to_path_buf());
                // Skip asset copying if output dir IS the project root
                // (fs::copy to self truncates files to 0 bytes)
                if output_canon != project_canon {
                    for dir_name in &["logo", "assets", "resources", "images"] {
                        let resource_dir = project_root.join(dir_name);
                        if resource_dir.is_dir() {
                            let dest = output_dir.join(dir_name);
                            let _ = copy_dir_recursive_standalone(&resource_dir, &dest);
                        }
                    }
                }
            }
        }

        match format {
            OutputFormat::Text => println!("Wrote executable: {}", exe_path.display()),
            OutputFormat::Json => {
                let result = serde_json::json!({
                    "success": true,
                    "output": exe_path.to_string_lossy(),
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
    }

    // --- i18n: generate Android values-xx/ resources ---
    if is_android {
        if let (Some(ref table), Some(ref config)) = (&i18n_table, &i18n_config) {
            if !table.keys.is_empty() {
                let output_dir = exe_path.parent().unwrap_or(Path::new("."));
                let res_dir = output_dir.join("res");
                for (locale_idx, locale) in config.locales.iter().enumerate() {
                    let values_dir = if locale_idx == 0 {
                        res_dir.join("values") // default locale
                    } else {
                        res_dir.join(format!("values-{}", locale))
                    };
                    let _ = fs::create_dir_all(&values_dir);
                    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n");
                    for (key_idx, key) in table.keys.iter().enumerate() {
                        let flat_idx = locale_idx * table.keys.len() + key_idx;
                        let value = table.translations.get(flat_idx).cloned().unwrap_or_else(|| key.clone());
                        // Sanitize key for Android resource name (alphanumeric + underscore)
                        let res_name: String = key.chars().map(|c| {
                            if c.is_alphanumeric() || c == '_' { c } else { '_' }
                        }).collect();
                        // Escape XML special chars
                        let escaped = value.replace('&', "&amp;").replace('<', "&lt;")
                            .replace('>', "&gt;").replace('"', "&quot;").replace('\'', "\\'");
                        xml.push_str(&format!("    <string name=\"{}\">{}</string>\n", res_name, escaped));
                    }
                    xml.push_str("</resources>\n");
                    let _ = fs::write(values_dir.join("strings.xml"), &xml);
                }
                match format {
                    OutputFormat::Text => println!("  Generated res/values-*/strings.xml for {} locale(s)", config.locales.len()),
                    OutputFormat::Json => {}
                }
            }
        }
    }

    // Strip debug symbols from the final binary (reduces size significantly)
    // Skip for iOS/Android/HarmonyOS cross-compilation — host strip can't handle
    // foreign architectures (macOS BSD strip fails on ELF with the noisy
    // "non-object and non-archive file" warning).
    // Skip for watchOS — bundling above already moved exe_path into the .app
    // Skip when PERRY_DEBUG_SYMBOLS=1 is set — keep symbols for crash debugging
    if !is_dylib && !is_ios && !is_visionos && !is_tvos && !is_watchos && !is_harmonyos
        && target.as_deref() != Some("android")
        && std::env::var("PERRY_DEBUG_SYMBOLS").is_err() {
        if ctx.needs_plugins {
            // When plugins are enabled, use strip -x to keep exported symbols
            // (dlopen'd plugins need to resolve hone_host_api_* from the main executable)
            let _ = std::process::Command::new("strip").arg("-x").arg(&exe_path).status();
        } else {
            let _ = std::process::Command::new("strip").arg(&exe_path).status();
        }
    }

    // Print binary size
    if let OutputFormat::Text = format {
        if let Ok(meta) = fs::metadata(&exe_path) {
            let size_mb = meta.len() as f64 / 1_048_576.0;
            println!("Binary size: {:.1}MB", size_mb);
        }
    }

    if !args.keep_intermediates {
        for obj_path in &obj_paths {
            let _ = fs::remove_file(obj_path);
        }
    }

    let final_output_path = result_app_dir.unwrap_or(exe_path);

    let codegen_cache_stats = if object_cache.is_enabled() {
        Some((object_cache.hits(), object_cache.misses(), object_cache.stores(), object_cache.store_errors()))
    } else { None };

    Ok(CompileResult {
        output_path: final_output_path,
        target: target.unwrap_or_else(|| "native".to_string()),
        bundle_id: result_bundle_id,
        is_dylib,
        codegen_cache_stats,
    })
}





#[cfg(test)]
mod windows_link_tests {
    use super::windows_pe_subsystem_flag;

    // Regression guard for issue #120: without an explicit subsystem flag the
    // MSVC linker historically defaulted to WINDOWS (2), silently detaching
    // stdout/stderr so console.log output never reached the terminal.

    #[test]
    fn cli_build_uses_console_subsystem() {
        assert_eq!(windows_pe_subsystem_flag(false), "/SUBSYSTEM:CONSOLE");
    }

    #[test]
    fn ui_build_uses_windows_subsystem() {
        assert_eq!(windows_pe_subsystem_flag(true), "/SUBSYSTEM:WINDOWS");
    }
}
