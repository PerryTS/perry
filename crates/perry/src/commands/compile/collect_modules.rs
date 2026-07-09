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
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::commands::progress::{ProgressSnapshot, VerboseProgress};
use crate::OutputFormat;

use super::{
    cached_resolve_import, declaration_sidecar_for_resolved_import, extract_compile_package_dir,
    has_perry_native_library, is_declaration_file, is_in_compile_package,
    is_in_perry_native_package, is_js_file, is_recognized_text_asset, parse_cached,
    parse_native_library_manifest, parse_package_specifier, CompilationContext, JsModule,
    ParseCache,
};

mod crypto_ns;
mod dynamic_glob;
mod eval_worker;
mod feature_detect;
mod import_helpers;
mod native_addon;
mod parse_error;
mod static_require_transform;
#[cfg(test)]
mod tests;
mod wasm_asset;

use dynamic_glob::expand_dynamic_import_glob;
use eval_worker::materialize_eval_worker_source;
use import_helpers::{
    cached_resolve_import_with_lexical_base, collect_js_module_imports, env_defines_for_lowering,
};
// Re-exported at `pub(super)` because `compile.rs` (the parent module) calls
// `collect_modules::known_node_submodule_key` directly.
pub(super) use import_helpers::known_node_submodule_key;
use native_addon::refuse_compile_package_native_addon;
use parse_error::annotate_parse_error;
use static_require_transform::transform_static_literal_requires;
use wasm_asset::{is_wasm_asset, synthesize_wasm_stub_module};

const MAX_CROSS_MODULE_INLINE_PRIOR_MODULES: usize = 128;

/// Next.js wall 54 (part 2): recursively gather every `*.js` file under `dir`
/// (page/route loaders + turbopack chunks). Symlinks are not followed; errors
/// reading a subdirectory are skipped silently (best-effort discovery).
fn collect_js_files_recursive(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_js_files_recursive(&path, out);
        } else if file_type.is_file() && path.extension().and_then(|e| e.to_str()) == Some("js") {
            out.push(path);
        }
    }
}

/// Next.js wall 54 (part 2): true for a module discovered under a standalone
/// bundle's `.next/server/**` tree (page/route/chunk modules loaded by a
/// runtime-computed path). Matched by the `.next` then `server` path-component
/// sequence so it never false-matches a user file merely named `next` or a
/// `node_modules/.next-*` package. Used by init classification (these modules
/// must be eager so they self-register under their path at startup) and topo
/// ordering (chunks before the page loaders that `R.c()` them).
pub(super) fn is_nextjs_runtime_module(path: &std::path::Path) -> bool {
    let comps: Vec<&std::ffi::OsStr> = path.components().map(|c| c.as_os_str()).collect();
    comps
        .windows(2)
        .any(|w| w[0] == std::ffi::OsStr::new(".next") && w[1] == std::ffi::OsStr::new("server"))
}

