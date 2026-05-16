//! Module discovery + transitive import walk.
//!
//! Tier 2.1 follow-up (v0.5.341) — extracts `collect_modules` (~380
//! LOC) from `compile.rs`. Walks the import graph from the entry
//! file, lowers every TypeScript module to HIR, classifies each as
//! native-compiled vs JS-runtime-loaded, and accumulates the result
//! in `CompilationContext.native_modules` / `js_modules`. Runs
//! per-module HIR passes (inline_functions, transform_generators)
//! before adding the module to the context. Source hashes feed the
//! V2.2 codegen cache key derivation.

use anyhow::{anyhow, Result};
use perry_hir::ModuleKind;
use perry_transform::{
    gather_cross_module_anon_classes, gather_cross_module_methods,
    gather_cross_module_methods_with_extern_imports, inline_finally_into_returns, inline_functions,
    transform_async_to_generator, transform_generators, MethodCandidate,
};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::OutputFormat;

use super::{
    cached_resolve_import, extract_compile_package_dir, has_perry_native_library,
    is_declaration_file, is_in_compile_package, is_in_perry_native_package, is_js_file,
    parse_cached, parse_native_library_manifest, parse_package_specifier, CompilationContext,
    JsModule, ParseCache,
};

/// Issue #841: Node.js submodules that Perry knows about at the
/// resolver level (no perry-stdlib backing, no compiled-source backing)
/// but for which we still want to provide a minimal import surface so
/// `typeof import-name === "function"` and `import * as ns` work.
///
/// Each entry returns the bare submodule key that matches
/// `perry_runtime::node_submodules::SUBMODULES[i].key`. Codegen routes
/// every named/namespace import from these specifiers through the
/// runtime singleton getters in that module.
pub(super) fn known_node_submodule_key(source: &str) -> Option<&'static str> {
    let normalized = source.strip_prefix("node:").unwrap_or(source);
    match normalized {
        "timers/promises" => Some("timers_promises"),
        "readline/promises" => Some("readline_promises"),
        "stream/promises" => Some("stream_promises"),
        "stream/consumers" => Some("stream_consumers"),
        "sys" => Some("sys"),
        _ => None,
    }
}

