//! Check command - validates TypeScript compatibility without compiling

use anyhow::{anyhow, Result};
use clap::Args;
use perry_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEmitter, Diagnostics, JsonEmitter, SourceCache, Span,
    TerminalEmitter,
};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

use super::deps::{
    check_node_builtin_imports, compatibility_to_diagnostics, scan_project_file_for_issues,
    unresolved_imports_to_diagnostics, CompatibilityIssue, DependencyResolver, IssueKind,
};
use super::fix_applier::FixApplier;
use super::fixer::{Confidence, Fixer};
use super::progress::{ProgressSnapshot, VerboseProgress};
use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Input TypeScript file or directory
    #[arg(default_value = ".")]
    pub input: PathBuf,

    /// Check dependencies in node_modules for compatibility
    #[arg(long)]
    pub check_deps: bool,

    /// Scan all dependencies (not just direct imports)
    #[arg(long)]
    pub deep_deps: bool,

    /// Show all issues including hints
    #[arg(long)]
    pub all: bool,

    /// Treat warnings as errors
    #[arg(long)]
    pub strict: bool,

    /// Automatically fix issues where possible
    #[arg(long)]
    pub fix: bool,

    /// Show what fixes would be applied without modifying files
    #[arg(long)]
    pub fix_dry_run: bool,

    /// Include medium-confidence fixes (inferred types)
    #[arg(long)]
    pub fix_unsafe: bool,

    /// Compile target the check is being run for (`harmonyos`,
    /// `ios`, `android`, …). When set, imports of `perry/ui` /
    /// `perry/system` / `perry/updater` symbols that the runtime
    /// stubs out for that target produce a `NoOpStub` warning so
    /// the user finds out before running the binary (#464).
    /// Defaults to native host — the stub scan is skipped because
    /// host UI crates own those symbols with real impls.
    #[arg(long)]
    pub target: Option<String>,
}

/// Collect all TypeScript files in a directory
fn collect_ts_files(path: &PathBuf) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if path.is_file() {
        if path.extension().is_some_and(|ext| ext == "ts") {
            files.push(path.clone());
        }
        return Ok(files);
    }

    for entry in WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip node_modules
        if path.components().any(|c| c.as_os_str() == "node_modules") {
            continue;
        }

        if path.is_file() && path.extension().is_some_and(|ext| ext == "ts") {
            // Skip declaration files
            if !path.to_string_lossy().ends_with(".d.ts") {
                files.push(path.to_path_buf());
            }
        }
    }

    Ok(files)
}

