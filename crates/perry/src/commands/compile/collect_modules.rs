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
use perry_hir::{Expr, ModuleKind, Stmt};
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
    is_in_perry_native_package, is_js_file, parse_cached, parse_native_library_manifest,
    parse_package_specifier, CompilationContext, JsModule, ParseCache,
};

mod create_require_transform;
mod crypto_ns;
mod dynamic_glob;
mod parse_error;
#[cfg(test)]
mod tests;

use create_require_transform::transform_create_require_literal_requires;
use crypto_ns::module_uses_global_crypto_namespace;
use dynamic_glob::expand_dynamic_import_glob;
use parse_error::annotate_parse_error;

const MAX_CROSS_MODULE_INLINE_PRIOR_MODULES: usize = 128;

/// #5009: build the bare-name → literal map perry-hir lowering consults to fold
/// `process.env.<NAME>` reads (`perry_hir::env_define_lookup`). Strips the
/// `process.env.` prefix the `perry.define` keys carry and converts each
/// [`super::DefineValue`] to the matching [`perry_hir::EnvDefine`]. Keys that
/// aren't `process.env.*` are skipped (only env defines are honored today).
fn env_defines_for_lowering(
    define: &HashMap<String, super::DefineValue>,
) -> HashMap<String, perry_hir::EnvDefine> {
    define
        .iter()
        .filter_map(|(key, val)| {
            let name = key.strip_prefix("process.env.")?;
            let ev = match val {
                super::DefineValue::Str(s) => perry_hir::EnvDefine::Str(s.clone()),
                super::DefineValue::Bool(b) => perry_hir::EnvDefine::Bool(*b),
                super::DefineValue::Number(n) => perry_hir::EnvDefine::Num(*n),
                super::DefineValue::Null => perry_hir::EnvDefine::Null,
            };
            Some((name.to_string(), ev))
        })
        .collect()
}