/// Collect all modules to compile (transitive closure of imports)
pub(super) fn collect_modules(
    entry_path: &PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    enable_js_runtime: bool,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    mut parse_cache: Option<&mut ParseCache>,
) -> Result<()> {
    let canonical = entry_path
        .canonicalize()
        .map_err(|e| anyhow!("Failed to canonicalize {}: {}", entry_path.display(), e))?;

    if visited.contains(&canonical) {
        return Ok(());
    }
    visited.insert(canonical.clone());

    // Check if this file should be handled by JS runtime instead of native compilation
    // This includes: JS files, declaration files (.d.ts), JSON files, or any file in node_modules when JS runtime is enabled
    let is_json = canonical.extension().and_then(|e| e.to_str()) == Some("json");
    let is_in_node_modules = canonical.to_string_lossy().contains("node_modules");
    let is_perry_native = is_in_node_modules && is_in_perry_native_package(&canonical);
    let is_in_compiled_pkg = (is_in_node_modules && is_in_compile_package(&canonical, &ctx.compile_packages))
        || ctx.compile_package_dirs.values().any(|dir| {
            if canonical.starts_with(dir) {
                // Exclude nested node_modules/ inside the compiled package
                // (e.g., @solana/web3.js/node_modules/bs58/ is NOT part of @solana/web3.js)
                let relative = canonical.strip_prefix(dir).unwrap_or(canonical.as_ref());
                !relative.to_string_lossy().contains("node_modules/")
            } else {
                false
            }
        })
        // A file whose canonical path resolves to inside a perry.nativeLibrary package
        // but is NOT under any node_modules/ component (i.e., reached via a file: dep
        // that places the package inside the project root, as in #209 "file:./vendor/bloom/")
        // must still be compiled natively, not handed to the JS runtime.
        // Guard with !is_in_node_modules so this branch never fires for the standard
        // node_modules/ioredis, node_modules/ethers etc. paths that already have their
        // own handling (is_perry_native above).
        || (!is_in_node_modules && is_in_perry_native_package(&canonical));
    let should_use_js_runtime = (is_js_file(&canonical) && !is_in_compiled_pkg)
        || is_declaration_file(&canonical)
        || is_json
        || (enable_js_runtime && is_in_node_modules && !is_perry_native && !is_in_compiled_pkg);

    // Skip JSON files — they're data, not code (imported via `with { type: "json" }`)
    if is_json {
        return Ok(());
    }

    if should_use_js_runtime {
        // Skip declaration files - they're just type information
        if is_declaration_file(&canonical) {
            return Ok(());
        }

        // Perry native extension packages (ioredis, ethers, mysql2, ws, dotenv) are handled
        // entirely by Perry's built-in stdlib — they must NOT be loaded into V8.
        if is_perry_native {
            return Ok(());
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;

        let specifier = canonical.to_string_lossy().to_string();
        ctx.js_modules.insert(
            specifier.clone(),
            JsModule {
                path: canonical.clone(),
                source,
                specifier,
            },
        );
        ctx.needs_js_runtime = true;

        // We don't parse JS/node_modules files for their imports (V8 will handle that at runtime)
        return Ok(());
    }

    // It's a TypeScript file to compile natively
    let raw_source = fs::read_to_string(&canonical)
        .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;

    // Issue #348: when a `compilePackages` target ships CommonJS (e.g. React
    // 18's `module.exports = require('./cjs/react.production.min.js')`),
    // rewrite the source as ESM before SWC parses it. Only fires for files
    // inside a `compilePackages` target — user TypeScript and ESM-shaped
    // packages skip the wrap. See `cjs_wrap.rs` for the wrap shape.
    let source = if is_in_compiled_pkg && super::cjs_wrap::is_commonjs(&raw_source) {
        super::cjs_wrap::wrap_commonjs(&raw_source, &canonical)
    } else {
        raw_source
    };

    // Note (#686): we no longer hash source bytes here. The object cache key
    // is now keyed on a post-transform HIR fingerprint computed inside the
    // rayon codegen job (see compile.rs's main per-module closure), so
    // formatter-only edits hit the cache. Removing the per-source hash also
    // removes one bytes scan per module from the collect path.

    let filename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.ts");

    // Use a relative path from project root for unique module names
    // This ensures files like "routes/auth.ts" and "middleware/auth.ts" have different names
    let module_name = canonical
        .strip_prefix(&ctx.project_root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| filename.to_string());

    // Parse via the optional in-memory cache (only populated by `perry dev`).
    // On a cache hit, we reuse the AST from the previous rebuild — the single
    // largest time sink in the hot rebuild path on unchanged files.
    let ast_module_owned: swc_ecma_ast::Module;
    let ast_module: &swc_ecma_ast::Module = match parse_cache.as_deref_mut() {
        Some(cache) => parse_cached(cache, &canonical, &source, filename)?,
        None => {
            ast_module_owned = perry_parser::parse_typescript(&source, filename)
                .map_err(|e| anyhow!("Failed to parse {}: {}", canonical.display(), e))?;
            &ast_module_owned
        }
    };
    let source_file_path = canonical.to_string_lossy().to_string();

    // If type checking is enabled, resolve types from tsgo before lowering
    let resolved_types = if ctx.type_checker.is_some() {
        let positions = crate::commands::typecheck::collect_untyped_positions(ast_module);
        if !positions.is_empty() {
            let client = ctx.type_checker.as_mut().unwrap();
            match crate::commands::typecheck::resolve_types_for_file(
                client,
                &source_file_path,
                &positions,
            ) {
                Ok(types) => {
                    if !types.is_empty() {
                        Some(types)
                    } else {
                        None
                    }
                }
                Err(_) => None, // Silently continue without resolved types on error
            }
        } else {
            None
        }
    } else {
        None
    };

    // Pass cross-module class field types so type inference can resolve
    // `someLocal.field` where the local's declared type is a class defined
    // in another module (and that module was already lowered earlier in
    // the walk OR via the post-pass re-lowering kick-off below). Empty on
    // the first pre-walk; populated for the second authoritative walk.
    let imported_class_fields = if ctx.cross_module_class_field_types.is_empty() {
        None
    } else {
        Some(&ctx.cross_module_class_field_types)
    };
    // Issue #444: this module is the user-supplied entry iff its canonical
    // path matches the one stashed by `compile.rs::run_with_parse_cache`
    // before the first `collect_modules` invocation. Bundle-extension
    // entries don't update `entry_canonical`, so their `import.meta.main`
    // correctly resolves to false.
    let is_entry_module = ctx.entry_canonical.as_ref() == Some(&canonical);
    // Issue #668: external means "module reached via npm package import".
    // We can't rely on the canonical path containing "/node_modules/" because
    // bun's pnpm-style installs symlink each package file out to a single
    // shared copy (e.g. `@perryts/redis` source lives at /Users/.../perry/redis
    // even when imported as `@perryts/redis`). Project-root containment is
    // robust against this — user code lives under `project_root`, library
    // imports canonicalize away from it. Files imported via `/node_modules/`
    // also keep the legacy fall-through behavior for the same reason.
    let is_external_module = !canonical.starts_with(&ctx.project_root)
        || canonical.to_string_lossy().contains("/node_modules/")
        || entry_path.to_string_lossy().contains("/node_modules/");
    // Refs #665: install per-thread override so HIR's `is_native_module`
    // returns false for packages the user opted into via
    // `perry.compilePackages`. Without this, the HIR lowering at
    // `expr_member::lower_member` treats `obj.prop` on a registered
    // native instance as a zero-arg FFI getter call (`NativeMethodCall {
    // method, args: [] }`), which for compile-package-overridden classes
    // routes through `js_native_call_method` and returns `0.0` — the
    // bug Ralph hit as `typeof limiter.consume === "number"`. With the
    // override in place, `is_native_module("rate-limiter-flexible")`
    // returns false, the import is not registered as a native module,
    // `limiter` is not tagged as a native instance, and `limiter.consume`
    // lowers as a real `PropertyGet` → codegen's class-method-bind path
    // synthesizes a `BOUND_METHOD_FUNC_PTR` closure. The thread-local
    // is rayon-safe (each worker thread has its own copy) and cleared
    // immediately after the lower call so it can't leak to subsequent
    // unrelated work on the same thread.
    perry_hir::set_compile_packages_override(ctx.compile_packages.clone());
    let lower_result = perry_hir::lower_module_full(
        ast_module,
        &module_name,
        &source_file_path,
        *next_class_id,
        resolved_types,
        imported_class_fields,
        is_entry_module,
        is_external_module,
    );
    perry_hir::clear_compile_packages_override();
    let (mut hir_module, new_next_class_id) = lower_result?;
    *next_class_id = new_next_class_id; // Update the global class_id counter

    // Issue #100: const-fold dynamic `import()` paths, register each
    // resolved target as a regular import edge (marked `is_dynamic`), and
    // detect top-level `await` so codegen can chain the init Promise on
    // the dispatch side. Unresolvable / over-cap arguments surface as a
    // structured compile error here — never propagating to codegen.
    perry_hir::detect_top_level_await(&mut hir_module);
    let mut dyn_errors: Vec<String> = Vec::new();
    let mut new_dyn_imports: Vec<String> = Vec::new();
    // Issue #100: collect the module's top-level `const` locals once so
    // the resolver can follow `import(localStringVar)` and
    // `` import(`./prefix_${localStringVar}.ts`) `` paths transitively.
    let module_const_locals = perry_hir::collect_module_const_locals(&hir_module);
    perry_hir::for_each_dynamic_import_mut(&mut hir_module, &mut |expr| {
        if let perry_hir::Expr::DynamicImport { paths, arg } = expr {
            if !paths.is_empty() {
                // Already resolved (e.g. a second pass on the same module).
                return;
            }
            let mut visiting: std::collections::HashSet<u32> = std::collections::HashSet::new();
            match perry_hir::resolve_import_path_with_consts(
                arg,
                &module_const_locals,
                &mut visiting,
            ) {
                perry_hir::Resolution::Set(set) => {
                    if set.len() > perry_hir::DYNAMIC_IMPORT_PATH_CAP {
                        dyn_errors.push(format!(
                            "dynamic import() in module {}: resolves to {} possible paths \
                             (limit: {})\n  note: consider enumerating with a ternary or \
                             registry object",
                            module_name,
                            set.len(),
                            perry_hir::DYNAMIC_IMPORT_PATH_CAP
                        ));
                        return;
                    }
                    for p in &set {
                        if !new_dyn_imports.contains(p) {
                            new_dyn_imports.push(p.clone());
                        }
                    }
                    *paths = set;
                }
                perry_hir::Resolution::Unresolved(reason) => {
                    dyn_errors.push(format!(
                        "dynamic import() in module {}: {}",
                        module_name, reason
                    ));
                }
            }
        }
    });
    if !dyn_errors.is_empty() {
        return Err(anyhow!("{}", dyn_errors.join("\n")));
    }
    for source in new_dyn_imports {
        // A dynamic edge to the same source as a static import is folded
        // into the existing edge (just flip is_dynamic off — the static
        // edge already gives us full namespace materialization). A
        // dynamic-only target appears as a new import with empty
        // specifiers and `is_dynamic = true`.
        if hir_module
            .imports
            .iter()
            .any(|i| i.source == source && !i.is_dynamic)
        {
            continue;
        }
        let is_native = perry_hir::is_native_module(&source);
        let module_kind = if is_native {
            ModuleKind::NativeRust
        } else {
            ModuleKind::NativeCompiled
        };
        hir_module.imports.push(perry_hir::Import {
            source,
            specifiers: Vec::new(),
            is_native,
            module_kind,
            resolved_path: None,
            type_only: false,
            is_dynamic: true,
        });
    }

    // Process imports and update their resolved paths and module kinds
    for import in &mut hir_module.imports {
        // Skip type-only imports — they were recorded for class-metadata flow
        // (see lower.rs's #446 comment: a per-specifier `import { type Foo }`
        // is preserved so Foo's class info reaches `imported_classes` for
        // method dispatch) but they MUST NOT be loaded as runtime modules.
        // Without this skip, `import type { StandardSchemaV1 } from
        // "@standard-schema/spec"` (Effect's only `@standard-schema` use,
        // a type-only reference) queued the package's V8 fallback. The
        // spec ships an empty `src_exports = {}` at runtime, so any
        // `something._tag` from the import binding then threw
        // `TypeError: Cannot read properties of undefined (reading '_tag')`
        // during Effect's module init. Refs #321, #684.
        if import.type_only {
            continue;
        }

        // Apply package alias (e.g., @parse/node-apn → perry-push from perry.packageAliases)
        if let Some(alias) = ctx.package_aliases.get(import.source.as_str()).cloned() {
            import.source = alias;
            import.is_native = perry_hir::is_native_module(&import.source);
        }

        // Refs #665: an opt-in via `perry.compilePackages` overrides the
        // built-in native binding. HIR lowering set `is_native` based on the
        // NATIVE_MODULES manifest alone; downgrade it here so the import
        // falls through to file resolution (cjs_wrap + native codegen) and
        // the user's `node_modules` copy wins. Mirrors the parallel check in
        // `resolve::resolve_import`.
        if import.is_native {
            let (import_pkg_name, _) = super::resolve::parse_package_specifier(&import.source);
            if ctx.compile_packages.contains(&import_pkg_name) {
                import.is_native = false;
            }
        }

        if import.is_native {
            import.module_kind = ModuleKind::NativeRust;
            if import.source == "perry/ui" {
                ctx.needs_ui = true;
            }
            // perry/media (issue #351) lives in the platform UI crates
            // (libperry_ui_macos.a etc.) because AVPlayer / MediaPlayer /
            // GStreamer / Media Foundation are tightly coupled to the
            // same per-platform code that hosts the widget tree. So a
            // perry/media import triggers UI lib linking even when the
            // program uses no widgets.
            if import.source == "perry/media" {
                ctx.needs_ui = true;
            }
            // perry/system: most bindings (preferences, locale, device
            // info) live in stdlib, but the audio-recording, geolocation,
            // and image-picker FFIs live in libperry_ui_*.a alongside the
            // platform-specific OS framework integrations (CoreLocation,
            // AVAudioEngine, PHPickerViewController, etc.). Trigger UI
            // lib linking when any of those names is imported. (#552)
            if import.source == "perry/system" {
                let triggers_ui = import.specifiers.iter().any(|s| match s {
                    perry_hir::ImportSpecifier::Named { imported, .. } => matches!(
                        imported.as_str(),
                        "audioStart"
                            | "audioStop"
                            | "audioGetLevel"
                            | "audioGetPeak"
                            | "audioGetWaveform"
                            | "audioSetOutputFilename"
                            | "audioStartRecording"
                            | "audioStopRecording"
                            | "geolocationGetCurrent"
                            | "geolocationWatch"
                            | "geolocationStopWatch"
                            | "geolocationRequestPermission"
                            | "imagePickerPick"
                            | "networkGetStatus"
                            | "networkOnChange"
                            | "networkStopOnChange"
                            | "appOnOpenUrl"
                            | "appGetLaunchUrl"
                    ),
                    // Namespace imports — opt in conservatively (covers
                    // `import * as system from "perry/system"; system.audioStartRecording()`).
                    perry_hir::ImportSpecifier::Namespace { .. } => true,
                    perry_hir::ImportSpecifier::Default { .. } => false,
                });
                if triggers_ui {
                    ctx.needs_ui = true;
                }
            }
            // perry/background (issue #538) — BGTaskScheduler/WorkManager
            // bindings live in libperry_ui_*.a alongside the platform
            // OS-framework integration, so importing this module always
            // triggers UI lib linking.
            if import.source == "perry/background" {
                ctx.needs_ui = true;
            }
            if import.source == "perry/plugin" {
                ctx.needs_plugins = true;
            }
            if import.source == "perry/thread" {
                // perry/thread spawns OS workers and translates panics to
                // promise rejections via `catch_unwind` — auto-mode keeps
                // panic = "unwind" when this is set.
                ctx.needs_thread = true;
            }
            if perry_hir::requires_stdlib(&import.source) {
                ctx.needs_stdlib = true;
                // Track for `--minimal-stdlib` feature computation. Strip
                // any "node:" prefix so the mapping table sees the bare
                // module name.
                let normalized = import
                    .source
                    .strip_prefix("node:")
                    .unwrap_or(&import.source)
                    .to_string();
                ctx.native_module_imports.insert(normalized);
            }
            continue;
        }

        if let Some((resolved_path, kind)) = cached_resolve_import(&import.source, &canonical, ctx)
        {
            import.resolved_path = Some(resolved_path.to_string_lossy().to_string());
            import.module_kind = kind;

            match kind {
                ModuleKind::NativeCompiled => {
                    // Record compile package directory for dedup (first-found wins).
                    // When the same package exists in multiple nested node_modules/,
                    // we always resolve to the first-found copy to avoid duplicate symbols.
                    let module_name = &import.source;
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        let (pkg_name, _) = parse_package_specifier(module_name);
                        if ctx.compile_packages.contains(&pkg_name)
                            && !ctx.compile_package_dirs.contains_key(&pkg_name)
                        {
                            if let Some(pkg_dir) =
                                extract_compile_package_dir(&resolved_path, &pkg_name)
                            {
                                ctx.compile_package_dirs.insert(pkg_name, pkg_dir);
                            } else {
                                // Symlinked local package: canonical path is outside node_modules.
                                // Walk up from resolved_path to find the package root (dir with package.json).
                                let mut dir = resolved_path.parent();
                                while let Some(d) = dir {
                                    if d.join("package.json").exists() {
                                        ctx.compile_package_dirs.insert(pkg_name, d.to_path_buf());
                                        break;
                                    }
                                    dir = d.parent();
                                }
                            }
                        }
                    }
                    // Collect native library manifest (FFI functions, build config)
                    // Only for package imports (not relative imports within the same package)
                    if !module_name.starts_with('.')
                        && !module_name.starts_with('/')
                        && !ctx
                            .native_libraries
                            .iter()
                            .any(|nl| nl.module == *module_name)
                    {
                        // Walk up to find the package directory with perry.nativeLibrary
                        // Works for both node_modules packages and symlinked local packages
                        let mut pkg_dir = resolved_path.parent();
                        while let Some(dir) = pkg_dir {
                            if dir.join("package.json").exists() && has_perry_native_library(dir) {
                                if let Some(manifest) =
                                    parse_native_library_manifest(dir, module_name, target)
                                {
                                    // #466 Phase 2: refuse to load a wrapper whose
                                    // declared abiVersion is incompatible with the
                                    // bundled perry-ffi. Missing field warns but
                                    // continues during the v0.5.x cycle.
                                    if let Err(msg) =
                                        super::resolve::validate_abi_version(&manifest)
                                    {
                                        return Err(anyhow::anyhow!("{}", msg));
                                    }
                                    match format {
                                        OutputFormat::Text => println!(
                                            "  Native library: {} ({} FFI functions)",
                                            manifest.module,
                                            manifest.functions.len()
                                        ),
                                        OutputFormat::Json => {}
                                    }
                                    ctx.native_libraries.push(manifest);
                                }
                                break;
                            }
                            pkg_dir = dir.parent();
                        }
                    }
                    // Recursively collect TypeScript modules
                    collect_modules(
                        &resolved_path,
                        ctx,
                        visited,
                        enable_js_runtime,
                        format,
                        target,
                        next_class_id,
                        skip_transforms,
                        parse_cache.as_deref_mut(),
                    )?;
                }
                ModuleKind::Interpreted => {
                    // Perry native extension packages (ioredis, ethers, ws, mysql2, dotenv)
                    // are handled entirely by Perry's built-in stdlib at codegen time.
                    // They must NOT be loaded into V8 — skip them entirely.
                    if is_in_perry_native_package(&resolved_path) {
                        continue;
                    }

                    // Skip declaration files (.d.ts) - they only contain type information
                    if is_declaration_file(&resolved_path) {
                        continue;
                    }

                    // Auto-enable JS runtime for JavaScript imports

                    // Even for Interpreted imports, collect native library manifest if
                    // the resolved package has perry.nativeLibrary (handles symlinked packages
                    // where has_perry_native_library returns false for the symlink path but the
                    // canonical resolved path walks up to the correct package.json).
                    let module_name = &import.source;
                    if !module_name.starts_with('.')
                        && !module_name.starts_with('/')
                        && !ctx
                            .native_libraries
                            .iter()
                            .any(|nl| nl.module == *module_name)
                    {
                        let mut pkg_dir = resolved_path.parent();
                        while let Some(dir) = pkg_dir {
                            if dir.join("package.json").exists() && has_perry_native_library(dir) {
                                if let Some(manifest) =
                                    parse_native_library_manifest(dir, module_name, target)
                                {
                                    // #466 Phase 2: refuse to load a wrapper whose
                                    // declared abiVersion is incompatible with the
                                    // bundled perry-ffi. Missing field warns but
                                    // continues during the v0.5.x cycle.
                                    if let Err(msg) =
                                        super::resolve::validate_abi_version(&manifest)
                                    {
                                        return Err(anyhow::anyhow!("{}", msg));
                                    }
                                    match format {
                                        OutputFormat::Text => println!(
                                            "  Native library: {} ({} FFI functions)",
                                            manifest.module,
                                            manifest.functions.len()
                                        ),
                                        OutputFormat::Json => {}
                                    }
                                    ctx.native_libraries.push(manifest);
                                }
                                break;
                            }
                            pkg_dir = dir.parent();
                        }
                    }

                    match format {
                        OutputFormat::Text => {
                            println!(
                                "  JS module: {} -> {}",
                                import.source,
                                resolved_path.display()
                            );
                        }
                        OutputFormat::Json => {}
                    }

                    // Collect JS module
                    collect_modules(
                        &resolved_path,
                        ctx,
                        visited,
                        enable_js_runtime,
                        format,
                        target,
                        next_class_id,
                        skip_transforms,
                        parse_cache.as_deref_mut(),
                    )?;
                }
                ModuleKind::NativeRust => {
                    // Native Rust modules are handled by stdlib
                }
            }
        } else {
            // Could not resolve - might be a Node.js builtin or missing module
            // Issue #629: hard-error on namespace imports (`import * as X from ...`)
            // for unresolved modules. Pre-fix the codegen catch-all produced a
            // typeof-"object" empty-namespace stub; property access cleanly read
            // undefined, but the cascade ("X is undefined" / silent no-ops when
            // calling missing methods) is worse than a compile-time failure
            // because the user has no idea their namespace is empty. Named
            // imports (`import { foo } from "..."`) and bare side-effect
            // imports still warn-and-continue per the existing behavior, since
            // those produce more pointed runtime errors at the actual missing
            // binding rather than silently no-op-ing every method call.
            let has_namespace_specifier = import
                .specifiers
                .iter()
                .any(|s| matches!(s, perry_hir::ImportSpecifier::Namespace { .. }));
            // Issue #841: known Node submodules (`node:timers/promises`,
            // `node:readline/promises`, `node:stream/promises`,
            // `node:stream/consumers`, `node:sys`) have no stdlib backing
            // but we DO ship a runtime namespace stub for them via
            // `js_node_submodule_namespace`. Skip the hard-error so the
            // compile.rs registration loop can wire the namespace local
            // through to that runtime helper.
            if has_namespace_specifier && known_node_submodule_key(&import.source).is_none() {
                return Err(anyhow::anyhow!(
                    "Could not resolve namespace import `import * as ... from \"{source}\"` in {filename}.\n\
                     Perry has no stdlib bindings for this module path, so the namespace would compile to an empty object \
                     — every method call on it would silently no-op at runtime. Pick one:\n  \
                       • switch to named imports: `import {{ foo }} from \"{source}\"` (still resolves through whatever backing exists, but fails fast at the actual missing binding),\n  \
                       • remove the import if it's unused,\n  \
                       • or add the module to perry-stdlib / perry-ext-* / perry.compilePackages.",
                    source = import.source,
                    filename = filename,
                ));
            }
            if !import.is_native && known_node_submodule_key(&import.source).is_none() {
                match format {
                    OutputFormat::Text => {
                        println!(
                            "  Warning: Could not resolve import '{}' from {}",
                            import.source, filename
                        );
                    }
                    OutputFormat::Json => {}
                }
            }
        }
    }

    // Process re-exports
    for export in &hir_module.exports {
        let source = match export {
            perry_hir::Export::ReExport { source, .. } => Some(source),
            perry_hir::Export::ExportAll { source } => Some(source),
            // `export * as Foo from "./Foo"` (#310): pull the source file
            // into the module graph the same way the other re-export
            // shapes do. Without this, the consumer's `import { Foo }`
            // would resolve to the re-exporter, but `Foo`'s actual
            // implementation file would never be visited and codegen
            // would have no symbols to dispatch against.
            perry_hir::Export::NamespaceReExport { source, .. } => Some(source),
            perry_hir::Export::Named { .. } => None,
        };
        if let Some(src) = source {
            if let Some((resolved_path, kind)) =
                cached_resolve_import(src.as_str(), &canonical, ctx)
            {
                match kind {
                    ModuleKind::NativeCompiled => {
                        collect_modules(
                            &resolved_path,
                            ctx,
                            visited,
                            enable_js_runtime,
                            format,
                            target,
                            next_class_id,
                            skip_transforms,
                            parse_cache.as_deref_mut(),
                        )?;
                    }
                    ModuleKind::Interpreted => {
                        if enable_js_runtime {
                            collect_modules(
                                &resolved_path,
                                ctx,
                                visited,
                                enable_js_runtime,
                                format,
                                target,
                                next_class_id,
                                skip_transforms,
                                parse_cache.as_deref_mut(),
                            )?;
                        }
                    }
                    ModuleKind::NativeRust => {}
                }
            }
        }
    }

    // Issue #535 — `perry/ui` `state<T>` desugar pass.
    let is_harmonyos = matches!(target, Some("harmonyos") | Some("harmonyos-simulator"));
    if !is_harmonyos {
        perry_transform::state_desugar::run(&mut hir_module);
    }

    // Run HIR transforms AFTER imports/re-exports have been recursively
    // collected, so `ctx.native_modules` already contains every dependency
    // of this module. The cross-module method-inlining harvester below
    // pulls inlinable methods from those prior modules — without this
    // ordering, a consumer (e.g. `sync-hotpath.test.ts`) would inline
    // BEFORE `world.ts` finished processing, missing every `World.*`
    // candidate and leaving the hot `world.set(...)` call as a runtime
    // dispatch.
    //
    // Pre-existing constraint: `transform_async_to_generator` runs AFTER
    // `inline_functions` (so inlined async bodies are still rewritten)
    // and BEFORE `transform_generators` (which consumes the generator
    // shape it produces). Issue #256.
    if !skip_transforms {
        let mut extra_methods: std::collections::HashMap<(String, String), MethodCandidate> =
            std::collections::HashMap::new();
        if std::env::var("PERRY_INLINE_DEBUG").is_ok() {
            eprintln!(
                "[INLINE-DRIVER] processing {}: prior modules={:?}",
                hir_module.name,
                ctx.native_modules
                    .values()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
            );
        }
        for prior_module in ctx.native_modules.values() {
            // The strict harvester rejects ExternFuncRef-using methods.
            // The loose variant records each required extern name;
            // `inline_functions` filters by destination imports.
            // First-write-wins on key collision (rare — issue #309 cycle
            // breaker). Strict-harvest entries are functionally equivalent
            // when colliding with the loose variant (same body), so
            // either ordering is correct.
            for (k, v) in gather_cross_module_methods_with_extern_imports(prior_module) {
                extra_methods.entry(k).or_insert(v);
            }
            for (k, v) in gather_cross_module_methods(prior_module) {
                extra_methods.entry(k).or_insert(v);
            }
        }
        // Cross-module field-type info: `(class_name, field_name) ->
        // field_class_name`. Lets the inliner's `resolve_receiver_class`
        // walk a chain like `world.commandBuffer.set(...)` — without it,
        // the receiver match bails at the first PropertyGet and the call
        // stays a runtime dispatch. Built from every prior module's
        // class.fields where the type is `Named(...)`.
        let mut extra_class_fields: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        for prior_module in ctx.native_modules.values() {
            for class in &prior_module.classes {
                for f in &class.fields {
                    if let perry_types::Type::Named(field_class) = &f.ty {
                        extra_class_fields
                            .entry((class.name.clone(), f.name.clone()))
                            .or_insert_with(|| field_class.clone());
                    }
                }
            }
        }
        // Cross-module anon-shape classes. Names are content-addressed
        // (FNV-1a hash of the canonical shape key), so dedup-by-name across
        // modules is correct: any two modules that synthesized a class for
        // the same closed-shape literal end up with byte-identical class
        // definitions under the same name. Required so that when
        // `inline_functions` copies a method body referencing
        // `__AnonShape_<hash>` into this module, codegen can resolve the
        // class definition (otherwise the field list is missing and the
        // literal lowers as a bare object with all properties dropped).
        let mut extra_anon_classes: std::collections::HashMap<String, perry_hir::Class> =
            std::collections::HashMap::new();
        for prior_module in ctx.native_modules.values() {
            for (k, v) in gather_cross_module_anon_classes(prior_module) {
                extra_anon_classes.entry(k).or_insert(v);
            }
        }
        // Interprocedural deforestation. Runs BEFORE inline_functions
        // so the inliner sees deforested signatures (the rewritten
        // function takes an accumulator param; inlined call sites then
        // already use the new shape). Intra-module only — see
        // `deforest::run` doc-comment for limitations and the manual
        // ABC451D validation.
        perry_transform::deforest::run(&mut hir_module);
        inline_functions(
            &mut hir_module,
            &extra_methods,
            &extra_class_fields,
            &extra_anon_classes,
        );
        // Static-trip-count for-loop unroll. Runs AFTER the inliner so any
        // inlined function bodies' loops also get unrolled. Runs BEFORE the
        // async/generator transforms — those transforms pre-emptively rewrite
        // control flow into state-machine shapes that the unroll match would
        // no longer recognize. Doing it pre-async keeps the analysis simple.
        // image_convolution's 5x5 blur kernel: outer ky and inner kx both
        // become 25 fully-unrolled stmts with `KERNEL[ky+2][kx+2]` collapsed
        // to compile-time integer literals — see crates/perry-transform/
        // src/unroll.rs.
        perry_transform::unroll_static_loops(&mut hir_module);
        // Inline `finally` bodies before each abrupt completion
        // (`return` / `break` / `continue` / labeled-break / labeled-
        // continue) reachable inside a `try { ... } finally { Y }`
        // shape. Must run BEFORE `transform_async_to_generator` because
        // the async transform flattens `try`/`finally` into a flat
        // state-machine sequence — an abrupt completion in the body
        // terminates the state, leaving the appended finally as dead
        // code. Issue #536.
        inline_finally_into_returns(&mut hir_module);
        transform_async_to_generator(&mut hir_module);
        transform_generators(&mut hir_module);
    }

    // Detect fetch() usage — js_fetch_with_options lives in perry-stdlib
    if hir_module.uses_fetch {
        ctx.needs_stdlib = true;
        ctx.uses_fetch = true;
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
    // auto-optimize. Text-grep the serialized Debug form of the HIR — these
    // variants are rare enough that the cost is negligible and avoids
    // writing a new visitor.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
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
        {
            ctx.needs_stdlib = true;
            ctx.uses_crypto_builtins = true;
        }
    }

    // Detect readline usage via process.stdin.setRawMode / .on (#347
    // Phase 2). These don't go through an `import 'readline'` statement,
    // so the import-based needs_stdlib detection above misses them.
    // The codegen lowers ProcessStdinSetRawMode / ProcessStdinOn to direct
    // extern calls to js_readline_set_raw_mode / js_readline_stdin_on
    // which live in perry-stdlib::readline; without stdlib linked, those
    // symbols are unresolved.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        if hir_debug.contains("ProcessStdinSetRawMode") || hir_debug.contains("ProcessStdinOn") {
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

    ctx.native_modules.insert(canonical, hir_module);
    Ok(())
}