pub fn run(args: CheckArgs, format: OutputFormat, use_color: bool, verbose: u8) -> Result<()> {
    let project_root = if args.input.is_file() {
        args.input
            .parent()
            .unwrap_or(&args.input)
            .to_path_buf()
            .canonicalize()
            .unwrap_or_else(|_| args.input.clone())
    } else {
        args.input.canonicalize().unwrap_or(args.input.clone())
    };

    let files = collect_ts_files(&project_root)?;

    if files.is_empty() {
        match format {
            OutputFormat::Text => println!("No TypeScript files found."),
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({
                        "success": true,
                        "files": 0,
                        "errors": 0,
                        "warnings": 0,
                    })
                );
            }
        }
        return Ok(());
    }

    if verbose > 0 || matches!(format, OutputFormat::Text) {
        match format {
            OutputFormat::Text => {
                println!("Checking {} file(s)...", files.len());
                if args.check_deps {
                    println!("Dependency checking enabled.");
                }
            }
            OutputFormat::Json => {}
        }
    }

    let mut source_cache = SourceCache::new();
    let mut all_diagnostics = Diagnostics::new();
    let mut checked_files = 0;
    let mut visited = HashSet::new();
    let mut dep_resolver = DependencyResolver::new(project_root.clone());
    let mut fix_applier = FixApplier::new();
    let progress = VerboseProgress::new(format, verbose);
    let min_confidence = if args.fix_unsafe {
        Confidence::Medium
    } else {
        Confidence::High
    };

    for file in &files {
        let canonical = match file.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };

        if visited.contains(&canonical) {
            continue;
        }
        visited.insert(canonical.clone());

        let source = match fs::read_to_string(&canonical) {
            Ok(s) => s,
            Err(e) => {
                if verbose > 0 {
                    eprintln!("Warning: Could not read {}: {}", canonical.display(), e);
                }
                continue;
            }
        };

        let filename = canonical.to_string_lossy().to_string();

        // Parse with diagnostics
        progress.record(ProgressSnapshot {
            stage: "check-parse",
            module_path: Some(&canonical),
            visited: Some(checked_files + 1),
            total: Some(files.len()),
            ..Default::default()
        });
        let parse_result = match perry_parser::parse_typescript_with_cache(
            &source,
            &filename,
            &mut source_cache,
        ) {
            Ok(result) => result,
            Err(e) => {
                if verbose > 0 {
                    eprintln!("Parse error in {}: {}", canonical.display(), e);
                }
                continue;
            }
        };

        all_diagnostics.extend(parse_result.diagnostics);

        // Run fixer analysis if --fix or --fix-dry-run is enabled
        if args.fix || args.fix_dry_run {
            let fixable_issues =
                Fixer::analyze(&parse_result.module, parse_result.file_id, &source);
            for issue in &fixable_issues {
                fix_applier.add_issue(issue, &canonical, &source, min_confidence);

                // Add a diagnostic for each fixable issue so it shows in output
                let diag = match &issue.kind {
                    super::fixer::FixableKind::AnyType { .. } => Diagnostic::warning(
                        DiagnosticCode::AnyTypeUsage,
                        format!("{} (--fix to apply)", issue.message),
                    )
                    .with_span(issue.span)
                    .build(),
                    super::fixer::FixableKind::TemplateLiteral => Diagnostic::warning(
                        DiagnosticCode::UnsupportedFeature,
                        format!("{} (--fix to apply)", issue.message),
                    )
                    .with_span(issue.span)
                    .build(),
                };
                all_diagnostics.push(diag);
            }
        }

        // Extract imports from AST even before lowering (for dependency checking)
        if args.check_deps {
            extract_imports_from_ast(
                &parse_result.module,
                &canonical,
                &mut dep_resolver,
                &progress,
                checked_files + 1,
                files.len(),
            );
        }

        // Stub scan (#464): warn on imports of `perry/ui` /
        // `perry/system` / `perry/updater` symbols that the runtime
        // stubs out for the requested target. Skipped when no target
        // is specified — the host UI crate owns those symbols.
        if target_stubs_out_symbols(args.target.as_deref()) {
            scan_imports_for_stubs(
                &parse_result.module,
                parse_result.file_id,
                &mut all_diagnostics,
            );
        }

        // Scan source for dynamic patterns (eval, new Function, etc.)
        if args.check_deps {
            let issues = scan_project_file_for_issues(&canonical, &source);
            for issue in issues {
                let diag = issue_to_diagnostic(&issue);
                all_diagnostics.push(diag);
            }
        }

        // Try to lower to HIR to catch more errors
        let file_id = parse_result.file_id;
        // #503: stash the source text so the dynamic-dispatch check can
        // resolve `// @perry-allow-dynamic` annotations during `perry check`
        // the same way it does during a real build.
        progress.record(ProgressSnapshot {
            stage: "check-lower",
            module_path: Some(&canonical),
            visited: Some(checked_files + 1),
            total: Some(files.len()),
            ..Default::default()
        });
        perry_hir::set_current_module_source(source.clone());
        let lower_outcome = perry_hir::lower_module(&parse_result.module, &filename, &filename);
        perry_hir::clear_current_module_source();
        match lower_outcome {
            Ok(_hir_module) => {
                // Successfully lowered
            }
            Err(e) => {
                // If the lowering error carried a span, attach it so the
                // diagnostic emitter can print file:line:column and the
                // offending source snippet. Otherwise fall back to a
                // location-less message (but still tag it with the file_id
                // so the emitter can at least show the filename). #5249:
                // shared with the `compile` path so both front-ends render
                // lowering errors identically.
                all_diagnostics.push(super::lower_diagnostic::lower_error_to_diagnostic(
                    &e, file_id, &filename,
                ));
            }
        }

        checked_files += 1;
    }

    // Check dependencies if requested. #854: a `dep_issues_count` local
    // used to track errors-from-deps separately but it was never read —
    // the summary path below reads `all_diagnostics.error_count()` which
    // already folds in everything we extend into the diagnostic stream.
    if args.check_deps {
        // Check for unresolved imports
        let unresolved = dep_resolver.get_unresolved_imports();
        if !unresolved.is_empty() {
            let unresolved_diags = unresolved_imports_to_diagnostics(unresolved, &source_cache);
            all_diagnostics.extend(unresolved_diags);
        }

        // Check for Node.js built-in imports (fs, path, http, etc.)
        let builtin_diags = check_node_builtin_imports(
            dep_resolver.get_all_imports(),
            dep_resolver.get_import_locations(),
        );
        if builtin_diags.has_errors() {
            all_diagnostics.extend(builtin_diags);
        }

        // Check package compatibility
        match format {
            OutputFormat::Text => {
                if verbose > 0 {
                    println!("Checking dependency compatibility...");
                }
            }
            OutputFormat::Json => {}
        }

        match dep_resolver.check_all_dependencies(&mut source_cache) {
            Ok(packages) => {
                let pkg_count = packages.len();
                let incompatible: Vec<_> = packages.iter().filter(|p| !p.is_compatible).collect();

                if matches!(format, OutputFormat::Text) && verbose > 0 {
                    println!(
                        "Scanned {} package(s), {} fully compatible",
                        pkg_count,
                        pkg_count - incompatible.len()
                    );
                }

                let compat_diags = compatibility_to_diagnostics(&packages);
                all_diagnostics.extend(compat_diags);
            }
            Err(e) => {
                if verbose > 0 {
                    eprintln!("Warning: Could not check dependencies: {}", e);
                }
            }
        }
    }

    // Emit diagnostics
    let stderr = std::io::stderr();

    match format {
        OutputFormat::Text => {
            let mut emitter = TerminalEmitter::new(stderr.lock(), use_color);
            emitter.emit_all(&all_diagnostics, &source_cache)?;

            println!();

            // Print summary
            let errors = all_diagnostics.error_count();
            let warnings = all_diagnostics.warning_count();

            if errors > 0 {
                if use_color {
                    println!(
                        "{}: {} error(s), {} warning(s)",
                        console::style("Check failed").red().bold(),
                        errors,
                        warnings
                    );
                } else {
                    println!("Check failed: {} error(s), {} warning(s)", errors, warnings);
                }
            } else if warnings > 0 && args.strict {
                if use_color {
                    println!(
                        "{}: {} warning(s) (strict mode)",
                        console::style("Check failed").yellow().bold(),
                        warnings
                    );
                } else {
                    println!("Check failed: {} warning(s) (strict mode)", warnings);
                }
            } else if warnings > 0 {
                if use_color {
                    println!(
                        "{}: {} warning(s)",
                        console::style("Check passed").yellow(),
                        warnings
                    );
                } else {
                    println!("Check passed: {} warning(s)", warnings);
                }
            } else {
                if use_color {
                    println!(
                        "{} - {} file(s) checked",
                        console::style("All checks passed!").green().bold(),
                        checked_files
                    );
                } else {
                    println!("All checks passed! - {} file(s) checked", checked_files);
                }
            }

            // Print compilation guarantee
            if args.check_deps && errors == 0 && (warnings == 0 || !args.strict) {
                println!();
                if use_color {
                    println!(
                        "{}",
                        console::style("✓ Parsing, HIR lowering, and dependency checks passed")
                            .green()
                    );
                    println!(
                        "{}",
                        console::style("  (codegen not verified — run `perry compile` for end-to-end validation)").dim()
                    );
                } else {
                    println!("[OK] Parsing, HIR lowering, and dependency checks passed");
                    println!("     (codegen not verified — run `perry compile` for end-to-end validation)");
                }
            } else if args.check_deps {
                println!();
                if use_color {
                    println!(
                        "{}",
                        console::style("✗ Compilation may fail due to issues above").red()
                    );
                } else {
                    println!("[FAIL] Compilation may fail due to issues above");
                }
            } else if errors == 0 {
                println!();
                println!("Note: Run with --check-deps to verify dependencies.");
            }

            // Handle fix output
            if args.fix_dry_run && fix_applier.pending_fixes() > 0 {
                println!();
                if use_color {
                    println!(
                        "{}",
                        console::style(format!(
                            "Would fix {} issue(s) in {} file(s)",
                            fix_applier.pending_fixes(),
                            fix_applier.pending_files()
                        ))
                        .cyan()
                    );
                } else {
                    println!(
                        "Would fix {} issue(s) in {} file(s)",
                        fix_applier.pending_fixes(),
                        fix_applier.pending_files()
                    );
                }
                println!("Run with --fix to apply changes.");
                println!();
                println!("{}", fix_applier.dry_run());
            } else if args.fix && fix_applier.pending_fixes() > 0 {
                let result = fix_applier.apply();
                println!();
                if use_color {
                    println!(
                        "{}",
                        console::style(format!(
                            "Fixed {} issue(s) in {} file(s)",
                            result.fixes_applied, result.files_modified
                        ))
                        .green()
                        .bold()
                    );
                } else {
                    println!(
                        "Fixed {} issue(s) in {} file(s)",
                        result.fixes_applied, result.files_modified
                    );
                }
                for err in &result.errors {
                    eprintln!("Error: {}", err);
                }
            }
        }
        OutputFormat::Json => {
            let mut emitter = JsonEmitter::new(std::io::stdout().lock());
            emitter.emit_all(&all_diagnostics, &source_cache)?;

            let errors = all_diagnostics.error_count();
            let warnings = all_diagnostics.warning_count();
            // Named "compilation_guaranteed" historically, but the check
            // does not run codegen — only parse, HIR lowering, and
            // dependency checks. Kept under the same key for JSON
            // backcompat, though a more accurate name would be
            // `frontend_checks_passed`.
            let compilation_guaranteed =
                args.check_deps && errors == 0 && (warnings == 0 || !args.strict);

            // Apply fixes if requested
            let (fixes_applied, files_modified) = if args.fix && fix_applier.pending_fixes() > 0 {
                let result = fix_applier.apply();
                (result.fixes_applied, result.files_modified)
            } else {
                (0, 0)
            };

            let summary = serde_json::json!({
                "type": "summary",
                "success": errors == 0 && (!args.strict || warnings == 0),
                "files_checked": checked_files,
                "errors": errors,
                "warnings": warnings,
                "hints": all_diagnostics.hint_count(),
                "deps_checked": args.check_deps,
                "compilation_guaranteed": compilation_guaranteed,
                "fixable_issues": fix_applier.pending_fixes(),
                "fixes_applied": fixes_applied,
                "files_modified": files_modified,
            });
            println!("{}", serde_json::to_string(&summary)?);
        }
    }

    let has_blocking_issues =
        all_diagnostics.has_errors() || (args.strict && all_diagnostics.warning_count() > 0);

    if has_blocking_issues {
        Err(anyhow!("Check failed with errors"))
    } else {
        Ok(())
    }
}

