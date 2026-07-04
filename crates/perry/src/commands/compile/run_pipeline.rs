//! The `run_with_parse_cache` compile orchestrator (pure code move).
//!
//! This is the full driver that lowers, codegens, links, and packages a
//! TypeScript program. It was relocated wholesale out of `compile.rs` to keep
//! the trunk small; all helpers it relies on live in sibling modules and are
//! reached via `use super::*`.

use super::*;

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::OutputFormat;

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
    // #4826: fold `--libc musl` into the effective target up-front (before any
    // downstream code reads `args.target`) so the rest of the pipeline only
    // ever sees the concrete `linux-musl` triple family.
    let mut args = args;
    args.target = apply_libc_to_target(args.target.take(), args.libc.as_deref())?;

    // #835 + #846: clear the codegen-side FFI provenance set up-front
    // so any leftover entries from a prior `perry dev` rebuild (or a
    // failed-build early-return that skipped our drain below) don't
    // bleed into this build's auto-link decisions.
    let _ = perry_codegen::ext_registry::take_used_providers();

    // #1663: make `--debug-symbols` retain a symbol table on every native
    // target, not just emit a PDB on Windows. Previously the flag was a no-op
    // on Linux/macOS, so a SIGSEGV in a compiled service (e.g. the Fastify +
    // @perryts/mysql crash reported in #1663) symbolized to an unreadable wall
    // of `??`, making runtime crashes nearly impossible to report. The
    // canonical knob for "keep symbols" is the PERRY_DEBUG_SYMBOLS env var,
    // which the codegen (`-g`/DWARF), the object-cache key, and the final
    // `strip` step already all honor. Promote the flag to that env var here —
    // single-threaded, before module codegen spawns rayon workers — so every
    // layer observes it uniformly. Only set (never unset): the flag is an
    // explicit opt-in, and a `perry dev` session that asked for symbols once
    // wants them for the rest of the session.
    if args.debug_symbols && std::env::var_os("PERRY_DEBUG_SYMBOLS").is_none() {
        std::env::set_var("PERRY_DEBUG_SYMBOLS", "1");
    }

    // `--trace <stages>` consolidates the scattered debug-dump knobs into one
    // flag. Parse it up-front (single-threaded, before codegen spawns rayon
    // workers) so the `llvm` stage can promote itself to the env vars the
    // codegen + linker already honor, exactly like `--debug-symbols` above.
    // `--focus NAME` alone implies `hir` — asking to focus something with no
    // stage selected obviously means "show me that function's HIR".
    let trace_stages: std::collections::HashSet<String> = args
        .trace
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_ascii_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let trace_all = trace_stages.contains("all");
    let trace_hir = trace_all
        || trace_stages.contains("hir")
        || args.print_hir
        || (trace_stages.is_empty() && args.focus.is_some());
    let trace_llvm = trace_all || trace_stages.contains("llvm");
    if trace_llvm {
        // Land .ll files in a predictable per-build directory so the user
        // doesn't have to remember PERRY_SAVE_LL / PERRY_LLVM_KEEP_IR. Don't
        // clobber an explicit env override.
        if std::env::var_os("PERRY_SAVE_LL").is_none() {
            // Absolute path: codegen runs the .ll write on rayon workers whose
            // cwd we don't want to depend on. Join against cwd up-front.
            let dir = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".perry-trace")
                .join("llvm");
            let _ = std::fs::create_dir_all(&dir);
            std::env::set_var("PERRY_SAVE_LL", &dir);
            std::env::set_var("PERRY_LLVM_KEEP_IR", "1");
            // The per-module object cache short-circuits codegen for unchanged
            // modules — which means `emit_module` (and thus the .ll write)
            // never runs and the trace dir comes up empty. Force a full
            // recompile for this build, exactly like --verify-native-regions.
            std::env::set_var("PERRY_NO_CACHE", "1");
            if matches!(format, OutputFormat::Text) {
                println!("[trace] LLVM IR → {}", dir.display());
            }
        }
    }

    // Canonicalize the input path first so its `.parent()` is an absolute directory.
    // Without this, a bare filename like `perry demo.ts` produced `Path::new("").parent()`
    // → fallback `"."`, and the walk-up loops below (package.json + perry.toml discovery)
    // immediately terminated because `PathBuf::from(".").pop()` returns false. That meant
    // perry.compilePackages / perry.packageAliases declared in a parent package.json were
    // silently ignored unless the user invoked perry from the directory containing it (#260).
    let project_root = args
        .input
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut ctx = CompilationContext::new(project_root.clone());
    ctx.cache_root = object_cache_project_root(&args.input, &project_root);
    let explain_lowering = if args.explain_lowering {
        Some(lowering_report::ExplainLoweringRun::prepare(
            &ctx.cache_root,
        )?)
    } else {
        None
    };
    // Resolve the on-disk cache directory ONCE, here, before any cache
    // consumer runs. Precedence: `--cache-dir` → `PERRY_CACHE_DIR` →
    // perry.toml `[perry] cacheDir` → package.json `perry.cacheDir` →
    // default `<cache_root>/node_modules/.cache/perry` (the find-cache-dir
    // convention). `cache_dir_override` reads the env + perry.toml +
    // package.json half; the CLI flag wins over all three. Relative
    // overrides resolve against `cache_root`. Computed here because the
    // build-cache probe below runs before
    // `host_config::apply_pkg_and_toml_config`, so the build cache must
    // already know the dir. host_config re-resolves `ctx.cache_dir` to the
    // same value when it parses the config alongside its sibling `perry.*`
    // fields — that pass owns the canonical read.
    let cache_dir_override = args
        .cache_dir
        .clone()
        .or_else(|| object_cache::cache_dir_override(&ctx.cache_root));
    ctx.cache_dir = object_cache::resolve_cache_dir(&ctx.cache_root, cache_dir_override.as_deref());
    // #5247: propagate `--debug-symbols` so `collect_modules` records the
    // CJS-wrap source mapping needed to render original-source line numbers.
    ctx.debug_symbols = args.debug_symbols;

    let build_cache_probe =
        BuildCacheProbe::new(&args, &project_root, &ctx.cache_root, &ctx.cache_dir);
    let mut build_cache_stats = build_cache_probe.probe();
    if build_cache_stats.hit {
        if let OutputFormat::Json = format {
            build_cache_probe.print_json_hit(&build_cache_stats)?;
        } else if verbose > 0 {
            println!("Build cache hit: {}", build_cache_stats.reason);
        }
        return Ok(build_cache_probe.compile_result_for_hit());
    }

    match format {
        OutputFormat::Text => println!("Collecting modules..."),
        OutputFormat::Json => {}
    }

    // Tier 2.x: package.json + perry.toml + i18n + google_auth config
    // loading lifted into compile/host_config.rs::apply_pkg_and_toml_config.
    let (i18n_config, i18n_translations) =
        apply_pkg_and_toml_config(&args, &project_root, &mut ctx, format)?;

    // #1680 (Phase 2 of #1677): run host-declared build-time codegen steps
    // (e.g. `ajv/standalone`, `prisma generate`) before module collection so
    // the eval-free generated output is on disk for the normal compile path.
    let skip_codegen = args.no_codegen || codegen_steps::skip_from_env();
    codegen_steps::run_codegen_steps(&ctx, skip_codegen, format)?;

    // #1681 (Phase 3 of #1677): self-hosted build-time `precompile(...)`.
    // If this is the capture subprocess, enter capture mode; otherwise, when
    // the entry uses `precompile(`, compile+run it via Perry itself (no node,
    // no V8) to evaluate the codegen at build time and install the captured
    // generated sources for the main compile below.
    precompile_capture::prepare_precompile(&args, &mut ctx, format)?;

    maybe_init_type_checker(&args, &project_root, format, &mut ctx);

    let mut visited = HashSet::new();
    let mut next_class_id: perry_hir::ClassId = 1; // Start at 1, 0 is reserved for "no parent"
    let skip_transforms = matches!(args.target.as_deref(), Some("web") | Some("wasm"));
    let progress = VerboseProgress::new(format, verbose);

    // Issue #444: canonicalize the user's entry path once so collect_modules
    // can compare every module's canonical path against it and set
    // `is_entry_module=true` only on the actual entry (driving
    // `import.meta.main`). Failures fall through silently — collect_modules
    // canonicalizes again and would surface any IO error there.
    if ctx.entry_canonical.is_none() {
        if let Ok(c) = args.input.canonicalize() {
            ctx.entry_canonical = Some(c);
        }
    }

    collect_modules(
        &args.input,
        &mut ctx,
        &mut visited,
        format,
        args.target.as_deref(),
        &mut next_class_id,
        skip_transforms,
        &progress,
        parse_cache.as_deref_mut(),
    )?;

    // Bundle extensions if --bundle-extensions specified
    let bundled_extensions: Vec<(PathBuf, String)> =
        if let Some(ext_dir) = args.bundle_extensions.clone() {
            bundle_extensions_into_ctx(
                &ext_dir,
                &args,
                &mut ctx,
                &mut visited,
                &mut next_class_id,
                skip_transforms,
                &progress,
                parse_cache.as_deref_mut(),
                format,
            )?
        } else {
            Vec::new()
        };

    rerun_collect_with_class_field_types(
        &args,
        &mut ctx,
        &mut visited,
        &mut next_class_id,
        skip_transforms,
        &progress,
        parse_cache.as_deref_mut(),
        format,
    )?;

    run_post_collect_preflight(&args, &mut ctx, format)?;

    // #2309: tree-shake the final module graph — prune unreachable
    // node_modules modules and re-raise any deferred refusal that survives.
    // No-op unless tree-shaking is enabled (byte-identical to pre-#2309).
    {
        let entry_canonical = ctx.entry_canonical.clone().unwrap_or_else(|| {
            args.input
                .canonicalize()
                .unwrap_or_else(|_| args.input.clone())
        });
        reachability::tree_shake(&mut ctx, &entry_canonical)?;
    }

    // --- Web/WASM target: emit WASM binary + JS runtime bridge ---
    if matches!(args.target.as_deref(), Some("web") | Some("wasm")) {
        #[cfg(feature = "backend-wasm")]
        {
            return compile_for_wasm(&ctx, &args, format);
        }
        #[cfg(not(feature = "backend-wasm"))]
        {
            anyhow::bail!(backend_disabled_msg(
                args.target.as_deref().unwrap_or("wasm"),
                "backend-wasm",
            ));
        }
    }

    // --- Widget targets: emit platform-specific source + optional native provider ---
    if matches!(
        args.target.as_deref(),
        Some("ios-widget") | Some("ios-widget-simulator")
    ) {
        #[cfg(feature = "backend-swiftui")]
        {
            return compile_for_ios_widget(&ctx, &args, format);
        }
        #[cfg(not(feature = "backend-swiftui"))]
        {
            anyhow::bail!(backend_disabled_msg("ios-widget", "backend-swiftui"));
        }
    }
    if matches!(
        args.target.as_deref(),
        Some("watchos-widget") | Some("watchos-widget-simulator")
    ) {
        #[cfg(feature = "backend-swiftui")]
        {
            return compile_for_watchos_widget(&ctx, &args, format);
        }
        #[cfg(not(feature = "backend-swiftui"))]
        {
            anyhow::bail!(backend_disabled_msg("watchos-widget", "backend-swiftui"));
        }
    }
    if args.target.as_deref() == Some("android-widget") {
        #[cfg(feature = "backend-glance")]
        {
            return compile_for_android_widget(&ctx, &args, format);
        }
        #[cfg(not(feature = "backend-glance"))]
        {
            anyhow::bail!(backend_disabled_msg("android-widget", "backend-glance"));
        }
    }
    if args.target.as_deref() == Some("wearos-tile") {
        #[cfg(feature = "backend-wear-tiles")]
        {
            return compile_for_wearos_tile(&ctx, &args, format);
        }
        #[cfg(not(feature = "backend-wear-tiles"))]
        {
            anyhow::bail!(backend_disabled_msg("wearos-tile", "backend-wear-tiles"));
        }
    }

    run_native_instance_fixups(&mut ctx);
    #[cfg(feature = "backend-arkts")]
    harvest_harmonyos_index_ets(&args, &mut ctx, format);

    let i18n_table = apply_i18n_pass(&mut ctx, i18n_config.as_ref(), &i18n_translations, format);

    if trace_hir {
        dump_hir_for_debug(&ctx, args.focus.as_deref());
    }

    write_i18n_key_registry(&ctx, i18n_table.as_ref());

    match format {
        OutputFormat::Text => println!("Generating code..."),
        OutputFormat::Json => {}
    }

    let mut obj_paths = Vec::new();
    let mut obj_cleanup_paths = Vec::new();

    // Get canonical path of entry module
    let entry_path = args
        .input
        .canonicalize()
        .unwrap_or_else(|_| args.input.clone());

    classify_eager_modules(&mut ctx, &entry_path);
    let non_entry_module_names: Vec<String> =
        topo_sort_non_entry_modules(&ctx, &entry_path, format, verbose);

    // Build a map of all exported enums from all modules (owned data, no borrows)
    // Key: (resolved_path, enum_name) -> Vec<(member_name, EnumValue)>
    let mut exported_enums: BTreeMap<(String, String), Vec<(String, perry_hir::EnumValue)>> =
        BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for en in &hir_module.enums {
            if en.is_exported {
                let members: Vec<(String, perry_hir::EnumValue)> = en
                    .members
                    .iter()
                    .map(|m| (m.name.clone(), m.value.clone()))
                    .collect();
                exported_enums.insert((path_str.clone(), en.name.clone()), members);
            }
        }
    }

    // Propagate enum re-exports: when module A has `export * from "./B"`,
    // all enums exported from B should also be accessible via A's path.
    loop {
        let mut new_enum_entries: Vec<((String, String), Vec<(String, perry_hir::EnumValue)>)> =
            Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                let source_str = match export {
                    perry_hir::Export::ExportAll { source } => Some((source.as_str(), None)),
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => Some((
                        source.as_str(),
                        Some((imported.as_str(), exported.as_str())),
                    )),
                    _ => None,
                };
                if let Some((source, re_export_names)) = source_str {
                    if let Some((resolved_source, _)) = resolve_import(
                        source,
                        path,
                        &ctx.project_root,
                        &ctx.compile_packages,
                        &ctx.compile_package_dirs,
                    ) {
                        let source_path_str = resolved_source.to_string_lossy().to_string();
                        for ((src_path, enum_name), members) in &exported_enums {
                            if src_path == &source_path_str {
                                let (propagate, exported_name) = match re_export_names {
                                    Some((imported, exported)) => {
                                        (enum_name == imported, exported.to_string())
                                    }
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
        if new_enum_entries.is_empty() {
            break;
        }
        for (key, members) in new_enum_entries {
            exported_enums.insert(key, members);
        }
    }

    // Fix imported enum references in all modules BEFORE building exported_classes
    // (exported_classes holds references into ctx.native_modules, so we need to do
    // the mutable fixup pass first)
    {
        let mut module_enums: BTreeMap<
            PathBuf,
            BTreeMap<String, Vec<(String, perry_hir::EnumValue)>>,
        > = BTreeMap::new();
        for (path, hir_module) in &ctx.native_modules {
            let mut imported_enums_for_module: BTreeMap<
                String,
                Vec<(String, perry_hir::EnumValue)>,
            > = BTreeMap::new();
            for import in &hir_module.imports {
                if import.module_kind != perry_hir::ModuleKind::NativeCompiled {
                    continue;
                }
                let resolved_path = match &import.resolved_path {
                    Some(p) => p.clone(),
                    None => continue,
                };
                for spec in &import.specifiers {
                    let (local_name, exported_name) = match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => {
                            (local.clone(), imported.clone())
                        }
                        perry_hir::ImportSpecifier::Default { local } => {
                            (local.clone(), local.clone())
                        }
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
    let mut all_type_aliases: std::collections::BTreeMap<String, perry_types::Type> =
        std::collections::BTreeMap::new();
    for hir_module in ctx.native_modules.values() {
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
    let mut all_program_type_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for hir_module in ctx.native_modules.values() {
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
    // Issue #489 followup: canonical defining path keyed by class id. The
    // re-export propagation loop below adds extra `(re_export_path,
    // class_name)` entries pointing at the same class, and the transitive
    // parent-class closure later picks `exported_classes`'s first BTreeMap
    // match by name — which is whichever path sorts earliest, often a
    // barrel `index.js` rather than the actual defining file. That gives
    // the imported parent class a `source_prefix` of the barrel, and the
    // codegen later emits dispatch references to
    // `perry_method_<barrel>__<Class>__<method>` while the source module
    // defines the symbol under `perry_method_<origin>__<Class>__<method>`
    // — undefined-symbol link error. Drizzle hits this:
    // `mysql-proxy/session.js` calls `.then` on a Promise; perry's name-
    // based dispatch picks `QueryPromise.then` from the transitive parent
    // closure (`MySqlPreparedQuery extends QueryPromise`), but the
    // canonical path is `query-promise.js`, not `index.js` which
    // re-exports it via `export *`.
    let mut class_canonical_path: std::collections::HashMap<perry_hir::ClassId, String> =
        std::collections::HashMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for class in &hir_module.classes {
            if class.is_exported {
                exported_classes.insert((path_str.clone(), class.name.clone()), class);
                class_canonical_path
                    .entry(class.id)
                    .or_insert_with(|| path_str.clone());
            }
        }
        // Issue #485: handle `export { Local as Exported }` for classes.
        // Without this, a module that declares `class Hono extends … {}` and
        // re-exports it via `export { Hono as HonoBase }` registers under
        // (path, "Hono") only — but the importer's lookup uses the imported
        // (alias) side: `(path, "HonoBase")`. The miss makes
        // `imported_classes` skip the entry entirely, so the importing module
        // gets no class metadata for HonoBase, no constructor symbol via
        // `imported_class_ctors`, and `super(...)` from a subclass (e.g. the
        // Hono class in hono.js extends HonoBase) silently no-ops — the
        // subclass's `app.fetch` / `app.get` / etc. arrow-class-field methods
        // are never installed onto `this`.
        for export in &hir_module.exports {
            if let perry_hir::Export::Named { local, exported } = export {
                if local == exported {
                    continue;
                }
                if let Some(class) = hir_module
                    .classes
                    .iter()
                    .find(|c| c.name == *local && c.is_exported)
                {
                    exported_classes
                        .entry((path_str.clone(), exported.clone()))
                        .or_insert(class);
                }
            }
        }
    }

    // Set of exported VARIABLES (not functions) — keyed by (module_path, name).
    // Used to distinguish variable getters from function references when an
    // ExternFuncRef appears as a value in an importing module.
    let mut exported_var_names: BTreeSet<(String, String)> = BTreeSet::new();
    // Build a map of all exported functions with their param counts from all modules
    let mut exported_func_param_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    // Issue #608 — parallel map: which exported functions have a trailing
    // `...rest` parameter. Cross-module call sites consult this to bundle
    // trailing args into a `js_array_alloc(n)` rest array before the call,
    // mirroring the same-module fast path that uses `func_signatures`'s
    // has_rest bit. Without this map, `import { sql } from "pkg"` followed
    // by `sql\`hello ${x}\`` (which the HIR desugars to `sql(stringsArr, x)`)
    // emits a 2-arg call whose callee reads `params` as the raw 2nd arg
    // instead of `[x]`. Sparse map (only `true` entries stored).
    let mut exported_func_has_rest: BTreeMap<(String, String), bool> = BTreeMap::new();
    // #1816: exported functions whose trailing param is the HIR-synthesized
    // `arguments` rest (a body that references `arguments`). These need the
    // cross-module call to bundle ALL passed args into that param (matching
    // `arguments.length` spec semantics), not just the trailing ones — distinct
    // from a real `...rest`. effect's `pipe`/`dual` are the load-bearing case.
    let mut exported_func_synthetic_arguments: BTreeSet<(String, String)> = BTreeSet::new();
    // Build a map of all exported functions with their return types from all modules
    let mut exported_func_return_types: BTreeMap<(String, String), perry_types::Type> =
        BTreeMap::new();
    // Set of exported functions that were declared `async` in their source module.
    // We track this separately because users routinely write `async function f() { ... }`
    // without an explicit `Promise<T>` annotation, in which case `func.return_type` is the
    // inner type or `Type::Any` and importers can't infer async-ness from the return type alone.
    let mut exported_async_funcs: BTreeSet<(String, String)> = BTreeSet::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for func in &hir_module.functions {
            if func.is_exported {
                exported_func_param_counts
                    .insert((path_str.clone(), func.name.clone()), func.params.len());
                exported_func_return_types.insert(
                    (path_str.clone(), func.name.clone()),
                    func.return_type.clone(),
                );
                if func.is_async {
                    exported_async_funcs.insert((path_str.clone(), func.name.clone()));
                }
                if func.params.last().is_some_and(|p| p.is_rest) {
                    exported_func_has_rest.insert((path_str.clone(), func.name.clone()), true);
                }
                if func
                    .params
                    .last()
                    .is_some_and(|p| p.is_rest && p.name == "arguments")
                {
                    exported_func_synthetic_arguments.insert((path_str.clone(), func.name.clone()));
                }
            }
        }
        // Also register exported_functions aliases (e.g., "default" → actual function)
        // This handles `export default funcName` where the export name differs from the function name
        for (export_name, func_id) in &hir_module.exported_functions {
            if let Some(func) = hir_module.functions.iter().find(|f| f.id == *func_id) {
                let key = (path_str.clone(), export_name.clone());
                exported_func_param_counts
                    .entry(key.clone())
                    .or_insert(func.params.len());
                exported_func_return_types
                    .entry(key.clone())
                    .or_insert_with(|| func.return_type.clone());
                if func.is_async {
                    exported_async_funcs.insert(key.clone());
                }
                if func.params.last().is_some_and(|p| p.is_rest) {
                    exported_func_has_rest.entry(key.clone()).or_insert(true);
                }
                if func
                    .params
                    .last()
                    .is_some_and(|p| p.is_rest && p.name == "arguments")
                {
                    exported_func_synthetic_arguments.insert(key);
                }
            }
        }
        // Debug: print superstruct exports
        if path_str.contains("superstruct") {
            eprintln!(
                "[DEBUG] superstruct: {} functions ({} exported), {} exported_functions entries",
                hir_module.functions.len(),
                hir_module
                    .functions
                    .iter()
                    .filter(|f| f.is_exported)
                    .count(),
                hir_module.exported_functions.len()
            );
            for (name, _fid) in &hir_module.exported_functions {
                eprintln!("[DEBUG]   exported_function: {}", name);
            }
        }

        // Also scan init statements for exported closures (arrow functions assigned to const)
        // These are in exported_objects but not in functions, so they need param counts too
        let exported_set: std::collections::HashSet<&String> =
            hir_module.exported_objects.iter().collect();
        for stmt in &hir_module.init {
            if let perry_hir::ir::Stmt::Let {
                name,
                init: Some(expr),
                ..
            } = stmt
            {
                if exported_set.contains(name) {
                    if let perry_hir::ir::Expr::Closure {
                        params,
                        return_type,
                        is_async,
                        ..
                    } = expr
                    {
                        exported_func_param_counts
                            .insert((path_str.clone(), name.clone()), params.len());
                        exported_func_return_types
                            .insert((path_str.clone(), name.clone()), return_type.clone());
                        if *is_async {
                            exported_async_funcs.insert((path_str.clone(), name.clone()));
                        }
                        if params.last().is_some_and(|p| p.is_rest) {
                            exported_func_has_rest.insert((path_str.clone(), name.clone()), true);
                        }
                    }
                }
            }
        }
    }

    // Populate exported_var_names: closures-assigned-to-const are in BOTH
    // `exported_objects` and `exported_func_param_counts`, but their
    // `perry_fn_<src>__<name>` symbol is a ZERO-arg getter (returns the
    // global closure pointer), not the function body — so at call sites
    // we still need to fetch the value via the getter and then closure-call.
    // The `is_function_alias` exclusion keeps `function foo(){}` decls out
    // (their perry_fn_<…> symbol IS the function body).
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        let is_function_decl: std::collections::HashSet<&String> = hir_module
            .functions
            .iter()
            .filter(|f| f.is_exported)
            .map(|f| &f.name)
            .collect();
        for obj_name in &hir_module.exported_objects {
            if is_function_decl.contains(obj_name) {
                continue;
            }
            let key = (path_str.clone(), obj_name.clone());
            exported_var_names.insert(key);
        }
    }

    // Build a map of all exports from all modules: module_path -> HashMap<export_name, origin_module_path>
    // This is used for namespace imports (`import * as X from './module'`) to resolve all exports
    let mut all_module_exports: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    // Issue #678: parallel map carrying the *origin name* alongside the
    // origin path. When `ink/build/index.js` says `export { default as
    // render } from './render.js'`, `all_module_exports[ink_path]["render"]
    // = render_js_path` and `all_module_export_origin_names[ink_path]
    // ["render"] = "default"`. The codegen consumer of an import that
    // resolves through this chain forms `perry_fn_<render_js>__default`
    // instead of `perry_fn_<render_js>__render` — without it the linker
    // fails on the missing `_perry_fn_<render_js>__render` symbol.
    let mut all_module_export_origin_names: BTreeMap<String, BTreeMap<String, String>> =
        BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        let exports = all_module_exports.entry(path_str.clone()).or_default();
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
        // `export type X` / `export interface X` still lower to an
        // `Export::Named` (so type re-export chains resolve), but they are
        // TYPE-ONLY — erased at runtime, with no `perry_fn_*` symbol. They must
        // not enter the runtime export set: that set drives `import * as ns`
        // materialization (Object.keys/for-in), and a phantom type name there
        // resolves to a bogus closure value that breaks consumers enumerating
        // the namespace (drizzle's `drizzle(pool, { schema })`, where the schema
        // module also `export type Customer = …` alongside the real tables).
        // A name that is ALSO a value export (declaration merging, a class)
        // stays — only names that are exclusively types are dropped.
        let value_export_names: std::collections::HashSet<&str> = hir_module
            .functions
            .iter()
            .filter(|f| f.is_exported)
            .map(|f| f.name.as_str())
            .chain(hir_module.exported_objects.iter().map(|s| s.as_str()))
            .chain(
                hir_module
                    .classes
                    .iter()
                    .filter(|c| c.is_exported)
                    .map(|c| c.name.as_str()),
            )
            .chain(
                hir_module
                    .enums
                    .iter()
                    .filter(|e| e.is_exported)
                    .map(|e| e.name.as_str()),
            )
            .collect();
        let type_only_export_names: std::collections::HashSet<String> = hir_module
            .type_aliases
            .iter()
            .map(|t| t.name.clone())
            .chain(hir_module.interfaces.iter().map(|i| i.name.clone()))
            .filter(|n| !value_export_names.contains(n.as_str()))
            .collect();
        // Named exports (export { foo, bar as baz })
        for export in &hir_module.exports {
            if let perry_hir::Export::Named { local, exported } = export {
                if type_only_export_names.contains(exported) {
                    continue;
                }
                exports.insert(exported.clone(), path_str.clone());
                // #1758: a LOCAL renamed export of a CLASS
                // (`export { Number$ as Number }`, no `from`) must record the
                // origin (local) name so importers resolve `ns.Number` to the
                // defining class `Number$`. The re-export propagation loop below
                // only records origin names for cross-module
                // `export { X as Y } from "src"`. Without this, the
                // namespace-member class value-read (property_get.rs) looks up
                // `class_ids["Number"]` (the export alias) — a miss — and
                // `S.Number` falls back to the global `Number`, losing all
                // inherited statics (effect's `S.Number.ast` → undefined →
                // Schema decode crash). Scoped to classes: renamed var/func
                // exports route through wrapper-symbol emission that keys on the
                // export name, and feeding the origin name there breaks linking.
                if local != exported
                    && hir_module
                        .classes
                        .iter()
                        .any(|c| c.name == *local && c.is_exported)
                {
                    all_module_export_origin_names
                        .entry(path_str.clone())
                        .or_default()
                        .insert(exported.clone(), local.clone());
                }
            }
            // ReExport is handled in the propagation loop below (avoids borrow issues)
        }
    }

    // Propagate exports through ExportAll and ReExport chains
    loop {
        // (module_path, export_name, origin_path, origin_name_in_origin).
        // The fourth tuple element drives Issue #678's per-export
        // origin-name map: when a re-export renames a name across a hop
        // (`export { default as render } from './render.js'`), the
        // consumer must use the *origin* name (`default`) as the symbol
        // suffix, not the consumer-visible one (`render`).
        let mut new_export_entries: Vec<(String, String, String, String)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                let current_exports = all_module_exports.get(&path_str);
                                for (name, origin) in source_exports {
                                    // ESM semantics: `export * from "src"`
                                    // re-exports every named export EXCEPT
                                    // `default`. Leaking it made barrels
                                    // claim a default binding they never
                                    // define, which breaks the #4872
                                    // has-default probe that decides whether
                                    // a default import can bind to
                                    // `perry_fn_<src>__default`.
                                    if name == "default" {
                                        continue;
                                    }
                                    let already_exists = current_exports
                                        .map(|e| e.contains_key(name))
                                        .unwrap_or(false);
                                    if !already_exists {
                                        // `export * from "src"` doesn't
                                        // rename — origin_name == export_name.
                                        // But if `src` itself remapped this
                                        // name (e.g. `export { default as
                                        // foo } from './x.js'`), propagate
                                        // the deeper origin name across this
                                        // transitive hop.
                                        let deep_origin_name = all_module_export_origin_names
                                            .get(&source_path_str)
                                            .and_then(|m| m.get(name))
                                            .cloned()
                                            .unwrap_or_else(|| name.clone());
                                        new_export_entries.push((
                                            path_str.clone(),
                                            name.clone(),
                                            origin.clone(),
                                            deep_origin_name,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            if let Some(source_exports) = all_module_exports.get(&source_path_str) {
                                if let Some(origin) = source_exports.get(imported) {
                                    let current_exports = all_module_exports.get(&path_str);
                                    let already_correct = current_exports
                                        .and_then(|e| e.get(exported.as_str()))
                                        .map(|v| v == origin)
                                        .unwrap_or(false);
                                    if !already_correct {
                                        // Walk one more hop: if `src` itself
                                        // remapped `imported` to a deeper
                                        // origin name (`src` did its own
                                        // `export { default as imported }
                                        // from "..."`), record THAT deeper
                                        // name so the consumer's symbol-suffix
                                        // resolution skips both hops.
                                        let deep_origin_name = all_module_export_origin_names
                                            .get(&source_path_str)
                                            .and_then(|m| m.get(imported))
                                            .cloned()
                                            .unwrap_or_else(|| imported.clone());
                                        new_export_entries.push((
                                            path_str.clone(),
                                            exported.clone(),
                                            origin.clone(),
                                            deep_origin_name,
                                        ));
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
                                    perry_hir::ImportSpecifier::Named { local: l, imported } => {
                                        (l == local, imported.clone())
                                    }
                                    perry_hir::ImportSpecifier::Default { local: l } => {
                                        (l == local, "default".to_string())
                                    }
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(
                                        &import.source,
                                        path,
                                        &ctx.project_root,
                                        &ctx.compile_packages,
                                        &ctx.compile_package_dirs,
                                    ) {
                                        let source_path_str =
                                            resolved_source.to_string_lossy().to_string();
                                        if let Some(source_exports) =
                                            all_module_exports.get(&source_path_str)
                                        {
                                            if let Some(origin) = source_exports.get(&imported_name)
                                            {
                                                let current_exports =
                                                    all_module_exports.get(&path_str);
                                                let already_correct = current_exports
                                                    .and_then(|e| e.get(exported.as_str()))
                                                    .map(|v| v == origin)
                                                    .unwrap_or(false);
                                                if !already_correct {
                                                    let deep_origin_name =
                                                        all_module_export_origin_names
                                                            .get(&source_path_str)
                                                            .and_then(|m| m.get(&imported_name))
                                                            .cloned()
                                                            .unwrap_or_else(|| {
                                                                imported_name.clone()
                                                            });
                                                    new_export_entries.push((
                                                        path_str.clone(),
                                                        exported.clone(),
                                                        origin.clone(),
                                                        deep_origin_name,
                                                    ));
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
        if new_export_entries.is_empty() {
            break;
        }
        for (module_path, name, origin, origin_name) in new_export_entries {
            all_module_exports
                .entry(module_path.clone())
                .or_default()
                .insert(name.clone(), origin);
            // Only record the origin-name entry when it actually differs
            // from the export name (the common identity case is implicit —
            // the codegen helper falls back to the imported name when no
            // entry is present). This keeps the map sparse and easy to
            // reason about.
            if origin_name != name {
                all_module_export_origin_names
                    .entry(module_path)
                    .or_default()
                    .insert(name, origin_name);
            }
        }
    }

    // Also propagate exported_func_param_counts AND exported_func_has_rest
    // through ExportAll/ReExport/Named chains.
    //
    // Drizzle-sqlite blocker: pre-fix the rest-only table only carried entries
    // for the SOURCE module of the function declaration (e.g.
    // `drizzle-orm/better-sqlite3/driver.js::drizzle`), so when a downstream
    // module re-exported it via `export * from "./driver.js"` (the canonical
    // npm-package barrel pattern in `drizzle-orm/better-sqlite3/index.js`),
    // the re-exported entry was never written. Consumers importing `drizzle`
    // from `"drizzle-orm/better-sqlite3"` (resolving to index.js) looked up
    // `(index.js, "drizzle")` in `exported_func_has_rest`, missed → no rest
    // bundling at the call site → `function drizzle(...params)` ran with
    // `params` as raw f64 args instead of a bundled array → `params[0]`
    // indexed into a non-array and read undefined. Symptom: `drizzle(sqlite)`
    // saw `params[0] === undefined`, took the `params[0] === void 0` branch
    // and constructed a fresh `new Client()` (heap wrapper, NOT the
    // small-handle Database the user passed), and every downstream
    // `this.client.prepare(...)` failed with `prepare is not a function`.
    // Refs #645 deeper followup, #488.
    loop {
        let mut new_func_entries: Vec<((String, String), usize, bool)> = Vec::new();
        for (path, hir_module) in &ctx.native_modules {
            let path_str = path.to_string_lossy().to_string();
            for export in &hir_module.exports {
                match export {
                    perry_hir::Export::ExportAll { source } => {
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), &param_count) in &exported_func_param_counts
                            {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), func_name.clone());
                                    if !exported_func_param_counts.contains_key(&key) {
                                        let has_rest = exported_func_has_rest
                                            .get(&(src_path.clone(), func_name.clone()))
                                            .copied()
                                            .unwrap_or(false);
                                        new_func_entries.push((key, param_count, has_rest));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), &param_count) in &exported_func_param_counts
                            {
                                if src_path == &source_path_str && func_name == imported {
                                    let key = (path_str.clone(), exported.clone());
                                    if !exported_func_param_counts.contains_key(&key) {
                                        let has_rest = exported_func_has_rest
                                            .get(&(src_path.clone(), func_name.clone()))
                                            .copied()
                                            .unwrap_or(false);
                                        new_func_entries.push((key, param_count, has_rest));
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::Named { local, exported } => {
                        for import in &hir_module.imports {
                            for spec in &import.specifiers {
                                let (matches, imported_name) = match spec {
                                    perry_hir::ImportSpecifier::Named { local: l, imported } => {
                                        (l == local, imported.clone())
                                    }
                                    perry_hir::ImportSpecifier::Default { local: l } => {
                                        (l == local, "default".to_string())
                                    }
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(
                                        &import.source,
                                        path,
                                        &ctx.project_root,
                                        &ctx.compile_packages,
                                        &ctx.compile_package_dirs,
                                    ) {
                                        let source_path_str =
                                            resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(&param_count) =
                                            exported_func_param_counts.get(&key_src)
                                        {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_func_param_counts.contains_key(&key) {
                                                let has_rest = exported_func_has_rest
                                                    .get(&key_src)
                                                    .copied()
                                                    .unwrap_or(false);
                                                new_func_entries.push((key, param_count, has_rest));
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
        if new_func_entries.is_empty() {
            break;
        }
        for (key, param_count, has_rest) in new_func_entries {
            exported_func_param_counts.insert(key.clone(), param_count);
            if has_rest {
                exported_func_has_rest.insert(key, true);
            }
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
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), return_type) in &exported_func_return_types
                            {
                                if src_path == &source_path_str {
                                    let key = (path_str.clone(), func_name.clone());
                                    if !exported_func_return_types.contains_key(&key) {
                                        new_func_entries.push((key.clone(), return_type.clone()));
                                    }
                                    let async_key = (source_path_str.clone(), func_name.clone());
                                    let propagated_async_key =
                                        (path_str.clone(), func_name.clone());
                                    if exported_async_funcs.contains(&async_key)
                                        && !exported_async_funcs.contains(&propagated_async_key)
                                    {
                                        new_async_entries.push(propagated_async_key);
                                    }
                                }
                            }
                        }
                    }
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            let source_path_str = resolved_source.to_string_lossy().to_string();
                            for ((src_path, func_name), return_type) in &exported_func_return_types
                            {
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
                                    perry_hir::ImportSpecifier::Named { local: l, imported } => {
                                        (l == local, imported.clone())
                                    }
                                    perry_hir::ImportSpecifier::Default { local: l } => {
                                        (l == local, "default".to_string())
                                    }
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(
                                        &import.source,
                                        path,
                                        &ctx.project_root,
                                        &ctx.compile_packages,
                                        &ctx.compile_package_dirs,
                                    ) {
                                        let source_path_str =
                                            resolved_source.to_string_lossy().to_string();
                                        let key_src = (source_path_str, imported_name);
                                        if let Some(return_type) =
                                            exported_func_return_types.get(&key_src)
                                        {
                                            let key = (path_str.clone(), exported.clone());
                                            if !exported_func_return_types.contains_key(&key) {
                                                new_func_entries
                                                    .push((key.clone(), return_type.clone()));
                                            }
                                            let propagated_async_key =
                                                (path_str.clone(), exported.clone());
                                            if exported_async_funcs.contains(&key_src)
                                                && !exported_async_funcs
                                                    .contains(&propagated_async_key)
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
        if new_func_entries.is_empty() && new_async_entries.is_empty() {
            break;
        }
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
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
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
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        if let Some((resolved_source, _)) = resolve_import(
                            source,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
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
                                    perry_hir::ImportSpecifier::Named { local: l, imported } => {
                                        (l == local, imported.clone())
                                    }
                                    perry_hir::ImportSpecifier::Default { local: l } => {
                                        (l == local, "default".to_string())
                                    }
                                    _ => (false, String::new()),
                                };
                                if matches {
                                    if let Some((resolved_source, _)) = resolve_import(
                                        &import.source,
                                        path,
                                        &ctx.project_root,
                                        &ctx.compile_packages,
                                        &ctx.compile_package_dirs,
                                    ) {
                                        let source_path_str =
                                            resolved_source.to_string_lossy().to_string();
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
        if new_entries.is_empty() {
            break;
        }
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
    if matches!(
        target.as_deref(),
        Some("harmonyos") | Some("harmonyos-simulator")
    ) && find_harmonyos_sdk().is_none()
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
        let mut features: Vec<String> = features_str
            .split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        let is_mobile = matches!(
            target.as_deref(),
            Some("ios")
                | Some("ios-simulator")
                | Some("visionos")
                | Some("visionos-simulator")
                | Some("android")
                | Some("wearos")
                | Some("watchos")
                | Some("watchos-simulator")
                | Some("tvos")
                | Some("tvos-simulator")
                | Some("harmonyos")
                | Some("harmonyos-simulator")
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
        if matches!(
            target.as_deref(),
            Some("harmonyos") | Some("harmonyos-simulator")
        ) && !features.iter().any(|f| f == "ohos-napi")
        {
            features.push("ohos-napi".to_string());
        }
        features
    } else if matches!(
        target.as_deref(),
        Some("harmonyos") | Some("harmonyos-simulator")
    ) {
        // User didn't pass --features at all; still auto-enable ohos-napi.
        vec!["ohos-napi".to_string()]
    } else {
        Vec::new()
    };

    // Pre-compute native library FFI functions
    let ffi_functions: Vec<(
        String,
        Vec<perry_api_manifest::NativeAbiType>,
        perry_api_manifest::NativeAbiType,
    )> = ctx
        .native_libraries
        .iter()
        .flat_map(|lib| {
            lib.functions
                .iter()
                .map(|f| (f.name.clone(), f.params.clone(), f.returns.clone()))
        })
        .collect();

    // #1110 (follow-up): every loaded `perry.nativeLibrary` static
    // archive carries unresolved references to `perry_ffi_promise_new`
    // / `perry_ffi_promise_resolve_bits` / `perry_ffi_spawn_blocking`
    // (the C-ABI shims that perry-ffi declares and perry-stdlib
    // defines — see `crates/perry-stdlib/src/perry_ffi_async.rs`).
    // Wrappers like `@perryts/storekit` invariably use them — every
    // `returns: "promise"` manifest entry compiles to a perry-ffi
    // call site that pulls the symbol in. If the user's TS source
    // never touched anything else from `perry-stdlib`'s surface, the
    // existing `ctx.needs_stdlib` heuristic stayed `false` and the
    // link command was `Linking (runtime-only)…`, with the
    // perry_ffi_* symbols then surfacing as `Undefined symbols for
    // architecture arm64` at the final ld step. Force-enable stdlib
    // linkage whenever any nativeLibrary manifest is loaded.
    if !ctx.native_libraries.is_empty() {
        ctx.needs_stdlib = true;
    }

    // Pre-compute JS module specifiers in canonical order before this
    // graph-wide list is cloned into every module's CompileOptions and
    // object-cache key.
    let mut js_module_specifiers: Vec<String> = ctx.js_modules.keys().cloned().collect();
    js_module_specifiers.sort();

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
    let bitcode_link = std::env::var("PERRY_LLVM_BITCODE_LINK").ok().as_deref() == Some("1");

    // V2.2: Per-module object cache at `<cache_dir>/objects/<target>/<key>.o`.
    // Disabled when the user passed `--no-cache`, when `PERRY_NO_CACHE=1`, or
    // when we're in bitcode-link mode (the artifacts aren't object files), or
    // when native-region verification is enabled and lowering must run.
    // Key derivation: `compute_object_cache_key(opts, source_hash, perry_version)`.
    let cache_env_disabled = std::env::var("PERRY_NO_CACHE").ok().as_deref() == Some("1");
    let verify_native_regions = args.verify_native_regions
        || args.explain_lowering
        || std::env::var("PERRY_VERIFY_NATIVE_REGIONS").ok().as_deref() == Some("1");
    let disable_buffer_fast_path = args.disable_buffer_fast_path
        || std::env::var("PERRY_DISABLE_BUFFER_FAST_PATH")
            .ok()
            .as_deref()
            == Some("1");
    let cache_enabled =
        !args.no_cache && !cache_env_disabled && !bitcode_link && !verify_native_regions;
    // Target dir name for the cache layout. Using the resolved LLVM triple
    // keeps cross-compile caches from colliding with native-host caches.
    let cache_target_dir = target.as_deref().unwrap_or("host");
    let object_cache = ObjectCache::new(&ctx.cache_dir, cache_target_dir, cache_enabled);
    let perry_version = env!("CARGO_PKG_VERSION");

    // Issue #100: precompute the dynamic-import plumbing so the rayon
    // per-module compile worker has everything it needs.
    //
    //  1. `dyn_target_paths`: every native-module path that is the
    //     target of at least one `await import("...")` site anywhere
    //     in the program. Those modules need a `__perry_ns_<prefix>`
    //     global emitted + populated at the end of their `__init`.
    //  2. `path_to_module_name`: lookup from resolved path back to the
    //     `Module::name` string used for flatten_exports / Export
    //     source-key resolution.
    //  3. `per_module_namespace_entries`: for each dynamic-import
    //     target, the resolved `NamespaceEntry` list — driven by
    //     `flatten_exports` then enriched with kind info (Var /
    //     Function / Class / NestedNamespace) by walking the source
    //     module's HIR. Computed once here so the parallel codegen
    //     workers don't need cross-module HIR access.
    //  4. `per_module_dyn_import_targets`: for each module's own
    //     `Expr::DynamicImport` sites, the map from path-arg string
    //     to target sanitized prefix. Codegen at the dispatch site
    //     reads `@__perry_ns_<target_prefix>`.
    let sanitize_module_name = |s: &str| -> String {
        let mut out: String = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if out
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            out.insert(0, '_');
        }
        out
    };
    let mut path_to_module_name: HashMap<PathBuf, String> = HashMap::new();
    let mut module_name_to_path: HashMap<String, PathBuf> = HashMap::new();
    for (path, hir_module) in &ctx.native_modules {
        path_to_module_name.insert(path.clone(), hir_module.name.clone());
        module_name_to_path.insert(hir_module.name.clone(), path.clone());
    }
    // Build a normalized HIR-by-name map for `flatten_exports`. Each
    // module's `Export::ReExport::source`, `Export::ExportAll::source`,
    // and `Export::NamespaceReExport::source` strings hold the raw
    // specifier as written in source (`"./inner.ts"`); flatten_exports
    // keys its lookup on `Module::name`. Rewrite the source field of
    // every export to the target module's `Module::name` (via
    // `resolve_import` → `path_to_module_name`) so the cross-module
    // lookup resolves the right HIR.
    let mut module_name_to_module: HashMap<String, perry_hir::Module> = HashMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let mut rewritten = hir_module.clone();
        for export in rewritten.exports.iter_mut() {
            match export {
                perry_hir::Export::ReExport { source, .. }
                | perry_hir::Export::ExportAll { source }
                | perry_hir::Export::NamespaceReExport { source, .. } => {
                    if let Some((resolved_path, _)) = resolve_import(
                        source,
                        path,
                        &ctx.project_root,
                        &ctx.compile_packages,
                        &ctx.compile_package_dirs,
                    ) {
                        if let Some(name) = path_to_module_name.get(&resolved_path) {
                            *source = name.clone();
                        }
                    }
                }
                perry_hir::Export::Named { .. } => {}
            }
        }
        module_name_to_module.insert(hir_module.name.clone(), rewritten);
    }
    // Set of native-module paths that are dynamic-import targets. We
    // also build a parallel set keyed by Module::name for flatten_exports.
    let mut dyn_target_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for hir_module in ctx.native_modules.values() {
        for import in &hir_module.imports {
            // `is_dynamic` covers dynamic-only synthetic edges;
            // `is_dynamic_target` (#1672) covers a static edge that is
            // ALSO the target of a dynamic `import()` in the same module.
            // Both need the target to emit `@__perry_ns_<prefix>`.
            if !(import.is_dynamic || import.is_dynamic_target) {
                continue;
            }
            if let Some(rp) = &import.resolved_path {
                dyn_target_paths.insert(PathBuf::from(rp));
            }
        }
    }
    // Per-module precomputed namespace_entries (keyed by path).
    let mut per_module_namespace_entries: HashMap<PathBuf, Vec<perry_codegen::NamespaceEntry>> =
        HashMap::new();
    for target_path in &dyn_target_paths {
        let target_hir = match ctx.native_modules.get(target_path) {
            Some(m) => m,
            None => continue, // native/JS module — handled elsewhere
        };
        let target_name = target_hir.name.clone();
        let lookup = |s: &str| module_name_to_module.get(s);
        let flat = perry_hir::flatten_exports(&target_name, &lookup);
        let mut entries: Vec<perry_codegen::NamespaceEntry> = Vec::new();
        for fe in flat {
            // Locate source module's HIR (where the binding lives).
            let source_mod = module_name_to_module.get(&fe.source_module);
            let source_prefix = source_mod
                .map(|m| sanitize_module_name(&m.name))
                .unwrap_or_else(|| sanitize_module_name(&fe.source_module));
            let kind = if let Some(nested) = &fe.nested_namespace_of {
                let nested_prefix = module_name_to_module
                    .get(nested)
                    .map(|m| sanitize_module_name(&m.name))
                    .unwrap_or_else(|| sanitize_module_name(nested));
                perry_codegen::NamespaceEntryKind::NestedNamespace {
                    source_prefix: nested_prefix,
                }
            } else if fe.source_module == target_name {
                // Local binding — find what kind it is in target_hir.
                if let Some(func) = target_hir
                    .functions
                    .iter()
                    .find(|f| f.name == fe.source_local)
                {
                    let scoped = format!(
                        "perry_fn_{}__{}",
                        sanitize_module_name(&target_hir.name),
                        sanitize_module_name(&func.name)
                    );
                    perry_codegen::NamespaceEntryKind::LocalFunction {
                        wrap_symbol: format!("__perry_wrap_{}", scoped),
                    }
                } else if let Some(class) = target_hir
                    .classes
                    .iter()
                    .find(|c| c.name == fe.source_local)
                {
                    perry_codegen::NamespaceEntryKind::LocalClass { class_id: class.id }
                } else if let Some(global) = target_hir
                    .globals
                    .iter()
                    .find(|g| g.name == fe.source_local)
                {
                    let gname = format!(
                        "perry_global_{}__{}",
                        sanitize_module_name(&target_hir.name),
                        global.id
                    );
                    perry_codegen::NamespaceEntryKind::LocalVar { global_name: gname }
                } else {
                    // Best-effort: treat unknown locals as Var sourced
                    // by getter. This covers re-export shapes that the
                    // local-detection misses; the cross-module getter
                    // for the same module returns the value too.
                    perry_codegen::NamespaceEntryKind::ForeignVar {
                        source_prefix: sanitize_module_name(&target_hir.name),
                        source_local: fe.source_local.clone(),
                    }
                }
            } else {
                // Cross-module binding. Determine if it's a function in
                // the source module so codegen can emit the closure
                // singleton path; otherwise treat as a foreign var
                // (`perry_fn_<src>__<local>()` getter).
                if let Some(src) = source_mod {
                    if let Some(func) = src.functions.iter().find(|f| f.name == fe.source_local) {
                        perry_codegen::NamespaceEntryKind::ForeignFunction {
                            source_prefix: source_prefix.clone(),
                            source_local: fe.source_local.clone(),
                            param_count: func.params.len(),
                        }
                    } else if let Some(class) =
                        src.classes.iter().find(|c| c.name == fe.source_local)
                    {
                        perry_codegen::NamespaceEntryKind::LocalClass { class_id: class.id }
                    } else {
                        perry_codegen::NamespaceEntryKind::ForeignVar {
                            source_prefix: source_prefix.clone(),
                            source_local: fe.source_local.clone(),
                        }
                    }
                } else {
                    perry_codegen::NamespaceEntryKind::ForeignVar {
                        source_prefix: source_prefix.clone(),
                        source_local: fe.source_local.clone(),
                    }
                }
            };
            entries.push(perry_codegen::NamespaceEntry {
                name: fe.name,
                kind,
            });
        }
        per_module_namespace_entries.insert(target_path.clone(), entries);
    }
    // For each consumer module, map every `Expr::DynamicImport` arg-path
    // string (as resolved in `collect_modules`) to the target's
    // sanitized prefix. Built by scanning the consumer's imports for
    // `is_dynamic == true` (dynamic-only edges) or `is_dynamic_target ==
    // true` (#1672: a static edge that is also a dynamic-import target)
    // and reading the `source` + `resolved_path`.
    let mut per_module_dyn_import_targets: HashMap<PathBuf, HashMap<String, String>> =
        HashMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let mut local_map: HashMap<String, String> = HashMap::new();
        for import in &hir_module.imports {
            if !(import.is_dynamic || import.is_dynamic_target) {
                continue;
            }
            let rp = match &import.resolved_path {
                Some(p) => PathBuf::from(p),
                None => {
                    // #1671: a dynamic `import('hono/jsx/server')` resolves to a
                    // known node-submodule with no compiled-source backing — the
                    // runtime ships its namespace. Record a sentinel prefix the
                    // dynamic-import codegen recognises and routes to
                    // `js_node_submodule_namespace` (instead of rejecting).
                    if let Some(key) =
                        self::collect_modules::known_node_submodule_key(&import.source)
                    {
                        local_map.insert(import.source.clone(), format!("__node_submod__{}", key));
                    } else if import.is_native {
                        // #1673: a dynamic `import('node:crypto')` /
                        // `import('node:util')` targets a general native builtin
                        // that is NOT in the node-submodule table and has no
                        // compiled-source backing. The runtime builds its
                        // namespace object via `js_create_native_module_namespace`
                        // (the same object `require('node:crypto')` and `import *
                        // as` produce). Record a `__native_mod__<name>` sentinel,
                        // keyed by the `node:`-stripped module name, that the
                        // dynamic-import codegen routes to that builder. An
                        // unsupported builtin never reaches here (`is_native` is
                        // false for it → no map entry → the dispatch rejects,
                        // matching Node's failure mode).
                        let native_name = import
                            .source
                            .strip_prefix("node:")
                            .unwrap_or(&import.source);
                        local_map.insert(
                            import.source.clone(),
                            format!("__native_mod__{}", native_name),
                        );
                    }
                    continue;
                }
            };
            let target_name = match path_to_module_name.get(&rp) {
                Some(n) => n.clone(),
                None => continue,
            };
            let target_prefix = sanitize_module_name(&target_name);
            local_map.insert(import.source.clone(), target_prefix);
        }
        if !local_map.is_empty() {
            per_module_dyn_import_targets.insert(path.clone(), local_map);
        }
    }

    let total_codegen_modules = ctx.native_modules.len();
    let codegen_modules_started = AtomicUsize::new(0);
    // Per-invocation object staging dir (2026-07-02 audit fleet P0).
    // Objects used to land at CWD-relative name-only paths, so two
    // concurrent perry compiles sharing a working directory overwrote each
    // other's `<module>.o` mid-link and each deleted the other's objects
    // afterwards — deterministically wrong binaries whenever the object
    // cache was bypassed (--no-cache / trace modes / store errors), and the
    // fixed-name stub objects collided even with the cache ON. pid + a
    // strictly-monotonic wall component (the linker.rs #509 discipline)
    // keeps simultaneous invocations disjoint; the dir is removed with the
    // intermediates below.
    let object_output_dir = {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("perry-objs-{}-{}", std::process::id(), nanos));
        std::fs::create_dir_all(&dir)?;
        dir
    };
    let compile_results: Vec<Result<NativeObjectArtifact, String>> = ctx
        .native_modules
        .par_iter()
        .map(|(path, hir_module)| {
            // Compile this module to LLVM IR (or .ll text in bitcode-link mode)
            // and return the object bytes for the linker to consume.
            let codegen_index = codegen_modules_started.fetch_add(1, Ordering::Relaxed) + 1;
            progress.record(ProgressSnapshot {
                stage: "codegen",
                module_path: Some(path),
                module_name: Some(&hir_module.name),
                visited: Some(codegen_index),
                total: Some(total_codegen_modules),
                collected: Some(total_codegen_modules),
                ..Default::default()
            });
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
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                if out
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false)
                {
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
            // Issue #753: every module receives the program-wide set of
            // Deferred module prefixes. The entry main filters these
            // out of its eager init call sequence; non-entry modules
            // ignore it. Empty when no module in the program is
            // Deferred (i.e. no dynamic `import()` sites).
            let deferred_module_prefixes: std::collections::HashSet<String> = ctx
                .native_modules
                .iter()
                .filter(|(_, m)| m.init_kind == perry_hir::ModuleInitKind::Deferred)
                .map(|(_, m)| sanitize_name(&m.name))
                .collect();
            // Next.js wall 54 (part 2): `(absolute_path, prefix)` for every
            // `.next/server/**` runtime module so the entry's `main` can record
            // its `__init` address by path (`js_register_path_init`). Only the
            // entry emits these; the runtime `require(absolutePath)` shim then
            // triggers the matching module's lazy init on first load.
            let nextjs_path_init_modules: Vec<(String, String)> = if is_entry {
                ctx.native_modules
                    .iter()
                    .filter(|(p, _)| {
                        self::collect_modules::is_nextjs_runtime_module(p)
                    })
                    .map(|(p, m)| {
                        (p.to_string_lossy().into_owned(), sanitize_name(&m.name))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            // Issue #753: prefixes of this module's static-import +
            // re-export source modules (non-entry only — the entry's
            // body is in `main`, not a `__init`). The wrapper at
            // `<prefix>__init` calls each dep's `__init` before
            // dispatching to `<prefix>__init_body`; this transitively
            // initializes any Deferred dep reached only through this
            // module's re-export chain. For Eager modules the calls
            // short-circuit on the idempotent guard's first-write
            // check (one load + cmp + cond_br each).
            let module_init_deps: Vec<String> = if is_entry {
                Vec::new()
            } else {
                let mut deps: Vec<String> = Vec::new();
                let mut seen: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let entry_prefix = ctx
                    .native_modules
                    .get(&entry_path)
                    .map(|m| sanitize_name(&m.name));
                let push_dep = |deps: &mut Vec<String>,
                                seen: &mut std::collections::HashSet<String>,
                                prefix: String| {
                    if Some(&prefix) == entry_prefix.as_ref() {
                        return;
                    }
                    if seen.insert(prefix.clone()) {
                        deps.push(prefix);
                    }
                };
                for import in &hir_module.imports {
                    // `is_deferred_require`: a function-local `require('S')`
                    // (lazy in Node). S must NOT chain into this module's init
                    // — it inits only when the require shim is actually called.
                    if import.is_dynamic || import.type_only || import.is_deferred_require {
                        continue;
                    }
                    if let Some(resolved) = &import.resolved_path {
                        let resolved_path = PathBuf::from(resolved);
                        if let Some(src_mod) = ctx.native_modules.get(&resolved_path) {
                            push_dep(&mut deps, &mut seen, sanitize_name(&src_mod.name));
                        }
                    }
                }
                for export in &hir_module.exports {
                    let src = match export {
                        perry_hir::Export::ExportAll { source } => Some(source.clone()),
                        perry_hir::Export::ReExport { source, .. } => Some(source.clone()),
                        perry_hir::Export::NamespaceReExport { source, .. } => {
                            Some(source.clone())
                        }
                        perry_hir::Export::Named { .. } => None,
                    };
                    if let Some(src) = src {
                        if let Some((resolved_path, _)) = resolve_import(
                            &src,
                            path,
                            &ctx.project_root,
                            &ctx.compile_packages,
                            &ctx.compile_package_dirs,
                        ) {
                            if let Some(src_mod) = ctx.native_modules.get(&resolved_path) {
                                push_dep(&mut deps, &mut seen, sanitize_name(&src_mod.name));
                            }
                        }
                    }
                }
                deps
            };
            // Build import → source-prefix table for cross-module
            // ExternFuncRef calls. For each Named import in this
            // module, look up the source module's HIR by resolved
            // path and capture its name. The LLVM codegen uses this
            // to generate `perry_fn_<source_prefix>__<name>`.
            let mut import_function_prefixes: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            // Issue #5621: ergonomic camelCase binding → snake_case
            // `js_<pkg>_*` FFI symbol. A `perry.nativeLibrary` package may
            // expose spec-faithful camelCase exports (`requestAdapter`)
            // over its manifest symbols (`js_webgpu_request_adapter`). When
            // a specifier matches a manifest function via the standard
            // `js_<pkg>_<snake>` ⇒ camelCase derivation (rather than a
            // byte-for-byte name match), we skip the wrapper registration
            // below AND record the alias here so the call site
            // (`lower_call`) rewrites the binding to its manifest symbol.
            let mut import_function_ffi_aliases: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            // Issue #678: parallel to `import_function_prefixes`. When the
            // import traverses a re-export rename (`export { default as render
            // } from './render.js'`), the consumer sees `render` but the
            // origin module emits the symbol with its own export name
            // (`default`). This map captures the consumer-name → origin-name
            // override so every `perry_fn_<src>__<suffix>` construction site
            // can pick the right suffix. Absent entries (the common case)
            // mean no rename — the consumer name is the origin name.
            let mut import_function_origin_names:
                std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            // Issue #678 followup: imports landing in `ModuleKind::Interpreted`
            // (V8 fallback). The codegen probes this map BEFORE
            // `perry_fn_<src>__<name>` symbol formation and routes hits
            // through `js_call_v8_export(specifier, name, args, argc)`.
            // Pre-fix, V8-backed imports were silently dropped from
            // `import_function_prefixes`, so the consumer's call
            // emitted a bare `call double @<name>` against an
            // undefined symbol — every `import { render } from "ink"`
            // (or similar where the package fell back to V8) failed at
            // link time with `Undefined symbols: _perry_fn_..._render`.
            let mut import_function_v8_specifiers:
                std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            // Issue #841: named-import → (submodule_key, exported_name)
            // for the five recognized Node submodules with no perry-stdlib
            // backing. Populated by a dedicated pass below; consumed by
            // codegen's `Expr::ExternFuncRef` value-form catch-all.
            let mut import_function_node_submodule:
                std::collections::HashMap<String, (String, String)> =
                std::collections::HashMap::new();
            // Issue #841 companion: local-namespace → submodule_key for
            // `import * as ns from "node:<submod>"`.
            let mut namespace_node_submodules:
                std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            // Issue #678 followup (namespace branch): local-namespace →
            // V8 module specifier for `import * as ns from "<v8-module>"`.
            // Populated in the V8-imports pass below at the same site that
            // would otherwise no-op on `ImportSpecifier::Namespace`. Used
            // by codegen's StaticMethodCall / namespace-member-call
            // lowering to route `ns.member(args)` through
            // `js_call_v8_export` when nothing else seeded
            // `import_function_prefixes` for the member.
            let mut namespace_v8_specifiers:
                std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            // Issue #680: per-namespace member resolution. Disambiguates
            // `random.make` vs `tracer.make` when multiple namespaces
            // export the same member name. Keyed by `(namespace_local,
            // member_name)` → `source_prefix`.
            let mut namespace_member_prefixes: std::collections::HashMap<(String, String), String> =
                std::collections::HashMap::new();
            // Issue #5924 (companion to #680/#678): per-namespace origin-name
            // resolution. `import_function_origin_names` is flat (keyed by
            // bare member name), so when two namespaces imported into the
            // same file both have a member with the same name and only ONE
            // of them is a re-export rename, the rename's origin-name
            // override clobbers the other namespace's (correct, unrenamed)
            // suffix. Keyed by `(namespace_local, member_name)` →
            // `origin_name`, mirroring `namespace_member_prefixes`.
            let mut namespace_member_origin_names:
                std::collections::HashMap<(String, String), String> =
                std::collections::HashMap::new();
            let mut namespace_imports: Vec<String> = Vec::new();
            // Issue #321: subset of `namespace_imports` populated only by the
            // named-import-of-namespace-reexport branch below (`import { Effect
            // } from "effect"` where effect's index.ts has `export * as Effect
            // from "./Effect.js"`). The codegen's StaticMethodCall arm consults
            // this to decide whether it can route var-shape members through
            // `js_closure_callN`; see the field doc in codegen.rs.
            let mut namespace_reexport_named_imports: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut imported_classes: Vec<perry_codegen::ImportedClass> = Vec::new();
            let mut imported_enums: Vec<(String, Vec<(String, perry_hir::EnumValue)>)> = Vec::new();
            let mut imported_async_set: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut imported_param_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut imported_return_types: std::collections::HashMap<String, perry_types::Type> =
                std::collections::HashMap::new();
            // Issue #608 — set of imported function names whose source-side
            // signature has a trailing `...rest` parameter. Built alongside
            // `imported_param_counts` from the source module's
            // `exported_func_has_rest` table; consulted by the cross-module
            // call site in `lower_call.rs` to bundle trailing args into a
            // single rest array. Sparse set (only `true` entries stored).
            let mut imported_has_rest: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            // #1816: imported functions whose trailing param is the synthesized
            // `arguments` rest — the cross-module call must bundle ALL args into
            // it, not just trailing. Built alongside `imported_has_rest`.
            let mut imported_synthetic_arguments: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut imported_vars: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            // Issue #629: register namespace imports BEFORE the main
            // resolution loop so unresolved-source bindings still flow
            // to the codegen's `namespace_imports` set. Without this,
            // the early `continue` for unresolved imports below means
            // `import * as fsp from "node:fs/promises"` (when
            // fs/promises has no perry-stdlib backing) leaves `fsp`
            // off the namespace list — the catch-all in
            // `Expr::ExternFuncRef` then returns TAG_TRUE and
            // `typeof fsp === "boolean"`. Registering here lets the
            // catch-all route through `js_unresolved_namespace_stub`
            // (typeof "object", missing properties → undefined).
            //
            // Issue #684: skip WHOLE-DECL type-only imports
            // (`import type * as X from "..."`). They're erased at
            // runtime — the local binding never appears in any
            // value-position expression, so registering it as a
            // namespace would only widen the per-namespace member
            // map below. Per-specifier type-only (`import { type Foo,
            // bar }`) is still handled because the same import has
            // value specifiers; the whole-decl flag is the one that
            // makes the entire import a no-op.
            for import in &hir_module.imports {
                if import.type_only {
                    continue;
                }
                for spec in &import.specifiers {
                    if let perry_hir::ImportSpecifier::Namespace { local } = spec {
                        if !namespace_imports.contains(local) {
                            namespace_imports.push(local.clone());
                        }
                    }
                }
            }

            for import in &hir_module.imports {
                if import.module_kind != perry_hir::ModuleKind::NativeCompiled {
                    continue;
                }
                // Issue #684: skip WHOLE-DECL type-only imports
                // (`import type * as X`, `import type { Foo }`). They
                // contribute zero runtime state — neither the namespace
                // binding nor the named members ever appear in a
                // value-position expression after type erasure. Pre-fix
                // the loop below treated them like value imports and
                // registered every export of the source module into
                // `import_function_prefixes` / `namespace_member_prefixes`,
                // which collided with later named-import registrations:
                //   effect's `ParseResult.ts` has both
                //     `import { TaggedError } from "./Data.js"`
                //     `import type * as Schema from "./Schema.js"`
                //   Schema.ts also exports `TaggedError`, so the type-only
                //   loop iteration registered `TaggedError → Schema_ts`
                //   into `import_function_prefixes`. If Schema.ts was
                //   processed AFTER Data.ts (HashMap iteration order is
                //   unstable), the Schema entry won — and top-level
                //   `class ParseError extends TaggedError("ParseError")`
                //   dispatched into Schema.ts's `TaggedError` instead of
                //   Data.ts's. Worse, Schema.ts is type-only so it isn't
                //   in `module_init_deps` either, meaning its backing
                //   global was still 0.0 — `js_closure_call1(0.0, ...)`
                //   threw `TypeError: value is not a function` during
                //   `ParseResult.ts__init`. Closes #684 (companion to
                //   #680's `module_init_deps` filter at L3234).
                if import.type_only {
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
                // PerryTS/storekit#1: when the import source is a package that
                // declares `perry.nativeLibrary` (e.g. `@perryts/storekit`),
                // its `.ts` source is a wrapper holding ambient `export
                // declare function` signatures — the real implementation lives
                // in the linked static library. There is no Perry wrapper
                // symbol `perry_fn_<src>__<name>` for the source to emit, so
                // registering the FFI specifier in `import_function_prefixes`
                // would route the caller through an undefined wrapper and
                // fail at link time. The per-specifier skip below lets
                // `lower_call.rs` fall through to the FFI-manifest path
                // (consults `ctx.ffi_signatures`, emits the call against the
                // FFI symbol declared in `package.json :: perry.nativeLibrary.
                // functions` plus a matching `declare external`).
                let native_library_for_import = ctx
                    .native_libraries
                    .iter()
                    .find(|nl| nl.module == import.source);

                for spec in &import.specifiers {
                    // Handle namespace imports (import * as X).
                    //
                    // Issue #4872: a DEFAULT import of a compiled module that
                    // has NO `default` export gets the same treatment. The
                    // CJS wrap lowers every `require('X')` to `import _req_N
                    // from 'X'`; when X resolves to an ESM barrel with only
                    // named exports (rxjs's src/index.ts, uid's index.mjs) or
                    // to a type-only interface surface with no exports at all
                    // (nestjs dist `*.interface.js`), there is no
                    // `perry_fn_<src>__default` symbol for the consumer to
                    // bind — the old fall-through registered the local as a
                    // callable function import and the link died on
                    // `__perry_wrap_perry_fn_<src>__default`. Node's
                    // `require(esm)` semantics hand back the module namespace
                    // object, so route the local through the namespace
                    // machinery: member reads resolve per-export to origin
                    // symbols, and a whole-value read materializes the
                    // namespace object (empty for zero-export modules).
                    let namespace_like_local: Option<&String> = match spec {
                        perry_hir::ImportSpecifier::Namespace { local } => Some(local),
                        perry_hir::ImportSpecifier::Default { local }
                            if !all_module_exports
                                .get(&resolved_path_str)
                                .is_some_and(|exports| exports.contains_key("default")) =>
                        {
                            Some(local)
                        }
                        _ => None,
                    };
                    if let Some(local) = namespace_like_local {
                        namespace_imports.push(local.clone());
                        // Register all exports from the source module
                        if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                            for (export_name, origin_path) in exports {
                                let origin_prefix =
                                    compute_module_prefix(origin_path, &ctx.project_root);
                                // Issue #5927: namespace members are a
                                // best-effort fallback in the flat
                                // `import_function_prefixes` map — the
                                // authoritative lookup for genuine
                                // namespace-member accesses is the
                                // per-namespace `namespace_member_prefixes`
                                // map populated unconditionally below. Use
                                // `or_insert` (never overwrite) so a PLAIN
                                // named import of the same bare name (which
                                // has NO other resolution path — a bare
                                // call has no namespace to scope against)
                                // always wins the flat map, regardless of
                                // which import statement is processed
                                // first. Pre-fix, `import { omit } from
                                // "remeda"` in the same file as `import {
                                // Context } from "effect"` (where
                                // effect's Context.ts also exports `omit`)
                                // meant whichever import was LATER in
                                // source order silently overwrote the
                                // other's flat-map entry — opencode's
                                // `provider.ts` has `Context` imported
                                // after `omit`, so `omit(...)` (a bare
                                // remeda call) resolved against
                                // `Context.ts`'s prefix instead of
                                // remeda's chunk.
                                import_function_prefixes
                                    .entry(export_name.clone())
                                    .or_insert_with(|| origin_prefix.clone());
                                // Issue #678: surface origin-name overrides
                                // for namespace-imported members too. A
                                // member reached via a re-export rename
                                // (`export { default as foo }`) needs the
                                // codegen to call `perry_fn_<origin>__default`
                                // when the consumer writes `ns.foo()`.
                                let resolved_origin_name = all_module_export_origin_names
                                    .get(&resolved_path_str)
                                    .and_then(|m| m.get(export_name))
                                    .cloned();
                                if let Some(ref origin_name) = resolved_origin_name {
                                    if origin_name != export_name {
                                        // Issue #5927: same `or_insert`
                                        // rationale as `import_function_prefixes`
                                        // above — never let a namespace
                                        // member's origin-name rename
                                        // overwrite a plain import's entry.
                                        import_function_origin_names
                                            .entry(export_name.clone())
                                            .or_insert_with(|| origin_name.clone());
                                    }
                                }
                                // Issue #5924: unconditionally register every
                                // member under the per-namespace key —
                                // mirrors `namespace_member_prefixes` below,
                                // which is also populated for every export,
                                // not just renamed ones. A *sparse* map here
                                // (only renamed members) would still force a
                                // fallback to the flat
                                // `import_function_origin_names` for
                                // unrenamed members, and that flat entry may
                                // belong to a DIFFERENT namespace imported
                                // into the same file. Storing every member's
                                // resolved suffix (renamed or not) lets the
                                // consumer treat a namespace hit as
                                // authoritative and never fall through.
                                namespace_member_origin_names.insert(
                                    (local.clone(), export_name.clone()),
                                    resolved_origin_name
                                        .clone()
                                        .unwrap_or_else(|| export_name.clone()),
                                );
                                // Issue #680: also register under the
                                // per-namespace key so `random.make` and
                                // `tracer.make` can be disambiguated.
                                namespace_member_prefixes.insert(
                                    (local.clone(), export_name.clone()),
                                    origin_prefix.clone(),
                                );

                                let key = (origin_path.clone(), export_name.clone());
                                if let Some(&param_count) = exported_func_param_counts.get(&key) {
                                    imported_param_counts.insert(export_name.clone(), param_count);
                                }
                                if exported_func_has_rest.get(&key).copied().unwrap_or(false) {
                                    imported_has_rest.insert(export_name.clone());
                                }
                                if exported_func_synthetic_arguments.contains(&key) {
                                    imported_synthetic_arguments.insert(export_name.clone());
                                }
                                // Issue #636: namespace-imported vars must
                                // route through the zero-arg getter at
                                // call sites (`ns.fn(args)` where `fn` is a
                                // `let`/`const` binding holding a closure
                                // — the canonical `export const make = (s)
                                // => ...` shape). Without this, the codegen
                                // falls through to the direct-call path
                                // which treats the getter's return value
                                // as the call result instead of invoking
                                // the closure with `args`. Mirrors the
                                // named-import branch at the var-detection
                                // arm below.
                                //
                                // Issue #4841: when the namespace member is a
                                // re-export of a CJS submodule's `default`
                                // (`import sfy from './sfy'; export { sfy }`,
                                // where `./sfy` is `module.exports = function`),
                                // the origin module records the var under its
                                // "default" suffix — NOT the consumer-visible
                                // member name. Probe both keys (mirrors the
                                // named-import arm) so the var-vs-function
                                // classification fires; otherwise `ns.sfy` takes
                                // the function path and wraps the default getter
                                // in a singleton closure, so `ns.sfy(args)`
                                // RETURNS the function value instead of being it
                                // (Stripe's `qs.stringify(...)` returned the qs
                                // function ⇒ `.replace is not a function`).
                                let origin_key_under_origin_name = resolved_origin_name
                                    .as_ref()
                                    .map(|n| (origin_path.clone(), n.clone()));
                                if exported_var_names.contains(&key)
                                    || origin_key_under_origin_name
                                        .as_ref()
                                        .map(|k| exported_var_names.contains(k))
                                        .unwrap_or(false)
                                {
                                    imported_vars.insert(export_name.clone());
                                }
                                if let Some(class) = exported_classes.get(&key) {
                                    let class_prefix = canonical_class_source_prefix(
                                        class,
                                        &class_canonical_path,
                                        &ctx.project_root,
                                        &origin_prefix,
                                    );
                                    imported_classes.push(perry_codegen::ImportedClass {
                                        name: class.name.clone(),
                                        local_alias: None,
                                        source_prefix: class_prefix,
                                        constructor_param_count: class
                                            .constructor
                                            .as_ref()
                                            .map(|c| c.params.len())
                                            .unwrap_or(0),
                                        has_own_constructor: class.constructor.is_some(),
                                        constructor_has_rest: class
                                            .constructor
                                            .as_ref()
                                            .map(|c| c.params.iter().any(|p| p.is_rest))
                                            .unwrap_or(false),
                                        has_instance_fields: !class.fields.is_empty(),
                                        method_names: class
                                            .methods
                                            .iter()
                                            .map(|m| m.name.clone())
                                            .collect(),
                                        method_param_counts: class
                                            .methods
                                            .iter()
                                            .map(|m| m.params.len())
                                            .collect(),
                                        method_has_rest: class
                                            .methods
                                            .iter()
                                            .map(|m| m.params.iter().any(|p| p.is_rest))
                                            .collect(),
                                        static_method_names: class
                                            .static_methods
                                            .iter()
                                            .map(|m| m.name.clone())
                                            .collect(),
                                        static_field_names: class
                                            .static_fields
                                            .iter()
                                            .filter(|f| f.key_expr.is_none())
                                            .map(|f| f.name.clone())
                                            .collect(),
                                        getter_names: class
                                            .getters
                                            .iter()
                                            .map(|(n, _)| n.clone())
                                            .collect(),
                                        setter_names: class
                                            .setters
                                            .iter()
                                            .map(|(n, _)| n.clone())
                                            .collect(),
                                        parent_name: class.extends_name.clone(),
                                        field_names: class
                                            .fields
                                            .iter()
                                            .filter(|f| f.key_expr.is_none())
                                            .map(|f| f.name.clone())
                                            .collect(),
                                        field_types: class
                                            .fields
                                            .iter()
                                            .filter(|f| f.key_expr.is_none())
                                            .map(|f| f.ty.clone())
                                            .collect(),
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
                        perry_hir::ImportSpecifier::Named { imported, local } => {
                            (local.clone(), imported.clone())
                        }
                        perry_hir::ImportSpecifier::Default { local } => {
                            (local.clone(), "default".to_string())
                        }
                        perry_hir::ImportSpecifier::Namespace { .. } => unreachable!(),
                    };

                    // PerryTS/storekit#1 + #5621: skip the wrapper-fn
                    // registration when this specifier names an FFI function
                    // declared in the source package's
                    // `perry.nativeLibrary.functions` manifest. The source
                    // `.ts` is ambient and has no Perry wrapper for the linker
                    // to resolve, so the FFI-manifest path in `lower_call.rs`
                    // must win. Two binding conventions route here:
                    //   1. Exact match — the binding name IS the symbol
                    //      (`js_storekit_load_products`, raw ambient style).
                    //   2. Ergonomic camelCase (#5621) — the binding
                    //      (`requestAdapter`) is the `js_<pkg>_<snake>` ⇒
                    //      camelCase derivation of the symbol. Record the alias
                    //      (keyed by the *local* binding, since call sites see
                    //      the local name) so the call site rewrites it; exact
                    //      matches need no alias.
                    if let Some(nl) = native_library_for_import {
                        let exact_symbol = nl
                            .functions
                            .iter()
                            .find(|f| f.name == exported_name)
                            .map(|f| f.name.clone());
                        // Collect ALL ergonomic matches so an ambiguous manifest
                        // (two symbols deriving the same camelCase binding) is
                        // rejected rather than silently bound to the first.
                        let ergonomic_matches: Vec<String> = if exact_symbol.is_some() {
                            Vec::new()
                        } else {
                            nl.functions
                                .iter()
                                .filter(|f| {
                                    ergonomic_export_alias(&nl.module, &f.name).as_deref()
                                        == Some(exported_name.as_str())
                                })
                                .map(|f| f.name.clone())
                                .collect()
                        };
                        if ergonomic_matches.len() > 1 {
                            return Err(format!(
                                "native library `{}` has ambiguous ergonomic exports for \
                                 `{}`: the manifest symbols {:?} all derive the same \
                                 camelCase binding. Rename the symbols so each derives a \
                                 distinct binding, or import one by its raw `js_*` name.",
                                nl.module, exported_name, ergonomic_matches
                            ));
                        }
                        let matched_symbol =
                            exact_symbol.or_else(|| ergonomic_matches.into_iter().next());
                        if let Some(symbol) = matched_symbol {
                            // No alias needed when the binding already IS the
                            // symbol (raw exact-match, unaliased).
                            if symbol != local_name {
                                import_function_ffi_aliases.insert(local_name.clone(), symbol);
                            }
                            continue;
                        }
                    }

                    // Issue #310: when the source module re-exports the
                    // imported name as a namespace (`export * as Foo from
                    // "./Foo"`), the local binding behaves identically to
                    // `import * as Foo from "pkg/Foo"` — `Foo.member` should
                    // dispatch through the namespace path. Detect this by
                    // looking at the source module's HIR exports for a
                    // `NamespaceReExport` whose name matches the imported
                    // name, then route the local through `namespace_imports`
                    // + register the namespace target's full export surface.
                    let mut handled_as_namespace_reexport = false;
                    if let Some(src_hir) = source_module {
                        for export in &src_hir.exports {
                            if let perry_hir::Export::NamespaceReExport {
                                source: ns_src,
                                name,
                            } = export
                            {
                                if name != &exported_name {
                                    continue;
                                }
                                let importer = std::path::Path::new(&resolved_path_str);
                                let Some((ns_target, _)) = resolve_import(
                                    ns_src,
                                    importer,
                                    &ctx.project_root,
                                    &ctx.compile_packages,
                                    &ctx.compile_package_dirs,
                                ) else {
                                    break;
                                };
                                let ns_target_str = ns_target.to_string_lossy().to_string();
                                let Some(target_exports) = all_module_exports.get(&ns_target_str)
                                else {
                                    break;
                                };
                                namespace_imports.push(local_name.clone());
                                // Issue #321: tag this local as a "named-import-
                                // of-namespace-reexport" so codegen's
                                // StaticMethodCall arm knows to route var-shape
                                // members through `js_closure_callN`. See the
                                // expr.rs StaticMethodCall comment for why this
                                // is scoped narrowly.
                                namespace_reexport_named_imports.insert(local_name.clone());
                                for (export_name, origin_path) in target_exports {
                                    let origin_prefix =
                                        compute_module_prefix(origin_path, &ctx.project_root);
                                    // Issue #5927: `or_insert` — see the
                                    // matching rationale on the
                                    // `namespace_like_local` branch above.
                                    // A namespace member is a best-effort
                                    // fallback in this flat map; a PLAIN
                                    // named import of the same bare name
                                    // has no other resolution path and
                                    // must always win, regardless of
                                    // import-statement order.
                                    import_function_prefixes
                                        .entry(export_name.clone())
                                        .or_insert_with(|| origin_prefix.clone());
                                    // Issue #5922 (companion to #680): also
                                    // register under the per-namespace key so
                                    // `Context.foo` and `Option.foo` resolve to
                                    // their own sources even when two
                                    // namespace-reexport targets imported into
                                    // the same file happen to export a member
                                    // with the same bare name. Without this,
                                    // codegen's `expr/static_method.rs` (plus
                                    // `namespace_call.rs` / `property_get.rs`
                                    // for lowercase-receiver call/read forms)
                                    // fall through to the flat
                                    // `import_function_prefixes`, which the
                                    // last-registered namespace silently wins.
                                    namespace_member_prefixes.insert(
                                        (local_name.clone(), export_name.clone()),
                                        origin_prefix.clone(),
                                    );
                                    // Issue #678: surface origin-name overrides
                                    // for the NamespaceReExport branch too.
                                    let resolved_origin_name = all_module_export_origin_names
                                        .get(&ns_target_str)
                                        .and_then(|m| m.get(export_name))
                                        .cloned();
                                    if let Some(ref origin_name) = resolved_origin_name {
                                        if origin_name != export_name {
                                            // Issue #5927: `or_insert` — see
                                            // the matching rationale above.
                                            import_function_origin_names
                                                .entry(export_name.clone())
                                                .or_insert_with(|| origin_name.clone());
                                        }
                                    }
                                    // Issue #5924: unconditionally register
                                    // every member under the per-namespace
                                    // key, mirroring `namespace_member_prefixes`
                                    // above (populated for every export, not
                                    // just renamed ones). Found via a
                                    // real-world `sst/opencode` compile:
                                    // `import { Effect, Layer, Context,
                                    // Schema, Types } from "effect"`
                                    // processes all five namespace-reexport
                                    // targets into this file's shared maps.
                                    // When an EARLIER-processed namespace
                                    // (e.g. `Effect`) re-exports a member
                                    // under a rename (e.g. `Service`), the
                                    // flat `import_function_origin_names`
                                    // entry it leaves behind clobbers a
                                    // LATER-processed namespace's (e.g.
                                    // `Context`'s) *unrenamed* member of the
                                    // same name — `Context.Service` resolved
                                    // to the wrong symbol suffix and linking
                                    // failed with an undefined
                                    // `perry_fn_..._Context_ts__<
                                    // wrong-suffix>` symbol. A *sparse*
                                    // per-namespace map (only renamed
                                    // members) doesn't fully fix this: an
                                    // unrenamed member with no entry here
                                    // would still fall back to the
                                    // (possibly contaminated) flat map, so
                                    // every member gets an entry — the
                                    // resolved origin name when renamed,
                                    // else the export name itself.
                                    namespace_member_origin_names.insert(
                                        (local_name.clone(), export_name.clone()),
                                        resolved_origin_name.unwrap_or_else(|| export_name.clone()),
                                    );

                                    let key = (origin_path.clone(), export_name.clone());
                                    if let Some(&param_count) = exported_func_param_counts.get(&key)
                                    {
                                        imported_param_counts
                                            .insert(export_name.clone(), param_count);
                                    }
                                    if exported_func_has_rest.get(&key).copied().unwrap_or(false) {
                                        imported_has_rest.insert(export_name.clone());
                                    }
                                    if exported_func_synthetic_arguments.contains(&key) {
                                        imported_synthetic_arguments.insert(export_name.clone());
                                    }
                                    // Issue #321: NamespaceReExport members
                                    // that are var-shaped exports (the
                                    // canonical `export const succeed = (v) =>
                                    // ...` shape in effect/Effect.ts and
                                    // co-equivalent re-export hubs) must land
                                    // in `imported_vars` so the codegen's
                                    // StaticMethodCall and namespace-member
                                    // call sites route through the zero-arg
                                    // getter + `js_closure_callN`. Without
                                    // this, `import { Effect } from "effect";
                                    // Effect.succeed(42)` emitted a 1-arg
                                    // direct call against the 0-arg getter
                                    // — the source returned the closure
                                    // pointer unchanged and `typeof
                                    // Effect.succeed(42)` was `"function"`,
                                    // and `runSync(program)` then threw
                                    // `Cannot read properties of undefined`
                                    // on `program._tag`. Mirrors the
                                    // `Namespace { local }` branch above.
                                    if exported_var_names.contains(&key) {
                                        imported_vars.insert(export_name.clone());
                                    }
                                    if let Some(class) = exported_classes.get(&key) {
                                        let class_prefix = canonical_class_source_prefix(
                                            class,
                                            &class_canonical_path,
                                            &ctx.project_root,
                                            &origin_prefix,
                                        );
                                        imported_classes.push(perry_codegen::ImportedClass {
                                            name: class.name.clone(),
                                            local_alias: None,
                                            source_prefix: class_prefix,
                                            constructor_param_count: class
                                                .constructor
                                                .as_ref()
                                                .map(|c| c.params.len())
                                                .unwrap_or(0),
                                            has_own_constructor: class.constructor.is_some(),
                                            constructor_has_rest: class
                                                .constructor
                                                .as_ref()
                                                .map(|c| c.params.iter().any(|p| p.is_rest))
                                                .unwrap_or(false),
                                            has_instance_fields: !class.fields.is_empty(),
                                            method_names: class
                                                .methods
                                                .iter()
                                                .map(|m| m.name.clone())
                                                .collect(),
                                            method_param_counts: class
                                                .methods
                                                .iter()
                                                .map(|m| m.params.len())
                                                .collect(),
                                            method_has_rest: class
                                                .methods
                                                .iter()
                                                .map(|m| m.params.iter().any(|p| p.is_rest))
                                                .collect(),
                                            static_method_names: class
                                                .static_methods
                                                .iter()
                                                .map(|m| m.name.clone())
                                                .collect(),
                                            static_field_names: class
                                                .static_fields
                                                .iter()
                                                .filter(|f| f.key_expr.is_none())
                                                .map(|f| f.name.clone())
                                                .collect(),
                                            getter_names: class
                                                .getters
                                                .iter()
                                                .map(|(n, _)| n.clone())
                                                .collect(),
                                            setter_names: class
                                                .setters
                                                .iter()
                                                .map(|(n, _)| n.clone())
                                                .collect(),
                                            parent_name: class.extends_name.clone(),
                                            field_names: class
                                                .fields
                                                .iter()
                                                .filter(|f| f.key_expr.is_none())
                                                .map(|f| f.name.clone())
                                                .collect(),
                                            field_types: class
                                                .fields
                                                .iter()
                                                .filter(|f| f.key_expr.is_none())
                                                .map(|f| f.ty.clone())
                                                .collect(),
                                            source_class_id: Some(class.id),
                                        });
                                    }
                                    if let Some(members) = exported_enums.get(&key) {
                                        imported_enums.push((export_name.clone(), members.clone()));
                                    }
                                }
                                handled_as_namespace_reexport = true;
                                break;
                            }
                        }
                    }
                    if handled_as_namespace_reexport {
                        continue;
                    }

                    let key = (resolved_path_str.clone(), exported_name.clone());

                    // Resolve the ORIGIN path of `exported_name` by following
                    // re-exports. `index.js`'s `export { pgTable } from "./table.js"`
                    // means the immediate import resolves to index.js but the
                    // actual `Let pgTable = (...) => ...` lives in table.js. The
                    // `exported_var_names` set is keyed by the ORIGIN path, so
                    // looking up `(index.js, "pgTable")` misses; we need to walk
                    // the re-export chain to find table.js. Refs #420.
                    let origin_path: String =
                        if let Some(exports) = all_module_exports.get(&resolved_path_str) {
                            if let Some(p) = exports.get(&exported_name) {
                                p.clone()
                            } else {
                                resolved_path_str.clone()
                            }
                        } else {
                            resolved_path_str.clone()
                        };
                    let origin_key = (origin_path.clone(), exported_name.clone());

                    // Resolve effective prefix (follow re-exports)
                    let effective_prefix = if origin_path != resolved_path_str {
                        compute_module_prefix(&origin_path, &ctx.project_root)
                    } else {
                        source_prefix.clone()
                    };

                    // Issue #<TBD>: only key by `exported_name` in the
                    // no-rename case (`local_name == exported_name`), where
                    // the HIR's `ExternFuncRef` genuinely carries that
                    // string. In the aliased case (#35/#321 above:
                    // `ExternFuncRef` carries the LOCAL name, unique per
                    // import site), inserting under `exported_name` too is
                    // not just redundant — `exported_name` is whatever the
                    // ORIGIN module happens to call it, so it can collide
                    // with an unrelated LOCAL alias elsewhere in the same
                    // file. Concrete repro: `import { a as n, c as a } from
                    // "./x"; import { a as t } from "./y"` — the second
                    // specifier's exported name "a" overwrote the first
                    // import's *local* alias "a" (from `c as a`), silently
                    // repointing `ExternFuncRef { name: "a" }` at module y
                    // instead of x. Only inserting the ALIASED case under
                    // `local_name` (never also under `exported_name`) keeps
                    // every key in this map unique per file — local names
                    // can't collide with each other (each `let`/import
                    // binding needs a distinct identifier), but exported
                    // names from different source modules can and do.
                    if local_name == exported_name {
                        import_function_prefixes
                            .insert(exported_name.clone(), effective_prefix.clone());
                    } else {
                        import_function_prefixes
                            .insert(local_name.clone(), effective_prefix.clone());
                    }

                    // Issue #678: if the import chain renames through a
                    // re-export (`export { default as render } from
                    // './render.js'`), the symbol in the origin module
                    // is `perry_fn_<origin>__default`, not
                    // `perry_fn_<origin>__render`. Surface the deeper
                    // origin name via `import_function_origin_names` so
                    // the codegen can pick the right suffix when forming
                    // the extern symbol. The map is sparse — entries are
                    // only inserted when origin_name != exported_name.
                    let resolved_origin_name = all_module_export_origin_names
                        .get(&resolved_path_str)
                        .and_then(|m| m.get(&exported_name))
                        .cloned();
                    if let Some(ref origin_name) = resolved_origin_name {
                        if origin_name != &exported_name {
                            import_function_origin_names
                                .insert(exported_name.clone(), origin_name.clone());
                            if local_name != exported_name {
                                import_function_origin_names
                                    .insert(local_name.clone(), origin_name.clone());
                            }
                        }
                    }

                    // Issue #35 (#321): companion to the HIR-side change in
                    // `module_decl.rs` (Named specifier now registers
                    // `(local, local)`, so an ALIASED named import's
                    // `ExternFuncRef` carries the unique LOCAL name). The
                    // origin module still emits its symbol under the EXPORTED
                    // name, so map `local → exported_name` (or the deeper
                    // re-export origin name when one applies) here so codegen
                    // forms `perry_fn_<src>__<exported>` rather than
                    // `perry_fn_<src>__<local>`. Mirrors the #901 Default-import
                    // override below. Only needed when `local != exported`
                    // (the alias case); the no-alias case carries the export
                    // name verbatim. Skip if the re-export-rename block above
                    // already inserted a (deeper) override for this local.
                    if matches!(spec, perry_hir::ImportSpecifier::Named { .. })
                        && local_name != exported_name
                        && !import_function_origin_names.contains_key(&local_name)
                    {
                        import_function_origin_names
                            .insert(local_name.clone(), exported_name.clone());
                    }

                    // Issue #901: companion to the HIR-side change at
                    // `crates/perry-hir/src/lower.rs`'s Default specifier
                    // (which now registers `(local, local)` instead of
                    // `(local, "default")`). The HIR's `ExternFuncRef` now
                    // carries the LOCAL name (unique per import site), so
                    // `import_function_prefixes.get(local)` resolves to the
                    // right source module. But the symbol the codegen emits
                    // must still be `perry_fn_<src>__default` (or whatever
                    // origin-name the source actually exports default as),
                    // not `perry_fn_<src>__<local>` — the source module emits
                    // its default-export symbol under the literal "default"
                    // suffix. Insert the local→"default" override (or the
                    // resolved origin name, when a re-export renamed it) so
                    // every `perry_fn_<src>__<suffix>` construction site
                    // probing `import_function_origin_names` picks the right
                    // suffix. Pre-fix two same-file default imports of
                    // different modules collided on the "default" key and
                    // pino's `SORTING_ORDER.ASC` threw because `_req_9`
                    // (`./lib/constants`) and `_req_10` (`./lib/tools`) both
                    // resolved to `./lib/tools`. Pairs with the HIR change;
                    // both must land for the resolution to be correct.
                    if matches!(spec, perry_hir::ImportSpecifier::Default { .. }) {
                        let suffix = resolved_origin_name
                            .clone()
                            .unwrap_or_else(|| exported_name.clone());
                        import_function_origin_names
                            .insert(local_name.clone(), suffix);
                    }

                    // Imported variables (not functions) — ExternFuncRef-as-value
                    // should call the getter, not wrap as closure. Look up by the
                    // ORIGIN path (where the `Let X = ...` actually lives), not
                    // the immediate import path. Without this, re-exports through
                    // `index.js` barrel files (drizzle's `pg-core/index.js`,
                    // hono's adapter index files, etc.) silently fall through to
                    // the direct-call path which treats the zero-arg getter's
                    // return value AS the call result — pgTable("users", cols)
                    // returned the closure handle (typeof === "function") with no
                    // pgTable body actually invoked.
                    //
                    // Issue #678 followup: when a re-export rename routes
                    // the import through `export default <var>`, the origin
                    // module's `exported_objects` carries the synthetic
                    // "default" entry (the only thing exported at that
                    // shape) — not the consumer-visible name. Probe both
                    // keys so the var-vs-function classification fires
                    // even when re-export renaming is in play.
                    let origin_key_under_origin_name = resolved_origin_name
                        .as_ref()
                        .map(|n| (origin_path.clone(), n.clone()));
                    if exported_var_names.contains(&origin_key)
                        || origin_key_under_origin_name
                            .as_ref()
                            .map(|k| exported_var_names.contains(k))
                            .unwrap_or(false)
                    {
                        imported_vars.insert(exported_name.clone());
                        if local_name != exported_name {
                            imported_vars.insert(local_name.clone());
                        }
                    }

                    // Imported classes
                    if let Some(class) = exported_classes.get(&key) {
                        let class_prefix = canonical_class_source_prefix(
                            class,
                            &class_canonical_path,
                            &ctx.project_root,
                            &effective_prefix,
                        );
                        // Issue #665: when the user wrote `import X from "pkg"`
                        // and `pkg`'s default export is a class, the importer
                        // still registers `exported_name="default"` into
                        // `import_function_prefixes` above. Codegen's wrapper-
                        // emission loop iterates that map and — for any name
                        // NOT in `imported_class_names` — emits a function
                        // wrapper that calls `perry_fn_<src>__default`, which
                        // the source module never defines (the source only has
                        // a `_Child_constructor` symbol). That declares an
                        // unresolved extern and the link step errors with
                        // `Undefined symbols: ___perry_wrap_perry_fn_<src>__default`.
                        // Push a SECOND ImportedClass entry whose `local_alias`
                        // is the exported_name (`"default"` for default imports,
                        // or the original-name for `{ Foo as Bar }`-style
                        // renames). codegen's `imported_class_names` builder
                        // adds both `ic.name` and `ic.local_alias`, so the
                        // exported_name lands in the set and the wrapper-
                        // emission loop takes the `is_class` no-op-stub branch
                        // instead of declaring a phantom function. The second
                        // entry also registers `class_ids[exported_name]`,
                        // letting consumer-side `Expr::ExternFuncRef { name:
                        // exported_name }` resolve to the class-id NaN-box.
                        if local_name != exported_name {
                            imported_classes.push(perry_codegen::ImportedClass {
                                name: class.name.clone(),
                                local_alias: Some(exported_name.clone()),
                                source_prefix: class_prefix.clone(),
                                constructor_param_count: class
                                    .constructor
                                    .as_ref()
                                    .map(|c| c.params.len())
                                    .unwrap_or(0),
                                has_own_constructor: class.constructor.is_some(),
                                constructor_has_rest: class
                                    .constructor
                                    .as_ref()
                                    .map(|c| c.params.iter().any(|p| p.is_rest))
                                    .unwrap_or(false),
                                has_instance_fields: !class.fields.is_empty(),
                                method_names: class
                                    .methods
                                    .iter()
                                    .map(|m| m.name.clone())
                                    .collect(),
                                method_param_counts: class
                                    .methods
                                    .iter()
                                    .map(|m| m.params.len())
                                    .collect(),
                                method_has_rest: class
                                    .methods
                                    .iter()
                                    .map(|m| m.params.iter().any(|p| p.is_rest))
                                    .collect(),
                                static_method_names: class
                                    .static_methods
                                    .iter()
                                    .map(|m| m.name.clone())
                                    .collect(),
                                static_field_names: class
                                    .static_fields
                                    .iter()
                                    .filter(|f| f.key_expr.is_none())
                                    .map(|f| f.name.clone())
                                    .collect(),
                                getter_names: class
                                    .getters
                                    .iter()
                                    .map(|(n, _)| n.clone())
                                    .collect(),
                                setter_names: class
                                    .setters
                                    .iter()
                                    .map(|(n, _)| n.clone())
                                    .collect(),
                                parent_name: class.extends_name.clone(),
                                field_names: class
                                    .fields
                                    .iter()
                                    .filter(|f| f.key_expr.is_none())
                                    .map(|f| f.name.clone())
                                    .collect(),
                                field_types: class
                                    .fields
                                    .iter()
                                    .filter(|f| f.key_expr.is_none())
                                    .map(|f| f.ty.clone())
                                    .collect(),
                                source_class_id: Some(class.id),
                            });
                        }
                        imported_classes.push(perry_codegen::ImportedClass {
                            name: class.name.clone(),
                            local_alias: if local_name != class.name {
                                Some(local_name.clone())
                            } else {
                                None
                            },
                            source_prefix: class_prefix,
                            constructor_param_count: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.len())
                                .unwrap_or(0),
                            has_own_constructor: class.constructor.is_some(),
                            constructor_has_rest: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.iter().any(|p| p.is_rest))
                                .unwrap_or(false),
                            has_instance_fields: !class.fields.is_empty(),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class
                                .methods
                                .iter()
                                .map(|m| m.params.len())
                                .collect(),
                            method_has_rest: class
                                .methods
                                .iter()
                                .map(|m| m.params.iter().any(|p| p.is_rest))
                                .collect(),
                            static_method_names: class
                                .static_methods
                                .iter()
                                .map(|m| m.name.clone())
                                .collect(),
                            static_field_names: class
                                .static_fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            field_types: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.ty.clone())
                                .collect(),
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

                    // Issue #608 — propagate has_rest alongside the param
                    // count so the cross-module call site can pack the
                    // trailing args into a rest array.
                    if exported_func_has_rest.get(&key).copied().unwrap_or(false) {
                        imported_has_rest.insert(exported_name.clone());
                        if local_name != exported_name {
                            imported_has_rest.insert(local_name.clone());
                        }
                    }
                    if exported_func_synthetic_arguments.contains(&key) {
                        imported_synthetic_arguments.insert(exported_name.clone());
                        if local_name != exported_name {
                            imported_synthetic_arguments.insert(local_name.clone());
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
                            constructor_param_count: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.len())
                                .unwrap_or(0),
                            has_own_constructor: class.constructor.is_some(),
                            constructor_has_rest: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.iter().any(|p| p.is_rest))
                                .unwrap_or(false),
                            has_instance_fields: !class.fields.is_empty(),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class
                                .methods
                                .iter()
                                .map(|m| m.params.len())
                                .collect(),
                            method_has_rest: class
                                .methods
                                .iter()
                                .map(|m| m.params.iter().any(|p| p.is_rest))
                                .collect(),
                            static_method_names: class
                                .static_methods
                                .iter()
                                .map(|m| m.name.clone())
                                .collect(),
                            static_field_names: class
                                .static_fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            field_types: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.ty.clone())
                                .collect(),
                            source_class_id: Some(class.id),
                        });
                    }
                }
            }

            // Issue #678 followup: V8-fallback imports. Native imports above
            // wire `perry_fn_<src>__<name>` extern symbols; V8 imports route
            // through the runtime bridge instead. We populate BOTH
            // `import_function_prefixes` (with a synthetic prefix so the
            // codegen's `Some(source_prefix) = prefixes.get(name)` arm fires
            // and the V8-specifier short-circuit inside it triggers) AND
            // `import_function_v8_specifiers` (the actual specifier the bridge
            // hands to `js_load_module`). The synthetic prefix never reaches
            // a `perry_fn_...` symbol because every codegen site probes
            // `import_function_v8_specifiers` first.
            for import in &hir_module.imports {
                if import.type_only {
                    continue;
                }
                if import.module_kind != perry_hir::ModuleKind::Interpreted {
                    continue;
                }
                // The V8 bridge takes a specifier string and resolves it
                // through deno_core's Node loader — bare specifiers like
                // "ink" and absolute paths both work. Prefer the resolved
                // canonical path (matches the `JsModule.specifier` key in
                // `ctx.js_modules`) so the same module-handle cache hits
                // across imports of the same package from different sites.
                let specifier = import
                    .resolved_path
                    .clone()
                    .unwrap_or_else(|| import.source.clone());
                let synthetic_prefix = format!("__v8__{}", sanitize_name(&specifier));
                for spec in &import.specifiers {
                    match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => {
                            import_function_prefixes
                                .insert(local.clone(), synthetic_prefix.clone());
                            import_function_v8_specifiers
                                .insert(local.clone(), specifier.clone());
                            if local != imported {
                                import_function_prefixes
                                    .insert(imported.clone(), synthetic_prefix.clone());
                                import_function_v8_specifiers
                                    .insert(imported.clone(), specifier.clone());
                                // Issue #818 (Effect.succeed pattern) follow-up:
                                // when an aliased named-import (`import { Foo
                                // as Bar }`) of a V8 module is used as a
                                // static-method receiver (`Bar.method(...)`),
                                // the codegen's StaticMethodCall arm sees
                                // class_name = "Bar" — but the V8 namespace
                                // exposes the property under "Foo". Record the
                                // local→imported mapping in
                                // `import_function_origin_names` so the bridge
                                // call reaches the right namespace property.
                                // Without this, aliased Effect-shaped imports
                                // would look up a missing property and fall to
                                // undefined.
                                import_function_origin_names
                                    .insert(local.clone(), imported.clone());
                            }
                        }
                        perry_hir::ImportSpecifier::Default { local } => {
                            import_function_prefixes
                                .insert(local.clone(), synthetic_prefix.clone());
                            import_function_v8_specifiers
                                .insert(local.clone(), specifier.clone());
                            // #1195 — `import YAML from "yaml"` lands here.
                            // When the local name is used as a static-method
                            // receiver (`YAML.parse(...)`), the StaticMethodCall
                            // arm in expr/static_method.rs looks up
                            // `import_function_origin_names[class_name]` to
                            // pick the namespace property name, falling back
                            // to the local name. Without this insert, the
                            // bridge would ask V8 for `ns.YAML` (which doesn't
                            // exist on the proxy module's namespace; it
                            // re-exports the default under the literal
                            // "default" key). Record the local→"default"
                            // override so the bridge resolves the right
                            // namespace property.
                            import_function_origin_names
                                .insert(local.clone(), "default".to_string());
                        }
                        perry_hir::ImportSpecifier::Namespace { local } => {
                            // Namespace bindings (`import * as X from "ink"`)
                            // are already registered into `namespace_imports`
                            // by the pre-loop above. For pure-namespace usage
                            // with no companion `Named` import, the V8 module
                            // has no static export list to register members
                            // against — so we record `local → specifier` here.
                            // The codegen's StaticMethodCall arm and the
                            // namespace-member-call arm in `lower_call.rs`
                            // probe `namespace_v8_specifiers` and, on a hit,
                            // emit `js_call_v8_export(specifier, member,
                            // args, argc)` so `R.sum([1,2,3])` (`import * as
                            // R from "ramda"`) reaches V8 instead of falling
                            // to the `double_literal(0.0)` stub. Unblocks
                            // ramda / date-fns / jose / effect wildcard
                            // namespace usage.
                            namespace_v8_specifiers
                                .insert(local.clone(), specifier.clone());
                        }
                    }
                }
            }

            // Issue #841: register named + namespace imports from the
            // five recognized Node submodules — `node:timers/promises`,
            // `node:readline/promises`, `node:stream/promises`,
            // `node:stream/consumers`, `node:sys`. These don't resolve
            // to anything perry-stdlib can back, but the runtime ships
            // a `js_node_submodule_export_as_function` helper that
            // returns a function singleton for each known export, plus
            // `js_node_submodule_namespace` for namespace shapes.
            //
            // Without this registration the codegen's `ExternFuncRef`
            // value-form catch-all fell to TAG_TRUE, so `typeof
            // setTimeout` (from `node:timers/promises`) reported
            // `"boolean"` instead of `"function"`. Namespaces were
            // hard-errored at module-collection time pre-fix
            // (`collect_modules.rs::known_node_submodule_key`); they
            // now flow through and land here.
            for import in &hir_module.imports {
                if import.type_only {
                    continue;
                }
                let submod_key = match self::collect_modules::known_node_submodule_key(&import.source) {
                    Some(k) => k.to_string(),
                    None => continue,
                };
                for spec in &import.specifiers {
                    match spec {
                        perry_hir::ImportSpecifier::Named { imported, local } => {
                            // #1213: node:timers named imports (`import {
                            // setTimeout } from "node:timers"`) keep the global
                            // timer codegen fast-path (which handles the
                            // `setTimeout(fn, delay, ...args)` varargs form).
                            // Routing them through the submodule thunk here
                            // would drop varargs — only the `import * as`
                            // namespace shape uses the submodule.
                            if submod_key != "timers" {
                                // Register ONLY the local binding. For an
                                // aliased import (`import { setTimeout as ac5 }
                                // from "node:timers/promises"`) the in-scope
                                // name is `ac5`; the imported name `setTimeout`
                                // is NOT bound here — it still refers to the
                                // GLOBAL `setTimeout(callback, delay)`. A prior
                                // version also keyed the map by `imported` when
                                // `local != imported`, which made the bare
                                // global `setTimeout(fn, ms)` divert to the
                                // delay-first promises thunk and reject with
                                // `The "delay" argument must be of type number.
                                // Received function`. Keying only by `local`
                                // keeps the alias routed to the submodule export
                                // and leaves the unshadowed global intact.
                                import_function_node_submodule.insert(
                                    local.clone(),
                                    (submod_key.clone(), imported.clone()),
                                );
                            }
                        }
                        perry_hir::ImportSpecifier::Default { local } => {
                            // Default imports route to "default" — known Node
                            // submodules expose an object-valued default export
                            // that is distinct from the namespace object.
                            import_function_node_submodule.insert(
                                local.clone(),
                                (submod_key.clone(), "default".to_string()),
                            );
                        }
                        perry_hir::ImportSpecifier::Namespace { local } => {
                            namespace_node_submodules
                                .insert(local.clone(), submod_key.clone());
                            // Already in `namespace_imports` via the
                            // pre-loop at L3441; nothing else to do.
                        }
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
                    // Perry native-memory marker types
                    | "NativeArena" | "NativeArenaOwner"
                    | "PerryPod" | "PerryPodView"
                    | "PerryU32" | "PerryU64" | "PerryUSize"
                    | "PerryF32" | "PerryF64" | "PerryI32" | "PerryI64"
                    | "PerryBufferLen" | "PerryHandleId"
                )
            }
            let mut local_known: std::collections::HashSet<String> =
                std::collections::HashSet::new();
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
            fn type_has_unresolved<F: Fn(&str) -> bool>(ty: &perry_types::Type, check: &F) -> bool {
                use perry_types::Type;
                match ty {
                    Type::Named(name) => check(name),
                    Type::Generic { base, type_args } => {
                        check(base) || type_args.iter().any(|t| type_has_unresolved(t, check))
                    }
                    Type::Array(elem) => type_has_unresolved(elem, check),
                    Type::Promise(inner) => type_has_unresolved(inner, check),
                    Type::Union(variants) => variants.iter().any(|v| type_has_unresolved(v, check)),
                    Type::Tuple(items) => items.iter().any(|v| type_has_unresolved(v, check)),
                    Type::Function(ft) => {
                        ft.params
                            .iter()
                            .any(|(_, t, _)| type_has_unresolved(t, check))
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
            fn stmt_has_unresolved<F: Fn(&str) -> bool>(stmt: &perry_hir::Stmt, check: &F) -> bool {
                match stmt {
                    perry_hir::Stmt::Let { ty, .. } => type_has_unresolved(ty, check),
                    perry_hir::Stmt::If {
                        then_branch,
                        else_branch,
                        ..
                    } => {
                        stmts_have_unresolved(then_branch, check)
                            || else_branch
                                .as_ref()
                                .map(|a| stmts_have_unresolved(a, check))
                                .unwrap_or(false)
                    }
                    perry_hir::Stmt::While { body, .. } | perry_hir::Stmt::DoWhile { body, .. } => {
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
                    perry_hir::Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
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
            fn fn_has_unresolved<F: Fn(&str) -> bool>(f: &perry_hir::Function, check: &F) -> bool {
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
            if !references_interface && stmts_have_unresolved(&hir_module.init, &is_unresolved_name)
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
                            constructor_param_count: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.len())
                                .unwrap_or(0),
                            has_own_constructor: class.constructor.is_some(),
                            constructor_has_rest: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.iter().any(|p| p.is_rest))
                                .unwrap_or(false),
                            has_instance_fields: !class.fields.is_empty(),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class
                                .methods
                                .iter()
                                .map(|m| m.params.len())
                                .collect(),
                            method_has_rest: class
                                .methods
                                .iter()
                                .map(|m| m.params.iter().any(|p| p.is_rest))
                                .collect(),
                            static_method_names: class
                                .static_methods
                                .iter()
                                .map(|m| m.name.clone())
                                .collect(),
                            static_field_names: class
                                .static_fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            field_types: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.ty.clone())
                                .collect(),
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
            let mut visited_imports: std::collections::HashSet<String> =
                imported_classes.iter().map(|ic| ic.name.clone()).collect();
            // Issue #26 / #321: a class's `extends` parent must be resolved in
            // the CHILD's own source module — same-named classes in different
            // modules (effect's `Type` in SchemaAST.ts vs ParseResult.ts) are
            // distinct. The by-NAME `visited_imports` dedup above would import
            // only the first `Type` seen and skip the SchemaAST one, so
            // SchemaAST's `OptionalType extends Type` chain loses its real
            // parent's fields. Track parent additions by (path, name) identity
            // so the correct-module parent is pulled in even when its bare
            // name was already visited. Codegen's prefix-disambiguated parent
            // resolver then picks the right one.
            let mut visited_parent_paths: std::collections::HashSet<(String, String)> =
                std::collections::HashSet::new();
            // Worklist of INDICES into `imported_classes` (not names): a name
            // can map to several entries (same-named cross-module classes,
            // refs #26), so we must process the exact entry we added, not the
            // first by-name match.
            let mut closure_worklist: Vec<usize> = (0..imported_classes.len()).collect();
            while let Some(idx) = closure_worklist.pop() {
                if idx >= imported_classes.len() {
                    continue;
                }
                let field_types_clone = imported_classes[idx].field_types.clone();
                let parent_name_clone = imported_classes[idx].parent_name.clone();
                // The child's own canonical source path, used to resolve its
                // `extends` parent in the child's module scope.
                let child_src_path: Option<String> = imported_classes[idx]
                    .source_class_id
                    .and_then(|cid| class_canonical_path.get(&cid).cloned());
                // Issue #485: include the class's parent in the transitive
                // closure too. Without this, `import { Sub } from 'pkg'` where
                // `Sub extends Base` (and Base lives in another file inside
                // the same package) leaves Base unimported on this side, so
                // codegen builds Sub's per-class shape with zero parent-field
                // contribution. Sub instances allocate too few inline slots
                // and the parent's cross-module ctor's `this.field = …`
                // writes overflow the object header — `f.field` reads
                // undefined on the importing side.
                //
                // `is_parent_ref` marks the entry that came from `extends`
                // (vs a field-type reference): parent refs get path-aware
                // resolution + (path,name) dedup so the correct-module parent
                // is imported even past the bare-name dedup. Field-type refs
                // keep the legacy by-name behavior.
                let refs: Vec<(String, bool)> = field_types_clone
                    .iter()
                    .filter_map(|ty| match ty {
                        perry_types::Type::Named(n) => Some(n.clone()),
                        perry_types::Type::Generic { base, .. } => Some(base.clone()),
                        _ => None,
                    })
                    .map(|n| (n, false))
                    .chain(parent_name_clone.into_iter().map(|n| (n, true)))
                    .collect();
                for (ref_name, is_parent_ref) in refs {
                    // Issue #489: pick the canonical defining path for the
                    // parent class (where `class N { ... }` actually lives)
                    // rather than the first BTreeMap match by name (which
                    // can be a re-export barrel). Without this, drizzle's
                    // `MySqlPreparedQuery extends QueryPromise` chain pulls
                    // QueryPromise in under `drizzle-orm/index.js` (because
                    // `index.js` does `export * from "./query-promise.js"`
                    // and sorts before `query-promise.js`), and the dispatch
                    // table emits `perry_method_<index_js>__QueryPromise__then`
                    // — undefined symbol at link time.
                    //
                    // Issue #26: for a parent ref, prefer the same-named class
                    // in the CHILD's own source module before any global match.
                    let found = is_parent_ref
                        .then_some(())
                        .and(child_src_path.as_ref())
                        .and_then(|cp| {
                            exported_classes
                                .iter()
                                .find(|((path, cname), _)| cname == &ref_name && path == cp)
                        })
                        .or_else(|| {
                            exported_classes.iter().find(|((path, cname), class)| {
                                cname == &ref_name
                                    && class_canonical_path
                                        .get(&class.id)
                                        .map(|cp| cp == path)
                                        .unwrap_or(true)
                            })
                        })
                        .or_else(|| {
                            exported_classes
                                .iter()
                                .find(|((_, cname), _)| cname == &ref_name)
                        })
                        .map(|((path, _), class)| (path.clone(), *class));
                    // Dedup: parent refs key on (resolved_path, name) so a
                    // distinct same-named parent in another module is still
                    // imported; all other refs key on name only (legacy).
                    if is_parent_ref {
                        if let Some((src_path, _)) = &found {
                            if !visited_parent_paths
                                .insert((src_path.clone(), ref_name.clone()))
                            {
                                continue;
                            }
                            // Already have an entry under this name from a
                            // DIFFERENT module: still add this (path,name)
                            // variant so codegen can disambiguate, but skip
                            // re-pushing to the worklist by name below.
                        } else {
                            continue;
                        }
                    } else if visited_imports.contains(&ref_name) {
                        continue;
                    }
                    if let Some((src_path, class)) = found {
                        let class_prefix = compute_module_prefix(&src_path, &ctx.project_root);
                        // Issue #485: when the child's `parent_name` doesn't
                        // match the source class's `class.name` (because the
                        // parent was imported via a rename — `import { Base
                        // as HBase } from './base.js'` or
                        // `export { Base as HBase }` on the source side),
                        // expose the stub under the alias the child knows.
                        // Without this, codegen's `imported_class_stubs`
                        // would register the parent under "Base" while the
                        // child's `extends_name` is "HBase", and the
                        // packed-keys / slot-index walker fails to traverse
                        // the chain.
                        let alias = if ref_name != class.name {
                            Some(ref_name.clone())
                        } else {
                            None
                        };
                        imported_classes.push(perry_codegen::ImportedClass {
                            name: class.name.clone(),
                            local_alias: alias,
                            source_prefix: class_prefix,
                            constructor_param_count: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.len())
                                .unwrap_or(0),
                            has_own_constructor: class.constructor.is_some(),
                            constructor_has_rest: class
                                .constructor
                                .as_ref()
                                .map(|c| c.params.iter().any(|p| p.is_rest))
                                .unwrap_or(false),
                            has_instance_fields: !class.fields.is_empty(),
                            method_names: class.methods.iter().map(|m| m.name.clone()).collect(),
                            method_param_counts: class
                                .methods
                                .iter()
                                .map(|m| m.params.len())
                                .collect(),
                            method_has_rest: class
                                .methods
                                .iter()
                                .map(|m| m.params.iter().any(|p| p.is_rest))
                                .collect(),
                            static_method_names: class
                                .static_methods
                                .iter()
                                .map(|m| m.name.clone())
                                .collect(),
                            static_field_names: class
                                .static_fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            getter_names: class.getters.iter().map(|(n, _)| n.clone()).collect(),
                            setter_names: class.setters.iter().map(|(n, _)| n.clone()).collect(),
                            parent_name: class.extends_name.clone(),
                            field_names: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.name.clone())
                                .collect(),
                            field_types: class
                                .fields
                                .iter()
                                .filter(|f| f.key_expr.is_none())
                                .map(|f| f.ty.clone())
                                .collect(),
                            source_class_id: Some(class.id),
                        });
                        visited_imports.insert(ref_name.clone());
                        // Process the entry we just pushed (by index, so a
                        // same-named distinct-module class isn't skipped). Refs #26.
                        closure_worklist.push(imported_classes.len() - 1);
                    }
                }
            }

            // Type aliases from all modules
            let type_alias_map: std::collections::HashMap<String, perry_types::Type> =
                all_type_aliases
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

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
                        let ext_prefix =
                            compute_module_prefix(&ext_path.to_string_lossy(), &ctx.project_root);
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
            let js_module_specifiers_vec: Vec<String> = js_module_specifiers.clone();

            let opts = perry_codegen::CompileOptions {
                target: resolved_triple,
                is_entry_module: is_entry,
                non_entry_module_prefixes,
                import_function_prefixes,
                import_function_ffi_aliases,
                import_function_origin_names,
                import_function_v8_specifiers,
                import_function_node_submodule,
                namespace_node_submodules,
                namespace_v8_specifiers,
                namespace_member_prefixes,
                namespace_member_origin_names,
                emit_ir_only: bitcode_link,
                verify_native_regions,
                disable_buffer_fast_path,
                namespace_imports,
                namespace_reexport_named_imports,
                imported_classes,
                imported_enums,
                imported_async_funcs: imported_async_set,
                type_aliases: type_alias_map,
                imported_func_param_counts: imported_param_counts,
                imported_func_has_rest: imported_has_rest,
                imported_func_synthetic_arguments: imported_synthetic_arguments,
                imported_func_return_types: imported_return_types,
                imported_vars,

                // Feature plumbing
                output_type: args.output_type.clone(),
                needs_stdlib: ctx.needs_stdlib,
                needs_ui: ctx.needs_ui,
                needs_geisterhand: ctx.needs_geisterhand,
                geisterhand_port: ctx.geisterhand_port,
                enabled_features: compiled_features.clone(),
                native_module_init_names: native_module_init_names_vec,
                js_module_specifiers: js_module_specifiers_vec,
                bundled_extensions: bundled_ext_vec,
                native_library_functions: ffi_functions.clone(),
                i18n_table: i18n_snapshot.clone(),
                fast_math: ctx.fast_math,
                fp_contract_mode: ctx.fp_contract_mode,
                app_metadata: ctx.app_metadata.clone(),
                // Issue #100: namespace_entries empty unless this
                // module is a dynamic-import target; the consumer-side
                // dispatch map is empty unless this module performs
                // dynamic imports.
                namespace_entries: per_module_namespace_entries
                    .get(path)
                    .cloned()
                    .unwrap_or_default(),
                dynamic_import_path_to_prefix: per_module_dyn_import_targets
                    .get(path)
                    .cloned()
                    .unwrap_or_default(),
                nextjs_path_init_modules,
                deferred_module_prefixes,
                module_init_deps,
                // Issue #842: signal side-effect-only dynamic-import
                // targets to codegen so it still emits
                // `@__perry_ns_<prefix>` + populator. `dyn_target_paths`
                // is the authoritative set built from every consumer's
                // `import.is_dynamic` resolved paths; `namespace_entries`
                // alone is insufficient because it's empty when the
                // target has no `export` statements.
                is_dynamic_import_target: dyn_target_paths.contains(path),
                // #5247: source-location tracking for the dynamic call-dispatch
                // throw path. Gated by `--debug-symbols` so the default build is
                // unchanged (no source read, no per-call emission). When on, read
                // the module's original source so codegen can map a Call's byte
                // offset to a 1-based line.
                debug_locations: args.debug_symbols,
                // #5247: source consulted to turn a node's `byte_offset` into a
                // line. For a CommonJS module the offsets are in WRAPPED-source
                // coordinates (perry parsed the injected-IIFE text), so we hand
                // codegen the WRAPPED source — counting newlines up to a wrapped
                // offset against the original would be off by the preamble byte
                // length. `debug_source_line_offset` (below) then converts the
                // wrapped line back to the original line. Non-wrapped modules
                // read the original from disk.
                module_source: if args.debug_symbols {
                    match ctx.cjs_wrap_debug_sources.get(path) {
                        Some(w) => Some(w.wrapped_source.clone()),
                        None => std::fs::read_to_string(path).ok(),
                    }
                } else {
                    None
                },
                // #5247 (CJS-wrap coordinate skew): the number of newlines the
                // injected wrapper prefix added before the original module body.
                // Codegen subtracts this from the wrapped line number so the
                // rendered location is in original-source coordinates. `0` for
                // non-wrapped modules (and the entire default build).
                debug_source_line_offset: if args.debug_symbols {
                    ctx.cjs_wrap_debug_sources
                        .get(path)
                        .map(|w| w.prefix_line_count)
                        .unwrap_or(0)
                } else {
                    0
                },
            };
            // V2.2 + #686 object cache lookup. The key hashes every
            // codegen-affecting field of `opts` together with this
            // module's post-transform HIR fingerprint and the perry
            // version. A hit returns the exact `.o` bytes we emitted
            // the last time opts + HIR were identical — cross-run bit
            // identity, not just semantic equivalence.
            //
            // The HIR fingerprint is computed inside this rayon job
            // (paralelizes the cost across modules and avoids an extra
            // serial O(modules) pass). Crucially, every HIR-mutating
            // pass (inline_functions, unroll_static_loops,
            // inline_finally_into_returns, transform_async_to_generator,
            // transform_generators per-module; transform_js_imports,
            // fix_local_native_instances, fix_cross_module_native_instances,
            // monomorphize_module, perry_codegen_arkts::emit_index_ets,
            // perry_transform::i18n::apply_i18n, fix_imported_enums
            // cross-module) has already run by the time we get here, so
            // the hash captures the exact tree that `compile_module`
            // will consume. `compile_module` takes `&Module` (shared
            // reference) — see crates/perry-codegen/src/codegen.rs:388 —
            // so it cannot mutate the HIR after the hash is taken.
            let (cache_key, hir_hash_for_diag) = if object_cache.is_enabled() {
                let hir_hash = perry_hir::stable_hash::hash_module(hir_module);
                (
                    Some(compute_object_cache_key(&opts, hir_hash, perry_version)),
                    Some(hir_hash),
                )
            } else {
                (None, None)
            };
            let obj_name = native_object_file_stem(&hir_module.name);
            // In bitcode mode the bytes are .ll text; use .ll extension.
            let ext = if bitcode_link { "ll" } else { "o" };
            let obj_path = object_output_dir.join(format!("{}.{}", obj_name, ext));

            if let Some((key, cached_path)) =
                cache_key.and_then(|k| object_cache.lookup_path(k).map(|path| (k, path)))
            {
                return Ok(NativeObjectArtifact {
                    path: cached_path,
                    bytes: None,
                    fingerprint: format!("cache:{:016x}", key),
                    cleanup_after_link: false,
                    reused_cache_path: true,
                    stored_cache_path: false,
                });
            }

            // PERRY_DEV_VERBOSE=1: report the per-module HIR + cache key on
            // every miss, so a user can diff hashes between builds and answer
            // "why didn't my cosmetic edit hit?" (#686 acceptance criterion).
            if let (Some(k), Some(hh)) = (cache_key, hir_hash_for_diag) {
                if std::env::var("PERRY_DEV_VERBOSE").as_deref() == Ok("1") {
                    eprintln!(
                        "  • cache miss: {} hir={:016x} key={:016x}",
                        hir_module.name, hh, k
                    );
                }
                // PERRY_CACHE_DEBUG_HIR=1: also dump the post-transform HIR of
                // misses to <cache_dir>/debug/<key>.txt so a user can diff two
                // miss-dumps and see exactly what differed. Best-effort — IO
                // errors never fail the build.
                if std::env::var("PERRY_CACHE_DEBUG_HIR").as_deref() == Ok("1") {
                    let dump_dir = ctx.cache_dir.join("debug");
                    if std::fs::create_dir_all(&dump_dir).is_ok() {
                        let dump_path = dump_dir.join(format!("{:016x}.txt", k));
                        let _ = std::fs::write(
                            &dump_path,
                            format!(
                                "module: {}\npath: {}\nhir_hash: {:016x}\ncache_key: {:016x}\n\n{:#?}\n",
                                hir_module.name,
                                path.display(),
                                hh,
                                k,
                                hir_module,
                            ),
                        );
                    }
                }
            }
            progress.heartbeat(ProgressSnapshot {
                stage: "codegen",
                module_path: Some(path),
                module_name: Some(&hir_module.name),
                visited: Some(codegen_index),
                total: Some(total_codegen_modules),
                collected: Some(total_codegen_modules),
                ..Default::default()
            });
            let object_code = perry_codegen::compile_module(hir_module, opts).map_err(|e| {
                format!(
                    "Error compiling module '{}' ({}) with --backend llvm: {:#}",
                    hir_module.name,
                    path.display(),
                    e
                )
            })?;
            let object_fingerprint = cache_key
                .map(|k| format!("cache:{:016x}", k))
                .unwrap_or_else(|| format!("bytes:{:016x}", djb2_hash(&object_code)));
            if let Some(cached_path) =
                cache_key.and_then(|k| object_cache.store_and_get_path(k, &object_code))
            {
                return Ok(NativeObjectArtifact {
                    path: cached_path,
                    bytes: None,
                    fingerprint: object_fingerprint,
                    cleanup_after_link: false,
                    reused_cache_path: false,
                    stored_cache_path: true,
                });
            }
            Ok(NativeObjectArtifact {
                path: obj_path,
                bytes: Some(object_code),
                fingerprint: object_fingerprint,
                cleanup_after_link: true,
                reused_cache_path: false,
                stored_cache_path: false,
            })
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
    let mut artifacts: Vec<NativeObjectArtifact> = Vec::new();
    for result in compile_results {
        match result {
            Ok(artifact) => artifacts.push(artifact),
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

    let object_cache_paths_reused = artifacts
        .iter()
        .filter(|artifact| artifact.reused_cache_path)
        .count();
    let object_cache_paths_stored = artifacts
        .iter()
        .filter(|artifact| artifact.stored_cache_path)
        .count();
    let object_temp_writes = artifacts
        .iter()
        .filter(|artifact| artifact.bytes.is_some())
        .count();
    let object_bytes_materialized: usize = artifacts
        .iter()
        .map(NativeObjectArtifact::materialized_bytes)
        .sum();

    let write_results: Vec<Result<(), (PathBuf, std::io::Error)>> = artifacts
        .par_iter()
        .filter_map(|artifact| {
            artifact.bytes.as_ref().map(|bytes| {
                fs::write(&artifact.path, bytes).map_err(|err| (artifact.path.clone(), err))
            })
        })
        .collect();

    // Bail on first write failure (I/O errors are usually disk-full /
    // permission, not per-file recoverable).
    for r in write_results {
        if let Err((path, e)) = r {
            return Err(anyhow!(
                "failed to write object file {}: {}",
                path.display(),
                e
            ));
        }
    }

    // Sequential print + obj_paths collection (output grouped, source
    // order preserved).
    let mut obj_fingerprints: Vec<Option<String>> = Vec::new();
    for artifact in artifacts {
        match format {
            OutputFormat::Text => {
                let label = if artifact.reused_cache_path {
                    "Reused cached object"
                } else if artifact.stored_cache_path {
                    "Stored cached object"
                } else if artifact.path.extension().and_then(|e| e.to_str()) == Some("ll") {
                    "Wrote LLVM IR"
                } else {
                    "Wrote object file"
                };
                println!("{}: {}", label, artifact.path.display());
            }
            OutputFormat::Json => {}
        }
        if artifact.cleanup_after_link {
            obj_cleanup_paths.push(artifact.path.clone());
        }
        obj_fingerprints.push(Some(artifact.fingerprint));
        obj_paths.push(artifact.path);
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
    let entry_module_name: Option<String> =
        ctx.native_modules.get(&entry_path).map(|h| h.name.clone());
    if !failed_modules.is_empty() {
        let entry_failed = entry_module_name
            .as_deref()
            .map(|name| failed_modules.iter().any(|m| m == name))
            .unwrap_or(false);

        // #3527: a per-module codegen failure produces a broken (or empty
        // stub) object. The driver historically linked empty `<prefix>__init`
        // stubs for *non-entry* failed modules and still reported success
        // (`COMPILE_EXIT=0`). For a real program that's a false positive: the
        // textbook Express app surfaced 49 modules failing codegen, linked
        // anyway, and `Bus error: 10`d at launch with zero output. The exit
        // code lied. Default to aborting the build on ANY module codegen
        // failure so the failure is visible in the exit status. The old
        // stub-link path — genuinely useful for the iterative "peel back one
        // blocker at a time" debugging the issue author did — stays available
        // behind `PERRY_ALLOW_PARTIAL_CODEGEN=1`.
        let allow_partial = std::env::var_os("PERRY_ALLOW_PARTIAL_CODEGEN").is_some();
        // A failed entry module always aborts: its `main` symbol is required
        // by the linker and an empty `<prefix>__init` stub doesn't satisfy
        // it. A non-entry failure aborts unless the partial-codegen hatch is
        // set.
        let will_abort = entry_failed || !allow_partial;

        let bar = "═".repeat(72);
        let (red_on, red_off, bold_on, bold_off) = if use_color {
            ("\x1b[1;31m", "\x1b[0m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "")
        };
        eprintln!();
        eprintln!("{}{}{}", red_on, bar, red_off);
        if entry_failed {
            eprintln!(
                "{}✗ ENTRY MODULE FAILED TO COMPILE — REFUSING TO LINK{}",
                red_on, red_off
            );
        } else if will_abort {
            eprintln!(
                "{}✗ {} module(s) failed to compile — REFUSING TO LINK{}",
                red_on,
                failed_modules.len(),
                red_off
            );
        } else {
            eprintln!(
                "{}⚠ {} module(s) failed to compile — linking with empty stubs{}",
                red_on,
                failed_modules.len(),
                red_off
            );
        }
        eprintln!("{}{}{}", red_on, bar, red_off);
        eprintln!();
        for m in &failed_modules {
            let is_entry = Some(m.as_str()) == entry_module_name.as_deref();
            let marker = if is_entry { " (entry)" } else { "" };
            eprintln!("  - {}{}{}{}", bold_on, m, marker, bold_off);
        }
        eprintln!();
        if entry_failed {
            eprintln!("Aborting: the entry module's `main` symbol is required by the linker.");
            eprintln!("Fix the codegen errors above (search for `Error compiling module`)");
            eprintln!("and re-run. The driver previously emitted an empty `<prefix>__init`");
            eprintln!("stub here and continued to link, which produced the misleading");
            eprintln!("`Undefined symbols: \"_main\"` error far downstream.");
            eprintln!();
            return Err(anyhow!(
                "entry module '{}' failed to compile (see errors above)",
                entry_module_name.as_deref().unwrap_or("?")
            ));
        } else if will_abort {
            eprintln!(
                "Aborting: {} module(s) above failed codegen. Linking the surviving",
                failed_modules.len()
            );
            eprintln!("objects with empty stubs would produce a binary that crashes (Bus");
            eprintln!("error / SIGSEGV) the moment any code in a failed module runs — so the");
            eprintln!("build fails here rather than emitting a misleading COMPILE_EXIT=0.");
            eprintln!();
            eprintln!("Fix the codegen errors above (search for `Error compiling module`),");
            eprintln!("or set `PERRY_ALLOW_PARTIAL_CODEGEN=1` to link empty `<prefix>__init`");
            eprintln!("stubs for the failed modules and surface deeper errors during");
            eprintln!("iterative debugging (the resulting binary is inert/unsafe in those");
            eprintln!("modules and may crash at runtime).");
            eprintln!();
            return Err(anyhow!(
                "{} module(s) failed to compile (see errors above); set \
                 PERRY_ALLOW_PARTIAL_CODEGEN=1 to link empty stubs anyway",
                failed_modules.len()
            ));
        } else {
            eprintln!("PERRY_ALLOW_PARTIAL_CODEGEN=1 set: continuing with linking. Empty");
            eprintln!("`<prefix>__init` stubs will be emitted for the failed modules so the");
            eprintln!("binary still links, but any code in those modules will be inert at");
            eprintln!("runtime (and may crash if actually invoked).");
            eprintln!();
        }
    }

    if let Some(explain_lowering) = explain_lowering.as_ref() {
        explain_lowering.emit(format)?;
    }

    // #835 + #846: fold the codegen-side FFI provenance registry into
    // ctx so the well-known flip and `needs_stdlib` decisions below see
    // the symbols codegen actually emitted, not just the modules the
    // user imported. Today, codegen for compiled-package code can emit
    // (e.g.) `js_node_http_create_server` or `js_readable_stream_new`
    // without any `import "node:http"` / `import "streams"` showing up
    // in `ctx.native_module_imports` — Effect's `Stream`, Express's
    // server, and similar shapes lower the FFI calls directly. The
    // registry (`crates/perry-codegen/src/ext_registry.rs`) records
    // every call-emission site against its providing crate; here we
    // drain that record and route each entry through the existing
    // `needs_stdlib` + `native_module_imports` machinery. Done before
    // `build_optimized_libs` so `compute_required_features` and the
    // well-known flip both see the augmented set.
    {
        use perry_codegen::ext_registry::{take_used_providers, OwnerKind};
        let providers = take_used_providers();
        for owner in providers {
            match owner {
                OwnerKind::Stdlib { feature } => {
                    ctx.needs_stdlib = true;
                    // Follow-up to #835/#846: codegen-emitted Stdlib
                    // FFIs (Effect `Stream`, etc.) flip needs_stdlib
                    // here, but the auto-optimize layer
                    // (`build_optimized_libs`) rebuilds perry-stdlib
                    // with only the features `compute_required_features`
                    // derived from `native_module_imports` — which is
                    // empty when no `import "streams"` appears in the
                    // user TS. Without the feature, the symbol's
                    // module is `#[cfg]`-gated out and the link fails
                    // with "Undefined symbols: _js_readable_stream_…".
                    // Inject the feature here so the rebuild includes
                    // the providing module.
                    if let Some(feat) = feature {
                        ctx.extra_stdlib_features.insert(feat);
                    }
                }
                OwnerKind::WellKnown(key) => {
                    // Inserting into native_module_imports flips the
                    // well-known mechanism for this binding. Also flip
                    // `needs_stdlib` because the link step's
                    // "Linking (with stdlib)..." vs "(runtime-only)"
                    // gate is what brings the well-known libs onto the
                    // command line (see link.rs:881-916).
                    ctx.native_module_imports.insert(key.to_string());
                    ctx.needs_stdlib = true;
                }
            }
        }
    }

    // Auto-mode: pick the smallest matching (features, panic) profile
    // for this binary and rebuild perry-runtime + perry-stdlib in a
    // hash-keyed target dir. Both halves fall back to the prebuilt full
    // libraries if the rebuild fails or the workspace source isn't on
    // disk. `--no-auto-optimize` disables runtime/stdlib rebuilds but
    // still resolves prebuilt well-known wrapper archives whose symbols
    // are absent from the full stdlib.
    //
    // The legacy `--minimal-stdlib` flag is now a no-op alias for
    // backward compat — auto-mode already does what it used to and more.
    let optimized_libs: OptimizedLibs = if args.no_auto_optimize {
        optimized_libs::resolve_no_auto_optimized_libs(&ctx, target.as_deref(), format, verbose)
    } else {
        build_optimized_libs(&ctx, target.as_deref(), &compiled_features, format, verbose)
    };
    let stdlib_lib_resolved: Option<PathBuf> = optimized_libs
        .stdlib
        .clone()
        .or_else(|| find_stdlib_library(target.as_deref()));

    // Generate stubs for missing symbols from unresolved imports (npm packages etc.)
    {
        use std::collections::HashSet;
        let mut undefined_syms: HashSet<String> = HashSet::new();
        let mut defined_syms: HashSet<String> = HashSet::new();
        // Prefer the auto-built runtime so the symbol-stub scan and the
        // final link see the same artifact (panic mode + feature set).
        let runtime_lib_path = optimized_libs
            .runtime
            .clone()
            .or_else(|| find_runtime_library(target.as_deref()).ok());
        let stdlib_lib_path = stdlib_lib_resolved.clone();
        // Check if stdlib will be linked - if so, it provides perry_runtime symbols (no stubs needed)
        let target_is_windows =
            matches!(target.as_deref(), Some("windows") | Some("windows-winui"))
                || (cfg!(target_os = "windows") && target.is_none());
        let will_link_stdlib = (ctx.needs_stdlib || target_is_windows) && stdlib_lib_path.is_some();
        // Issue #76 — when the wasm host is
        // being linked, scan its archive so the `perry_wasm_host_*` symbols
        // are recognised as defined and we don't synthesise empty stubs that
        // would shadow the real implementations.
        let use_wasm_host = ctx.needs_wasm_runtime || args.enable_wasm_runtime;
        let wasm_host_lib_path = if use_wasm_host {
            find_wasm_host_library(target.as_deref())
        } else {
            None
        };
        let mut all_scan_paths: Vec<PathBuf> = obj_paths.clone();
        if let Some(ref p) = runtime_lib_path {
            all_scan_paths.push(p.clone());
        }
        if ctx.needs_stdlib {
            if let Some(ref p) = stdlib_lib_path {
                all_scan_paths.push(p.clone());
            }
        }
        if let Some(ref p) = wasm_host_lib_path {
            all_scan_paths.push(p.clone());
        }
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
        let is_android = matches!(target.as_deref(), Some("android") | Some("wearos"));
        let is_harmonyos = matches!(
            target.as_deref(),
            Some("harmonyos") | Some("harmonyos-simulator")
        );
        let is_linux = matches!(target.as_deref(), Some(t) if t.starts_with("linux"))
            || (!cfg!(target_os = "macos") && !cfg!(target_os = "windows") && target.is_none());
        let is_windows = matches!(target.as_deref(), Some("windows") | Some("windows-winui"))
            || (cfg!(target_os = "windows") && target.is_none());
        // Symbol prefix depends on object format:
        // Mach-O targets (macOS, iOS, watchOS, tvOS): nm shows `_` prefix
        // COFF (Windows targets): no prefix
        // ELF (Linux/Android/HarmonyOS targets): no prefix
        // Use TARGET (what we're compiling to), not HOST (what we're running on)
        let is_macho = matches!(
            target.as_deref(),
            Some("ios")
                | Some("ios-simulator")
                | Some("ios-widget")
                | Some("ios-widget-simulator")
                | Some("visionos")
                | Some("visionos-simulator")
                | Some("macos")
                | Some("watchos")
                | Some("watchos-simulator")
                | Some("tvos")
                | Some("tvos-simulator")
        ) || (!is_windows
            && !is_linux
            && !is_android
            && !is_harmonyos
            && cfg!(target_os = "macos"));
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
        let scan_results: Vec<(HashSet<String>, HashSet<String>)> = all_scan_paths
            .par_iter()
            .map(|scan_path| {
                let mut local_undef = HashSet::new();
                let mut local_def = HashSet::new();
                if let Ok(output) = std::process::Command::new(&nm_cmd)
                    .arg("-g")
                    .arg(scan_path)
                    .output()
                {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let (st, sn) = if parts.len() == 3 {
                                (parts[1], parts[2])
                            } else {
                                (parts[0], parts[1])
                            };
                            let cn = if is_macho {
                                sn.strip_prefix('_').unwrap_or(sn)
                            } else {
                                sn
                            };
                            if st == "U" {
                                if cn.starts_with("__export_") || cn.starts_with("__wrapper_") {
                                    local_undef.insert(cn.to_string());
                                } else if !will_link_stdlib
                                    && (cn == "js_call_function"
                                        || cn == "js_load_module"
                                        || cn == "js_new_from_handle"
                                        || cn == "js_new_instance"
                                        || cn == "js_create_callback"
                                        || cn == "js_runtime_init"
                                        || cn == "js_set_property"
                                        || cn == "js_get_export"
                                        || cn == "js_await_js_promise")
                                {
                                    local_undef.insert(cn.to_string());
                                } else if is_windows
                                    && (cn.starts_with("perry_ui_")
                                        || cn.starts_with("perry_system_")
                                        || cn.starts_with("perry_plugin_")
                                        || cn.starts_with("perry_get_"))
                                {
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
            if let OutputFormat::Text = format {
                eprintln!("  Generating stubs for {} missing symbols ({} data, {} functions, {} identity)", missing.len(), md.len(), mf.len(), mi.len());
                for s in &missing {
                    eprintln!("    - {}", s);
                }
            }
            let stub_bytes =
                perry_codegen::stubs::generate_stub_object(&md, &mf, &mi, target.as_deref())?;
            let stub_path = object_output_dir.join("_perry_stubs.o");
            fs::write(&stub_path, &stub_bytes)?;
            obj_cleanup_paths.push(stub_path.clone());
            obj_paths.push(stub_path);
            obj_fingerprints.push(None);
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
        let ll_files: Vec<PathBuf> = obj_paths
            .iter()
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ll"))
            .cloned()
            .collect();
        let stub_objs: Vec<PathBuf> = obj_paths
            .iter()
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
                    // Clean up intermediate .ll files unless the caller
                    // explicitly requested debuggable compiler artifacts.
                    if !args.keep_intermediates {
                        for ll in &ll_files {
                            let _ = fs::remove_file(ll);
                        }
                    }
                    // Replace obj_paths with the merged .o + any stubs.
                    // The merged object is derived after codegen-cache
                    // materialization, so the original per-module cache
                    // fingerprints are no longer a trusted proxy for these
                    // bytes.
                    obj_cleanup_paths.push(linked_obj.clone());
                    let mut linked_obj_paths = vec![linked_obj];
                    linked_obj_paths.extend(stub_objs);
                    obj_fingerprints = vec![None; linked_obj_paths.len()];
                    obj_paths = linked_obj_paths;
                    true
                }
                Err(e) => {
                    eprintln!(
                        "  bitcode-link: pipeline failed ({}), falling back to normal link",
                        e
                    );
                    false
                }
            }
        }
    } else if bitcode_link {
        // bitcode_link was requested but runtime .bc wasn't produced.
        // Fall back: compile any .ll files to .o via clang -c.
        eprintln!("  bitcode-link: runtime .bc not available, falling back to normal link");
        let mut new_obj_paths: Vec<PathBuf> = Vec::new();
        let mut new_obj_fingerprints: Vec<Option<String>> = Vec::new();
        for (idx, p) in obj_paths.iter().enumerate() {
            if p.extension().and_then(|e| e.to_str()) == Some("ll") {
                let ll_text = fs::read_to_string(p)?;
                let obj_bytes =
                    perry_codegen::linker::compile_ll_to_object(&ll_text, target.as_deref())?;
                let obj_path = p.with_extension("o");
                fs::write(&obj_path, &obj_bytes)?;
                if !args.keep_intermediates {
                    let _ = fs::remove_file(p);
                }
                obj_cleanup_paths.push(obj_path.clone());
                new_obj_paths.push(obj_path);
                new_obj_fingerprints.push(None);
            } else {
                new_obj_paths.push(p.clone());
                new_obj_fingerprints.push(obj_fingerprints.get(idx).cloned().unwrap_or(None));
            }
        }
        obj_paths = new_obj_paths;
        obj_fingerprints = new_obj_fingerprints;
        false
    } else {
        false
    };

    // Generate JS bundle if needed
    let _js_bundle_path = if !ctx.js_modules.is_empty() {
        let bundle_path = generate_js_bundle(&ctx, Path::new("."))?;
        match format {
            OutputFormat::Text => println!("Generated JS bundle: {}", bundle_path.display()),
            OutputFormat::Json => {}
        }
        // Issue #818 follow-up: embed every JS module's source into the
        // final binary too. The V8 fallback `ModuleLoader` consults this
        // map before falling back to disk, so the resulting binary needs
        // no `node_modules/` co-located at runtime. The compiled `.o`
        // contributes a `__attribute__((constructor))` that calls
        // `js_register_embedded_module` once per bundled file.
        let tmp_dir = std::env::temp_dir().join(format!("perry-embed-{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp_dir);
        match generate_embedded_js_object(&ctx, &tmp_dir) {
            Ok(obj) => {
                if matches!(format, OutputFormat::Text) {
                    println!("Embedded JS bundle: {}", obj.display());
                }
                obj_cleanup_paths.push(obj.clone());
                obj_paths.push(obj);
                obj_fingerprints.push(None);
            }
            Err(e) => {
                // Don't hard-fail — the on-disk `__perry_js_bundle.js`
                // still exists and the runtime falls back to filesystem
                // reads. Surface a warning so the build is visibly
                // degraded rather than silently shipping a binary that
                // requires `node_modules/`.
                eprintln!(
                    "warning: failed to embed JS bundle into binary ({}); the resulting binary will still require node_modules/ at runtime",
                    e
                );
            }
        }
        Some(bundle_path)
    } else {
        None
    };

    let raw_stem = args
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    // Issue #500: the input file stem flows into argv as `-o <stem>.dylib`
    // (and friends) to the linker. A pathological input filename like
    // `@evil.ts` re-triggers the ld64 response-file class of bug
    // (originally fixed in #467 for `package.json` names only). Route
    // through the shared sanitizer so the entire char-class is scrubbed
    // at one fuzz-tested choke point.
    let stem_owned = super::super::sanitize::sanitize_for_linker_argv(raw_stem);
    let stem = stem_owned.as_str();
    let is_dylib = args.output_type == "dylib";
    // #1088 — staticlib output: a Rust/C/C++ host links our `.a` / `.lib`
    // alongside `libperry_runtime.a` (and friends) and drives the event
    // loop itself via the FFI surface in `perry-runtime/src/event_pump.rs`
    // (`perry_poll`, `perry_has_work`, `perry_next_wake_ms`,
    // `perry_set_wake_callback`). Behaves like `dylib` at the codegen
    // layer (no `main` emission, `perry_module_init` entrypoint), but the
    // link step uses `ar` instead of `cc -shared`.
    let is_staticlib = args.output_type == "staticlib";
    // #854: kept as documentation of the library-output predicate; the
    // exe_path closure below branches on is_dylib/is_staticlib directly,
    // so this aggregate is currently unread.
    let _is_library_output = is_dylib || is_staticlib;
    // Capture the args fields that helpers downstream of the
    // `args.output.unwrap_or_else(...)` partial-move still need.
    // Per the saved feedback note on this file: any helper extracted
    // from `run_with_parse_cache` after this point must take individual
    // fields, not `&CompileArgs`.
    let input_path_owned: PathBuf = args.input.clone();
    let app_bundle_id_owned: Option<String> = args.app_bundle_id.clone();
    let exe_path = match args.output {
        // #4771: a user-supplied `-o NAME` without an extension won't launch
        // from PowerShell/cmd on a Windows target (and `.dll`/`.lib` are the
        // expected library shapes). Default the extension to the
        // target-appropriate one unless the user already gave one (e.g.
        // `-o app.appx` is respected verbatim). Non-Windows targets keep the
        // bare name — Unix executables are conventionally extension-less.
        Some(p) => {
            let is_windows_output =
                matches!(target.as_deref(), Some("windows") | Some("windows-winui"))
                    || (target.is_none() && cfg!(target_os = "windows"));
            if is_windows_output && p.extension().is_none() {
                p.with_extension(windows_default_output_extension(is_dylib, is_staticlib))
            } else {
                p
            }
        }
        None => default_output_path(is_dylib, is_staticlib, target.as_deref(), stem),
    };

    // The default output path when no `-o` is given. Extracted to a free fn so
    // the `-o`-provided extension-defaulting above stays readable.
    fn default_output_path(
        is_dylib: bool,
        is_staticlib: bool,
        target: Option<&str>,
        stem: &str,
    ) -> PathBuf {
        if is_dylib {
            #[cfg(target_os = "macos")]
            {
                PathBuf::from(format!("{}.dylib", stem))
            }
            #[cfg(not(target_os = "macos"))]
            {
                PathBuf::from(format!("{}.so", stem))
            }
        } else if is_staticlib {
            // #1088 — Windows hosts expect `.lib`; everywhere else uses
            // the Unix `lib<stem>.a` convention so the archive is reachable
            // from `-l<stem>` at the host's link step.
            if matches!(target, Some("windows") | Some("windows-winui"))
                || (target.is_none() && cfg!(target_os = "windows"))
            {
                PathBuf::from(format!("{}.lib", stem))
            } else {
                PathBuf::from(format!("lib{}.a", stem))
            }
        } else if matches!(target, Some("harmonyos") | Some("harmonyos-simulator")) {
            // HarmonyOS apps ship as .so loaded by the ArkTS runtime via
            // napi_module_register — there is no standalone executable
            // shipping shape. `lib` prefix matches the dlopen name used by
            // the generated ArkTS shim (`import entry from 'libapp.so'`).
            PathBuf::from(format!("lib{}.so", stem))
        } else if matches!(target, Some("windows") | Some("windows-winui"))
            || (target.is_none() && cfg!(target_os = "windows"))
        {
            PathBuf::from(format!("{}.exe", stem))
        } else {
            PathBuf::from(stem)
        }
    }

    if !failed_modules.is_empty() {
        // The loud failure summary + abort already ran earlier (right
        // after the parallel compile loop). #3527: reaching this block
        // with a non-empty `failed_modules` now implies the caller set
        // `PERRY_ALLOW_PARTIAL_CODEGEN=1` — without it, any module failure
        // returns `Err` up there. So by the time we get here we know the
        // entry module compiled OK and every entry in `failed_modules` is
        // a non-entry module the caller has explicitly opted to stub out
        // so the binary can still link.
        // Generate one empty `<prefix>__init` per failed module — the
        // entry main and any consumer module call each non-entry init
        // in order, so the symbols need to exist or the linker fails.
        //
        // #837 fix: the old format was `_perry_init_<sanitized>`, which
        // was the naming convention before the codegen switched to
        // `<prefix>__init` for module initializers (see
        // crates/perry-codegen/src/codegen.rs:4668). The stub symbols
        // never matched the consumer-side declarations, so any program
        // with a failed-but-stubbable module dep — for example uuid's
        // sha1.js, which v5.js imports and the codegen can't yet lower
        // because of Uint8Array.of with 20 args — failed at link with
        // `Undefined symbols: _<prefix>__init`. Tracking the codegen
        // naming closes the link without papering over the underlying
        // module-failure: the binary still links, the stubbed module
        // body is inert, and any actual call into the missing exports
        // remains the symptom that surfaces the real bug.
        let sanitize_module_name = |m: &str| -> String {
            let mut out: String = m
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                out.insert(0, '_');
            }
            out
        };
        let stub_init_names: Vec<String> = failed_modules
            .iter()
            .map(|m| format!("{}__init", sanitize_module_name(m)))
            .collect();
        // #903 follow-up (uuid regression): also emit closure-wrapper
        // stubs for the named exports of each failed module. Pre-#903 a
        // consumer's `import sha1 from "./sha1.js"` collided in the
        // shared `import_function_prefixes["default"]` slot with the
        // same file's `import v35 from "./v35.js"`, so the consumer-
        // side reference resolved to v35.js's wrapper symbol — which
        // existed because v35.js compiles fine. #903 corrected the
        // resolution so each default binding tracks its own source,
        // which surfaced uuid's preexisting sha1.js codegen failure
        // (`Uint8Array.of` with 20 args bails at lower_call.rs:~3226)
        // as a link error: `__perry_wrap_perry_fn_<sha1.js>__default`
        // is referenced by v5.js but never defined because sha1.js's
        // compile aborted before reaching the wrapper-emission loops
        // in codegen.rs:~2697 / ~2810.
        //
        // The link error is the symptom; the root cause (sha1.js
        // codegen) stays open. Emit no-op wrapper stubs so the link
        // succeeds — consumers that never call into the failed module
        // (uuid `v4()` is the canonical case; it doesn't use sha1)
        // run correctly, and consumers that DO call in observe a
        // NaN-boxed undefined return value (matching the inert
        // `__init` behavior).
        let mut stub_wrapper_names: Vec<String> = Vec::new();
        let mut stub_func_names: Vec<String> = Vec::new();
        for module_name in &failed_modules {
            let prefix = sanitize_module_name(module_name);
            // Look up the module's HIR (parse + lower succeeded; only
            // codegen failed, so the exports are known). The
            // `failed_modules` entry is `hir.name` from the codegen
            // error message at the par_iter site, not the original
            // path key, so iterate the native_modules map to find
            // the matching HIR.
            let Some(hir) = ctx.native_modules.values().find(|h| h.name == *module_name) else {
                continue;
            };
            for export in &hir.exports {
                if let perry_hir::Export::Named { exported, .. } = export {
                    let sanitized_exp = exported
                        .chars()
                        .map(|c| {
                            if c.is_ascii_alphanumeric() || c == '_' {
                                c
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>();
                    // Closure-wrapper form: consumer reads the import
                    // as a function value (`js_closure_alloc_singleton(
                    // @__perry_wrap_perry_fn_<src>__<name>)`).
                    let wrap_sym = format!("__perry_wrap_perry_fn_{}__{}", prefix, sanitized_exp);
                    stub_wrapper_names.push(wrap_sym);
                    // Direct-call form: consumer invokes the import
                    // by name (`perry_fn_<src>__<name>(args…)`). For
                    // a failed module the function never received a
                    // body, so emit a nullary stub returning undefined.
                    // The link only cares about the symbol existing;
                    // an arity mismatch at the call site lowers to an
                    // LLVM `call` with whatever args the consumer
                    // pushed — the body just discards them and
                    // returns undefined. Same fallback shape the
                    // empty `<prefix>__init` stub uses.
                    let direct_sym = format!("perry_fn_{}__{}", prefix, sanitized_exp);
                    stub_func_names.push(direct_sym);
                }
            }
        }
        // Combine the `__init` stubs and the direct-call stubs into
        // one `missing_func_symbols` bucket — both share the nullary-
        // returning-undefined shape. Dedup to keep LLVM from
        // complaining about duplicate definitions in case the same
        // export is named twice (e.g. an alias).
        stub_func_names.extend(stub_init_names);
        stub_func_names.sort();
        stub_func_names.dedup();
        stub_wrapper_names.sort();
        stub_wrapper_names.dedup();
        if !stub_func_names.is_empty() || !stub_wrapper_names.is_empty() {
            let stub_bytes = perry_codegen::stubs::generate_stub_object_full(
                &[],
                &stub_func_names,
                &[],
                &stub_wrapper_names,
                target.as_deref(),
            )?;
            let stub_path = object_output_dir.join("_perry_failed_stubs.o");
            fs::write(&stub_path, &stub_bytes)?;
            obj_cleanup_paths.push(stub_path.clone());
            obj_paths.push(stub_path);
            obj_fingerprints.push(None);
        }
    }

    if args.no_link {
        let codegen_cache_stats = if object_cache.is_enabled() {
            Some((
                object_cache.hits(),
                object_cache.misses(),
                object_cache.stores(),
                object_cache.store_errors(),
            ))
        } else {
            None
        };
        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            is_dylib,
            codegen_cache_stats,
            link_cache_stats: None,
            build_cache_stats: None,
        });
    }

    // #5731 — embed static assets into the binary. Merge `--embed` with
    // `perry.embed` / `[compile] embed`, expand globs/directories relative to
    // the project root, and emit a registration object linked alongside the
    // user objects. The runtime serves these via `perry` / node:fs at runtime.
    let embedded_assets = embed::resolve_embedded_assets(&args.embed, &project_root)?;
    if !embedded_assets.is_empty() {
        if let Some(obj) =
            embed::generate_embedded_asset_object(&embedded_assets, &object_output_dir)?
        {
            obj_cleanup_paths.push(obj.clone());
            obj_paths.push(obj);
            obj_fingerprints.push(None);
        }
        let total: u64 = embedded_assets
            .iter()
            .filter_map(|(_, p)| fs::metadata(p).ok().map(|m| m.len()))
            .sum();
        if let OutputFormat::Text = format {
            println!(
                "Embedding {} asset(s) ({} bytes) into the executable",
                embedded_assets.len(),
                total
            );
        }
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
    let is_visionos = matches!(
        target.as_deref(),
        Some("visionos-simulator") | Some("visionos")
    );
    let is_android = matches!(target.as_deref(), Some("android") | Some("wearos"));
    let is_harmonyos = matches!(
        target.as_deref(),
        Some("harmonyos") | Some("harmonyos-simulator")
    );
    let is_linux = matches!(target.as_deref(), Some(t) if t.starts_with("linux"))
        || (target.is_none() && cfg!(target_os = "linux"));
    let _is_windows = matches!(target.as_deref(), Some("windows") | Some("windows-winui"))
        || (target.is_none() && cfg!(target_os = "windows"));
    // is_watchos / is_tvos are defined below (near the per-platform link step).
    // The is_cross_* bindings used to live here, but they're now derived
    // inside `link::build_and_run_link` which is the only consumer.

    // #1088 — staticlib output: bundle the object files into a `.a` / `.lib`
    // archive. Skip runtime / stdlib linking entirely; the Rust/C/C++ host
    // is expected to link `libperry_runtime.a` (and any extension archives
    // it uses) alongside our archive at its own link step. Codegen already
    // emits `perry_module_init` instead of `main` (see is_dylib branch in
    // codegen/entry.rs, which now also covers `staticlib`).
    if is_staticlib {
        let is_windows_target =
            matches!(target.as_deref(), Some("windows") | Some("windows-winui"))
                || (target.is_none() && cfg!(target_os = "windows"));
        // Best-effort: drop a stale archive first so `ar` doesn't append to a
        // previous build's contents.
        let _ = fs::remove_file(&exe_path);
        let mut cmd = if is_windows_target {
            // MSVC `lib.exe` is the standard host on Windows; mingw users
            // can override with `AR=...` since `cc::ar_name()` parity isn't
            // available here.
            let mut c = Command::new("lib.exe");
            c.arg(format!("/OUT:{}", exe_path.display()));
            c
        } else {
            let mut c = Command::new("ar");
            // `c` create, `r` insert/replace, `s` write index. Matches what
            // rustc invokes via cc-rs for `crate-type = staticlib`.
            c.arg("crs").arg(&exe_path);
            c
        };
        for obj_path in &obj_paths {
            cmd.arg(obj_path);
        }
        let status = cmd.status()?;
        if !status.success() {
            return Err(anyhow!("Archiving staticlib failed"));
        }

        match format {
            OutputFormat::Text => println!("Wrote static archive: {}", exe_path.display()),
            OutputFormat::Json => {
                println!("{{\"output\": \"{}\"}}", exe_path.display());
            }
        }

        // #1088 follow-up: emit `<output>.linkdeps.json` next to the archive
        // so the host's build system can discover exactly which extra
        // archives it must add to its own link line. Perry already resolved
        // this set above (build_optimized_libs, the well-known table flips,
        // jsruntime / wasm-host finders) — emit it as a machine-readable
        // sidecar instead of forcing hosts to scrape the build log or
        // re-derive it from `well_known_bindings.toml`.
        // `libfoo.a` -> `libfoo.linkdeps.json`, `foo.lib` -> `foo.linkdeps.json`.
        // Drops the archive extension so the sidecar isn't named
        // `*.a.linkdeps.json`, which trips some tooling that strips file
        // extensions to derive a target's "name".
        let manifest_path = exe_path.with_extension("linkdeps.json");
        let mut link_archives: Vec<serde_json::Value> = Vec::new();
        let push_archive = |link_archives: &mut Vec<serde_json::Value>, role: &str, path: &Path| {
            let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            link_archives.push(serde_json::json!({
                "role": role,
                "path": abs.display().to_string(),
            }));
        };
        let runtime_lib_for_manifest = optimized_libs
            .runtime
            .clone()
            .or_else(|| find_runtime_library(target.as_deref()).ok());
        if let Some(p) = &runtime_lib_for_manifest {
            push_archive(&mut link_archives, "runtime", p);
        }
        if let Some(p) = &stdlib_lib_resolved {
            push_archive(&mut link_archives, "stdlib", p);
        }
        if ctx.needs_wasm_runtime || args.enable_wasm_runtime {
            if let Some(p) = find_wasm_host_library(target.as_deref()) {
                push_archive(&mut link_archives, "wasm-host", &p);
            }
        }
        if ctx.needs_ui {
            if let Some(p) = find_ui_library(target.as_deref()) {
                push_archive(&mut link_archives, "ui", &p);
            }
        }
        for p in &optimized_libs.well_known_libs {
            push_archive(&mut link_archives, "well-known", p);
        }
        let archive_abs = exe_path.canonicalize().unwrap_or_else(|_| exe_path.clone());
        let manifest = serde_json::json!({
            "version": 1,
            "archive": archive_abs.display().to_string(),
            "entry_symbol": "perry_module_init",
            "target": target.clone().unwrap_or_else(|| "native".to_string()),
            "link_archives": link_archives,
        });
        if let Err(e) = fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        ) {
            // Best-effort: a failed sidecar write shouldn't fail the
            // build — the archive is the load-bearing artifact, the
            // manifest is convenience. Surface the error so the host
            // can fall back to scraping `--verbose` output if needed.
            eprintln!(
                "warning: failed to write linkdeps manifest at {}: {}",
                manifest_path.display(),
                e
            );
        } else if let OutputFormat::Text = format {
            println!("Wrote link manifest: {}", manifest_path.display());
        }

        if !args.keep_intermediates {
            for obj_path in &obj_cleanup_paths {
                let _ = fs::remove_file(obj_path);
            }
            // Best-effort: drop the per-invocation staging dir (only when
            // empty — keep_intermediates or stray files leave it in place).
            let _ = fs::remove_dir(&object_output_dir);
        }

        let codegen_cache_stats = if object_cache.is_enabled() {
            Some((
                object_cache.hits(),
                object_cache.misses(),
                object_cache.stores(),
                object_cache.store_errors(),
            ))
        } else {
            None
        };
        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            // Reuse the dylib flag downstream — both library outputs share the
            // "no embedded event loop, host drives `perry_module_init`" shape.
            is_dylib: true,
            codegen_cache_stats,
            link_cache_stats: None,
            build_cache_stats: None,
        });
    }

    // For dylib output, skip runtime/stdlib linking — symbols resolve from host at dlopen time
    if is_dylib {
        let is_dylib_windows = matches!(target.as_deref(), Some("windows") | Some("windows-winui"))
            || (target.is_none() && cfg!(target_os = "windows"));
        let has_plugin_deactivate = ctx
            .native_modules
            .values()
            .any(|m| m.exported_functions.iter().any(|(n, _)| n == "deactivate"));
        let mut cmd = if is_dylib_windows {
            // Windows — emit a .dll via lld-link. The plugin DLL's external
            // references to `perry_*` / `js_*` resolve against the host
            // process at LoadLibrary time, just like macOS
            // `-flat_namespace -undefined dynamic_lookup`.
            //
            // A .def file IS still needed here — lld-link's default is to
            // emit an empty export table, and the host's `loadPlugin` calls
            // `GetProcAddress(handle, "plugin_activate")` to find the
            // plugin's entry point. The `LIBRARY` directive names the DLL
            // and the `EXPORTS` section lists the three plugin ABI symbols
            // that the codegen layer emits for the dylib's entry module
            // (see `compile_module_entry`). `plugin_deactivate` is
            // optional and only listed when the user's `deactivate`
            // function is actually exported.
            //
            // `/FORCE:UNRESOLVED` lets the linker produce the DLL even though
            // every `perry_*` / `js_*` symbol is undefined; the loader fills
            // them in from the host at LoadLibrary time. Without it, the
            // link fails with LNK2019 on the first unresolved `js_*` symbol
            // and no DLL is emitted.
            //
            // We use lld-link rather than MSVC link.exe here: lld-link honors
            // /FORCE:UNRESOLVED on the LLVM .o files that Perry emits (treating
            // the missing symbols as warnings that produce a runnable DLL),
            // whereas MSVC link.exe returns 0 without writing the DLL — see
            // the cross-linker note in `select_linker_command`.
            let linker = find_lld_link().unwrap_or_else(|| PathBuf::from("lld-link"));
            let mut c = Command::new(linker);
            c.arg("/NOLOGO").arg("/DLL").arg("/FORCE:UNRESOLVED");
            let stem = exe_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("perry_plugin");
            let def_path = std::env::temp_dir().join(format!(
                "perry_plugin_dylib_{}_{}.def",
                std::process::id(),
                stem
            ));
            if let Ok(mut def_file) = std::fs::File::create(&def_path) {
                use std::io::Write;
                let _ = writeln!(def_file, "LIBRARY {}", stem);
                let _ = writeln!(def_file, "EXPORTS");
                let _ = writeln!(def_file, "    plugin_activate");
                let _ = writeln!(def_file, "    perry_plugin_abi_version");
                if has_plugin_deactivate {
                    let _ = writeln!(def_file, "    plugin_deactivate");
                }
            }
            c.arg(format!("/DEF:{}", def_path.display()));
            c
        } else if is_linux {
            let mut c = Command::new("cc");
            c.arg("-shared");
            c
        } else {
            // macOS — use flat_namespace so plugins can resolve symbols from the host
            let mut c = Command::new("cc");
            c.arg("-dynamiclib")
                .arg("-flat_namespace")
                .arg("-undefined")
                .arg("dynamic_lookup");
            c
        };

        for obj_path in &obj_paths {
            cmd.arg(obj_path);
        }

        if is_dylib_windows {
            // MSVC link.exe takes the output path as `/OUT:<path>`, not `-o`.
            cmd.arg(format!("/OUT:{}", exe_path.display()));
            // Pull in the MSVC static C runtime (libcmt) so the CRT
            // auto-generated DllMain + `_fltused` etc. resolve. Without
            // this, `LoadLibraryW` of the plugin DLL returns
            // `ERROR_DLL_INIT_FAILED` (Win32 error 1114) because the
            // plugin's auto-emitted `DllMain` references unresolved
            // CRT symbols. `/FORCE:UNRESOLVED` lets the link succeed
            // with those still-unresolved entries, but the loader
            // fails DLL_PROCESS_ATTACH. Linking libcmt resolves
            // everything in the plugin itself.
            cmd.arg("/defaultlib:libcmt");
        } else {
            cmd.arg("-o").arg(&exe_path);
        }

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
            for obj_path in &obj_cleanup_paths {
                let _ = fs::remove_file(obj_path);
            }
            // Best-effort: drop the per-invocation staging dir (only when
            // empty — keep_intermediates or stray files leave it in place).
            let _ = fs::remove_dir(&object_output_dir);
        }

        let codegen_cache_stats = if object_cache.is_enabled() {
            Some((
                object_cache.hits(),
                object_cache.misses(),
                object_cache.stores(),
                object_cache.store_errors(),
            ))
        } else {
            None
        };
        return Ok(CompileResult {
            output_path: exe_path,
            target: target.clone().unwrap_or_else(|| "native".to_string()),
            bundle_id: None,
            is_dylib: true,
            codegen_cache_stats,
            link_cache_stats: None,
            build_cache_stats: None,
        });
    }

    // When geisterhand is enabled, prefer the geisterhand-enabled runtime
    // (has the registry, dispatch queue, and pump functions). Otherwise
    // prefer the auto-mode rebuild (which may be panic=abort) over the
    // prebuilt one. Auto-mode never enables panic=abort when geisterhand
    // is on, so the geisterhand path always uses the prebuilt variant.
    let runtime_lib = if ctx.needs_geisterhand {
        // The geisterhand-enabled runtime/UI/registry libs live in
        // target/geisterhand and are auto-built on first use. On a cold
        // build they don't exist yet at this point — the link step builds
        // any missing ones, but that runs *after* runtime_lib is resolved.
        // Build them now, before selecting the runtime, so we don't fall
        // through to find_runtime_library() and pick the *host* runtime
        // (wrong target + wrong feature set). That fallback is what makes a
        // cold `--target ios --enable-geisterhand` fail with "building for
        // 'iOS-simulator', but linking in object file built for 'macOS'"
        // (#1311 Ask #2). This mirrors the missing-libs check in the link
        // step and is idempotent — that check then finds them present.
        let gh_missing = find_geisterhand_runtime(target.as_deref()).is_none()
            || find_geisterhand_library(target.as_deref()).is_none()
            || (ctx.needs_stdlib && find_geisterhand_stdlib(target.as_deref()).is_none())
            || (ctx.needs_ui && find_geisterhand_ui(target.as_deref()).is_none());
        if gh_missing {
            build_geisterhand_libs(target.as_deref(), format)?;
        }
        match find_geisterhand_runtime(target.as_deref()) {
            Some(gh_rt) => gh_rt,
            None => find_runtime_library(target.as_deref())?,
        }
    } else if let Some(auto_rt) = optimized_libs.runtime.clone() {
        auto_rt
    } else {
        find_runtime_library(target.as_deref())?
    };
    // #1383 — under --enable-geisterhand, prefer the geisterhand-built stdlib
    // over the auto-optimized one. `build_geisterhand_libs` (already run above
    // when selecting `runtime_lib`) compiles perry-stdlib into target/geisterhand
    // with its full default feature set (incl. `async-runtime` → the
    // `perry_ffi_promise_*` shims) against the geisterhand-featured, hash-
    // consistent perry-runtime. The auto-optimized stdlib (`stdlib_lib_resolved`)
    // is rebuilt with --no-default-features and a feature set computed from the
    // app's *TS* imports, so it omits async-runtime when the async surface comes
    // from a native binding (@perryts/storekit/google-auth/play-billing) rather
    // than TS — producing the `Undefined symbols: _perry_ffi_promise_new` link
    // failure this issue describes. Linking the geisterhand stdlib also keeps the
    // bundled perry-runtime hash-consistent with `gh_runtime`. Fall back to the
    // auto-optimized stdlib when geisterhand is off or its stdlib isn't present.
    let stdlib_lib = if ctx.needs_geisterhand {
        find_geisterhand_stdlib(target.as_deref()).or_else(|| stdlib_lib_resolved.clone())
    } else {
        stdlib_lib_resolved.clone()
    };
    let is_watchos = matches!(
        target.as_deref(),
        Some("watchos") | Some("watchos-simulator")
    );
    let is_tvos = matches!(target.as_deref(), Some("tvos") | Some("tvos-simulator"));

    // Issue #76 — locate the wasmi-based host library when WebAssembly runtime
    // support is requested. Absence is
    // a hard error when codegen detected `WebAssembly.*` usage, otherwise the
    // flag-only case silently degrades to None (the user will hit a link
    // error on first use, with the symbol name as the breadcrumb).
    let wasm_host_lib = if ctx.needs_wasm_runtime || args.enable_wasm_runtime {
        match find_wasm_host_library(target.as_deref()) {
            Some(lib) => {
                if let OutputFormat::Text = format {
                    println!("Using wasmi WebAssembly host runtime");
                }
                Some(lib)
            }
            None => {
                if ctx.needs_wasm_runtime {
                    return Err(anyhow!(
                        "WebAssembly.* used but libperry_wasm_host.a not found. Build it with: cargo build --release -p perry-wasm-host"
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
    let link_cache_status = build_and_run_link(
        &args.input,
        &ctx,
        target.as_deref(),
        &obj_paths,
        &obj_fingerprints,
        &compiled_features,
        &runtime_lib,
        &stdlib_lib,
        &optimized_libs.well_known_libs,
        optimized_libs.prefer_well_known_before_stdlib,
        &wasm_host_lib,
        &exe_path,
        format,
        args.debug_symbols,
    )?;

    // HarmonyOS: emit the ArkTS EntryAbility + Index page next to the .so,
    // then bundle everything into a .hap. The ArkTS shim's import name is
    // templated off the actual .so filename so it matches at dlopen time.
    if is_harmonyos {
        if let Some(output_dir) = exe_path.parent() {
            let so_filename = exe_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("libperry_app.so");
            let stem = exe_path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("app")
                .trim_start_matches("lib");
            // Phase 2 v1 caveat: the destructive Index.ets harvest now happens
            // BEFORE codegen (see the harmonyos branch right after the i18n
            // transform pass). By the time we reach the post-link block here,
            // ctx.harmonyos_index_ets has the harvested ArkUI (if any). We just
            // pass it through to the EntryAbility/Index.ets writer.
            let index_ets = ctx.harmonyos_index_ets.as_deref();
            resources::stage_native_library_artifacts(&ctx, output_dir, format)?;
            let native_resources_dir = output_dir.join("NativeLibraries");
            match emit_harmonyos_arkts_stubs(output_dir, so_filename, index_ets) {
                Err(e) => eprintln!("Warning: failed to emit ArkTS shim: {}", e),
                Ok(()) => {
                    if matches!(format, OutputFormat::Text) {
                        println!("Wrote ArkTS shim: {}/ets/", output_dir.display());
                    }
                    let sdk = find_harmonyos_sdk();
                    // Locate the user's `assets/` folder so harmonyos_hap can
                    // copy it into the HAP's `resources/rawfile/`. Walk up
                    // from the entry file's directory looking for `assets/`
                    // — handles the common shape `<root>/src/app.ts` +
                    // `<root>/assets/icon.png` and the simpler in-root case.
                    let assets_dir = {
                        let mut probe = project_root.clone();
                        let mut found: Option<PathBuf> = None;
                        for _ in 0..4 {
                            let candidate = probe.join("assets");
                            if candidate.is_dir() {
                                found = Some(candidate);
                                break;
                            }
                            if !probe.pop() {
                                break;
                            }
                        }
                        found
                    };
                    let hap_args = crate::commands::harmonyos_hap::HapBuildArgs {
                        so_path: &exe_path,
                        ets_dir: &output_dir.join("ets"),
                        stem,
                        sdk_native: sdk.as_deref(),
                        quiet: !matches!(format, OutputFormat::Text),
                        // Phase 2 v7: forward CLI signing flags through to
                        // sign_hap. Each is None when the user didn't pass
                        // the flag; sign_hap then falls through to env var
                        // → saved config → bail.
                        p12_keystore: args.p12_keystore.as_deref(),
                        p12_password: args.p12_password.as_deref(),
                        cert_chain: args.harmonyos_cert.as_deref(),
                        profile: args.harmonyos_profile.as_deref(),
                        key_alias: args.harmonyos_key_alias.as_deref(),
                        assets_dir: assets_dir.as_deref(),
                        native_resources_dir: Some(native_resources_dir.as_path()),
                    };
                    match crate::commands::harmonyos_hap::build_hap(&hap_args) {
                        Ok(res) => {
                            if matches!(format, OutputFormat::Text) {
                                println!(
                                    "Wrote HAP: {} ({}, ets: {})",
                                    res.hap_path.display(),
                                    if res.signed { "signed" } else { "unsigned" },
                                    if res.abc_compiled {
                                        "bytecode"
                                    } else {
                                        "source"
                                    },
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
                        // Refs #564: use the shared probe helper so we also
                        // catch `target/<host-triple>/release/` when cargo
                        // is configured with a pinned default target.
                        let crate_target_dir = target_config.crate_path.join("target");
                        let candidate = library_search::locate_native_lib_artifact(
                            &crate_target_dir,
                            target.as_deref(),
                            lib_name,
                        );
                        if let Some(candidate) = candidate {
                            let dest = output_dir.join(lib_name);
                            if let Err(e) = fs::copy(&candidate, &dest) {
                                eprintln!(
                                    "Warning: failed to copy companion library {}: {}",
                                    lib_name, e
                                );
                            } else {
                                match format {
                                    OutputFormat::Text => {
                                        println!("Copied companion library: {}", lib_name)
                                    }
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
        let (app_dir, bundle_id) = build_ios_app_bundle(
            &input_path_owned,
            app_bundle_id_owned.as_deref(),
            &ctx,
            &exe_path,
            stem,
            target.as_deref(),
            &compiled_features,
            i18n_table.as_ref(),
            i18n_config.as_ref(),
            format,
        )?;
        result_bundle_id = Some(bundle_id);
        result_app_dir = Some(app_dir);
    } else if is_visionos {
        let (app_dir, bundle_id) = bundle_for_visionos(
            &exe_path,
            stem,
            target.as_deref(),
            &args.input,
            &ctx,
            i18n_table.as_ref(),
            i18n_config.as_ref(),
            format,
        )?;
        result_bundle_id = Some(bundle_id);
        result_app_dir = Some(app_dir);
    } else if is_watchos {
        let (app_dir, bundle_id) = bundle_for_watchos(
            &exe_path,
            stem,
            target.as_deref(),
            &args.input,
            &ctx,
            format,
        )?;
        result_bundle_id = Some(bundle_id);
        result_app_dir = Some(app_dir);
    } else if is_tvos {
        let (app_dir, bundle_id) = bundle_for_tvos(
            &exe_path,
            stem,
            target.as_deref(),
            &args.input,
            &ctx,
            format,
        )?;
        result_bundle_id = Some(bundle_id);
        result_app_dir = Some(app_dir);
    } else {
        // For Windows/Linux (non-bundle targets), copy asset directories next to the exe
        // so that resolve_asset_path can find them relative to the executable.
        if let Some(output_dir) = exe_path.parent() {
            let source_dir = args
                .input
                .canonicalize()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            if let Some(src_dir) = source_dir {
                let mut project_root = src_dir.clone();
                for _ in 0..5 {
                    if project_root.join("package.json").exists() {
                        break;
                    }
                    if let Some(parent) = project_root.parent() {
                        project_root = parent.to_path_buf();
                    } else {
                        break;
                    }
                }
                fn copy_dir_recursive_standalone(
                    src: &std::path::Path,
                    dst: &std::path::Path,
                ) -> std::io::Result<()> {
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
                let output_canon = output_resolved
                    .canonicalize()
                    .unwrap_or_else(|_| output_resolved.clone());
                let project_canon = project_root
                    .canonicalize()
                    .unwrap_or_else(|_| project_root.to_path_buf());
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
            if !is_harmonyos {
                resources::stage_native_library_artifacts(&ctx, output_dir, format)?;
            }
        }

        match format {
            OutputFormat::Text => println!("Wrote executable: {}", exe_path.display()),
            OutputFormat::Json => {
                let codegen_cache = summarize_codegen_cache_stats(&object_cache).map(
                    |(hits, misses, stores, store_errors)| {
                        serde_json::json!({
                            "hits": hits,
                            "misses": misses,
                            "stores": stores,
                            "store_errors": store_errors,
                            "path_reuses": object_cache.path_reuses(),
                            "hit_bytes_materialized": object_cache.bytes_materialized(),
                            "object_temp_writes": object_temp_writes,
                            "object_bytes_materialized": object_bytes_materialized,
                            "object_cache_paths_reused": object_cache_paths_reused,
                            "object_cache_paths_stored": object_cache_paths_stored,
                        })
                    },
                );
                let link_cache_stats = link_cache_status.stats();
                let result = serde_json::json!({
                    "success": true,
                    "output": exe_path.to_string_lossy(),
                    "native_modules": ctx.native_modules.len(),
                    "js_modules": ctx.js_modules.len(),
                    "build_cache": {
                        "hit": false,
                        "miss_reason": build_cache_stats.reason,
                    },
                    "codegen_cache": codegen_cache,
                    "link_cache": {
                        "linked": link_cache_stats.linked,
                        "skipped": link_cache_stats.skipped,
                        "object_fingerprints_used": link_cache_stats.object_fingerprints_used,
                        "object_files_hashed": link_cache_stats.object_files_hashed,
                        "external_inputs_hashed": link_cache_stats.external_inputs_hashed,
                    },
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }

        // #506 — emit `<binary>.sandbox` next to the binary when
        // `--emit-sandbox` (or the equivalent env / package.json
        // knob) is set. macOS only for the MVP; other platforms
        // log a once-per-build note that the kernel-sandbox MVP
        // is macOS-only and the matching seccomp / AppContainer /
        // ... support lands as #506 follow-up.
        if ctx.emit_sandbox {
            #[cfg(target_os = "macos")]
            {
                match super::super::sandbox_profile::emit_macos_sandbox_profile(&ctx, &exe_path) {
                    Ok(path) => match format {
                        OutputFormat::Text => {
                            println!("Wrote sandbox profile: {}", path.display())
                        }
                        OutputFormat::Json => {}
                    },
                    Err(e) => match format {
                        OutputFormat::Text => {
                            eprintln!("warning: failed to emit sandbox profile: {}", e);
                        }
                        OutputFormat::Json => {}
                    },
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                if let OutputFormat::Text = format {
                    eprintln!(
                        "note: `--emit-sandbox` is macOS-only in this MVP; Linux seccomp + Windows AppContainer support tracked under #506."
                    );
                }
            }
        }
    }

    emit_android_i18n_resources(
        is_android,
        i18n_table.as_ref(),
        i18n_config.as_ref(),
        &exe_path,
        format,
    );

    if link_cache_status.stats().linked {
        strip_final_binary(
            &ctx,
            &exe_path,
            target.as_deref(),
            is_dylib,
            is_ios,
            is_visionos,
            is_tvos,
            is_watchos,
            is_harmonyos,
        );
        write_link_cache_manifest(&link_cache_status, &exe_path);
    }

    let mut build_cache_runtime_inputs = Vec::new();
    build_cache_runtime_inputs.push(runtime_lib.clone());
    if let Some(path) = &stdlib_lib_resolved {
        build_cache_runtime_inputs.push(path.clone());
    }
    build_cache_runtime_inputs.extend(optimized_libs.well_known_libs.iter().cloned());
    if let Some(path) = &wasm_host_lib {
        build_cache_runtime_inputs.push(path.clone());
    }
    let build_cache_object_fingerprints: Vec<String> =
        obj_fingerprints.iter().filter_map(Clone::clone).collect();
    build_cache_probe.write_manifest_after_success(
        &mut build_cache_stats,
        &ctx,
        &exe_path,
        target.as_deref(),
        &compiled_features,
        &build_cache_object_fingerprints,
        &build_cache_runtime_inputs,
    );

    emit_attestation_sidecar(&ctx, &exe_path, format);

    print_binary_size(format, &exe_path);

    cleanup_intermediates(args.keep_intermediates, &obj_cleanup_paths);

    // #5206 / #5230: visible end-of-compile notice listing every
    // ahead-of-time-unsupported site that was compiled to a deferred runtime
    // error instead of blocking the build — runtime-unknown `eval(...)` /
    // `new Function(<dynamic body>)` and non-resolvable dynamic `import(...)`.
    // Strict mode (`--strict-eval` / `--strict-dynamic-import` / `perry.eval =
    // "error"` / `perry.dynamicImport = "error"` / `perry.strict`) never reaches
    // here for a covered site — it fails the build earlier. Text format only
    // (JSON consumers get a clean machine-readable result on stdout).
    print_deferred_eval_notice(format);

    let final_output_path = result_app_dir.unwrap_or(exe_path);
    let codegen_cache_stats = summarize_codegen_cache_stats(&object_cache);

    Ok(CompileResult {
        output_path: final_output_path,
        target: target.unwrap_or_else(|| "native".to_string()),
        bundle_id: result_bundle_id,
        is_dylib,
        codegen_cache_stats,
        link_cache_stats: Some(link_cache_status.stats()),
        build_cache_stats: Some(build_cache_stats),
    })
}
