//! Per-phase sub-routines extracted from `run_with_parse_cache`.
//!
//! Each helper is a pure refactor — a linear block lifted verbatim from the
//! orchestrator with the signature pared down to only the references the
//! original inline form already touched. The orchestrator stays the single
//! owner of mutable state; these wrappers just keep phase boundaries
//! visible in the file outline and let the orchestrator stay short.
//!
//! Group A — pre-collect bootstrap (`maybe_init_type_checker`,
//! `bundle_extensions_into_ctx`, `rerun_collect_with_class_field_types`,
//! `apply_geisterhand_args`). Issue #1105 (PR 1) was the original split.
//!
//! Group B — post-collect preflight (`enforce_js_runtime_gate`,
//! `recompute_common_project_root`, `enforce_capability_policy`,
//! `enforce_egress_policy`, `enforce_lockdown_policy`,
//! `validate_min_windows_version`). All run after `collect_modules` and
//! before the platform-specific early-return targets; together they used to
//! occupy ~350 lines of `run_with_parse_cache`.
//!
//! Group C — native-instance fixups + HarmonyOS harvest + i18n apply +
//! print_hir (`run_native_instance_fixups`, `harvest_harmonyos_index_ets`,
//! `apply_i18n_pass`, `dump_hir_for_debug`). Run after the platform
//! early-returns and before codegen dispatch.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::OutputFormat;

use super::audit_manifest::write_audit_manifest_logging_failures;
use super::collect_modules::collect_modules;
use super::resolve::discover_extension_entries;
use super::resources::find_project_root_for_resources;
use crate::commands::progress::VerboseProgress;

use super::{CompilationContext, CompileArgs, ParseCache};

// ============================================================================
// Group A: pre-collect bootstrap
// ============================================================================

/// Issue #1105 (PR 1): self-contained sub-routines extracted from
/// `run_with_parse_cache`. Pure refactor — each helper holds a
/// linear block lifted verbatim from the orchestrator. No logic
/// change; signatures take only the references each block already
/// touched in the inline form. The orchestrator stays the single
/// owner of state; these are convenience wrappers so phase
/// boundaries are visible in the file outline.
pub(super) fn maybe_init_type_checker(
    args: &CompileArgs,
    project_root: &Path,
    format: OutputFormat,
    ctx: &mut CompilationContext,
) {
    if !args.type_check {
        return;
    }
    match crate::commands::typecheck::TsGoClient::spawn(project_root) {
        Ok(mut client) => {
            // Try to load the project's tsconfig.json
            if let Some(tsconfig) = crate::commands::typecheck::find_tsconfig(project_root) {
                match format {
                    OutputFormat::Text => println!("  Type checking enabled (tsgo)"),
                    OutputFormat::Json => {}
                }
                if let Err(e) = client.load_project(&tsconfig) {
                    match format {
                        OutputFormat::Text => eprintln!(
                            "  Warning: tsgo project load failed: {}. Continuing without type checking.",
                            e
                        ),
                        OutputFormat::Json => {}
                    }
                } else {
                    ctx.type_checker = Some(client);
                }
            } else {
                match format {
                    OutputFormat::Text => {
                        eprintln!("  Warning: No tsconfig.json found. Type checking disabled.")
                    }
                    OutputFormat::Json => {}
                }
            }
        }
        Err(e) => match format {
            OutputFormat::Text => eprintln!("  Warning: {}", e),
            OutputFormat::Json => {}
        },
    }
}

/// Collect each `--bundle-extensions` entry into the same
/// `CompilationContext` as the user entry. Returns `(canonical_path,
/// plugin_id)` pairs so the orchestrator can later embed the plugin
/// IDs in the final binary.
#[allow(clippy::too_many_arguments)]
pub(super) fn bundle_extensions_into_ctx(
    ext_dir: &Path,
    args: &CompileArgs,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    progress: &VerboseProgress,
    mut parse_cache: Option<&mut ParseCache>,
    format: OutputFormat,
) -> Result<Vec<(PathBuf, String)>> {
    let ext_entries = discover_extension_entries(ext_dir)?;
    match format {
        OutputFormat::Text => println!("Bundling {} extension(s)...", ext_entries.len()),
        OutputFormat::Json => {}
    }
    let mut bundled_extensions: Vec<(PathBuf, String)> = Vec::new();
    for (entry_path, plugin_id) in &ext_entries {
        match format {
            OutputFormat::Text => {
                println!("  Extension: {} ({})", plugin_id, entry_path.display())
            }
            OutputFormat::Json => {}
        }
        collect_modules(
            entry_path,
            ctx,
            visited,
            format,
            args.target.as_deref(),
            next_class_id,
            skip_transforms,
            progress,
            parse_cache.as_deref_mut(),
        )?;
        bundled_extensions.push((entry_path.canonicalize()?, plugin_id.clone()));
    }
    Ok(bundled_extensions)
}