/// Collect all modules to compile (transitive closure of imports)
pub(super) fn collect_modules(
    entry_path: &PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    progress: &VerboseProgress,
    mut parse_cache: Option<&mut ParseCache>,
) -> Result<()> {
    let mut states: HashMap<PathBuf, VisitState> = HashMap::new();
    let mut stack = vec![WorkFrame::Enter(entry_path.clone())];
    // Next.js wall 54 (part 2): a standalone `server.js` loads its page, route,
    // and turbopack chunk modules from `<entry_dir>/.next/server/**` by a path
    // computed at request time (`require(getPagePath(...))`, turbopack
    // `R.c("chunkpath")`) — never via a static `import`/`require` literal — so
    // the import walk below never reaches them and they would not be AOT
    // compiled. Seed every `.next/server/**/*.js` file as an additional root so
    // each compiles natively and self-registers under its absolute path (see
    // cjs_wrap `__perry_register_path_module`), letting the runtime
    // `require(absolutePath)` resolve it. Detected only when the entry sits next
    // to a `.next/server` directory (a Next.js standalone bundle).
    if let Some(entry_dir) = entry_path.parent() {
        let next_server_dir = entry_dir.join(".next").join("server");
        if next_server_dir.is_dir() {
            let mut next_js_files = Vec::new();
            collect_js_files_recursive(&next_server_dir, &mut next_js_files);
            if !next_js_files.is_empty() {
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "Next.js standalone: discovered {} runtime module(s) under {}",
                        next_js_files.len(),
                        next_server_dir.display()
                    );
                }
                // Push after the entry so the entry is processed first; order
                // among the discovered files does not matter (the walk dedups).
                for f in next_js_files {
                    stack.push(WorkFrame::Enter(f));
                }
            }
        }
    }
    while let Some(frame) = stack.pop() {
        match frame {
            WorkFrame::Enter(next_path) => {
                let canonical = next_path.canonicalize().map_err(|e| {
                    anyhow!("Failed to canonicalize {}: {}", next_path.display(), e)
                })?;

                if matches!(
                    states.get(&canonical),
                    Some(VisitState::InProgress | VisitState::Done)
                ) {
                    continue;
                }
                if visited.contains(&canonical) {
                    states.insert(canonical, VisitState::Done);
                    continue;
                }

                states.insert(canonical.clone(), VisitState::InProgress);
                visited.insert(canonical.clone());
                progress.record(ProgressSnapshot {
                    stage: "collect-module",
                    module_path: Some(&canonical),
                    visited: Some(visited.len()),
                    collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
                    ..Default::default()
                });

                let discovered = collect_module_one(
                    &next_path,
                    canonical.clone(),
                    ctx,
                    visited,
                    format,
                    target,
                    next_class_id,
                    progress,
                    parse_cache.as_deref_mut(),
                )?;

                if let Some(prepared) = discovered.finish {
                    stack.push(WorkFrame::Finish(prepared));
                } else {
                    states.insert(canonical, VisitState::Done);
                }
                for child in discovered.children.into_iter().rev() {
                    stack.push(WorkFrame::Enter(child));
                }
            }
            WorkFrame::Finish(prepared) => {
                let canonical = prepared.canonical.clone();
                collect_module_finish(prepared, ctx, visited, target, skip_transforms, progress)?;
                states.insert(canonical, VisitState::Done);
            }
        }
    }
    Ok(())
}

enum VisitState {
    InProgress,
    Done,
}

enum WorkFrame {
    Enter(PathBuf),
    Finish(PreparedModule),
}

struct ModuleDiscovery {
    finish: Option<PreparedModule>,
    children: Vec<PathBuf>,
}

struct PreparedModule {
    canonical: PathBuf,
    module_name: String,
    hir_module: perry_hir::Module,
}

