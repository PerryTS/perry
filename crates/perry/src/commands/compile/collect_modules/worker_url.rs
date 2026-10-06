//! Worker entries named by `new URL("<literal>", import.meta.url)`.
//!
//! A program often builds a worker's URL in one module (upm's `workers.ts`
//! helpers, a default option) and constructs the Worker in another, through a
//! value the call-site resolver cannot follow. In a program that uses
//! worker_threads, every such literal that names a script is compiled as a
//! worker entry: its module gets a dynamic edge from the module holding the
//! literal (so it is compiled, but only runs when something starts it), and
//! the driver registers it in the run-time worker entry table.
//!
//! The literal may name a script that is not a worker at all, so a candidate
//! whose module fails to compile is a warning, not an error.

use super::*;

/// One `new URL("<literal>", import.meta.url)` that names a script.
pub(crate) struct WorkerUrlCandidate {
    /// Canonical path of the module holding the literal.
    importer: PathBuf,
    /// The specifier of the edge: the literal, or the absolute path it names.
    source: String,
    /// Canonical path of the script it names.
    target: PathBuf,
    /// The script's path as the program spells it (before symlinks resolve).
    lexical: PathBuf,
}

/// Record the worker_threads use and the URL literals of one module.
pub(super) fn record(
    hir_module: &perry_hir::Module,
    lexical_path: &Path,
    canonical: &Path,
    ctx: &mut CompilationContext,
) {
    if perry_hir::module_uses_worker_threads(hir_module) {
        ctx.uses_worker_threads = true;
    }
    for url in perry_hir::worker_url_literals(hir_module) {
        // With a `file:` base the URL names an absolute path; resolve that
        // spelling (it keeps the usual `.js` -> `.ts` rule).
        let source = match &url.base {
            Some(base) => match url::Url::parse(base)
                .and_then(|base| base.join(&url.literal))
                .ok()
                .and_then(|joined| joined.to_file_path().ok())
            {
                Some(path) => path.to_string_lossy().into_owned(),
                None => continue,
            },
            None => url.literal,
        };
        let Some(resolved) =
            cached_resolve_import_with_lexical_base(&source, lexical_path, canonical, ctx)
        else {
            // A URL to a file that does not exist is not a worker entry.
            continue;
        };
        if resolved.kind != ModuleKind::NativeCompiled {
            continue;
        }
        ctx.worker_url_candidates.push(WorkerUrlCandidate {
            importer: canonical.to_path_buf(),
            source,
            target: resolved.canonical_path,
            lexical: resolved.source_path,
        });
    }
}

/// Once the walk has drained, accept the pending candidates of a program that
/// uses worker_threads. Returns the targets still to be collected.
pub(super) fn accept_pending(
    ctx: &mut CompilationContext,
    visited: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    if !ctx.uses_worker_threads {
        return Vec::new();
    }
    let mut to_collect = Vec::new();
    for candidate in std::mem::take(&mut ctx.worker_url_candidates) {
        if !visited.contains(&candidate.target) && !to_collect.contains(&candidate.target) {
            to_collect.push(candidate.target.clone());
        }
        ctx.reexport_pruner.implicit_root(&candidate.target);
        ctx.worker_url_accepted.push(candidate);
    }
    to_collect
}

/// After collection: link every accepted candidate whose module compiled. The
/// module holding the literal gets a dynamic edge to it, and the program gets
/// a worker entry under both spellings of its path.
pub(super) fn link_accepted(ctx: &mut CompilationContext, format: OutputFormat) {
    let mut announced = HashSet::new();
    for candidate in std::mem::take(&mut ctx.worker_url_accepted) {
        if !ctx.native_modules.contains_key(&candidate.target) {
            continue;
        }
        let Some(importer) = ctx.native_modules.get_mut(&candidate.importer) else {
            continue;
        };
        add_dynamic_edge(importer, &candidate.source, &candidate.target);
        if announced.insert(candidate.target.clone()) && matches!(format, OutputFormat::Text) {
            eprintln!("  Worker entry: {}", candidate.target.display());
        }
        ctx.worker_url_entries
            .push((candidate.lexical, candidate.target));
    }
    if !ctx.worker_url_entries.is_empty() {
        // The worker runtime and its entry table live in the stdlib.
        ctx.needs_stdlib = true;
        ctx.native_module_imports
            .insert("worker_threads".to_string());
    }
}

/// The same rule `collect_module_one` uses for `import()` targets: reuse a
/// static edge that exists at run time, keep an existing dynamic edge, and
/// otherwise add a dynamic one.
fn add_dynamic_edge(module: &mut perry_hir::Module, source: &str, target: &Path) {
    if let Some(existing) = module
        .imports
        .iter_mut()
        .find(|i| i.source == source && !i.type_only && !i.runtime_erased)
    {
        if !existing.is_dynamic {
            existing.is_dynamic_target = true;
        }
        return;
    }
    module.imports.push(perry_hir::Import {
        source: source.to_string(),
        specifiers: Vec::new(),
        is_native: false,
        module_kind: ModuleKind::NativeCompiled,
        resolved_path: Some(target.to_string_lossy().into_owned()),
        type_only: false,
        runtime_erased: false,
        is_dynamic: true,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    });
}
