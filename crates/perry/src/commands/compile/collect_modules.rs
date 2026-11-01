//! Module discovery + transitive import walk.
//! Walks the import graph, lowers TypeScript to HIR, classifies native-compiled
//! versus JS-runtime-loaded modules, and accumulates them in the compilation context.
//! Per-module HIR passes run before insertion, and source hashes feed
//! the V2.2 codegen cache key.

use anyhow::{anyhow, Result};
use perry_hir::ModuleKind;
use perry_transform::{
    gather_cross_module_anon_classes, gather_cross_module_functions, gather_cross_module_methods,
    gather_cross_module_methods_with_extern_imports, inline_finally_into_returns, inline_functions,
    transform_async_to_generator, transform_generators, FunctionCandidate, MethodCandidate,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::collect_modules_helpers::{
    file_loader_import_sources, imported_file_asset_name, looks_like_generated_module,
    text_loader_import_sources,
};
use crate::commands::progress::{ProgressSnapshot, VerboseProgress};
use crate::OutputFormat;

use super::{
    cached_resolve_import, declaration_sidecar_for_resolved_import, extract_compile_package_dir,
    has_perry_native_library, is_declaration_file, is_in_perry_native_package,
    is_recognized_text_asset, parse_cached, parse_native_library_manifest, parse_package_specifier,
    CompilationContext, JsModule, ParseCache,
};

mod binding_faithfulness;
mod crypto_ns;
mod discovery;
mod dynamic_glob;
mod eval_worker;
mod feature_detect;
mod import_helpers;
mod import_meta_require;
mod import_meta_resolve;
mod json_module;
mod native_addon;
mod native_esm_namespace;
mod parse_error;
pub(crate) mod reexport_prune;
mod script_string;
mod source_prep;
#[cfg(test)]
mod source_prep_tests;
pub(super) mod static_require_transform;
#[cfg(test)]
mod tests;
mod walk;
mod wasm_asset;
mod worker;
pub(crate) mod worker_url;

use binding_faithfulness::audit_native_binding_choice;
pub(super) use discovery::is_nextjs_runtime_module;
use discovery::{aot_promotion_is_authorized, collect_js_files_recursive};
use dynamic_glob::{expand_dynamic_import_glob, unparsable_candidate_stub};
use eval_worker::materialize_eval_worker_source;
pub(super) use import_helpers::known_node_submodule_key;
use import_helpers::{
    cached_resolve_import_with_lexical_base, collect_js_module_imports,
    ensure_bunfs_import_resolves, env_defines_for_lowering,
};
use import_meta_require::rewrite_import_meta_require_addons;
pub(super) use native_addon::{collect_declared_addons, package_has_unsupported_node_addon};
use native_addon::{
    collect_or_refuse_node_addon, note_native_addon_wrapper, refuse_compile_package_native_addon,
};
use parse_error::annotate_parse_error;
pub(super) use source_prep::{PreparedSources, WalkSources};
#[cfg(test)]
pub(super) use walk::collect_modules;
pub(super) use walk::collect_modules_with_sources;
use wasm_asset::{is_wasm_asset, synthesize_wasm_module};

const MAX_CROSS_MODULE_INLINE_PRIOR_MODULES: usize = 128;

/// #11616: every quoted `node:<name>` literal in `source` (`"`, `'` or a
/// template without substitutions).
fn node_specifier_literals(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = source[at..].find("node:") {
        let start = at + found;
        at = start + 5;
        let Some(&quote) = start.checked_sub(1).and_then(|i| bytes.get(i)) else {
            continue;
        };
        if !matches!(quote, b'"' | b'\'' | b'`') {
            continue;
        }
        let end = source[at..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '/'))
            .map_or(source.len(), |i| at + i);
        if end > at && bytes.get(end) == Some(&quote) {
            out.push(&source[start..end]);
        }
    }
    out
}

fn register_bunfs_literal_assets(source: &str, ctx: &mut CompilationContext) {
    let Some(root) = ctx.bunfs_root.clone() else {
        return;
    };
    for (name, path) in super::embed::resolve_bunfs_literal_assets(source, &root) {
        if !ctx
            .embedded_assets
            .iter()
            .any(|(existing_name, _)| existing_name == &name)
        {
            ctx.embedded_assets.push((name, path));
        }
    }
}

enum VisitState {
    InProgress,
    Done,
}

enum WorkFrame {
    Enter(PathBuf),
    /// A worker entry named only by a `new URL("<literal>", import.meta.url)`.
    /// If its module fails to compile, that is a warning: the literal may not
    /// name a worker at all.
    EnterWorkerUrl(PathBuf),
    Finish(PreparedModule),
}

struct ModuleDiscovery {
    finish: Option<PreparedModule>,
    children: Vec<PathBuf>,
}

pub(crate) struct PreparedModule {
    canonical: PathBuf,
    module_name: String,
    hir_module: perry_hir::Module,
}