#[allow(clippy::too_many_arguments)]
fn collect_module_one(
    entry_path: &PathBuf,
    canonical: PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    progress: &VerboseProgress,
    mut parse_cache: Option<&mut ParseCache>,
) -> Result<ModuleDiscovery> {
    let mut pending = Vec::new();

    // Check if this file should be handled by JS runtime instead of native compilation
    // This includes: JS files, declaration files (.d.ts), JSON files, or any file in node_modules when JS runtime is enabled
    let is_json = canonical.extension().and_then(|e| e.to_str()) == Some("json");
    // #5223: text-asset imports (`import s from "./x.txt"`). A recognized text
    // extension is read verbatim and synthesized into a native module whose
    // default export is the file contents as a JS string (see the text branch
    // below, mirroring the JSON-module path). `.wasm` is out of scope.
    let is_text_asset = is_recognized_text_asset(&canonical);
    // #5235: `.wasm` ESM import. The file is binary (not valid UTF-8), so it
    // must NOT be read as a string. We read the bytes, parse the export section,
    // and synthesize a throwing-stub module (see the wasm branch below). Real
    // `.wasm` ESM instantiation is the companion issue #5234.
    let is_wasm = is_wasm_asset(&canonical);
    // Match a real `node_modules/` directory COMPONENT, not a substring: a
    // file whose NAME contains "node_modules" (e.g. turbopack's bundled chunks
    // `.next/server/chunks/ssr/node_modules_next_dist_…._.js`) is NOT in
    // node_modules and must compile natively, not get force-routed to the
    // (removed) JS runtime. (Next.js wall 54.)
    let is_in_node_modules = canonical
        .components()
        .any(|c| c.as_os_str() == "node_modules");
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
    // #668 / node-core (#800): a *user* `.js`/`.cjs`/`.mjs` file (entry or a
    // project source, i.e. NOT under node_modules) is fed through the native
    // AOT pipeline like a `.ts` file rather than treated as a JS module. The
    // extension is not the signal — content is: most plain `.js` is valid
    // TypeScript-subset, and CommonJS shapes (`require(...)`, `module.exports`)
    // are rewritten to ESM by `cjs_wrap` just below, the same translation
    // already trusted for `compilePackages` targets. node_modules `.js` keeps
    // the JS-module classification (post-#1696 there is no V8 fallback, so it
    // surfaces as an unsupported-module error rather than silently running).
    let should_use_js_runtime =
        (is_js_file(&canonical) && !is_in_compiled_pkg && is_in_node_modules)
            || is_declaration_file(&canonical);

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

        // Perry native extension packages (ioredis, ethers, mysql2, ws, dotenv) are handled
        // entirely by Perry's built-in stdlib — they must NOT be loaded into V8.
        if is_perry_native {
            return Ok(ModuleDiscovery {
                finish: None,
                children: pending,
            });
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;
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

    // #5235: `.wasm` ESM import — defer. Read the BYTES (never as UTF-8; the
    // file is binary), parse the WebAssembly export section, and synthesize a
    // TypeScript stub module whose exports are throw-on-call functions. Strict
    // mode makes it a hard error; the default policy defers it (records the
    // shared end-of-compile notice and keeps building) so a build with a
    // peripheral `.wasm` dep compiles + runs its core — the wasm feature throws
    // only if reached. Real `.wasm` ESM instantiation is the companion #5234.
    //
    // The synthesized source flows through the exact same parse/lower/codegen
    // pipeline as the #5223 text-asset and JSON synthetic modules below — we
    // just feed `raw_source` from the stub instead of reading the file as text.
    let raw_source = if is_wasm {
        let display_name = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("module.wasm");
        let loc = canonical.to_string_lossy().to_string();
        // Strict mode (broad `perry.strict` / `--strict-dynamic-import` /
        // `perry.dynamicImport = "error"`) turns the deferred `.wasm` import into
        // a hard compile error. `PERRY_ALLOW_EVAL=1` forces defer (shared AOT
        // escape hatch), mirroring the dynamic-import deferral (#5230).
        if ctx.strict_dynamic_import && !perry_hir::eval_classifier::eval_override_enabled() {
            return Err(anyhow!(
                ".wasm import {} cannot run in an ahead-of-time compiled binary \
                 — full .wasm ESM instantiation is tracked in #5234 (strict mode)",
                loc
            ));
        }
        let bytes = fs::read(&canonical)
            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;
        let stub = synthesize_wasm_stub_module(&bytes, display_name);
        perry_hir::record_deferred_aot_site(".wasm import", loc);
        stub.source
    } else {
        // It's a TypeScript (or synthetic JSON/text) file to compile natively.
        fs::read_to_string(&canonical)
            .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?
    };
    // JSON module import: turn the data file into a native ESM module whose
    // default export is the parsed value. JSON is a syntactic subset of a JS
    // expression, so `export default <json>;` parses and lowers like any other
    // module. Validate as JSON first so a malformed file yields a clear error
    // rather than a confusing TS parse failure on the synthesized source.
    let raw_source = if is_json {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&raw_source) {
            return Err(anyhow!(
                "Failed to parse JSON module {}: {}",
                canonical.display(),
                e
            ));
        }
        format!("export default {};\n", raw_source.trim())
    } else if is_text_asset {
        // #5223: text-asset import. The file's contents are exposed verbatim as
        // the module's default export (a JS string). We never TS-parse the raw
        // text — instead we synthesize `export default "<escaped-contents>";`.
        // `serde_json::to_string` of a string produces a valid double-quoted JS
        // string literal with all required escaping (newlines, quotes, control
        // chars, unicode), so the contents round-trip byte-for-byte.
        let literal = serde_json::to_string(&raw_source).map_err(|e| {
            anyhow!(
                "Failed to encode text asset {} as a string literal: {}",
                canonical.display(),
                e
            )
        })?;
        format!("export default {};\n", literal)
    } else {
        raw_source
    };
    if is_in_compiled_pkg {
        refuse_compile_package_native_addon(ctx, &canonical)?;
    }

    // Issue #348: when a `compilePackages` target ships CommonJS (e.g. React
    // 18's `module.exports = require('./cjs/react.production.min.js')`),
    // rewrite the source as ESM before SWC parses it. Only fires for files
    // inside a `compilePackages` target — user TypeScript and ESM-shaped
    // packages skip the wrap. See `cjs_wrap.rs` for the wrap shape.
    // Fire for `compilePackages` targets (the original #348 case) AND for any
    // user file outside node_modules (#668 / #800): a user `.js` or `.ts`
    // written in CommonJS — `require(...)` / `module.exports` with no
    // top-level ESM — is rewritten to ESM here so `require("literal")` lands
    // as a static namespace import and flows through the normal resolution +
    // init-order + codegen path. A file that already has top-level
    // `import`/`export` is not CommonJS (`is_commonjs` returns false) and is
    // left untouched.
    let was_cjs_wrapped =
        (is_in_compiled_pkg || !is_in_node_modules) && super::cjs_wrap::is_commonjs(&raw_source);
    // #5247: when `--debug-symbols` is on, capture where the original module
    // body lands inside the wrapped output so source-location resolution can
    // map a wrapped-coordinate byte offset back to an original-source line.
    // `None` unless we both wrapped this module AND debug symbols are on, so
    // the default build does no extra work.
    let mut cjs_wrap_body_prefix_lines: Option<u32> = None;
    let source = if was_cjs_wrapped {
        if ctx.debug_symbols {
            let (wrapped, body_off) =
                super::cjs_wrap::wrap_commonjs_with_body_offset(&raw_source, &canonical, target);
            // Newlines before the original body in the wrapped output = the
            // wrapper prefix line count. Recorded only when the body was
            // located; otherwise we skip the skew correction (graceful
            // degrade to the uncorrected line rather than a wrong one).
            cjs_wrap_body_prefix_lines = body_off.map(|off| {
                wrapped.as_bytes()[..off]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count() as u32
            });
            wrapped
        } else {
            super::cjs_wrap::wrap_commonjs_for_target(&raw_source, &canonical, target)
        }
    } else {
        raw_source
    };
    // #5247: the static-require transform may prepend `import * as` lines,
    // shifting BOTH the prefix and the body down by the same number of lines.
    // Capture the wrapped line count, run the transform, then add the line
    // delta to the prefix so the wrapped-line → original-line subtraction is
    // computed against the FINAL parsed source.
    let lines_before_transform = source.bytes().filter(|&b| b == b'\n').count();
    let source = transform_static_literal_requires(&source, &ctx.compile_packages);
    if was_cjs_wrapped && ctx.debug_symbols {
        if let Some(prefix_lines) = cjs_wrap_body_prefix_lines {
            let lines_after_transform = source.bytes().filter(|&b| b == b'\n').count();
            let added_lines = lines_after_transform.saturating_sub(lines_before_transform) as u32;
            ctx.cjs_wrap_debug_sources.insert(
                canonical.clone(),
                super::types::CjsWrapDebugSource {
                    wrapped_source: source.clone(),
                    prefix_line_count: prefix_lines + added_lines,
                },
            );
        }
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
    progress.record(ProgressSnapshot {
        stage: "parse",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
        ..Default::default()
    });
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
    let lower_result = perry_hir::lower_module_full(
        ast_module,
        &module_name,
        &source_file_path,
        *next_class_id,
        resolved_types,
        imported_class_fields,
        imported_class_accessors,
        is_entry_module,
        is_external_module,
    );
    progress.heartbeat(ProgressSnapshot {
        stage: "lower",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
        ..Default::default()
    });
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
    *next_class_id = new_next_class_id; // Update the global class_id counter

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
            byte_offset,
            synchronous,
            ..
        } = expr
        {
            let synchronous = *synchronous;
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
                        );
                        if matches.len() > perry_hir::DYNAMIC_IMPORT_PATH_CAP {
                            dyn_errors.push(format!(
                                "dynamic import() in module {} ({}): glob '{}*{}' matched {} files \
                                 (limit: {})",
                                module_name,
                                canonical.display(),
                                prefix,
                                suffix,
                                matches.len(),
                                perry_hir::DYNAMIC_IMPORT_PATH_CAP
                            ));
                            return;
                        }
                        if !matches.is_empty() {
                            for p in &matches {
                                if !new_dyn_imports.contains(p) {
                                    new_dyn_imports.push(p.clone());
                                }
                            }
                            dynamic_path_sets.push(DynImportOutcome::Resolved(matches));
                            return;
                        }
                    }
                    // #5389 Tier 2: a synchronous `require(expr)` whose specifier
                    // doesn't const-fold (and didn't glob-match above) falls back
                    // to the Tier-1 ambient createRequire-backed `require` at
                    // codegen — builtins resolve by string, unknown packages throw
                    // the descriptive ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE. Leave
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
                        let msg = format!(
                            "dynamic import() of a runtime-computed path cannot run in an \
                             ahead-of-time compiled binary ({loc})"
                        );
                        perry_hir::record_deferred_aot_site("import(...)", loc);
                        dynamic_path_sets.push(DynImportOutcome::Deferred(msg));
                    }
                }
            }
        }
    });
    let mut worker_path_sets: Vec<Vec<String>> = Vec::new();
    perry_hir::for_each_worker_new(&hir_module, &mut |expr| {
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
            let mut visiting: std::collections::HashSet<u32> = std::collections::HashSet::new();
            match perry_hir::resolve_import_path_with_context(
                filename.as_ref(),
                &module_const_locals,
                &dynamic_param_literals,
                &dynamic_local_literals,
                &mut visiting,
            ) {
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
                    if set.len() != 1 {
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
                    for p in &set {
                        if !new_dyn_imports.contains(p) {
                            new_dyn_imports.push(p.clone());
                        }
                    }
                    worker_path_sets.push(set);
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
                             this Worker will throw if constructed at runtime",
                            module_name, reason
                        );
                    }
                    worker_path_sets.push(Vec::new());
                }
            }
        }
    });
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
        if let perry_hir::Expr::WorkerNew { paths, .. } = expr {
            if paths.is_empty() {
                if let Some(set) = worker_path_sets.next() {
                    *paths = set;
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
        if let Some(existing) = hir_module
            .imports
            .iter_mut()
            .find(|i| i.source == source && !i.is_dynamic)
        {
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
            is_dynamic: true,
            is_dynamic_target: false,
            is_deferred_require: false,
            is_adopted_require: false,
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

        // `node:fs/promises` itself is runtime-backed, but
        // FileHandle.readableWebStream() needs perry-stdlib's Web Streams
        // factory even when user code never imports `node:stream/web`.
        if import
            .source
            .strip_prefix("node:")
            .unwrap_or(&import.source)
            == "fs/promises"
        {
            ctx.needs_stdlib = true;
            ctx.native_module_imports.insert("fs/promises".to_string());
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

        if let Some(resolved) =
            cached_resolve_import_with_lexical_base(&import.source, entry_path, &canonical, ctx)
        {
            let resolved_path = resolved.canonical_path;
            let source_path = resolved.source_path;
            let kind = resolved.kind;
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

    // Next.js lazy-require: the CJS→ESM wrap names a binding `_lazyreq_N` when
    // every `require('S')` call site is inside a function body (lazy in Node).
    // Tag the import so `classify_eager_modules` leaves the target Deferred —
    // matching Node, which only loads such a module when the enclosing function
    // runs (e.g. jsonwebtoken, required only inside Next.js's request handlers).
    // The require shim triggers the target's `__init` on first `require()`, so
    // an over-eager classification is self-correcting at runtime. Limited to
    // Perry-compiled (`NativeCompiled`) targets — native stdlib / V8 modules
    // have their own init paths.
    if was_cjs_wrapped {
        for import in &mut hir_module.imports {
            if import.type_only
                || import.is_dynamic
                || import.is_native
                || import.module_kind != perry_hir::ModuleKind::NativeCompiled
            {
                continue;
            }
            let is_lazy = import.specifiers.iter().any(|s| {
                let local = match s {
                    perry_hir::ImportSpecifier::Default { local } => local,
                    perry_hir::ImportSpecifier::Namespace { local } => local,
                    perry_hir::ImportSpecifier::Named { local, .. } => local,
                };
                local.starts_with("_lazyreq_")
            });
            if is_lazy {
                import.is_deferred_require = true;
            }
            // #5257: every import here was synthesized from a `require('S')`,
            // which under CJS returns the exports object — so a no-`default`
            // target must route through the namespace machinery (#4872), not
            // trip the static-ESM default gate. Tag so the gate skips them.
            import.is_adopted_require = true;
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
                let kind = resolved.kind;
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
                    ModuleKind::NativeCompiled => pending.push(source_path),
                    ModuleKind::Interpreted => {
                        // JS runtime (V8) support was removed, so interpreted
                        // node_modules dependencies are not followed. A direct
                        // `.js` import is caught by the `should_use_js_runtime`
                        // gate at the top of `collect_modules` and surfaced as
                        // a hard error after collection completes.
                    }
                    ModuleKind::NativeRust => {}
                }
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

fn collect_module_finish(
    prepared: PreparedModule,
    ctx: &mut CompilationContext,
    visited: &HashSet<PathBuf>,
    target: Option<&str>,
    skip_transforms: bool,
    progress: &VerboseProgress,
) -> Result<()> {
    let PreparedModule {
        canonical,
        module_name,
        mut hir_module,
    } = prepared;

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
        progress.record(ProgressSnapshot {
            stage: "transform",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
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
        let enable_cross_module_inline =
            ctx.native_modules.len() <= MAX_CROSS_MODULE_INLINE_PRIOR_MODULES;
        if std::env::var("PERRY_INLINE_DEBUG").is_ok() && !enable_cross_module_inline {
            eprintln!(
                "[INLINE-DRIVER] skipping cross-module inline harvest for {}: prior_modules={} budget={}",
                hir_module.name,
                ctx.native_modules.len(),
                MAX_CROSS_MODULE_INLINE_PRIOR_MODULES
            );
        }
        if enable_cross_module_inline {
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
        }
        // Cross-module field-type info: `(class_name, field_name) ->
        // field_class_name`. Lets the inliner's `resolve_receiver_class`
        // walk a chain like `world.commandBuffer.set(...)` — without it,
        // the receiver match bails at the first PropertyGet and the call
        // stays a runtime dispatch. Built from every prior module's
        // class.fields where the type is `Named(...)`.
        let mut extra_class_fields: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        if enable_cross_module_inline {
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
        let mut extra_anon_classes: std::collections::HashMap<String, &perry_hir::Class> =
            std::collections::HashMap::new();
        if enable_cross_module_inline {
            for prior_module in ctx.native_modules.values() {
                for (k, v) in gather_cross_module_anon_classes(prior_module) {
                    extra_anon_classes.entry(k).or_insert(v);
                }
            }
        }
        // Interprocedural deforestation. Runs BEFORE inline_functions
        // so the inliner sees deforested signatures (the rewritten
        // function takes an accumulator param; inlined call sites then
        // already use the new shape). Intra-module only — see
        // `deforest::run` doc-comment for limitations and the manual
        // ABC451D validation.
        progress.record(ProgressSnapshot {
            stage: "transform-deforest",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        perry_transform::deforest::run(&mut hir_module);
        progress.record(ProgressSnapshot {
            stage: "transform-inline-functions",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
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
        progress.record(ProgressSnapshot {
            stage: "transform-unroll-static-loops",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        perry_transform::unroll_static_loops(&mut hir_module);
        // Inline `finally` bodies before each abrupt completion
        // (`return` / `break` / `continue` / labeled-break / labeled-
        // continue) reachable inside a `try { ... } finally { Y }`
        // shape. Must run BEFORE `transform_async_to_generator` because
        // the async transform flattens `try`/`finally` into a flat
        // state-machine sequence — an abrupt completion in the body
        // terminates the state, leaving the appended finally as dead
        // code. Issue #536.
        progress.record(ProgressSnapshot {
            stage: "transform-inline-finally",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        inline_finally_into_returns(&mut hir_module);
        progress.record(ProgressSnapshot {
            stage: "transform-async-to-generator",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        transform_async_to_generator(&mut hir_module);
        progress.record(ProgressSnapshot {
            stage: "transform-generators",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        transform_generators(&mut hir_module);
    }

    // Set optional-feature gates (regex/temporal/url/crypto/events/etc.) so
    // auto-optimize links only the runtime subsystems this module can reach.
    feature_detect::detect_optional_feature_usage(ctx, &hir_module);

    let collected_after_insert = ctx.native_modules.len() + ctx.js_modules.len() + 1;
    progress.record(ProgressSnapshot {
        stage: "collected",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(collected_after_insert),
        ..Default::default()
    });
    ctx.native_modules.insert(canonical, hir_module);
    Ok(())
}