/// Cross-module class field type propagation pass. Pass 1 lowered every
/// native module without knowledge of imported classes' field types, so
/// for-of loops over fields like `someLocal.removes` (where `someLocal:
/// SomeClassFromAnotherModule`, `removes: Set<...>`) silently iterated 0
/// times — the iterable's static type was unknown and the `SetValues`
/// wrap at `lower_decl.rs:3737-3747` was skipped. Harvest field types
/// from every just-lowered class, then re-lower the entire module set
/// with that map seeded into each `LoweringContext`. The double pass is
/// wasted work for modules that only consume locally-defined classes,
/// but the per-module cost is dominated by SWC parsing (cached by
/// `parse_cache`) not HIR lowering, so the overhead in practice is
/// small. See ECS demo-simple repro / #412.
#[allow(clippy::too_many_arguments)]
pub(super) fn rerun_collect_with_class_field_types(
    args: &CompileArgs,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    progress: &VerboseProgress,
    mut parse_cache: Option<&mut ParseCache>,
    format: OutputFormat,
) -> Result<()> {
    if ctx.native_modules.len() <= 1 {
        return Ok(());
    }
    let mut field_map: HashMap<String, Vec<(String, perry_types::Type)>> = HashMap::new();
    let mut accessor_map: HashMap<String, perry_hir::ClassAccessorNames> = HashMap::new();
    let mut parent_map: HashMap<String, String> = HashMap::new();
    for hir_module in ctx.native_modules.values() {
        for class in &hir_module.classes {
            let fields: Vec<(String, perry_types::Type)> = class
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            field_map.entry(class.name.clone()).or_insert(fields);
            let mut accessors = perry_hir::ClassAccessorNames::default();
            for (name, _) in &class.getters {
                accessors.insert_getter(name.clone());
            }
            for (name, _) in &class.setters {
                accessors.insert_setter(name.clone());
            }
            accessor_map.entry(class.name.clone()).or_insert(accessors);
            if let Some(parent_name) = &class.extends_name {
                parent_map
                    .entry(class.name.clone())
                    .or_insert_with(|| parent_name.clone());
            }
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (class_name, parent_name) in parent_map.clone() {
            let parent_accessors = accessor_map.get(&parent_name).cloned().unwrap_or_default();
            if parent_accessors.is_empty() {
                continue;
            }
            let entry = accessor_map.entry(class_name).or_default();
            changed |= entry.extend_from(&parent_accessors);
        }
    }
    if field_map.is_empty() && accessor_map.is_empty() {
        return Ok(());
    }
    ctx.cross_module_class_field_types = field_map;
    ctx.cross_module_class_accessors = accessor_map;
    ctx.native_modules.clear();
    visited.clear();
    *next_class_id = 1;
    collect_modules(
        &args.input,
        ctx,
        visited,
        format,
        args.target.as_deref(),
        next_class_id,
        skip_transforms,
        progress,
        parse_cache.as_deref_mut(),
    )?;
    if let Some(ext_dir) = &args.bundle_extensions {
        let ext_entries = discover_extension_entries(ext_dir)?;
        for (entry_path, _plugin_id) in &ext_entries {
            collect_modules(
                entry_path,
                ctx,
                visited,
                format,
                args.target.as_deref(),
                next_class_id,
                skip_transforms,
                progress,
                parse_cache.as_deref_mut(),
            )?;
        }
    }
    Ok(())
}

/// Apply the geisterhand CLI knobs (`--enable-geisterhand`,
/// `--geisterhand-port=<N>`) to the compilation context. Geisterhand
/// is the in-process input fuzzer; setting `needs_geisterhand=true`
/// links the geisterhand-enabled runtime and starts an HTTP server
/// at runtime.
pub(super) fn apply_geisterhand_args(args: &CompileArgs, ctx: &mut CompilationContext) {
    if args.enable_geisterhand || args.geisterhand_port.is_some() {
        ctx.needs_geisterhand = true;
        if let Some(port) = args.geisterhand_port {
            ctx.geisterhand_port = port;
        }
    }
}

// ============================================================================
// Group B: post-collect preflight (audit / lockdown / SBOM / windows-version)
// ============================================================================

/// Refuse any build that reaches a JavaScript module the resolver can
/// only evaluate through a runtime JS engine. Perry no longer ships a
/// runtime JS runtime (`perry-jsruntime`, V8 via `deno_core`, was
/// removed) — these binaries are V8-free and compile TypeScript
/// ahead-of-time only. The check fires after dep collection so the
/// diagnostic can name every file that introduced the dependency.
pub(super) fn enforce_js_runtime_gate(ctx: &CompilationContext) -> Result<()> {
    let importers = &ctx.js_runtime_importers;
    if importers.is_empty() {
        return Ok(());
    }
    let mut detail = String::new();
    // Cap the printed list at the first eight importers — pathological
    // builds can pull in dozens of node_modules JS files and we'd
    // rather show the head of the list than a 60-line error.
    let limit = 8usize;
    for path in importers.iter().take(limit) {
        let pkg = super::audit_manifest::package_name_for_path(&path.to_string_lossy())
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();
        let declaration = ctx
            .declaration_sidecars
            .get(path)
            .map(|p| format!(" (declarations: {})", p.display()))
            .unwrap_or_default();
        detail.push_str(&format!("\n  - {}{}{}", path.display(), pkg, declaration));
    }
    if importers.len() > limit {
        detail.push_str(&format!("\n  ... and {} more", importers.len() - limit));
    }
    let mut packages: Vec<String> = importers
        .iter()
        .filter_map(|path| super::audit_manifest::package_name_for_path(&path.to_string_lossy()))
        .filter(|pkg| !ctx.compile_packages.contains(pkg))
        .collect();
    packages.sort();
    packages.dedup();
    let package_hint = if packages.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nPackage hint: the following npm package(s) are still routed \
             to runtime JavaScript because they are not in \
             `perry.compilePackages`: {}. If you have reviewed and trust \
             them, add them to both `perry.compilePackages` and \
             `perry.allow.compilePackages`.",
            packages.join(", ")
        )
    };
    let mut declaration_packages: Vec<String> = importers
        .iter()
        .filter(|path| ctx.declaration_sidecars.contains_key(*path))
        .filter_map(|path| super::audit_manifest::package_name_for_path(&path.to_string_lossy()))
        .filter(|pkg| !ctx.compile_packages.contains(pkg))
        .collect();
    declaration_packages.sort();
    declaration_packages.dedup();
    let declaration_hint = if declaration_packages.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nDeclaration hint: Perry found `.d.ts` sidecar metadata for: {}. \
             Declarations describe the API shape, but compiling a dependency's \
             JavaScript implementation into native code still requires the host \
             trust opt-in: add the package to both `perry.compilePackages` and \
             `perry.allow.compilePackages` after review.",
            declaration_packages.join(", ")
        )
    };
    anyhow::bail!(
        "JavaScript runtime (V8) support has been removed. This build of \
         Perry compiles TypeScript ahead-of-time only and cannot evaluate \
         JavaScript modules at runtime. The build pulled in a JS runtime \
         via the following file(s):{detail}\n\
         \n\
         Port the offending module(s) to TypeScript, add the owning package \
         to `perry.compilePackages` so it is compiled natively, or replace \
         it with a native Perry stdlib equivalent.{package_hint}{declaration_hint}"
    );
}

