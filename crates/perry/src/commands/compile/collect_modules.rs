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
use perry_transform::{inline_functions, transform_async_to_generator, transform_generators};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::OutputFormat;

use super::{
    cached_resolve_import, djb2_hash, extract_compile_package_dir,
    has_perry_native_library, is_declaration_file, is_in_compile_package,
    is_in_perry_native_package, is_js_file, parse_cached, parse_native_library_manifest,
    parse_package_specifier, CompilationContext, JsModule, ParseCache,
};

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
        ctx.js_modules.insert(specifier.clone(), JsModule {
            path: canonical.clone(),
            source,
            specifier,
        });
        ctx.needs_js_runtime = true;

        // We don't parse JS/node_modules files for their imports (V8 will handle that at runtime)
        return Ok(());
    }

    // It's a TypeScript file to compile natively
    let source = fs::read_to_string(&canonical)
        .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;

    // Record the source hash for V2.2's per-module object cache. Computed here
    // (instead of in the rayon codegen loop) so the cache key doesn't force a
    // second read of the source bytes — we already have them.
    ctx.module_source_hashes
        .insert(canonical.clone(), djb2_hash(source.as_bytes()));

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
            match crate::commands::typecheck::resolve_types_for_file(client, &source_file_path, &positions) {
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

    let (mut hir_module, new_next_class_id) = perry_hir::lower_module_with_class_id_and_types(
        ast_module, &module_name, &source_file_path, *next_class_id, resolved_types,
    )?;
    *next_class_id = new_next_class_id; // Update the global class_id counter

    if !skip_transforms {
        // Apply function inlining optimization
        inline_functions(&mut hir_module);

        // Issue #256: rewrite plain async functions into generators with
        // was_plain_async set, so the generator transform below produces
        // a state machine wrapped in an async-step driver. Must run AFTER
        // inline_functions (so inlined async bodies are also rewritten)
        // and BEFORE transform_generators (which consumes the generator
        // shape we produce).
        transform_async_to_generator(&mut hir_module);

        // Transform generator functions into state machines
        transform_generators(&mut hir_module);
    }

    // Process imports and update their resolved paths and module kinds
    for import in &mut hir_module.imports {
        // Apply package alias (e.g., @parse/node-apn → perry-push from perry.packageAliases)
        if let Some(alias) = ctx.package_aliases.get(import.source.as_str()).cloned() {
            import.source = alias;
            import.is_native = perry_hir::is_native_module(&import.source);
        }

        if import.is_native {
            import.module_kind = ModuleKind::NativeRust;
            if import.source == "perry/ui" {
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
                let normalized = import.source.strip_prefix("node:")
                    .unwrap_or(&import.source)
                    .to_string();
                ctx.native_module_imports.insert(normalized);
            }
            continue;
        }

        if let Some((resolved_path, kind)) = cached_resolve_import(&import.source, &canonical, ctx) {
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
                        if ctx.compile_packages.contains(&pkg_name) && !ctx.compile_package_dirs.contains_key(&pkg_name) {
                            if let Some(pkg_dir) = extract_compile_package_dir(&resolved_path, &pkg_name) {
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
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        if !ctx.native_libraries.iter().any(|nl| nl.module == *module_name) {
                            // Walk up to find the package directory with perry.nativeLibrary
                            // Works for both node_modules packages and symlinked local packages
                            let mut pkg_dir = resolved_path.parent();
                            while let Some(dir) = pkg_dir {
                                if dir.join("package.json").exists() && has_perry_native_library(dir) {
                                    if let Some(manifest) = parse_native_library_manifest(dir, module_name, target) {
                                        match format {
                                            OutputFormat::Text => println!("  Native library: {} ({} FFI functions)", manifest.module, manifest.functions.len()),
                                            OutputFormat::Json => {}
                                        }
                                        ctx.native_libraries.push(manifest);
                                    }
                                    break;
                                }
                                pkg_dir = dir.parent();
                            }
                        }
                    }
                    // Recursively collect TypeScript modules
                    collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms, parse_cache.as_deref_mut())?;
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
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        if !ctx.native_libraries.iter().any(|nl| nl.module == *module_name) {
                            let mut pkg_dir = resolved_path.parent();
                            while let Some(dir) = pkg_dir {
                                if dir.join("package.json").exists() && has_perry_native_library(dir) {
                                    if let Some(manifest) = parse_native_library_manifest(dir, module_name, target) {
                                        match format {
                                            OutputFormat::Text => println!("  Native library: {} ({} FFI functions)", manifest.module, manifest.functions.len()),
                                            OutputFormat::Json => {}
                                        }
                                        ctx.native_libraries.push(manifest);
                                    }
                                    break;
                                }
                                pkg_dir = dir.parent();
                            }
                        }
                    }

                    match format {
                        OutputFormat::Text => {
                            println!("  JS module: {} -> {}", import.source, resolved_path.display());
                        }
                        OutputFormat::Json => {}
                    }

                    // Collect JS module
                    collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms, parse_cache.as_deref_mut())?;
                }
                ModuleKind::NativeRust => {
                    // Native Rust modules are handled by stdlib
                }
            }
        } else {
            // Could not resolve - might be a Node.js builtin or missing module
            // For now, treat unresolved non-native imports as errors
            if !import.is_native {
                match format {
                    OutputFormat::Text => {
                        println!("  Warning: Could not resolve import '{}' from {}", import.source, filename);
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
            perry_hir::Export::Named { .. } => None,
        };
        if let Some(src) = source {
            if let Some((resolved_path, kind)) = cached_resolve_import(src, &canonical, ctx) {
                match kind {
                    ModuleKind::NativeCompiled => {
                        collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms, parse_cache.as_deref_mut())?;
                    }
                    ModuleKind::Interpreted => {
                        if enable_js_runtime {
                            collect_modules(&resolved_path, ctx, visited, enable_js_runtime, format, target, next_class_id, skip_transforms, parse_cache.as_deref_mut())?;
                        }
                    }
                    ModuleKind::NativeRust => {}
                }
            }
        }
    }

    // Detect fetch() usage — js_fetch_with_options lives in perry-stdlib
    if hir_module.uses_fetch {
        ctx.needs_stdlib = true;
        ctx.uses_fetch = true;
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
        {
            ctx.needs_stdlib = true;
            ctx.uses_crypto_builtins = true;
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