/// Issue #818: scan a JS module's source for static ESM imports /
/// re-exports / string-literal dynamic imports, resolve each one
/// against the module's directory (with `resolve_with_extensions` so
/// extensionless and folder-index lookups work the same way they do at
/// import-time), and return the deduped list of file paths to add to
/// the bundle.
///
/// Bare specifiers (`react`, `@foo/bar`) and unresolvable relative
/// paths are skipped: bare specifiers are the V8 fallback's job to
/// resolve via the node_modules tree (we don't have a `require.resolve`
/// equivalent here without a full parse), and unresolvable relatives
/// just leak the same runtime error the V8 loader would have produced
/// anyway. This keeps the scan cheap and side-effect free.
pub(super) fn collect_js_module_imports(file_path: &std::path::Path, source: &str) -> Vec<PathBuf> {
    use std::sync::OnceLock;
    static IMPORT_RE: OnceLock<regex::Regex> = OnceLock::new();
    static EXPORT_FROM_RE: OnceLock<regex::Regex> = OnceLock::new();
    static DYNAMIC_IMPORT_RE: OnceLock<regex::Regex> = OnceLock::new();
    static BARE_IMPORT_RE: OnceLock<regex::Regex> = OnceLock::new();

    // `import ... from "spec"` — matches default/named/namespace forms.
    let import_re = IMPORT_RE.get_or_init(|| {
        regex::Regex::new(r#"(?m)^\s*import\s+(?:[^'"]+?\s+from\s+)?['"]([^'"]+)['"]"#)
            .expect("import regex")
    });
    // Bare side-effect import: `import "./foo.js";`
    let bare_re = BARE_IMPORT_RE.get_or_init(|| {
        regex::Regex::new(r#"(?m)^\s*import\s+['"]([^'"]+)['"]"#).expect("bare import regex")
    });
    // `export ... from "spec"` — covers `export *`, `export * as ns`,
    // `export { a, b }`. Captures the specifier.
    let export_re = EXPORT_FROM_RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?m)^\s*export\s+(?:\*(?:\s+as\s+\w+)?|\{[^}]*\})\s+from\s+['"]([^'"]+)['"]"#,
        )
        .expect("export from regex")
    });
    // Dynamic `import("spec")` — string-literal only.
    let dyn_re = DYNAMIC_IMPORT_RE.get_or_init(|| {
        regex::Regex::new(r#"\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)"#).expect("dynamic import regex")
    });

    let mut specs: Vec<String> = Vec::new();
    for cap in import_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }
    for cap in bare_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }
    for cap in export_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }
    for cap in dyn_re.captures_iter(source) {
        specs.push(cap[1].to_string());
    }

    let parent = match file_path.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for spec in specs {
        // Only follow relative or absolute paths — bare specifiers like
        // `react` need the node_modules resolver which is more invasive
        // to call here. The original entry walker (TS path) already
        // pulled bare-specifier dependencies in via `cached_resolve_import`,
        // so the most common case (top-level package brings in submodules)
        // is covered. Inside a package's `node_modules` tree, all
        // sibling imports are relative-path anyway.
        if !(spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/')) {
            continue;
        }
        let candidate = if spec.starts_with('/') {
            PathBuf::from(&spec)
        } else {
            parent.join(&spec)
        };
        if let Some(resolved) = super::resolve::resolve_with_extensions(&candidate) {
            if let Ok(canon) = resolved.canonicalize() {
                if seen.insert(canon.clone()) {
                    out.push(canon);
                }
            }
        }
    }
    out
}

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
        // node:timers — only the `import * as timers` namespace shape routes
        // through the submodule namespace; named imports keep the global
        // fast-path (gated in compile.rs). (#1213)
        "timers" => Some("timers"),
        "vm" => Some("vm"),
        "timers/promises" => Some("timers_promises"),
        "fs/promises" => Some("fs_promises"),
        "readline/promises" => Some("readline_promises"),
        "stream/promises" => Some("stream_promises"),
        "stream/consumers" => Some("stream_consumers"),
        // #1545: node:stream/web (WHATWG Web Streams). Named imports bind to
        // function singletons so `typeof ReadableStream === "function"`;
        // `new ReadableStream(...)` / `new CountQueuingStrategy(...)` are lowered
        // through the builtin-constructor dispatch in codegen regardless of the
        // import binding (see lower_call/builtin.rs), so these thunks only ever
        // run if the class is called *without* `new`.
        "stream/web" => Some("stream_web"),
        "sys" => Some("sys"),
        "test" => Some("test"),
        "test/reporters" => Some("test_reporters"),
        // Pino downstream (#906 follow-up): `require('node:diagnostics_channel')`
        // returns the module exports object. The CJS-wrap rewrites this as
        // `import diagChan from 'node:diagnostics_channel'`. Pre-fix the
        // codegen catch-all returned TAG_TRUE for that ExternFuncRef, so
        // `diagChan.tracingChannel(...)` threw
        // `TypeError: (boolean).tracingChannel is not a function`. Routing
        // through the namespace stub gives `diagChan` a real object whose
        // `tracingChannel` field is a callable thunk that hands back a
        // TracingChannel-shaped stub object — enough for pino to read
        // `asJsonChan.hasSubscribers === false` and take the fast path
        // without ever entering the tracing-instrumentation branch.
        "diagnostics_channel" => Some("diagnostics_channel"),
        "trace_events" => Some("trace_events"),
        // #1671: hono JSX runtime/streaming helpers. Perry renders JSX with the
        // built-in `js_jsx` runtime, so these submodules have no compiled-source
        // backing — they expose function singletons (jsx/jsxs/Fragment/JSXNode,
        // renderToReadableStream/Suspense) for code that imports the helpers
        // directly. Note these are NOT `node:`-prefixed; the strip above is a
        // no-op and they match verbatim.
        "hono/jsx/server" => Some("hono_jsx_server"),
        "hono/jsx/streaming" => Some("hono_jsx_streaming"),
        _ => None,
    }
}