fn is_bare_package_specifier(source: &str) -> bool {
    !source.starts_with('.') && !source.starts_with('/')
}

fn module_provides_export(
    ctx: &mut CompilationContext,
    module_path: &Path,
    export_name: &str,
    seen: &mut HashSet<(PathBuf, String)>,
) -> bool {
    let key = (module_path.to_path_buf(), export_name.to_string());
    if !seen.insert(key) {
        return false;
    }

    let Some(module) = ctx.native_modules.get(module_path) else {
        return false;
    };
    let exports = module.exports.clone();

    for export in exports {
        match export {
            perry_hir::Export::Named { exported, .. } if exported == export_name => {
                return true;
            }
            perry_hir::Export::NamespaceReExport { name, .. } if name == export_name => {
                return true;
            }
            perry_hir::Export::ReExport {
                source,
                imported,
                exported,
            } if exported == export_name => {
                if let Some((resolved, perry_hir::ModuleKind::NativeCompiled)) =
                    super::cached_resolve_import(&source, module_path, ctx)
                {
                    if module_provides_export(ctx, &resolved, &imported, seen) {
                        return true;
                    }
                }
            }
            perry_hir::Export::ExportAll { source } if export_name != "default" => {
                if let Some((resolved, perry_hir::ModuleKind::NativeCompiled)) =
                    super::cached_resolve_import(&source, module_path, ctx)
                {
                    if module_provides_export(ctx, &resolved, export_name, seen) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }

    false
}

/// Validate default imports from package entries that Perry compiles natively.
///
/// Perry lowers JS-module default imports to a `__default` wrapper symbol. If a
/// package entry is ESM-shaped and exposes only named exports, that wrapper has
/// no producer and the user sees a native linker error. Match Node's static ESM
/// behavior for the package-import case by rejecting the import while the module
/// graph still has source/package context.
pub(super) fn enforce_package_default_exports(ctx: &mut CompilationContext) -> Result<()> {
    let import_edges: Vec<(PathBuf, String, String, String)> = ctx
        .native_modules
        .iter()
        .flat_map(|(importer_path, module)| {
            module.imports.iter().filter_map(|import| {
                if import.type_only
                    || import.is_dynamic
                    || import.is_native
                    // Issue #5257: an adopted `require('S')` (CJS wrap synthesized
                    // `import _req_N from 'S'`) is NOT a static-ESM default import —
                    // `require()` hands back the exports object, so a named-only / CJS
                    // target is valid and must route through the namespace machinery
                    // instead of failing Node's "no default export" rule.
                    || import.is_adopted_require
                    || import.module_kind != perry_hir::ModuleKind::NativeCompiled
                    || !is_bare_package_specifier(&import.source)
                {
                    return None;
                }
                let resolved_path = import.resolved_path.as_ref()?;
                let default_local =
                    import
                        .specifiers
                        .iter()
                        .find_map(|specifier| match specifier {
                            perry_hir::ImportSpecifier::Default { local } => Some(local.clone()),
                            _ => None,
                        })?;
                Some((
                    importer_path.clone(),
                    import.source.clone(),
                    resolved_path.clone(),
                    default_local,
                ))
            })
        })
        .collect();

    for (importer_path, source, resolved_path, local) in import_edges {
        let target_path = PathBuf::from(&resolved_path);
        if !ctx.native_modules.contains_key(&target_path) {
            continue;
        }
        if !module_provides_export(ctx, &target_path, "default", &mut HashSet::new()) {
            anyhow::bail!(
                "The requested package '{}' does not provide an export named 'default' \
                 (imported as '{}' in {}). Resolved package entry: {}",
                source,
                local,
                importer_path.display(),
                target_path.display()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod js_runtime_gate_tests {
    use std::path::PathBuf;

    use super::{enforce_js_runtime_gate, enforce_package_default_exports, CompilationContext};

    fn empty_module(name: &str) -> perry_hir::Module {
        perry_hir::Module::new(name)
    }

    #[test]
    fn diagnostic_suggests_missing_compile_package_on_windows_paths() {
        let mut ctx = CompilationContext::new(PathBuf::from(r"C:\repo"));
        ctx.compile_packages.insert("ps-node".to_string());
        ctx.compile_packages.insert("table-parser".to_string());
        ctx.js_runtime_importers.push(PathBuf::from(
            r"C:\repo\node_modules\connected-domain\index.js",
        ));

        let message = enforce_js_runtime_gate(&ctx)
            .expect_err("runtime JS importer must fail the V8-free gate")
            .to_string();

        assert!(message.contains("connected-domain"));
        assert!(message.contains("perry.compilePackages"));
        assert!(message.contains("perry.allow.compilePackages"));
    }

    #[test]
    fn diagnostic_does_not_suggest_already_opted_in_packages() {
        let mut ctx = CompilationContext::new(PathBuf::from("/repo"));
        ctx.compile_packages.insert("already-native".to_string());
        ctx.js_runtime_importers
            .push(PathBuf::from("/repo/node_modules/already-native/index.js"));

        let message = enforce_js_runtime_gate(&ctx)
            .expect_err("runtime JS importer must fail the V8-free gate")
            .to_string();

        assert!(!message.contains("Package hint:"));
    }

    #[test]
    fn diagnostic_mentions_declaration_sidecar_for_typed_js_package() {
        let mut ctx = CompilationContext::new(PathBuf::from("/repo"));
        let implementation = PathBuf::from("/repo/node_modules/typed-js/dist/index.js");
        let declaration = PathBuf::from("/repo/node_modules/typed-js/dist/index.d.ts");
        ctx.js_runtime_importers.push(implementation.clone());
        ctx.declaration_sidecars
            .insert(implementation, declaration.clone());

        let message = enforce_js_runtime_gate(&ctx)
            .expect_err("runtime JS importer must fail the V8-free gate")
            .to_string();

        assert!(message.contains(&format!("declarations: {}", declaration.display())));
        assert!(message.contains("Declaration hint:"));
        assert!(message.contains("typed-js"));
    }

    #[test]
    fn package_default_import_without_default_export_fails_preflight() {
        let mut ctx = CompilationContext::new(PathBuf::from("/repo"));
        let importer_path = PathBuf::from("/repo/main.ts");
        let package_path = PathBuf::from("/repo/node_modules/pkg/index.ts");

        let mut importer = empty_module("main");
        importer.imports.push(perry_hir::Import {
            source: "pkg".to_string(),
            specifiers: vec![perry_hir::ImportSpecifier::Default {
                local: "Pkg".to_string(),
            }],
            is_native: false,
            module_kind: perry_hir::ModuleKind::NativeCompiled,
            resolved_path: Some(package_path.to_string_lossy().to_string()),
            type_only: false,
            is_dynamic: false,
            is_dynamic_target: false,
            is_deferred_require: false,
            is_adopted_require: false,
        });

        let mut package = empty_module("pkg");
        package.exports.push(perry_hir::Export::Named {
            local: "named".to_string(),
            exported: "named".to_string(),
        });

        ctx.native_modules.insert(importer_path, importer);
        ctx.native_modules.insert(package_path, package);

        let message = enforce_package_default_exports(&mut ctx)
            .expect_err("missing package default export must fail before codegen")
            .to_string();

        assert!(message.contains("pkg"));
        assert!(message.contains("default"));
        assert!(message.contains("Resolved package entry"));
    }
}

/// Recompute project_root as the common ancestor of all module paths.
/// The initial project_root is the parent of the entry file, but modules may be in sibling
/// directories (e.g., entry in workers/, modules in lib/). This ensures unique module names.
pub(super) fn recompute_common_project_root(ctx: &mut CompilationContext) {
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
                        let filename = path
                            .file_name()
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
}

/// #501: host-controlled per-package capability enforcement. When
/// `perry.permissions` is set, walk every dep module's HIR and
/// refuse any stdlib call site whose required capability token
/// isn't in the dep's allow-list (or the `*` default). Host code
/// is granted `*` unconditionally — gating host code is the
/// `--lockdown` mode (#496), not per-package policy. Empty policy
/// → pass disabled (preserves existing build behavior).
pub(super) fn enforce_capability_policy(ctx: &CompilationContext) -> Result<()> {
    if ctx.permissions.is_empty() {
        return Ok(());
    }
    let mut all_violations: Vec<perry_hir::CapabilityViolation> = Vec::new();
    for (path, hir_module) in &ctx.native_modules {
        let source = path.to_string_lossy().into_owned();
        let v = perry_hir::audit_module_capabilities(
            hir_module,
            &source,
            &ctx.permissions,
            ctx.host_package_name.as_deref(),
        );
        all_violations.extend(v);
    }
    if !all_violations.is_empty() {
        let limit = 12usize;
        let mut detail = String::new();
        for v in all_violations.iter().take(limit) {
            let pkg_label = v
                .package
                .as_deref()
                .map(|p| format!("`{}`", p))
                .unwrap_or_else(|| "<host>".to_string());
            detail.push_str(&format!(
                "\n  - {pkg} {kind} at {source} requires `{cap}`",
                pkg = pkg_label,
                kind = v.kind,
                source = v.source,
                cap = v.required,
            ));
        }
        if all_violations.len() > limit {
            detail.push_str(&format!(
                "\n  ... and {} more",
                all_violations.len() - limit
            ));
        }
        anyhow::bail!(
            "per-package capability policy refused {} stdlib call site(s):{detail}\n\
             \n\
             `perry.permissions` provides a static guarantee that each\n\
             dependency only reaches the stdlib surfaces you've explicitly\n\
             granted it. Refusing the build. (#501)\n\
             \n\
             To fix each violation, either:\n\
             - Add the missing capability token to the package's entry in\n\
               `perry.permissions` in your host `package.json`, or\n\
             - Set `\"*\": [\"<token>\"]` to grant the default, or\n\
             - Replace the offending dep with one that doesn't need the\n\
               capability.\n\
             \n\
             Run `perry audit --sbom` (when #495 lands) to see every\n\
             stdlib surface each dep would need.",
            all_violations.len(),
        );
    }
    Ok(())
}

/// #502: compile-time URL/host egress allowlist. When the host
/// has opted in via `perry.allowedHosts`, walk every HIR module
/// and refuse any literal URL/host that doesn't match a pattern
/// there. Non-literal URLs/hosts are refused unless
/// `perry.allowDynamicHosts: true`. All violations across all
/// modules are collected and surfaced in a single diagnostic so
/// the user can fix every site at once.
pub(super) fn enforce_egress_policy(ctx: &CompilationContext) -> Result<()> {
    if ctx.allowed_hosts.is_empty() {
        return Ok(());
    }
    let mut all_violations: Vec<perry_hir::EgressViolation> = Vec::new();
    for (path, hir_module) in &ctx.native_modules {
        let source = path.to_string_lossy().into_owned();
        let v = perry_hir::audit_module_egress(
            hir_module,
            &source,
            &ctx.allowed_hosts,
            ctx.allow_dynamic_hosts,
        );
        all_violations.extend(v);
    }
    if !all_violations.is_empty() {
        let mut detail = String::new();
        let limit = 12usize;
        for v in all_violations.iter().take(limit) {
            let lit = match v.literal.as_deref() {
                Some(s) => format!("\"{}\"", s),
                None => "<non-literal>".to_string(),
            };
            let why = match v.reason {
                perry_hir::EgressRefusalReason::LiteralNotAllowed => {
                    "literal host not in `perry.allowedHosts`"
                }
                perry_hir::EgressRefusalReason::NonLiteralAndDynamicForbidden => {
                    "non-literal URL/host (`perry.allowDynamicHosts: true` not set)"
                }
            };
            detail.push_str(&format!(
                "\n  - {}: {} → {} ({})",
                v.source, v.kind, lit, why
            ));
        }
        if all_violations.len() > limit {
            detail.push_str(&format!(
                "\n  ... and {} more",
                all_violations.len() - limit
            ));
        }
        anyhow::bail!(
            "egress allowlist refused {} call site(s):{detail}\n\
             \n\
             `perry.allowedHosts` provides a static guarantee that this binary's\n\
             outbound network surface matches the declared list. Refusing the build.\n\
             (#502)\n\
             \n\
             Options:\n\
             - Add the offending host(s) to `perry.allowedHosts` in your host\n\
               `package.json` (exact `\"api.example.com\"`, subdomain wildcard\n\
               `\"*.cdn.example.com\"`, or URL-prefix `\"https://.../path/*\"`).\n\
             - Set `\"*\"` in `allowedHosts` to disable host gating (escape hatch\n\
               that defeats the static guarantee — use only for migration).\n\
             - For non-literal URLs, set `perry.allowDynamicHosts: true` if the\n\
               computed shape is intentional. Code review then has to trust the\n\
               value of every variable that reaches `fetch(...)`.",
            all_violations.len(),
        );
    }
    Ok(())
}

/// #496: `--lockdown` mode — fail the build if any standard
/// arbitrary-code-execution surface is reachable. The check has
/// three parts and runs after `collect_modules` so every
/// dependency-graph signal is settled before we emit the
/// diagnostic. All three are collected into one combined error
/// so the reviewer can address every offending site at once.
pub(super) fn enforce_lockdown_policy(ctx: &CompilationContext) -> Result<()> {
    if !ctx.lockdown {
        return Ok(());
    }
    let mut reasons: Vec<String> = Vec::new();

    if !ctx.native_libraries.is_empty() {
        let mut names: Vec<&str> = ctx
            .native_libraries
            .iter()
            .map(|n| n.module.as_str())
            .collect();
        names.sort();
        names.dedup();
        reasons.push(format!(
            "`perry.nativeLibrary` archives referenced by: {}",
            names.join(", ")
        ));
    }

    let mut hir_violations: Vec<perry_hir::LockdownViolation> = Vec::new();
    for (path, hir_module) in &ctx.native_modules {
        let source = path.to_string_lossy().into_owned();
        let v = perry_hir::audit_module_lockdown(hir_module, &source);
        hir_violations.extend(v);
    }
    if !hir_violations.is_empty() {
        let limit = 12usize;
        let mut detail = String::new();
        for v in hir_violations.iter().take(limit) {
            detail.push_str(&format!("\n      - {}: {}", v.source, v.kind));
        }
        if hir_violations.len() > limit {
            detail.push_str(&format!(
                "\n      ... and {} more",
                hir_violations.len() - limit
            ));
        }
        reasons.push(format!(
            "`child_process.*` reached from {} call site(s):{}",
            hir_violations.len(),
            detail
        ));
    }

    if !reasons.is_empty() {
        let mut formatted = String::new();
        for r in &reasons {
            formatted.push_str(&format!("\n  - {}", r));
        }
        anyhow::bail!(
            "`--lockdown` refused the build because the following \
             arbitrary-code-execution surfaces are reachable:{formatted}\n\
             \n\
             Lockdown is the single-flag opt-in to \"this app is provably \
             free of arbitrary-code-execution vectors\" (#496). When set, \
             the build refuses to link perry-jsruntime, refuses any \
             perry.nativeLibrary archive reference, and refuses any source \
             module that reaches child_process.* — all in one combined \
             check so reviewers see the whole surface at once.\n\
             \n\
             To run without lockdown for one build:\n\
             - Pass `--lockdown=false` explicitly on the CLI, or\n\
             - Set `PERRY_LOCKDOWN=0` in the environment.\n\
             \n\
             To make the build actually lockdown-clean: remove the \
             offending surfaces, or replace them with native Perry \
             equivalents (e.g. `perry/thread` for child_process workloads, \
             native Perry stdlib for jsruntime-only deps)."
        );
    }
    Ok(())
}

/// Validate --min-windows-version. Accepted: "7", "8", "10". Anything
/// else is a hard error so typos like `--min-windows-version=11` fail
/// loudly instead of silently behaving like the default. See issue #303
/// and `docs/src/platforms/windows-7.md`.
pub(super) fn validate_min_windows_version(
    args: &CompileArgs,
    ctx: &mut CompilationContext,
) -> Result<()> {
    match args.min_windows_version.as_str() {
        "7" | "8" | "10" => {
            ctx.min_windows_version = args.min_windows_version.clone();
        }
        other => {
            anyhow::bail!(
                "--min-windows-version: expected '7', '8', or '10', got '{}'. \
                 See docs/src/platforms/windows-7.md for the trade-offs.",
                other
            );
        }
    }
    Ok(())
}

/// Resolve the Windows PE subsystem override into `ctx.windows_subsystem`.
///
/// Precedence:
///   1. `--windows-subsystem` when set to `console`/`windows` (explicit
///      CLI intent wins).
///   2. `perry.toml [windows] subsystem` — the source that survives the
///      `perry publish` worker round-trip, since the dev shell's flags don't
///      transfer but perry.toml is uploaded with `--project` (mirrors
///      `resolve_optional_framework_dir`'s env→toml fallback).
///   3. `auto` — defer to the `needs_ui` import heuristic at link time.
///
/// Unknown values from either source are a hard error so typos fail loudly
/// instead of silently linking a console window onto a GUI game.
pub(super) fn validate_windows_subsystem(
    args: &CompileArgs,
    ctx: &mut CompilationContext,
) -> Result<()> {
    fn check(value: &str, source: &str) -> Result<()> {
        match value {
            "auto" | "console" | "windows" => Ok(()),
            other => {
                anyhow::bail!("{source}: expected 'auto', 'console', or 'windows', got '{other}'.")
            }
        }
    }

    check(&args.windows_subsystem, "--windows-subsystem")?;

    // 1. Explicit CLI override wins.
    if args.windows_subsystem != "auto" {
        ctx.windows_subsystem = args.windows_subsystem.clone();
        return Ok(());
    }

    // 2. CLI left at default — consult perry.toml [windows] subsystem.
    let project_root = find_project_root_for_resources(&args.input, true);
    if let Ok(content) = std::fs::read_to_string(project_root.join("perry.toml")) {
        if let Ok(doc) = content.parse::<toml::Table>() {
            if let Some(sub) = super::app_metadata::toml_string(&doc, "windows", "subsystem") {
                check(&sub, "perry.toml [windows] subsystem")?;
                ctx.windows_subsystem = sub;
                return Ok(());
            }
        }
    }

    // 3. Stays "auto" (ctx default) — import heuristic decides.
    Ok(())
}

/// #6125: resolve the CPU-baseline knob into the canonical `PERRY_TARGET_CPU`
/// env var, which codegen (`clang -march`/`-mcpu`), the object/build cache
/// keys, and the auto-optimize runtime/stdlib rebuild (`-C target-cpu`) all
/// honor uniformly.
///
/// Precedence: `--march` → `PERRY_TARGET_CPU` already in the environment →
/// perry.toml `[build] march` → `[build] native_tuning` (`false` is shorthand
/// for `generic`, `true` for `native`). The toml sources are what survive the
/// `perry publish` worker round-trip, like `[windows] subsystem` above.
/// Mirrors the `--debug-symbols` → `PERRY_DEBUG_SYMBOLS` promotion: must run
/// single-threaded, before module codegen spawns rayon workers, so every
/// layer observes one value.
pub(super) fn promote_cpu_baseline_env(args: &CompileArgs) {
    if let Some(march) = args
        .march
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        std::env::set_var("PERRY_TARGET_CPU", march);
        return;
    }
    if std::env::var_os("PERRY_TARGET_CPU").is_some() {
        return;
    }
    let project_root = find_project_root_for_resources(&args.input, true);
    let Ok(content) = std::fs::read_to_string(project_root.join("perry.toml")) else {
        return;
    };
    let Ok(doc) = content.parse::<toml::Table>() else {
        return;
    };
    let march = super::app_metadata::toml_string(&doc, "build", "march")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let tuning = doc
                .get("build")
                .and_then(|b| b.get("native_tuning"))
                .and_then(|v| v.as_bool())?;
            Some(if tuning { "native" } else { "generic" }.to_string())
        });
    if let Some(march) = march {
        std::env::set_var("PERRY_TARGET_CPU", march);
    }
}

/// Run the full post-collect preflight chain (capability / egress /
/// lockdown / SBOM / geisterhand / windows-version / project_root
/// recompute / module-count print). All gates collected here so the
/// orchestrator hits one call instead of seven.
pub(super) fn run_post_collect_preflight(
    args: &CompileArgs,
    ctx: &mut CompilationContext,
    format: OutputFormat,
) -> Result<()> {
    enforce_js_runtime_gate(ctx)?;
    enforce_package_default_exports(ctx)?;
    recompute_common_project_root(ctx);

    let total_modules = ctx.native_modules.len() + ctx.js_modules.len();
    match format {
        OutputFormat::Text => {
            println!(
                "Found {} module(s): {} native, {} JavaScript",
                total_modules,
                ctx.native_modules.len(),
                ctx.js_modules.len()
            );
        }
        OutputFormat::Json => {}
    }

    enforce_capability_policy(ctx)?;
    enforce_egress_policy(ctx)?;
    enforce_lockdown_policy(ctx)?;

    // #495: emit a behavioral SBOM at `<cache_dir>/audit.json` (default
    // `node_modules/.cache/perry/audit.json`). The
    // manifest captures, per source module, the stdlib symbols
    // actually called from the lowered HIR. Foundation for the
    // host-controlled per-package capability enforcement issue (#501)
    // and for `perry audit --diff` change review. Best-effort write
    // — a missing directory or filesystem error is logged but
    // doesn't fail the build, since the SBOM is observational
    // metadata, not a correctness gate.
    write_audit_manifest_logging_failures(ctx, format);

    apply_geisterhand_args(args, ctx);
    validate_min_windows_version(args, ctx)?;
    validate_windows_subsystem(args, ctx)?;
    Ok(())
}

// ============================================================================
// Group C: native-instance fixups + HarmonyOS harvest + i18n + print_hir
// ============================================================================

/// Fix local native instances (parallel, per-module). Earlier revisions
/// also ran a `transform_js_imports` pass here, gated on
/// `needs_js_runtime`; that step went away when runtime JS (V8) support
/// was removed, leaving only the native-instance fixup.
pub(super) fn run_native_instance_fixups(ctx: &mut CompilationContext) {
    use rayon::prelude::*;
    ctx.native_modules
        .par_iter_mut()
        .for_each(|(_, hir_module)| {
            perry_hir::fix_local_native_instances(hir_module);
        });

    // Build map of exported native instances from all modules. Must
    // run AFTER fix_local_native_instances above so the exports list
    // reflects post-rewrite state.
    let mut exported_instances: BTreeMap<(String, String), perry_hir::ExportedNativeInstance> =
        BTreeMap::new();
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
    let mut exported_func_return_instances: BTreeMap<
        (String, String),
        perry_hir::ExportedNativeInstance,
    > = BTreeMap::new();
    for (path, hir_module) in &ctx.native_modules {
        let path_str = path.to_string_lossy().to_string();
        for (func_name, native_module, native_class) in
            &hir_module.exported_func_return_native_instances
        {
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
    ctx.native_modules
        .par_iter_mut()
        .for_each(|(_, hir_module)| {
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
}

/// --- HarmonyOS Phase 2: harvest perry/ui App({body: ...}) into ArkUI ---
///
/// Runs BEFORE codegen so the LLVM backend never sees the App call (it
/// would otherwise try to emit `perry_ui_app_create` / `_set_body` / `_run`
/// FFIs that are unresolved on OHOS — there's no `perry-ui-harmonyos` crate
/// by design, since OHOS owns its own UI tree via ArkTS).
///
/// `emit_index_ets` walks the entry module's `init`, finds the App call's
/// `body:` expression, emits a declarative `pages/Index.ets`, and replaces
/// the `Stmt::Expr(NativeMethodCall { method: "App" })` with a no-op
/// `Stmt::Expr(Number(0.0))`. After the strip, codegen sees a logic-only
/// module — Perry's `main()` runs in `EntryAbility.onCreate` and ArkUI
/// renders the harvested page on `onWindowStageCreate`.
///
/// Also flips `ctx.needs_ui` back to false so the link path skips the
/// perry-ui-* lib check (which would fail on the OHOS target since no
/// such lib exists).
#[cfg(feature = "backend-arkts")]
pub(super) fn harvest_harmonyos_index_ets(
    args: &CompileArgs,
    ctx: &mut CompilationContext,
    format: OutputFormat,
) {
    if !matches!(
        args.target.as_deref(),
        Some("harmonyos") | Some("harmonyos-simulator")
    ) {
        return;
    }
    // Compute entry path locally — the canonical `entry_path` binding is
    // declared further down in run_with_parse_cache (at the codegen-loop
    // entry-detection site) and isn't in scope here yet. This local copy
    // is identical: ctx.native_modules is keyed by canonicalized paths.
    let entry_path_local = args
        .input
        .canonicalize()
        .unwrap_or_else(|_| args.input.clone());
    if let Some(entry_hir) = ctx.native_modules.get_mut(&entry_path_local) {
        match perry_codegen_arkts::emit_index_ets(entry_hir) {
            Ok(Some(harvest)) => {
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "  harmonyos: harvested perry/ui App({{body: ...}}) → \
                         {} bytes ArkUI Index.ets, {} callback(s) (perry-codegen-arkts)",
                        harvest.ets_source.len(),
                        harvest.callbacks.len(),
                    );
                }

                // Phase 2 v2 callback bridge: inject one
                // `perry_arkts_register_callback(idx, closure)` call
                // per harvested closure into module.init, so when
                // main() runs the closures get registered into the
                // runtime slot table that NAPI's invokeCallback
                // dispatches against on ArkUI tap events.
                //
                // Stmts go BEFORE the no-op the strip pass left
                // behind, so the closures are registered before any
                // user-visible side effect — important if the user
                // wrote logic after `App(...)` that depends on the
                // closures already being registered.
                if !harvest.callbacks.is_empty() {
                    let registrations: Vec<perry_hir::ir::Stmt> = harvest
                        .callbacks
                        .into_iter()
                        .enumerate()
                        .map(|(idx, closure)| {
                            perry_hir::ir::Stmt::Expr(perry_hir::ir::Expr::NativeMethodCall {
                                module: "perry/arkts".to_string(),
                                class_name: None,
                                object: None,
                                method: "registerCallback".to_string(),
                                args: vec![perry_hir::ir::Expr::Number(idx as f64), closure],
                            })
                        })
                        .collect();
                    // Splice registrations to the front of init.
                    let mut new_init = registrations;
                    new_init.append(&mut entry_hir.init);
                    entry_hir.init = new_init;
                }

                ctx.harmonyos_index_ets = Some(harvest.ets_source);
            }
            Ok(None) => {
                // Logic-only program (no `App({...})` literal — perfectly
                // valid; e.g. `import { state } from "perry/ui"` for shared
                // state between modules without a top-level UI mount).
                // Falls through to needs_ui=false below.
            }
            Err(e) => {
                eprintln!(
                    "Warning: perry-codegen-arkts harvest failed ({}); \
                     falling back to blank window.",
                    e
                );
            }
        }
    }
    // HarmonyOS has no `perry-ui-harmonyos` crate by design — the
    // ArkUI side handles UI via the harvested Index.ets, and any
    // `perry_ui_*` / `perry_system_*` / `perry_updater_*` symbols
    // that survive into the .so resolve via the no-op stubs in
    // `perry-runtime/src/ui_harmonyos_stubs.rs` (build.rs auto-
    // generates them from the dispatch tables — see #395 + #399).
    // So flipping `needs_ui = false` is always safe regardless of
    // harvest outcome — and required, because the build path at
    // `optimized_libs.rs` would otherwise try to compile a
    // nonexistent `perry-ui-harmonyos` crate. Closes #400.
    ctx.needs_ui = false;
}

/// --- i18n: apply i18n transform pass ---
pub(super) fn apply_i18n_pass(
    ctx: &mut CompilationContext,
    i18n_config: Option<&perry_transform::i18n::I18nConfig>,
    i18n_translations: &BTreeMap<String, BTreeMap<String, String>>,
    format: OutputFormat,
) -> Option<perry_transform::i18n::I18nStringTable> {
    if let Some(config) = i18n_config {
        let table =
            perry_transform::i18n::apply_i18n(&mut ctx.native_modules, config, i18n_translations);
        // Report diagnostics
        for diag in &table.diagnostics {
            match diag.severity {
                perry_transform::i18n::I18nSeverity::Warning => match format {
                    OutputFormat::Text => eprintln!("  i18n warning: {}", diag.message),
                    OutputFormat::Json => {}
                },
                perry_transform::i18n::I18nSeverity::Error => match format {
                    OutputFormat::Text => eprintln!("  i18n error: {}", diag.message),
                    OutputFormat::Json => {}
                },
            }
        }
        match format {
            OutputFormat::Text => {
                if !table.keys.is_empty() {
                    println!(
                        "  i18n: {} localizable string(s) detected",
                        table.keys.len()
                    );
                }
            }
            OutputFormat::Json => {}
        }
        // The LLVM backend threads i18n through `CompileOptions::i18n_table`
        // (set per-job at the dispatch site below). No thread-local needed.
        Some(table)
    } else {
        None
    }
}

/// Debug dump triggered by `--print-hir`.
/// Print one HIR function's signature + lowered body. Shared between the
/// top-level-function and class-method dump paths so focus mode renders
/// methods identically to free functions.
fn dump_hir_function(func: &perry_hir::Function, indent: &str) {
    println!(
        "{}- {} (params: {}, type_params: {}, async: {}, exported: {})",
        indent,
        func.name,
        func.params.len(),
        func.type_params.len(),
        func.is_async,
        func.is_exported
    );
    for p in &func.params {
        println!("{}    param {} (id={}): {:?}", indent, p.name, p.id, p.ty);
    }
    for (i, stmt) in func.body.iter().enumerate() {
        println!("{}    [{}] {:?}", indent, i, stmt);
    }
}

/// Debug dump triggered by `--print-hir` (focus `None`) or
/// `--trace hir` / `--trace hir --focus NAME`.
///
/// When `focus` is `Some(needle)`, only functions, class methods, and
/// classes whose name contains `needle` are printed — and the
/// imports/exports/init-statement sections are suppressed — so a single
/// function's lowered body is readable instead of buried in a full-module
/// dump. When `focus` is `None`, behaves exactly like the historical
/// full dump.
pub(super) fn dump_hir_for_debug(ctx: &CompilationContext, focus: Option<&str>) {
    let matches = |name: &str| focus.map(|f| name.contains(f)).unwrap_or(true);

    for (path, hir_module) in &ctx.native_modules {
        // In focus mode, skip whole modules that contain no match so the
        // output isn't a wall of empty module headers.
        if focus.is_some() {
            let any = hir_module.functions.iter().any(|f| matches(&f.name))
                || hir_module
                    .classes
                    .iter()
                    .any(|c| matches(&c.name) || c.methods.iter().any(|m| matches(&m.name)));
            if !any {
                continue;
            }
        }

        match focus {
            Some(f) => println!("\n=== HIR trace (focus: {:?}): {} ===", f, path.display()),
            None => println!("\n=== HIR (after monomorphization): {} ===", path.display()),
        }
        println!("Module: {}", hir_module.name);

        if focus.is_none() {
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
        }

        let funcs: Vec<_> = hir_module
            .functions
            .iter()
            .filter(|f| matches(&f.name))
            .collect();
        println!("Functions: {}", funcs.len());
        for func in funcs {
            dump_hir_function(func, "  ");
        }

        let classes: Vec<_> = hir_module
            .classes
            .iter()
            .filter(|c| matches(&c.name) || c.methods.iter().any(|m| matches(&m.name)))
            .collect();
        println!("Classes: {}", classes.len());
        for cls in classes {
            println!(
                "  - {} (exported: {}, fields: {}, methods: {}, constructor: {})",
                cls.name,
                cls.is_exported,
                cls.fields.len(),
                cls.methods.len(),
                cls.constructor.is_some()
            );
            // In focus mode, print the bodies of matching methods (and all
            // methods when the class name itself matched). In full mode the
            // historical output stops at the counts above, so keep it.
            if focus.is_some() {
                let class_matched = matches(&cls.name);
                for m in cls
                    .methods
                    .iter()
                    .filter(|m| class_matched || matches(&m.name))
                {
                    dump_hir_function(m, "    ");
                }
            }
        }

        if focus.is_none() {
            println!("Init statements: {}", hir_module.init.len());
            for (i, stmt) in hir_module.init.iter().enumerate() {
                println!("  [{}] {:?}", i, stmt);
            }
        }
        println!("===========\n");
    }

    if focus.is_none() && !ctx.js_modules.is_empty() {
        println!("\n=== JavaScript Modules (interpreted) ===");
        for (specifier, module) in &ctx.js_modules {
            println!("  {} -> {}", specifier, module.path.display());
        }
        println!("===========\n");
    }
}