/// Extract imports from AST without full HIR lowering
fn extract_imports_from_ast(
    module: &perry_parser::swc_ecma_ast::Module,
    file_path: &PathBuf,
    dep_resolver: &mut DependencyResolver,
    progress: &VerboseProgress,
    visited: usize,
    total: usize,
) {
    use perry_parser::swc_ecma_ast::{ModuleDecl, ModuleItem};

    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(decl) => match decl {
                ModuleDecl::Import(import) => {
                    // Use as_str() to get &str from the Wtf8Atom
                    let source = import.src.value.as_str().unwrap_or("");
                    progress.record(ProgressSnapshot {
                        stage: "check-resolve-import",
                        module_path: Some(file_path),
                        import_specifier: Some(source),
                        visited: Some(visited),
                        total: Some(total),
                        ..Default::default()
                    });
                    dep_resolver.record_import(source, file_path);
                }
                ModuleDecl::ExportNamed(export) => {
                    if let Some(src) = &export.src {
                        let source = src.value.as_str().unwrap_or("");
                        progress.record(ProgressSnapshot {
                            stage: "check-resolve-import",
                            module_path: Some(file_path),
                            import_specifier: Some(source),
                            visited: Some(visited),
                            total: Some(total),
                            ..Default::default()
                        });
                        dep_resolver.record_import(source, file_path);
                    }
                }
                ModuleDecl::ExportAll(export) => {
                    let source = export.src.value.as_str().unwrap_or("");
                    progress.record(ProgressSnapshot {
                        stage: "check-resolve-import",
                        module_path: Some(file_path),
                        import_specifier: Some(source),
                        visited: Some(visited),
                        total: Some(total),
                        ..Default::default()
                    });
                    dep_resolver.record_import(source, file_path);
                }
                _ => {}
            },
            ModuleItem::Stmt(stmt) => {
                // Also check for require() calls in statements
                extract_requires_from_stmt(stmt, file_path, dep_resolver);
            }
        }
    }
}