#[allow(clippy::too_many_arguments)]
/// The CommonJS support module's path: under the cache dir, named by the
/// entry, so one program always compiles it at the same path.
pub(super) fn cjs_support_path(ctx: &mut CompilationContext) -> PathBuf {
    if let Some(p) = &ctx.cjs_support_path {
        return p.clone();
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ctx.entry_canonical.hash(&mut hasher);
    // Canonical, like every collected module's key: the file itself is
    // written only after the walk, so its directory is canonicalized.
    let dir = ctx.cache_dir.join("synthetic-modules");
    let _ = std::fs::create_dir_all(&dir);
    let dir = dir.canonicalize().unwrap_or(dir);
    let path = dir.join(format!("perry-cjs-{:016x}.mjs", hasher.finish()));
    // Compiler-generated code: compiled natively wherever the cache dir is
    // (by default under `node_modules/.cache`).
    ctx.aot_discovered_modules.insert(path.clone());
    ctx.cjs_support_path = Some(path.clone());
    path
}

fn collect_module_one(
    entry_path: &PathBuf,
    canonical: PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    progress: &VerboseProgress,
    parse_cache: Option<&mut ParseCache>,
    prefetch: Option<&source_prep::SourcePrefetch>,
    sources: &mut source_prep::WalkSources,
) -> Result<ModuleDiscovery> {
    let mut pending = Vec::new();

    // A `.node` member is a sidecar native module, never UTF-8 source. Record
    // an approved entry and stop this discovery frame before any JS/TS parser
    // or source reader sees its bytes.
    note_native_addon_wrapper(ctx, &canonical);
    if collect_or_refuse_node_addon(ctx, &canonical, target)? {
        return Ok(ModuleDiscovery {
            finish: None,
            children: pending,
        });
    }

    // Check if this file should be handled by JS runtime instead of native compilation
    // This includes: JS files, declaration files (.d.ts), JSON files, or any file in node_modules when JS runtime is enabled
    let is_json = canonical.extension().and_then(|e| e.to_str()) == Some("json");
    // #5223: text-asset imports (`import s from "./x.txt"`). A recognized text
    // extension is read verbatim and synthesized into a native module whose
    // default export is the file contents as a JS string (see the text branch
    // below, mirroring the JSON-module path).
    let is_text_asset =
        is_recognized_text_asset(&canonical) || ctx.text_loader_asset_paths.contains(&canonical);
    // Bun's `{ type: "file" }` loader exposes a virtual path string while
    // keeping the imported bytes out of the TypeScript parser. The importer
    // registers this canonical path before the child frame is visited.
    let imported_file_asset = ctx
        .file_loader_asset_paths
        .contains(&canonical)
        .then(|| {
            ctx.embedded_assets
                .iter()
                .find_map(|(name, path)| (path == &canonical).then(|| name.clone()))
        })
        .flatten();
    // #5234: `.wasm` is binary, so collect it through an executable synthetic
    // module instead of the UTF-8 source reader.
    let is_wasm = is_wasm_asset(&canonical);
    let source_prep::Route {
        is_in_node_modules,
        is_perry_native,
        is_in_compiled_pkg,
        should_use_js_runtime,
    } = source_prep::route(&canonical, &source_prep::RouteInputs::of(ctx));

    // #348 follow-up: JSON module imports (`import data from "./x.json"`,
    // optionally with `with { type: "json" }`) are NOT skipped — they compile
    // to a native module whose default export is the parsed data (synthesized
    // as `export default <json>;` just below). Previously JSON was handed to
    // the (now-removed) JS runtime / skipped outright, leaving the default
    // import bound to the empty-module sentinel — which broke cli-boxes (and
    // thus ink's `borderStyle` box-drawing).

    if should_use_js_runtime {
        // Skip declaration files - they're just type information
        if is_declaration_file(&canonical) {
            return Ok(ModuleDiscovery {
                finish: None,
                children: pending,
            });
        }

        // Perry native extension packages (ethers, mysql2, ws, dotenv) are handled
        // entirely by Perry's built-in stdlib — they must NOT be loaded into V8.
        if is_perry_native {
            return Ok(ModuleDiscovery {
                finish: None,
                children: pending,
            });
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;
        register_bunfs_literal_assets(&source, ctx);
        progress.record(ProgressSnapshot {
            stage: "collect-js-module",
            module_path: Some(&canonical),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });

        let specifier = canonical.to_string_lossy().to_string();
        // Issue #818: walk transitive ESM imports for JS modules so the
        // bundle contains every file the V8 fallback will be asked to load
        // at runtime. Without this, pure-ESM packages with relative
        // sub-module imports (e.g. hono's `dist/index.js` re-exporting
        // `./hono.js`, which re-exports `./hono-base.js`, …) would land
        // in `ctx.js_modules` with only the entry file, leaving every
        // transitive `./foo.js` to be resolved against disk at runtime —
        // fine when node_modules/ is co-located with the binary, but
        // produces a `Cannot resolve module` failure (and in some cases
        // a downstream segfault when the missing-module callback returns
        // an unboxed undefined to compiled native code) when the binary
        // is shipped on its own.
        //
        // We deliberately collect imports via a lightweight regex scan
        // rather than parsing every JS file through SWC. The bundler
        // only needs to know what file paths to embed; runtime
        // semantics (default vs named, conditional execution, dynamic
        // import) are still V8's job. The regex catches all the static
        // shapes we need to follow:
        //   import x from "./foo.js"
        //   import { a, b } from "./foo.js"
        //   import * as ns from "./foo.js"
        //   import "./side-effect.js"
        //   export { x } from "./foo.js"
        //   export * from "./foo.js"
        //   export * as ns from "./foo.js"
        // Dynamic `import("./foo.js")` with a string-literal argument is
        // also walked. Template-literal / variable specifiers can't be
        // resolved statically and are skipped (V8 will surface the
        // resolution failure at runtime, same as today).
        let transitive_paths = collect_js_module_imports(entry_path, &source);
        ctx.js_modules.insert(
            specifier.clone(),
            JsModule {
                path: canonical.clone(),
                source,
                specifier: specifier.clone(),
            },
        );
        // Record the file that reached a runtime-JS module so the
        // V8-free gate (enforced after dep collection) can name the
        // importer(s) in its refusal diagnostic. De-duplicate by
        // canonical path — many edges may resolve to the same JS file.
        if !ctx.js_runtime_importers.iter().any(|p| p == &canonical) {
            ctx.js_runtime_importers.push(canonical.clone());
        }
        if let Some(sidecar) = declaration_sidecar_for_resolved_import(&specifier, &canonical) {
            ctx.declaration_sidecars.insert(canonical.clone(), sidecar);
        }

        // Recurse into each resolved sibling. We re-enter
        // `collect_modules`, which re-runs the JS/native classification
        // — covering the case where a JS file re-imports something that
        // resolves to a TypeScript file under a `compilePackages` dir.
        for next in transitive_paths {
            pending.push(next);
        }
        return Ok(ModuleDiscovery {
            finish: None,
            children: pending,
        });
    }

    // #10735: this module's `require.main` (CJS preamble) must resolve to the
    // compile-time entry -- same comparison as `is_entry_module` below.
    let cjs_is_entry_module = ctx.entry_canonical.as_ref() == Some(&canonical);
    // The explicit file loader and the wasm adapter synthesize executable
    // TypeScript; extension-based JSON/text loaders must not reinterpret that
    // generated source as asset bytes.
    let plain_source = imported_file_asset.is_none() && !is_wasm;
    let prep_class = source_prep::PrepClass {
        wrap_eligible: is_in_compiled_pkg || !is_in_node_modules,
        is_json: is_json && plain_source,
        is_text: is_text_asset && plain_source,
        is_entry: cjs_is_entry_module,
    };
    // A plain file is read and prepared by `prepare_plain` — by the
    // compile's first walk when this is the second and the first prepared it
    // for this classification, else on a source-prep worker ahead of the walk
    // when one has it ready for this classification, otherwise here. Asset
    // adapters, `--bunfs-root` sources and sources that need the
    // `import.meta.require` rewrite take the walk-only path below.
    let read = if plain_source && ctx.bunfs_root.is_none() {
        let earlier = match sources {
            source_prep::WalkSources::Reuse(earlier) => earlier.take(&canonical, prep_class),
            source_prep::WalkSources::Keep(_) => None,
        };
        let read = match earlier {
            Some(read) => read,
            None => {
                let prefetched = prefetch
                    .and_then(|prefetch| prefetch.claim(&canonical))
                    .filter(|prefetched| prefetched.class == prep_class);
                match prefetched {
                    Some(prefetched) => prefetched.read?,
                    None => source_prep::prepare_plain(
                        &canonical,
                        prep_class,
                        &source_prep::PrepInputs::of(ctx, target),
                    )?,
                }
            }
        };
        if let source_prep::WalkSources::Keep(kept) = sources {
            kept.keep(&canonical, prep_class, &read);
        }
        Some(read)
    } else {
        None
    };
    let prepared = match read {
        Some(source_prep::ReadSource::Prepared {
            require_specs,
            node_targets,
            prepared,
        }) => {
            // Without `--bunfs-root` the `import.meta.require` rewrite and the
            // Bun asset scan leave this source alone; what remains of them is
            // the virtual-path check per literal `require()`.
            for specifier in &require_specs {
                ensure_bunfs_import_resolves(specifier, &canonical, ctx)?;
            }
            pending.extend(node_targets);
            prepared
        }
        read => {
            // A plain source that needs the `import.meta.require` rewrite:
            // only what follows the rewrite can come from an earlier walk.
            let after_rewrite = matches!(read, Some(source_prep::ReadSource::NeedsWalk(_)));
            let raw_source = match read {
                Some(source_prep::ReadSource::NeedsWalk(raw_source)) => raw_source,
                Some(source_prep::ReadSource::Prepared { .. }) => {
                    unreachable!("matched above")
                }
                // #5234: synthesize a TypeScript adapter that embeds and
                // instantiates the binary at module initialization, then feed it
                // through the normal native module pipeline used by JSON and text
                // assets.
                None => {
                    if let Some(asset_name) = &imported_file_asset {
                        let virtual_path = format!("$perryfs/{asset_name}");
                        let literal = serde_json::to_string(&virtual_path).map_err(|e| {
                            anyhow!(
                                "Failed to encode imported file asset path {}: {}",
                                canonical.display(),
                                e
                            )
                        })?;
                        format!("export default {literal};\n")
                    } else if is_wasm {
                        let bytes = fs::read(&canonical).map_err(|e| {
                            anyhow!("Failed to read {}: {}", canonical.display(), e)
                        })?;
                        let asset_name = ctx
                            .file_loader_asset_names
                            .get(&canonical)
                            .cloned()
                            .unwrap_or_else(|| {
                                imported_file_asset_name(&canonical, &ctx.cache_root)
                            });
                        if !ctx
                            .embedded_assets
                            .iter()
                            .any(|(_, path)| path == &canonical)
                        {
                            ctx.embedded_assets
                                .push((asset_name.clone(), canonical.clone()));
                        }
                        let virtual_path = format!("$perryfs/{asset_name}");
                        synthesize_wasm_module(&bytes, &virtual_path).source
                    } else {
                        // It's a TypeScript (or synthetic JSON/text) file to
                        // compile natively.
                        fs::read_to_string(&canonical)
                            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?
                    }
                }
            };
            // Bun exposes `import.meta.require`; unlike CommonJS `require`,
            // aliases of that function and URL-derived addon paths are
            // invisible to the ordinary static-require scan. Recover and
            // rewrite exact Node-API loads before HIR lowering so runtime
            // execution uses only the authenticated sidecar id. Run before the
            // Bun virtual-literal asset scan so a `.node` call target ships once
            // in the sidecar rather than also being embedded as inert data.
            let raw_source =
                rewrite_import_meta_require_addons(&raw_source, &canonical, ctx, target)?;
            register_bunfs_literal_assets(&raw_source, ctx);
            // CJS wrapping consumes literal `require()` sites and replaces them
            // with generated loader calls. Queue native targets before that
            // rewrite so the graph still authenticates and packages the
            // selected `.node` binary.
            for specifier in super::cjs_wrap::extract_require_specifiers(&raw_source) {
                ensure_bunfs_import_resolves(&specifier, &canonical, ctx)?;
                if let Some(target) =
                    super::resolve::resolve_relative_import_path(&specifier, &canonical)
                {
                    if target.extension().and_then(|extension| extension.to_str()) == Some("node") {
                        pending.push(target);
                    }
                }
            }
            let earlier = match sources {
                source_prep::WalkSources::Reuse(earlier) if after_rewrite => {
                    earlier.take_after_rewrite(&canonical, prep_class, &raw_source)
                }
                _ => None,
            };
            match earlier {
                Some(prepared) => Ok(prepared),
                None => {
                    let kept_raw = match sources {
                        source_prep::WalkSources::Keep(_) if after_rewrite => {
                            Some(raw_source.clone())
                        }
                        _ => None,
                    };
                    let prepared = source_prep::prepare_from_raw(
                        raw_source,
                        &canonical,
                        prep_class,
                        &source_prep::PrepInputs::of(ctx, target),
                    );
                    if let (source_prep::WalkSources::Keep(kept), Some(raw), Ok(prepared)) =
                        (&mut *sources, kept_raw, &prepared)
                    {
                        kept.keep_after_rewrite(&canonical, prep_class, raw, prepared);
                    }
                    prepared
                }
            }
        }
    };
    let source_prep::PreparedSource {
        class_family_syntax,
        source,
        was_cjs_wrapped,
        messages,
        computed_require_warnings,
        wrap_dump,
        stdlib_modules,
        debug_prefix_line_count,
    } = prepared?;
    if is_in_compiled_pkg && !ctx.aot_discovered_modules.contains(&canonical) {
        refuse_compile_package_native_addon(ctx, &canonical)?;
    }
    for message in messages {
        eprintln!("{message}");
    }
    if ctx.cjs_require_diagnostics {
        for warning in computed_require_warnings {
            eprintln!("{warning}");
        }
    }
    if let Some(dump) = wrap_dump {
        eprintln!("{dump}");
    }
    for module in stdlib_modules {
        ctx.needs_stdlib = true;
        ctx.native_module_imports.insert(module);
    }
    // `dns` is runtime-only, so `stdlib_modules` never lists it. Note a literal
    // `require('dns')` (and a `getBuiltinModule` mention of it) separately: it
    // arms the dns export router (`js_dns_install_exports`).
    let is_dns = |spec: &str| {
        matches!(
            spec.strip_prefix("node:").unwrap_or(spec),
            "dns" | "dns/promises"
        )
    };
    ctx.uses_dns |= super::cjs_wrap::extract_require_specifiers(&source)
        .iter()
        .any(|spec| is_dns(spec));
    if source.contains("getBuiltinModule") {
        ctx.uses_dns |= node_specifier_literals(&source)
            .into_iter()
            .any(|spec| is_dns(spec));
    }
    if let Some(prefix_line_count) = debug_prefix_line_count {
        ctx.cjs_wrap_debug_sources.insert(
            canonical.clone(),
            super::types::CjsWrapDebugSource {
                wrapped_source: source.clone(),
                prefix_line_count,
            },
        );
    }

    // Note (#686): we no longer hash source bytes here. The object cache key
    // is now keyed on a post-transform HIR fingerprint computed inside the
    // rayon codegen job (see compile.rs's main per-module closure), so
    // formatter-only edits hit the cache. Removing the per-source hash also
    // removes one bytes scan per module from the collect path.

    let filename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.ts");
    let parse_filename = canonical.to_str().unwrap_or(filename);

    // Include sibling/linked dependency directories before lowering, too.
    // A basename fallback makes external a/index.js and b/index.js share
    // tagged-template site IDs (and capture salts). Renaming the modules
    // after discovery cannot repair identities already embedded in HIR.
    // Relative paths keep those identities independent of the checkout.
    let module_name = super::asset_manifest::relative_path(&ctx.project_root, &canonical)
        .unwrap_or_else(|| canonical.clone())
        .to_string_lossy()
        .into_owned();

    // Parse via the optional in-memory cache (only populated by `perry dev`).
    // On a cache hit, we reuse the AST from the previous rebuild — the single
    // largest time sink in the hot rebuild path on unchanged files.
    progress.record(ProgressSnapshot {
        stage: "parse",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
        ..Default::default()
    });
    let source = unparsable_candidate_stub(
        ctx.glob_candidate_files.contains(&canonical),
        &source,
        parse_filename,
    )
    .unwrap_or(source);
    let ast_module_owned: swc_ecma_ast::Module;
    let ast_module: &swc_ecma_ast::Module = match parse_cache {
        Some(cache) => match parse_cached(cache, &canonical, &source, parse_filename) {
            Ok(m) => m,
            Err(e) => {
                return Err(annotate_parse_error(
                    e,
                    &canonical,
                    &source,
                    was_cjs_wrapped,
                ))
            }
        },
        None => match perry_parser::parse_typescript(&source, parse_filename) {
            Ok(m) => {
                ast_module_owned = m;
                &ast_module_owned
            }
            Err(e) => {
                return Err(annotate_parse_error(
                    anyhow!("Failed to parse {}: {}", canonical.display(), e),
                    &canonical,
                    &source,
                    was_cjs_wrapped,
                ));
            }
        },
    };
    let defined_module = ctx.parsed_defines.apply(ast_module);
    let ast_module = defined_module.as_ref().unwrap_or(ast_module);
    let resolved_module = import_meta_resolve::resolve_static(ast_module, &canonical, ctx)?;
    let ast_module = resolved_module.as_ref().unwrap_or(ast_module);
    let forwarding_module =
        reexport_prune::normalize_forwarding_barrel(ast_module, entry_path, ctx);
    let ast_module = forwarding_module.as_ref().unwrap_or(ast_module);
    let native_namespace_module = native_esm_namespace::rewrite_static(ast_module, &ctx.cache_dir)?;
    let ast_module = native_namespace_module.as_ref().unwrap_or(ast_module);
    // Layer-1 DCE: summarized in the first walk, pruned in the second.
    let demand_pruned = super::export_demand::visit(ctx, &canonical, ast_module, was_cjs_wrapped);
    let ast_module = demand_pruned.as_ref().unwrap_or(ast_module);
    let file_loader_sources = file_loader_import_sources(ast_module);
    let text_loader_sources = text_loader_import_sources(ast_module);
    let source_file_path = canonical.to_string_lossy().to_string();
    if !perry_hir::source_remap::is_covered(&canonical) {
        eprintln!(
            "warning: {} is outside --source-root; its real path is baked into the output",
            canonical.display()
        );
    }
    // JavaScript class fields are always [[Define]]; a TypeScript module
    // follows its tsconfig's `useDefineForClassFields`.
    let assign_class_fields = canonical
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "ts" | "tsx" | "mts" | "cts"))
        && !super::resolve::tsconfig_paths::use_define_for_class_fields(&canonical);

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
    let imported_class_accessors = if ctx.cross_module_class_accessors.is_empty() {
        None
    } else {
        Some(&ctx.cross_module_class_accessors)
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
    // #5009: install the `process.env.<NAME>` build-time defines so a static
    // `process.env.X` read folds to its `perry.define` literal at lowering —
    // esbuild-style, in every context and independent of tree-shaking. Keyed
    // by the bare env var name (the `process.env.` prefix stripped). Cleared
    // after the lower below (rayon-safe). The runtime-env default
    // (`NODE_ENV → "production"` for node_modules) stays in the tree-shake
    // `env_fold` pass; only explicit defines are folded here.
    perry_hir::set_env_defines(env_defines_for_lowering(&ctx.define));
    // #503: re-install the dynamic-stdlib-dispatch config on the current
    // thread before each lower. Driver may be a rayon worker that didn't
    // inherit the thread-local set on the main thread by `compile.rs`.
    perry_hir::set_refuse_dynamic_stdlib_dispatch(ctx.refuse_dynamic_stdlib_dispatch);
    perry_hir::set_allow_dynamic_stdlib_packages(ctx.allow_dynamic_stdlib_packages.clone());
    // #5206: re-install strict-eval mode on this (possibly rayon-worker)
    // thread before each lower, mirroring the dynamic-stdlib knob above.
    perry_hir::set_eval_strict_mode(ctx.strict_eval);
    // #5245: likewise re-install strict-unimplemented mode per worker thread.
    perry_hir::set_unimplemented_strict_mode(ctx.strict_unimplemented);
    // #503: stash the module source text so the dynamic-dispatch check
    // can look up `// @perry-allow-dynamic` line annotations adjacent to
    // any violation site without re-reading the file. Cleared right
    // after the lower call so it can't leak to unrelated work on this
    // thread.
    // #2309: arm the tree-shake deferral sink so `new Function` / #463
    // refusals in this module are recorded (and fall through) instead of
    // hard-erroring — but only for node_modules modules under tree-shaking.
    // User/host source refusals stay fatal. The sink is thread-local
    // (rayon-safe) and drained right after the lower below.
    let tree_shake_defer_armed = ctx.tree_shake && is_in_node_modules;
    if tree_shake_defer_armed {
        perry_hir::arm_deferral_sink();
    }
    perry_hir::set_current_module_source(source.clone());
    // #1681: re-install build-time precompile state on the (possibly rayon
    // worker) lowering thread — capture mode emits `precompile(EXPR)` source;
    // otherwise the captured results are substituted. Cleared after the lower.
    perry_hir::set_precompile_capture(ctx.precompile_capture);
    if !ctx.precompile_results.is_empty() {
        perry_hir::set_precompile_results(ctx.precompile_results.clone());
    }
    progress.record(ProgressSnapshot {
        stage: "lower",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
        ..Default::default()
    });
    // Expand only in the selected mode. Ordinary accessor/closure lowering then
    // owns captures, source-order semantics and the generated renderer imports.
    let solid_runtime = ctx.solid_jsx.runtime_for(entry_path)?;
    if solid_runtime.is_some() && !ctx.solid_client {
        ctx.solid_client = true;
        ctx.solid_client_recollect = true;
        ctx.resolve_cache.clear();
    }
    let solid_module = solid_runtime
        .as_deref()
        .and_then(|runtime| perry_hir::solid_jsx::lower_solid_jsx(ast_module, runtime));
    // #10848: fold this module's built-in member writes into the program-wide
    // set before lowering it. A patch first seen after other modules were
    // already lowered forces a re-walk (`rerun_collect_with_class_field_types`)
    // so those modules' direct calls see it too.
    let specialized_dynamic_code =
        perry_hir::static_dynamic_code::specialize(solid_module.as_ref().unwrap_or(ast_module));
    let dynamic_ast = specialized_dynamic_code
        .as_ref()
        .unwrap_or(solid_module.as_ref().unwrap_or(ast_module));
    ctx.realm_reach.merge(&perry_hir::realm_reach::scan(
        dynamic_ast,
        perry_codegen::realm_manifest::REALM,
    ));
    let patched_before = ctx.patched_builtins.len();
    let guarded_before = ctx
        .patched_builtins
        .dynamic_code_reach
        .disables_builtin_constants();
    perry_hir::dynamic_code::scan(
        dynamic_ast,
        &source_file_path,
        &mut ctx.patched_builtins.dynamic_code_reach,
    );
    // Every non-constant source site (eval, Function, vm, a runtime or
    // data: import) is evaluated by the interpreter, so it must be linked.
    if !ctx.patched_builtins.dynamic_code_reach.is_empty() {
        perry_hir::eval_classifier::note_dynamic_function_reachable();
    }
    perry_hir::scan_patched_builtins(dynamic_ast, &mut ctx.patched_builtins);
    // Global reach first seen here: modules lowered before it bound their
    // namespace calls unguarded (`patched_builtins::namespace_call_guarded`).
    let guarded_now = ctx
        .patched_builtins
        .dynamic_code_reach
        .disables_builtin_constants();
    if (ctx.patched_builtins.len() != patched_before || guarded_now != guarded_before)
        && ctx.patched_builtins_lowered_modules > 0
    {
        ctx.patched_builtins_grew_after_lower = true;
    }
    perry_hir::scan_proto_census(
        solid_module.as_ref().unwrap_or(ast_module),
        &mut ctx.proto_census,
    );
    perry_hir::set_patched_builtins(std::sync::Arc::new(ctx.patched_builtins.clone()));
    let lower_result = perry_hir::lower_module_full_with_platform_globals(
        solid_module.as_ref().unwrap_or(ast_module),
        &module_name,
        &source_file_path,
        *next_class_id,
        resolved_types,
        imported_class_fields,
        imported_class_accessors,
        is_entry_module,
        is_external_module,
        if ctx.bun_platform { &["Bun"] } else { &[] },
        assign_class_fields,
    );
    progress.heartbeat(ProgressSnapshot {
        stage: "lower",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
        ..Default::default()
    });
    ctx.patched_builtins_lowered_modules += 1;
    perry_hir::clear_patched_builtins();
    perry_hir::clear_compile_packages_override();
    perry_hir::clear_current_module_source();
    perry_hir::clear_precompile_state();
    perry_hir::clear_env_defines();
    // #2309: drain refusals deferred during this lower and tag them with the
    // canonical module path so the post-collection prune can decide whether
    // they survive. Done before the `?` below so a non-deferrable error can't
    // leak the armed sink onto the next module on this thread.
    if tree_shake_defer_armed {
        let module_str = canonical.to_string_lossy().to_string();
        for mut d in perry_hir::disarm_deferral_sink() {
            d.module = module_str.clone();
            ctx.deferred_refusals.push(d);
        }
    }
    // #5249: a lowering error carries a file-relative span but no file
    // identity, and only this module's `source` text is in scope here — so
    // resolve the span into a `file:line:col` + snippet diagnostic now,
    // matching what `perry check` prints, instead of letting the bare
    // locationless message propagate to the top-level error sink.
    let (mut hir_module, new_next_class_id) = match lower_result {
        Ok(v) => v,
        Err(e) => {
            if let Some(rendered) = crate::commands::lower_diagnostic::render_compile_lower_error(
                &e,
                &source_file_path,
                &source,
            ) {
                return Err(anyhow!("{}", rendered));
            }
            return Err(e);
        }
    };
    hir_module.class_family_syntax = class_family_syntax;
    // The CommonJS support module is the wrapper's own loader surface, like
    // the wrapper text the census never sees (`cjs_link`).
    if ctx.cjs_support_path.as_deref() == Some(canonical.as_path()) {
        if let Some(syntax) = &mut hir_module.class_family_syntax {
            syntax.module_capability_uses = 0;
        }
    }
    *next_class_id = new_next_class_id; // Update the global class_id counter

    // #9599: Bun platform mode is a real global mode, not merely a direct-call
    // syntax rewrite. Seed `globalThis.Bun` before every module initializer so
    // dependency code can observe it before the entry module runs. The native
    // namespace is cached by the runtime, making every idempotent assignment
    // install the same object and preserving `Bun === globalThis.Bun`.
    if ctx.bun_platform {
        hir_module.init.insert(
            0,
            perry_hir::Stmt::Expr(perry_hir::Expr::PropertySet {
                object: Box::new(perry_hir::Expr::PropertyGet {
                    byte_end: 0,
                    object: Box::new(perry_hir::Expr::GlobalGet(0)),
                    property: "globalThis".to_string(),
                    byte_offset: 0,
                }),
                property: "Bun".to_string(),
                value: Box::new(perry_hir::Expr::NativeModuleRef("bun".to_string())),
                byte_offset: 0,
                byte_end: 0,
            }),
        );
        // #10360: tell the runtime too, for Web APIs whose Bun behavior
        // differs from Node's. Same every-module seeding as above, so the
        // flag is on before any dependency's top-level code runs.
        hir_module.init.insert(
            0,
            perry_hir::Stmt::Expr(perry_hir::Expr::NativeMethodCall {
                module: "__perry_runtime".to_string(),
                class_name: None,
                object: None,
                method: "setBunPlatform".to_string(),
                args: Vec::new(),
            }),
        );
    }

    // Preserve native result types before async lowering splits awaited values
    // across synthetic locals. The later global fixup remains for inlined code.
    perry_hir::fix_local_native_instances(&mut hir_module);

    // #2309 Stage 2: fold build-time `process.env` branches BEFORE dynamic
    // `import()` edges are registered below, so a dead `import()` inside a
    // statically-false branch never enters the module graph. No-op unless
    // tree-shaking is enabled.
    if ctx.tree_shake {
        super::env_fold::fold_env_branches(&mut hir_module, &ctx.define, is_in_node_modules);
    }

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
    let dynamic_param_literals = perry_hir::collect_dynamic_import_param_literals(&hir_module);
    let dynamic_local_literals = perry_hir::collect_dynamic_import_local_candidate_literals(
        &hir_module,
        &module_const_locals,
        &dynamic_param_literals,
    );
    // #5230: re-install this module's source (cleared after the lower above) so
    // `current_module_line_at` can resolve a `file:line` for a deferred dynamic
    // import's notice/runtime-error message. Cleared again after the fill pass.
    perry_hir::set_current_module_source(source.clone());
    // Per-site outcome, aligned 1:1 with the `for_each_dynamic_import`
    // traversal order so the mutable fill pass below can apply them.
    // `Resolved(set)` populates `paths`; `Deferred(msg)` (#5230) leaves
    // `paths` empty and sets `deferred_error` so codegen lowers the site to a
    // rejected promise that throws `msg` only if reached.
    enum DynImportOutcome {
        Resolved(Vec<String>),
        Deferred(String),
    }
    let mut dynamic_path_sets: Vec<DynImportOutcome> = Vec::new();
    perry_hir::for_each_dynamic_import(&hir_module, &mut |expr| {
        if let perry_hir::Expr::DynamicImport {
            paths,
            arg,
            options,
            byte_offset,
            synchronous,
            ..
        } = expr
        {
            let synchronous = *synchronous;
            ctx.uses_dynamic_import_options |= options.is_some();
            let may_load_data = match options.as_deref() {
                None | Some(perry_hir::Expr::Undefined) => false,
                Some(perry_hir::Expr::Object(fields)) => fields.iter().any(|(key, value)| {
                    key == "with"
                        && match value {
                            perry_hir::Expr::Object(attributes) => {
                                attributes.iter().any(|(key, value)| {
                                    key == "type"
                                        && match value {
                                            perry_hir::Expr::String(loader) => matches!(
                                                loader.as_str(),
                                                "toml" | "json" | "text" | "file"
                                            ),
                                            _ => true,
                                        }
                                })
                            }
                            _ => true,
                        }
                }),
                _ => true,
            };
            if !paths.is_empty() {
                // Already resolved (e.g. a second pass on the same module).
                return;
            }
            let mut visiting: std::collections::HashSet<u32> = std::collections::HashSet::new();
            match perry_hir::resolve_import_path_with_context(
                arg.as_ref(),
                &module_const_locals,
                &dynamic_param_literals,
                &dynamic_local_literals,
                &mut visiting,
            ) {
                perry_hir::Resolution::Set(set) => {
                    if set.len() > perry_hir::DYNAMIC_IMPORT_PATH_CAP {
                        dyn_errors.push(format!(
                            "dynamic import() in module {} ({}): resolves to {} possible paths \
                             (limit: {})\n  note: consider enumerating with a ternary or \
                             registry object",
                            module_name,
                            canonical.display(),
                            set.len(),
                            perry_hir::DYNAMIC_IMPORT_PATH_CAP
                        ));
                        return;
                    }
                    for p in &set {
                        // Data files selected by import attributes are read at
                        // runtime, including literal absolute paths. Do not
                        // feed their contents to the TypeScript compiler.
                        if may_load_data
                            && (p.starts_with("file://") || std::path::Path::new(p).is_absolute())
                        {
                            continue;
                        }
                        if p.starts_with("data:text/javascript,") {
                            ctx.uses_data_url_dynamic_import = true;
                        }
                        if !new_dyn_imports.contains(p) {
                            new_dyn_imports.push(p.clone());
                        }
                    }
                    dynamic_path_sets.push(DynImportOutcome::Resolved(set));
                }
                perry_hir::Resolution::Unresolved(reason) => {
                    // #1674 sub-part B: a non-resolvable template specifier with
                    // a fixed relative-directory prefix/suffix
                    // (`import(`./plugins/${name}.ts`)`) globs the importing
                    // module's directory for matching files instead of erroring.
                    if let Some((prefix, suffix)) =
                        perry_hir::dynamic_import_glob_pattern(arg.as_ref(), &module_const_locals)
                    {
                        let matches = expand_dynamic_import_glob(
                            &source_file_path,
                            &prefix,
                            &suffix,
                            perry_hir::DYNAMIC_IMPORT_PATH_CAP,
                            synchronous,
                        );
                        if matches.entries > perry_hir::DYNAMIC_IMPORT_PATH_CAP {
                            dyn_errors.push(format!(
                                "dynamic import() in module {} ({}): glob '{}*{}' matched {} files \
                                 (limit: {})",
                                module_name,
                                canonical.display(),
                                prefix,
                                suffix,
                                matches.entries,
                                perry_hir::DYNAMIC_IMPORT_PATH_CAP
                            ));
                            return;
                        }
                        if !matches.specifiers.is_empty() {
                            for p in &matches.specifiers {
                                if !new_dyn_imports.contains(p) {
                                    new_dyn_imports.push(p.clone());
                                }
                            }
                            ctx.glob_candidate_files
                                .extend(matches.files.iter().filter_map(|f| f.canonicalize().ok()));
                            dynamic_path_sets.push(DynImportOutcome::Resolved(matches.specifiers));
                            return;
                        }
                    }
                    // #5389 Tier 2: a synchronous `require(expr)` whose specifier
                    // doesn't const-fold (and didn't glob-match above) falls back
                    // to the Tier-1 ambient createRequire-backed `require` at
                    // codegen — builtins resolve by string, unknown packages throw
                    // Node-compatible MODULE_NOT_FOUND. Leave
                    // `paths` empty (no `deferred_error`); the empty-paths +
                    // synchronous codegen arm emits the ambient require. This
                    // never participates in the strict-dynamic-import hard error.
                    if synchronous {
                        dynamic_path_sets.push(DynImportOutcome::Resolved(Vec::new()));
                        return;
                    }
                    // #5230: a genuinely runtime-computed specifier. This is the
                    // analog of #5206's runtime-unknown eval bucket. Strict mode
                    // (`--strict-dynamic-import` / `perry.dynamicImport = "error"`
                    // / `perry.strict`) restores the historical hard compile
                    // error. The default policy *defers* it: compile the site to
                    // a rejected promise that throws a descriptive Error only if
                    // reached, record it for the shared end-of-compile notice,
                    // and keep building so plugin-loader apps compile + run their
                    // core. `PERRY_ALLOW_EVAL=1` forces defer (shared AOT escape
                    // hatch).
                    if ctx.strict_dynamic_import
                        && !perry_hir::eval_classifier::eval_override_enabled()
                    {
                        dyn_errors.push(format!(
                            "dynamic import() in module {} ({}): {}",
                            module_name,
                            canonical.display(),
                            reason
                        ));
                    } else {
                        let line =
                            perry_hir::current_module_line_at(*byte_offset).filter(|&l| l != 0);
                        let loc = match line {
                            Some(l) => format!("{}:{}", source_file_path, l),
                            None => source_file_path.clone(),
                        };
                        // The message is baked into the program, so it names the
                        // remapped path; the build notice keeps the real one.
                        let baked_loc = perry_hir::source_remap::baked_path(&loc);
                        let msg = format!(
                            "dynamic import() of a runtime-computed path cannot run in an \
                             ahead-of-time compiled binary ({baked_loc})"
                        );
                        perry_hir::record_deferred_aot_site("import(...)", loc);
                        dynamic_path_sets.push(DynImportOutcome::Deferred(msg));
                    }
                }
            }
        }
    });
    let mut worker_path_sets: Vec<(Vec<String>, bool)> = Vec::new();
    let mut saw_worker_new = false;
    perry_hir::for_each_worker_new(&hir_module, &mut |expr| {
        saw_worker_new = true;
        if let perry_hir::Expr::WorkerNew {
            paths,
            filename,
            is_eval,
            ..
        } = expr
        {
            if !paths.is_empty() {
                return;
            }
            // Eval-mode Worker (`new Worker(src, { eval: true })`): the resolved
            // value is the worker SOURCE, not a path — parsed from the options
            // object at lowering (authoritative; handles single-line sources and
            // doesn't misclassify multi-line filenames).
            let eval_mode = *is_eval;
            let (resolution, partial) = if eval_mode {
                (
                    perry_hir::resolve_worker_eval_source(
                        filename.as_ref(),
                        &hir_module,
                        &module_const_locals,
                        &dynamic_param_literals,
                        &dynamic_local_literals,
                    ),
                    false,
                )
            } else {
                perry_hir::resolve_worker_path(
                    filename.as_ref(),
                    &hir_module,
                    &module_const_locals,
                    &dynamic_param_literals,
                    &dynamic_local_literals,
                )
            };
            match resolution {
                perry_hir::Resolution::Set(mut set) => {
                    if !eval_mode && set.len() > perry_hir::DYNAMIC_IMPORT_PATH_CAP {
                        dyn_errors.push(format!(
                            "worker_threads Worker in module {}: filename resolves to {} possible paths \
                             (limit: {})",
                            module_name,
                            set.len(),
                            perry_hir::DYNAMIC_IMPORT_PATH_CAP
                        ));
                        return;
                    }
                    if eval_mode && set.len() != 1 {
                        dyn_errors.push(format!(
                            "worker_threads Worker in module {}: filename must resolve to exactly one path for now, got {}",
                            module_name,
                            set.len()
                        ));
                        return;
                    }
                    if eval_mode {
                        // Eval-mode Worker (`new Worker(src, { eval: true })`):
                        // the resolved value is the worker SOURCE, not a path.
                        // Materialize it to a content-addressed temp `.js` file
                        // so the existing file-worker machinery compiles it as a
                        // module — instead of erroring "Worker target was not
                        // compiled" on a "path" that is actually source code.
                        match materialize_eval_worker_source(&set[0]) {
                            Ok(temp_path) => set[0] = temp_path,
                            Err(e) => {
                                dyn_errors.push(format!(
                                    "worker_threads eval Worker in module {}: failed to \
                                     materialize inline source: {}",
                                    module_name, e
                                ));
                                return;
                            }
                        }
                    }
                    let imports = if eval_mode {
                        set.clone()
                    } else {
                        match worker::resolve_candidates(
                            &mut set,
                            entry_path,
                            &canonical,
                            &module_name,
                            ctx,
                            format,
                        ) {
                            Ok(imports) => imports,
                            Err(error) => {
                                dyn_errors.push(error);
                                return;
                            }
                        }
                    };
                    for path in imports {
                        if !new_dyn_imports.contains(&path) {
                            new_dyn_imports.push(path);
                        }
                    }
                    worker_path_sets.push((set, partial));
                }
                perry_hir::Resolution::Unresolved(reason) => {
                    // Real-world packages (e.g. Next.js build-time worker
                    // pools) construct Workers on paths that are never hit
                    // when the compiled program runs. Warn and let codegen
                    // lower this WorkerNew to a runtime throw instead of
                    // failing the whole compile. Push an empty set to keep
                    // the fill pass aligned with resolved siblings.
                    if matches!(format, OutputFormat::Text) {
                        eprintln!(
                            "  Warning: worker_threads Worker in module {}: {} — \
                             at run time this Worker starts only if its file is a \
                             worker entry compiled into this binary, otherwise it \
                             throws ERR_WORKER_NOT_COMPILED",
                            module_name, reason
                        );
                    }
                    worker_path_sets.push((Vec::new(), false));
                }
            }
        }
    });
    // A global Web Worker has no `worker_threads` import to trigger stdlib
    // linking. WorkerNew codegen still calls the same native worker runtime,
    // so make that dependency explicit for every discovered constructor site.
    if saw_worker_new {
        ctx.needs_stdlib = true;
        ctx.native_module_imports
            .insert("worker_threads".to_string());
    }
    worker_url::record(&hir_module, entry_path, &canonical, ctx);
    drop(dynamic_local_literals);
    drop(module_const_locals);
    if !dyn_errors.is_empty() {
        perry_hir::clear_current_module_source();
        return Err(anyhow!("{}", dyn_errors.join("\n")));
    }
    let mut dynamic_path_sets = dynamic_path_sets.into_iter();
    perry_hir::for_each_dynamic_import_mut(&mut hir_module, &mut |expr| {
        if let perry_hir::Expr::DynamicImport {
            paths,
            deferred_error,
            ..
        } = expr
        {
            if paths.is_empty() && deferred_error.is_none() {
                match dynamic_path_sets.next() {
                    Some(DynImportOutcome::Resolved(set)) => *paths = set,
                    Some(DynImportOutcome::Deferred(msg)) => *deferred_error = Some(msg),
                    None => {}
                }
            }
        }
    });
    let mut worker_path_sets = worker_path_sets.into_iter();
    perry_hir::for_each_worker_new_mut(&mut hir_module, &mut |expr| {
        if let perry_hir::Expr::WorkerNew { paths, partial, .. } = expr {
            if paths.is_empty() {
                if let Some((set, incomplete)) = worker_path_sets.next() {
                    *paths = set;
                    *partial = incomplete;
                }
            }
        }
    });
    // #5230: done with the dynamic-import line resolution; don't leak this
    // module's source onto unrelated work on this (possibly rayon-worker) thread.
    perry_hir::clear_current_module_source();
    for source in new_dyn_imports {
        // A dynamic edge to the same source as a static import is folded
        // into the existing static edge: that edge already gives us full
        // namespace materialization + the eager init-order pin, both of
        // which depend on it staying `is_dynamic = false`. But the
        // dynamic `import()` dispatch still needs the source registered
        // as a dynamic-import target (#1672) — otherwise
        // `dynamic_import_path_to_prefix` has no entry for it and the
        // dispatch falls through to `js_promise_rejected(undefined)` even
        // though the module is compiled in. So mark the static edge with
        // `is_dynamic_target` instead of dropping the information.
        // A dynamic-only target appears as a new import with empty
        // specifiers and `is_dynamic = true`.
        //
        // Only a static edge that exists at run time can carry the dynamic
        // edge. `import type { X } from "./x.ts"` (`type_only`) and
        // `import { type X } from "./x.ts"` (`runtime_erased`) are erased:
        // a type-only target is never queued for compilation, and an erased
        // edge is skipped by init order and export pruning. Folding into one
        // dropped the real `import("./x.ts")` or `new Worker(new URL("./x.ts",
        // ...))` of the same file, so it gets its own dynamic edge instead.
        if let Some(existing) = hir_module.imports.iter_mut().find(|i| {
            i.source == source
                && !i.is_dynamic
                && !i.type_only
                && !i.runtime_erased
                && !native_esm_namespace::is_builtin(&source)
        }) {
            existing.is_dynamic_target = true;
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
            runtime_erased: false,
            is_dynamic: true,
            is_dynamic_target: false,
            is_deferred_require: false,
            is_adopted_require: false,
        });
    }

    // Process imports and update their resolved paths and module kinds
    for import in &mut hir_module.imports {
        // The CommonJS support module: written by the walk once the graph is
        // complete (`walk::write_cjs_support`), so it is not queued here.
        if import.source == super::cjs_wrap::support::SPECIFIER {
            import.resolved_path = Some(cjs_support_path(ctx).to_string_lossy().into_owned());
            import.module_kind = ModuleKind::NativeCompiled;
            continue;
        }
        // A native dynamic import owns the same ordinary ESM namespace as
        // a static namespace import. Keep default/named native imports native:
        // their binding values and method lowering already have the right ABI.
        if import.is_dynamic && native_esm_namespace::is_builtin(&import.source) {
            let path = native_esm_namespace::materialize(&ctx.cache_dir, &import.source)?;
            let path = path.canonicalize()?;
            import.resolved_path = Some(path.to_string_lossy().into_owned());
            import.is_native = false;
            import.module_kind = ModuleKind::NativeCompiled;
            pending.push(path);
            continue;
        }
        ensure_bunfs_import_resolves(&import.source, &canonical, ctx)?;
        // Resolve TypeScript type-only imports for metadata, but never queue
        // their target as a runtime module. The final graph may already
        // contain the target through a value import elsewhere; retaining its
        // canonical path here then lets run_pipeline attach that existing
        // class's field/method metadata to this consumer. This is compile-time
        // bookkeeping only: no init edge, binding, package capability, or V8
        // fallback is created. In particular, the `@standard-schema/spec`
        // case from #684 remains erased at runtime.
        if import.type_only {
            if let Some(alias) = ctx.package_aliases.get(import.source.as_str()).cloned() {
                import.source = alias;
                import.is_native = perry_hir::is_native_module(&import.source);
            }
            if !import.is_native {
                if let Some(resolved) = cached_resolve_import_with_lexical_base(
                    &import.source,
                    entry_path,
                    &canonical,
                    ctx,
                ) {
                    let resolved_path = resolved.canonical_path;
                    let kind = if resolved.kind == ModuleKind::Interpreted
                        && !is_in_perry_native_package(&resolved_path)
                        && !is_declaration_file(&resolved_path)
                        && aot_promotion_is_authorized(&resolved_path, ctx)
                    {
                        ModuleKind::NativeCompiled
                    } else {
                        resolved.kind
                    };
                    import.resolved_path = Some(resolved_path.to_string_lossy().to_string());
                    import.module_kind = kind;
                }
            }
            continue;
        }
        let uses_file_loader = file_loader_sources.contains(&import.source);
        let uses_text_loader = text_loader_sources.contains(&import.source);
        progress.record(ProgressSnapshot {
            stage: "resolve-import",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            import_specifier: Some(&import.source),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });

        // Apply package alias (e.g., @parse/node-apn → perry-push from perry.packageAliases)
        if let Some(alias) = ctx.package_aliases.get(import.source.as_str()).cloned() {
            import.source = alias;
            import.is_native = perry_hir::is_native_module(&import.source);
        }

        // `node:stream/web` is routed as a runtime submodule so its named
        // imports keep their singleton shape, but the implementations live in
        // perry-stdlib's `bundled-streams` module. Mark the import explicitly
        // instead of relying only on codegen-side FFI provenance, which object
        // cache hits can skip.
        if import
            .source
            .strip_prefix("node:")
            .unwrap_or(&import.source)
            == "stream/web"
        {
            ctx.needs_stdlib = true;
            ctx.native_module_imports.insert("stream/web".to_string());
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
            audit_native_binding_choice(&import.source, entry_path, &canonical, ctx)?;
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
                // On Xcode 27 the final linker uses this marker to compile
                // Perry's Swift NowPlaying MediaSession adapter. Older SDKs
                // keep using the existing MediaPlayer fallback.
                ctx.native_module_imports.insert("perry/media".to_string());
            }
            // iOS 27 adoption surface (#5536). The Rust ABI lives in
            // libperry_ui_ios.a, while Foundation Models also needs a tiny
            // Swift bridge compiled at final-link time. Keep a marker in the
            // existing import set so the linker can opt in without forcing
            // Swift/Xcode 26+ on unrelated iOS builds.
            if import.source == "perry/ios" {
                ctx.needs_ui = true;
                ctx.native_module_imports.insert("perry/ios".to_string());
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
                        // Notifications (#5283): `perry_system_notification_*`
                        // is implemented in the platform UI crates
                        // (perry-ui-windows MessageBox/toast, perry-ui-macos
                        // UNUserNotificationCenter, …), NOT in perry-stdlib. A
                        // console program that only imports `notificationSend`
                        // left the symbol with no real backing because nothing
                        // pulled the UI lib in — `notificationSend` then did
                        // nothing on Windows. Linking the UI lib for any
                        // notification* import gives it a real implementation.
                        "notificationSend"
                            | "notificationSchedule"
                            | "notificationCancel"
                            | "notificationRegisterRemote"
                            | "notificationOnReceive"
                            | "notificationOnBackgroundReceive"
                            | "notificationOnTap"
                            | "audioStart"
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
                            | "takeScreenshot"
                            | "getSafeAreaInsets"
                            | "networkGetStatus"
                            | "networkOnChange"
                            | "networkStopOnChange"
                            | "appOnOpenUrl"
                            | "appGetLaunchUrl"
                            // Haptics: perry_system_haptic_play lives in
                            // the platform UI crates (WKInterfaceDevice /
                            // UIFeedbackGenerator / VibrationEffect /
                            // NSHapticFeedbackManager), not perry-stdlib.
                            | "hapticPlay"
                            // Device identity: perry_get_device_idiom and
                            // perry_system_get_device_model also live only
                            // in the platform UI crates (UIDevice /
                            // sysctl hw.model / JNI Build.MODEL). Without
                            // this trigger, a console program importing
                            // only these fails to link ("undefined symbol:
                            // perry_get_device_idiom") — the #5972
                            // regression test had to import a perry/ui
                            // widget just to make the link succeed.
                            | "getDeviceIdiom"
                            | "getDeviceModel"
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
            // perry/audio (issue #1867) — AVAudioEngine on Apple, miniaudio
            // on Linux/Windows/Android (PR 2). The runtime symbols live in
            // libperry_ui_*.a same as perry/media, so importing any
            // perry/audio function triggers UI lib linking.
            if import.source == "perry/audio" {
                ctx.needs_ui = true;
            }
            if import.source == "perry/plugin" {
                ctx.needs_plugins = true;
            }
            if import.source == "perry/component" {
                ctx.native_module_imports.insert(import.source.clone());
            }
            if import.source == "perry/thread" {
                // perry/thread spawns OS workers and translates panics to
                // promise rejections via `catch_unwind` — auto-mode keeps
                // panic = "unwind" when this is set.
                ctx.needs_thread = true;
            }
            // Runtime-owned optional installs still need feature selection
            // when their imports do not require a stdlib dispatcher. This
            // includes Bun's backends and async-hooks observation for ALS.
            let normalized = import
                .source
                .strip_prefix("node:")
                .unwrap_or(&import.source);
            if matches!(normalized, "bun" | "async_hooks") {
                ctx.native_module_imports.insert(normalized.to_string());
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

        if script_string::resolve(ctx, &canonical, import, &mut pending)? {
            continue;
        }

        if let Some(resolved) =
            cached_resolve_import_with_lexical_base(&import.source, entry_path, &canonical, ctx)
        {
            let resolved_path = resolved.canonical_path;
            let source_path = resolved.source_path;
            if uses_text_loader {
                ctx.text_loader_asset_paths.insert(resolved_path.clone());
            }
            if uses_file_loader {
                let name = ctx
                    .file_loader_asset_names
                    .get(&resolved_path)
                    .cloned()
                    .unwrap_or_else(|| imported_file_asset_name(&resolved_path, &ctx.cache_root));
                ctx.file_loader_asset_paths.insert(resolved_path.clone());
                if !ctx
                    .embedded_assets
                    .iter()
                    .any(|(_, path)| path == &resolved_path)
                {
                    ctx.embedded_assets.push((name, resolved_path.clone()));
                }
            }
            let kind = if resolved.kind == ModuleKind::Interpreted
                && !is_in_perry_native_package(&resolved_path)
                && !is_declaration_file(&resolved_path)
                && aot_promotion_is_authorized(&resolved_path, ctx)
            {
                ctx.aot_discovered_modules.insert(resolved_path.clone());
                ModuleKind::NativeCompiled
            } else {
                resolved.kind
            };
            import.resolved_path = Some(resolved_path.to_string_lossy().to_string());
            import.module_kind = kind;
            if let Some(sidecar) =
                declaration_sidecar_for_resolved_import(&import.source, &resolved_path)
            {
                ctx.declaration_sidecars
                    .insert(resolved_path.clone(), sidecar);
            }

            match kind {
                ModuleKind::NativeCompiled => {
                    // Record every resolved compile-package root. Package
                    // identity is the canonical root, not the package name:
                    // nested versions remain distinct while two symlinks to
                    // the same physical copy canonicalize together.
                    let module_name = &import.source;
                    if !module_name.starts_with('.') && !module_name.starts_with('/') {
                        let (pkg_name, _) = parse_package_specifier(module_name);
                        if ctx.compile_packages.contains(&pkg_name) {
                            if let Some(pkg_dir) =
                                extract_compile_package_dir(&resolved_path, &pkg_name)
                            {
                                ctx.compile_package_dirs.insert(pkg_dir);
                            } else {
                                // Symlinked local package: canonical path is outside node_modules.
                                // Walk up from resolved_path to find the package root (dir with package.json).
                                let mut dir = resolved_path.parent();
                                while let Some(d) = dir {
                                    if d.join("package.json").exists() {
                                        ctx.compile_package_dirs.insert(d.to_path_buf());
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
                                    parse_native_library_manifest(dir, module_name, target)?
                                {
                                    // #466 Phase 2: refuse to load a wrapper whose
                                    // declared abiVersion is incompatible with the
                                    // bundled perry-ffi. Missing field warns but
                                    // continues during the v0.5.x cycle.
                                    if let Err(msg) =
                                        super::resolve::validate_abi_version(&manifest)
                                    {
                                        return Err(anyhow::anyhow!(
                                            "{}\n  in module: {}\n  import: {}",
                                            msg,
                                            canonical.display(),
                                            import.source
                                        ));
                                    }
                                    // #497: gate transitive
                                    // `perry.nativeLibrary` linkage on
                                    // host-controlled allowlist. The
                                    // package declared the manifest
                                    // itself; the host must
                                    // explicitly allow it.
                                    if !super::allowlist_matches(
                                        &manifest.module,
                                        &ctx.allow_native_library,
                                    ) {
                                        anyhow::bail!(
                                            "package `{}` declares `perry.nativeLibrary` \
                                             (links arbitrary native code into the binary) \
                                             but is not in your host \
                                             `perry.allow.nativeLibrary`. Review the package, \
                                             then add it to your host `package.json`:\n\
                                             \n\
                                               {{\n\
                                                 \"perry\": {{\n\
                                                   \"allow\": {{ \"nativeLibrary\": [\"{}\"] }}\n\
                                                 }}\n\
                                               }}\n\
                                             \n\
                                             Scope wildcard (`\"@scope/*\"`) and the universal \
                                             `\"*\"` escape hatch are both supported.\n\
                                             \n\
                                             For a one-off build, set \
                                             `PERRY_ALLOW_PERRY_FEATURES=1` in the environment. \
                                             (#497)\n\
                                             \n\
                                             Caused by import `{}` in module `{}`.",
                                            manifest.module,
                                            manifest.module,
                                            import.source,
                                            canonical.display(),
                                        );
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
                    pending.push(source_path);
                }
                ModuleKind::Interpreted => {
                    // Perry native extension packages (ethers, ws, mysql2, dotenv)
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
                                    parse_native_library_manifest(dir, module_name, target)?
                                {
                                    // #466 Phase 2: refuse to load a wrapper whose
                                    // declared abiVersion is incompatible with the
                                    // bundled perry-ffi. Missing field warns but
                                    // continues during the v0.5.x cycle.
                                    if let Err(msg) =
                                        super::resolve::validate_abi_version(&manifest)
                                    {
                                        return Err(anyhow::anyhow!(
                                            "{}\n  in module: {}\n  import: {}",
                                            msg,
                                            canonical.display(),
                                            module_name
                                        ));
                                    }
                                    // #497: gate transitive
                                    // `perry.nativeLibrary` linkage on
                                    // host-controlled allowlist. The
                                    // package declared the manifest
                                    // itself; the host must
                                    // explicitly allow it.
                                    if !super::allowlist_matches(
                                        &manifest.module,
                                        &ctx.allow_native_library,
                                    ) {
                                        anyhow::bail!(
                                            "package `{}` declares `perry.nativeLibrary` \
                                             (links arbitrary native code into the binary) \
                                             but is not in your host \
                                             `perry.allow.nativeLibrary`. Review the package, \
                                             then add it to your host `package.json`:\n\
                                             \n\
                                               {{\n\
                                                 \"perry\": {{\n\
                                                   \"allow\": {{ \"nativeLibrary\": [\"{}\"] }}\n\
                                                 }}\n\
                                               }}\n\
                                             \n\
                                             Scope wildcard (`\"@scope/*\"`) and the universal \
                                             `\"*\"` escape hatch are both supported.\n\
                                             \n\
                                             For a one-off build, set \
                                             `PERRY_ALLOW_PERRY_FEATURES=1` in the environment. \
                                             (#497)\n\
                                             \n\
                                             Caused by import `{}` in module `{}`.",
                                            manifest.module,
                                            manifest.module,
                                            module_name,
                                            canonical.display(),
                                        );
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

                    pending.push(source_path);
                }
                ModuleKind::NativeRust => {
                    // Native Rust modules are handled by stdlib
                }
            }
        } else {
            // Could not resolve - might be a Node.js builtin or missing module
            // Generated inputs must fail at collection time. Continuing with
            // an empty binding makes a stale/missing preparation step look
            // like a runtime application bug (OpenCode's injected
            // `opencode-web-ui.gen.ts` was the motivating case).
            if looks_like_generated_module(&import.source) {
                return Err(anyhow::anyhow!(
                    "Could not resolve generated module `{source}` imported by {filename} ({path}).\n\
                     Perry will not compile with a missing or stale generated input. Run the upstream preparation command first.\n\
                     If this module is a directory-to-file map, retry with:\n  \
                       --asset-module '{source}=<asset-directory>'\n\
                     For other generators, declare the command in package.json under `perry.codegen` and ensure it writes this module.",
                    source = import.source,
                    filename = filename,
                    path = canonical.display(),
                ));
            }
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
                if let Some(attempted_path) =
                    super::resolve::attempted_relative_import_path(&import.source, entry_path)
                {
                    return Err(anyhow::anyhow!(
                        "Could not resolve relative namespace import `import * as ... from \"{source}\"` in {filename} ({path}).\n\
                         The module file was not found at the path Perry tried:\n  \
                           {attempted_path}\n\
                         Check the import path, or ensure the build step that generates or stages this file runs before compiling.",
                        source = import.source,
                        filename = filename,
                        path = canonical.display(),
                        attempted_path = attempted_path.display(),
                    ));
                }
                return Err(anyhow::anyhow!(
                    "Could not resolve namespace import `import * as ... from \"{source}\"` in {filename} ({path}).\n\
                     Perry has no stdlib bindings for this module path, so the namespace would compile to an empty object \
                     — every method call on it would silently no-op at runtime. Pick one:\n  \
                       • switch to named imports: `import {{ foo }} from \"{source}\"` (still resolves through whatever backing exists, but fails fast at the actual missing binding),\n  \
                       • remove the import if it's unused,\n  \
                       • or add the module to perry-stdlib / perry-ext-* / perry.compilePackages.",
                    source = import.source,
                    filename = filename,
                    path = canonical.display(),
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

    // Deferred require (#10437, originally the Next.js lazy-require case): the
    // CJS→ESM wrap names a binding `_lazyreq_N` when every `require('S')` call
    // site is NOT guaranteed to run the moment the module loads — inside a
    // function body (lazy in Node: jsonwebtoken, required only inside Next.js's
    // request handlers), or inside a top-level control-flow block / braceless
    // equivalent that may never run (`if (forceNative) { require('./native') }`,
    // pg's optional native binding). Tag the import so `classify_eager_modules`
    // leaves the target Deferred — matching Node, which only loads such a
    // module when control flow actually reaches the call. The require shim
    // triggers the target's `__init` at that same call site, so an over-eager
    // classification is self-correcting at runtime (it just runs a bit early).
    // Limited to Perry-compiled (`NativeCompiled`) targets — native stdlib /
    // V8 modules have their own init paths.
    {
        let local_of = |s: &perry_hir::ImportSpecifier| match s {
            perry_hir::ImportSpecifier::Default { local } => local.clone(),
            perry_hir::ImportSpecifier::Namespace { local } => local.clone(),
            perry_hir::ImportSpecifier::Named { local, .. } => local.clone(),
        };
        // A static CommonJS edge (design CJS_LINK §2.2) imports its target's
        // record binding as `__perry_cjs_rec_N`: the target evaluates when an
        // edge first runs, so no import of it is an init-order edge.
        let edge_sources: HashSet<String> = if was_cjs_wrapped {
            hir_module
                .imports
                .iter()
                .filter(|i| {
                    i.specifiers
                        .iter()
                        .any(|s| local_of(s).starts_with("__perry_cjs_rec_"))
                })
                .map(|i| i.source.clone())
                .collect()
        } else {
            HashSet::new()
        };
        for import in &mut hir_module.imports {
            if import.type_only
                || import.is_dynamic
                || import.is_native
                || import.module_kind != perry_hir::ModuleKind::NativeCompiled
                || import.source == super::cjs_wrap::support::SPECIFIER
            {
                continue;
            }
            let is_lazy = edge_sources.contains(&import.source)
                || import
                    .specifiers
                    .iter()
                    .any(|s| local_of(s).starts_with("_lazyreq_"));
            if is_lazy {
                import.is_deferred_require = true;
            }
            if was_cjs_wrapped {
                // #5257: wrapped `require('S')` imports follow CommonJS
                // default/namespace interop rather than the static-ESM gate.
                import.is_adopted_require = true;
            }
        }
    }

    ctx.uses_dns |= hir_module.imports.iter().any(|i| is_dns(&i.source));
    ctx.reexport_pruner.imports(&hir_module.imports);

    // Process re-exports
    for (export_index, export) in hir_module.exports.iter().enumerate() {
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
            ensure_bunfs_import_resolves(src, &canonical, ctx)?;
            progress.record(ProgressSnapshot {
                stage: "resolve-re-export",
                module_path: Some(&canonical),
                module_name: Some(&module_name),
                import_specifier: Some(src),
                visited: Some(visited.len()),
                collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
                ..Default::default()
            });
            if let Some(resolved) =
                cached_resolve_import_with_lexical_base(src.as_str(), entry_path, &canonical, ctx)
            {
                let resolved_path = resolved.canonical_path;
                let source_path = resolved.source_path;
                let kind = if resolved.kind == ModuleKind::Interpreted
                    && !is_in_perry_native_package(&resolved_path)
                    && !is_declaration_file(&resolved_path)
                    && aot_promotion_is_authorized(&resolved_path, ctx)
                {
                    ctx.aot_discovered_modules.insert(resolved_path.clone());
                    ModuleKind::NativeCompiled
                } else {
                    resolved.kind
                };
                if let Some(sidecar) =
                    declaration_sidecar_for_resolved_import(src.as_str(), &resolved_path)
                {
                    ctx.declaration_sidecars
                        .insert(resolved_path.clone(), sidecar);
                }
                // #1110: a re-export from a `perry.nativeLibrary` package
                // (`export { foo } from "@perryts/storekit"`) is the only
                // path through which storekit's manifest reaches the
                // module graph — the import-walk above never visited it
                // because SWC's re-export lowering doesn't synthesize an
                // entry in `hir.imports`. Without the manifest in
                // `ctx.native_libraries`, every downstream module's
                // `opts.native_library_functions` is empty and the FFI
                // dispatch path in `lower_call.rs` falls through to
                // `perry_fn_<wrap>__<name>` (the wrapper symbol the
                // re-exporting module never emits), leading to an LLVM
                // verifier failure (`use of undefined value @<fn>`) on
                // any indirect call. Mirror the per-kind manifest
                // collection from the import-walk so the FFI surface
                // remains visible through any depth of re-export chain.
                let src_str = src.clone();
                if !src_str.starts_with('.')
                    && !src_str.starts_with('/')
                    && !ctx.native_libraries.iter().any(|nl| nl.module == src_str)
                {
                    let mut pkg_dir = resolved_path.parent();
                    while let Some(dir) = pkg_dir {
                        if dir.join("package.json").exists() && has_perry_native_library(dir) {
                            if let Some(manifest) =
                                parse_native_library_manifest(dir, &src_str, target)?
                            {
                                if let Err(msg) = super::resolve::validate_abi_version(&manifest) {
                                    return Err(anyhow::anyhow!(
                                        "{}\n  in module: {}\n  re-export: {}",
                                        msg,
                                        canonical.display(),
                                        src_str
                                    ));
                                }
                                if !super::allowlist_matches(
                                    &manifest.module,
                                    &ctx.allow_native_library,
                                ) {
                                    anyhow::bail!(
                                        "package `{}` declares `perry.nativeLibrary` \
                                         (links arbitrary native code into the binary) \
                                         but is not in your host \
                                         `perry.allow.nativeLibrary`. Review the package, \
                                         then add it to your host `package.json`:\n\
                                         \n\
                                           {{\n\
                                             \"perry\": {{\n\
                                               \"allow\": {{ \"nativeLibrary\": [\"{}\"] }}\n\
                                             }}\n\
                                           }}\n\
                                         \n\
                                         Scope wildcard (`\"@scope/*\"`) and the universal \
                                         `\"*\"` escape hatch are both supported.\n\
                                         \n\
                                         For a one-off build, set \
                                         `PERRY_ALLOW_PERRY_FEATURES=1` in the environment. \
                                         (#497)\n\
                                         \n\
                                         Caused by re-export `{}` in module `{}`.",
                                        manifest.module,
                                        manifest.module,
                                        src_str,
                                        canonical.display(),
                                    );
                                }
                                match format {
                                    OutputFormat::Text => println!(
                                        "  Native library: {} ({} FFI functions, via re-export)",
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

                match kind {
                    ModuleKind::NativeCompiled => {
                        if reexport_prune::record(
                            ctx,
                            &canonical,
                            entry_path,
                            export_index,
                            export,
                            &resolved_path,
                            &source_path,
                        ) {
                            pending.push(source_path);
                        }
                    }
                    ModuleKind::Interpreted => {
                        // JS runtime (V8) support was removed, so interpreted
                        // node_modules dependencies are not followed. A direct
                        // `.js` import is caught by the `should_use_js_runtime`
                        // gate at the top of `collect_modules` and surfaced as
                        // a hard error after collection completes.
                    }
                    ModuleKind::NativeRust => {}
                }
            } else if let Some(attempted_path) =
                super::resolve::attempted_relative_import_path(src, entry_path)
                    .filter(|p| !has_declaration_file(p))
            {
                // A relative re-export whose file is missing can never bind
                // (Node: ERR_MODULE_NOT_FOUND). Dropping the edge left every
                // name it should have provided as an undefined symbol at link
                // time, attributed to the re-exporting module.
                let hint = if looks_like_generated_module(src) {
                    "This looks like a generated module: run the upstream preparation step first."
                } else {
                    "Check the path, or ensure the build step that generates this file runs before compiling."
                };
                return Err(anyhow::anyhow!(
                    "Could not resolve re-export `from \"{src}\"` in {filename} ({path}).\n\
                     The module file was not found at the path Perry tried:\n  \
                       {attempted_path}\n\
                     {hint}",
                    filename = filename,
                    path = canonical.display(),
                    attempted_path = attempted_path.display(),
                ));
            }
        }
    }

    Ok(ModuleDiscovery {
        finish: Some(PreparedModule {
            canonical,
            module_name,
            hir_module,
        }),
        children: pending,
    })
}

/// A re-export whose only target is a `.d.ts` (`./types` or `./types.js` next
/// to `types.d.ts`) is type-only: it has no runtime module to resolve.
fn has_declaration_file(attempted: &Path) -> bool {
    let text = attempted.to_string_lossy();
    let stem = ["js", "mjs", "cjs", "ts", "mts", "cts"]
        .iter()
        .find_map(|ext| text.strip_suffix(&format!(".{ext}")))
        .unwrap_or(&text);
    [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|ext| Path::new(&format!("{stem}{ext}")).is_file())
}

mod finish;
pub(crate) use finish::collect_module_finish;

#[cfg(test)]
mod node_specifier_literal_tests {
    use super::node_specifier_literals;

    #[test]
    fn finds_quoted_node_specifiers_only() {
        let src = r#"const m = { zlib: "node:zlib", fsp: 'node:fs/promises', os: `node:os` };
            // node:path in a comment, "node:bad-name!" and `node:${x}` are not specifiers
            getBuiltinModule("node:crypto");"#;
        assert_eq!(
            node_specifier_literals(src),
            ["node:zlib", "node:fs/promises", "node:os", "node:crypto"]
        );
    }
}