fn nearest_package_root(path: &std::path::Path) -> Option<PathBuf> {
    let mut dir = path.parent();
    while let Some(candidate) = dir {
        if candidate.join("package.json").exists() {
            return Some(candidate.to_path_buf());
        }
        dir = candidate.parent();
    }
    None
}

fn package_root_for_compile_package(
    ctx: &CompilationContext,
    path: &std::path::Path,
) -> Option<PathBuf> {
    ctx.compile_package_dirs
        .values()
        .filter(|dir| path.starts_with(dir))
        .max_by_key(|dir| dir.components().count())
        .cloned()
        .or_else(|| nearest_package_root(path))
}

fn package_name_from_package_json(package_root: &std::path::Path) -> Option<String> {
    let package_json = fs::read_to_string(package_root.join("package.json")).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&package_json).ok()?;
    parsed
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn find_node_addon_file(dir: &std::path::Path, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name == "node_modules" || file_name == ".git" {
            continue;
        }
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("node") {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_node_addon_file(&path, max_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn node_addon_marker(package_root: &std::path::Path) -> Option<(&'static str, String)> {
    let binding_gyp = package_root.join("binding.gyp");
    if binding_gyp.exists() {
        return Some(("binding.gyp", binding_gyp.display().to_string()));
    }
    let prebuilds = package_root.join("prebuilds");
    if prebuilds.is_dir() {
        return Some(("prebuilds/", prebuilds.display().to_string()));
    }
    let package_json_path = package_root.join("package.json");
    if let Ok(package_json) = fs::read_to_string(&package_json_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&package_json) {
            if parsed
                .get("gypfile")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                return Some((
                    "package.json gypfile",
                    package_json_path.display().to_string(),
                ));
            }
            if package_json_dependency_uses_native_addon_loader(&parsed, "node-gyp-build")
                || package_json_dependency_uses_native_addon_loader(&parsed, "bindings")
            {
                return Some((
                    "native addon loader dependency",
                    package_json_path.display().to_string(),
                ));
            }
        }
    }
    if let Some(node_file) = find_node_addon_file(package_root, 5) {
        return Some(("*.node", node_file.display().to_string()));
    }
    None
}

fn package_json_dependency_uses_native_addon_loader(
    package_json: &serde_json::Value,
    loader_name: &str,
) -> bool {
    ["dependencies", "optionalDependencies"]
        .iter()
        .any(|section| {
            package_json
                .get(section)
                .and_then(|deps| deps.as_object())
                .is_some_and(|deps| deps.contains_key(loader_name))
        })
}

fn refuse_compile_package_native_addon(
    ctx: &mut CompilationContext,
    canonical: &std::path::Path,
) -> Result<()> {
    let Some(package_root) = package_root_for_compile_package(ctx, canonical) else {
        return Ok(());
    };
    if !ctx
        .checked_compile_package_native_addon_roots
        .insert(package_root.clone())
    {
        return Ok(());
    }
    if has_perry_native_library(&package_root) {
        return Ok(());
    }
    let Some((marker, marker_path)) = node_addon_marker(&package_root) else {
        return Ok(());
    };
    let package_name = package_name_from_package_json(&package_root)
        .unwrap_or_else(|| package_root.display().to_string());
    anyhow::bail!(
        "package `{}` is in `perry.compilePackages` but uses a Node native addon ({}) at {}.\n\
         Perry cannot load Node `.node` / N-API addons inside a native Perry binary. \
         Remove `{}` from `perry.compilePackages`, choose a pure JS/TS package, \
         or replace the native boundary with a Perry native binding \
         (`perry.nativeLibrary` / perry-ffi).",
        package_name,
        marker,
        marker_path,
        package_name
    );
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
            || is_declaration_file(&canonical)
            || is_json;

    // Skip JSON files — they're data, not code (imported via `with { type: "json" }`)
    if is_json {
        return Ok(ModuleDiscovery {
            finish: None,
            children: pending,
        });
    }

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
        let transitive_paths = collect_js_module_imports(&canonical, &source);
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

    // It's a TypeScript file to compile natively
    let raw_source = fs::read_to_string(&canonical)
        .map_err(|e| anyhow!("Failed to read {}: {}", canonical.display(), e))?;
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
    let source = if was_cjs_wrapped {
        super::cjs_wrap::wrap_commonjs_for_target(&raw_source, &canonical, target)
    } else {
        raw_source
    };
    let source = transform_create_require_literal_requires(&source, &ctx.compile_packages);

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
    let ast_module: &swc_ecma_ast::Module = match parse_cache.as_deref_mut() {
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
    let (mut hir_module, new_next_class_id) = lower_result?;
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
    let mut dynamic_path_sets: Vec<Vec<String>> = Vec::new();
    perry_hir::for_each_dynamic_import(&hir_module, &mut |expr| {
        if let perry_hir::Expr::DynamicImport { paths, arg } = expr {
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
                    dynamic_path_sets.push(set);
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
                            dynamic_path_sets.push(matches);
                            return;
                        }
                    }
                    dyn_errors.push(format!(
                        "dynamic import() in module {} ({}): {}",
                        module_name,
                        canonical.display(),
                        reason
                    ));
                }
            }
        }
    });
    let mut worker_path_sets: Vec<Vec<String>> = Vec::new();
    perry_hir::for_each_worker_new(&hir_module, &mut |expr| {
        if let perry_hir::Expr::WorkerNew {
            paths, filename, ..
        } = expr
        {
            if !paths.is_empty() {
                return;
            }
            let mut visiting: std::collections::HashSet<u32> = std::collections::HashSet::new();
            match perry_hir::resolve_import_path_with_context(
                filename.as_ref(),
                &module_const_locals,
                &dynamic_param_literals,
                &dynamic_local_literals,
                &mut visiting,
            ) {
                perry_hir::Resolution::Set(set) => {
                    if set.len() > perry_hir::DYNAMIC_IMPORT_PATH_CAP {
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
                    for p in &set {
                        if !new_dyn_imports.contains(p) {
                            new_dyn_imports.push(p.clone());
                        }
                    }
                    worker_path_sets.push(set);
                }
                perry_hir::Resolution::Unresolved(reason) => {
                    dyn_errors.push(format!(
                        "worker_threads Worker in module {}: {}",
                        module_name, reason
                    ));
                }
            }
        }
    });
    drop(dynamic_local_literals);
    drop(module_const_locals);
    if !dyn_errors.is_empty() {
        return Err(anyhow!("{}", dyn_errors.join("\n")));
    }
    let mut dynamic_path_sets = dynamic_path_sets.into_iter();
    perry_hir::for_each_dynamic_import_mut(&mut hir_module, &mut |expr| {
        if let perry_hir::Expr::DynamicImport { paths, .. } = expr {
            if paths.is_empty() {
                if let Some(set) = dynamic_path_sets.next() {
                    *paths = set;
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
                            | "takeScreenshot"
                            | "getSafeAreaInsets"
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

        if let Some((resolved_path, kind)) = cached_resolve_import(&import.source, &canonical, ctx)
        {
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
                    pending.push(resolved_path);
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

                    pending.push(resolved_path);
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
            if let Some((resolved_path, kind)) =
                cached_resolve_import(src.as_str(), &canonical, ctx)
            {
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
                    ModuleKind::NativeCompiled => pending.push(resolved_path),
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

    return Ok(ModuleDiscovery {
        finish: Some(PreparedModule {
            canonical,
            module_name,
            hir_module,
        }),
        children: pending,
    });
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
    // auto-optimize. Text-grep the serialized Debug form for the established
    // dedicated HIR variants. The global WebCrypto namespace path below uses
    // a structured walk because it is an ordinary `PropertyGet`.
    {
        let hir_debug: String = format!("{:?}{:?}", &hir_module.init, &hir_module.functions);
        let uses_global_crypto_namespace = module_uses_global_crypto_namespace(&hir_module);
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