/// Extract require() calls from statements (for CommonJS compatibility)
fn extract_requires_from_stmt(
    _stmt: &perry_parser::swc_ecma_ast::Stmt,
    _file_path: &PathBuf,
    _dep_resolver: &mut DependencyResolver,
) {
    // For now, we focus on ES module imports
    // require() support can be added later if needed
}

/// Returns true when the requested compile target stubs out
/// `perry/ui` / `perry/system` / `perry/updater` symbols at runtime —
/// currently only harmonyos. This is what gates the static stub scan
/// (#464); other targets resolve those modules to real platform UI
/// crates so a warning would be a false positive.
fn target_stubs_out_symbols(target: Option<&str>) -> bool {
    matches!(target, Some("harmonyos") | Some("harmonyos-simulator"))
}

/// Walk a parsed module's named imports against `STUB_MANIFEST` and
/// emit a `NoOpStub` warning for each one that maps to a stubbed
/// symbol on the requested target.
fn scan_imports_for_stubs(
    module: &perry_parser::swc_ecma_ast::Module,
    file_id: perry_diagnostics::FileId,
    diagnostics: &mut Diagnostics,
) {
    use perry_parser::swc_ecma_ast::{ImportSpecifier, ModuleDecl, ModuleExportName, ModuleItem};
    use perry_runtime::stub_diag::STUB_MANIFEST;

    for item in &module.body {
        let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
            continue;
        };
        let source = match import.src.value.as_str() {
            Some(s) => s,
            None => continue,
        };
        // Only `perry/ui` / `perry/system` / `perry/updater` /
        // `perry/background` imports can resolve to stubbed symbols.
        // Cheap pre-filter avoids walking the manifest for every import.
        if !matches!(
            source,
            "perry/ui" | "perry/system" | "perry/updater" | "perry/background"
        ) {
            continue;
        }

        for spec in &import.specifiers {
            let (local_name, span) = match spec {
                ImportSpecifier::Named(named) => {
                    let imported_name = match &named.imported {
                        Some(ModuleExportName::Ident(id)) => id.sym.as_str(),
                        Some(ModuleExportName::Str(s)) => s.value.as_str().unwrap_or(""),
                        None => named.local.sym.as_str(),
                    };
                    (imported_name.to_string(), named.span)
                }
                // `import * as X` and `import X` (default) bring the
                // whole module — we don't know which symbols the user
                // actually calls, so skip rather than warn for every
                // entry in the module's stub list.
                ImportSpecifier::Namespace(_) | ImportSpecifier::Default(_) => continue,
            };

            for entry in STUB_MANIFEST {
                if entry.module != source {
                    continue;
                }
                if entry.ts_name != Some(local_name.as_str()) {
                    continue;
                }
                let issue_tag = entry
                    .issue
                    .map(|t| format!(" (tracking: {})", t))
                    .unwrap_or_default();
                let msg = format!(
                    "`{}` from `{}` is a no-op stub on this target — {}{}",
                    local_name, source, entry.reason, issue_tag,
                );
                let diag = Diagnostic::warning(DiagnosticCode::NoOpStub, msg)
                    .with_span(Span::new(file_id, span.lo.0, span.hi.0))
                    .build();
                diagnostics.push(diag);
                break;
            }
        }
    }
}

/// Convert a CompatibilityIssue to a Diagnostic
fn issue_to_diagnostic(issue: &CompatibilityIssue) -> Diagnostic {
    let code = match issue.kind {
        IssueKind::DynamicCode => DiagnosticCode::EvalUsage,
        IssueKind::DynamicImport => DiagnosticCode::DynamicImport,
        IssueKind::AnyType => DiagnosticCode::AnyTypeUsage,
        IssueKind::DynamicPropertyAccess => DiagnosticCode::DynamicPropertyAccess,
        IssueKind::UnsupportedSyntax => DiagnosticCode::UnsupportedFeature,
        IssueKind::MissingTypes => DiagnosticCode::MissingTypeAnnotation,
    };

    let severity_fn = if issue.kind.severity() == "error" {
        Diagnostic::error
    } else {
        Diagnostic::warning
    };

    let location = if let Some(line) = issue.line {
        format!(" ({}:{})", issue.file.display(), line)
    } else {
        String::new()
    };

    severity_fn(code, format!("{}{}", issue.message, location)).build()
}
