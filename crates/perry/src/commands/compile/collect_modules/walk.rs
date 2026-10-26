use std::collections::HashMap;

use super::source_prep::{worker_count, PrepSettings, SourcePrefetch, WalkSources};
use super::*;

/// Collect all modules to compile (transitive closure of imports) in a
/// single walk.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_modules(
    entry_path: &PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    progress: &VerboseProgress,
    parse_cache: Option<&mut ParseCache>,
) -> Result<()> {
    collect_modules_with_sources(
        entry_path,
        ctx,
        visited,
        format,
        target,
        next_class_id,
        skip_transforms,
        progress,
        parse_cache,
        WalkSources::Keep(&mut super::source_prep::PreparedSources::default()),
    )
}

/// Collect all modules to compile (transitive closure of imports).
///
/// Worker threads prepare module sources ahead of the walk (see
/// `source_prep`); the walk itself, and everything that depends on its order,
/// runs on this thread exactly as without them. `sources` says whether the
/// walk keeps the sources it prepares for a later walk, or uses the ones an
/// earlier walk kept.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_modules_with_sources(
    entry_path: &PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    progress: &VerboseProgress,
    parse_cache: Option<&mut ParseCache>,
    mut sources: WalkSources,
) -> Result<()> {
    sources.start(ctx, target);
    let workers = worker_count(ctx);
    let settings = (workers > 0)
        .then(|| PrepSettings::of(ctx, target, &sources))
        .flatten();
    let Some(settings) = settings else {
        return walk_modules(
            entry_path,
            ctx,
            visited,
            format,
            target,
            next_class_id,
            skip_transforms,
            progress,
            parse_cache,
            None,
            &mut sources,
        );
    };
    let prefetch = SourcePrefetch::default();
    let stack_bytes = crate::compiler_stack_bytes();
    std::thread::scope(|scope| {
        for index in 0..workers {
            // A worker that fails to start only means less look-ahead: the
            // walk prepares any module no worker has.
            let _ = std::thread::Builder::new()
                .name(format!("perry-source-prep-{index}"))
                .stack_size(stack_bytes)
                .spawn_scoped(scope, || prefetch.work(&settings));
        }
        let result = walk_modules(
            entry_path,
            ctx,
            visited,
            format,
            target,
            next_class_id,
            skip_transforms,
            progress,
            parse_cache,
            Some(&prefetch),
            &mut sources,
        );
        prefetch.finish();
        result
    })
}

#[allow(clippy::too_many_arguments)]
fn walk_modules(
    entry_path: &PathBuf,
    ctx: &mut CompilationContext,
    visited: &mut HashSet<PathBuf>,
    format: OutputFormat,
    target: Option<&str>,
    next_class_id: &mut perry_hir::ClassId,
    skip_transforms: bool,
    progress: &VerboseProgress,
    mut parse_cache: Option<&mut ParseCache>,
    prefetch: Option<&SourcePrefetch>,
    sources: &mut WalkSources,
) -> Result<()> {
    // Hand every path pushed onto the walk stack to the source-prep workers,
    // in push order (the last pushed is popped first). Skips paths already
    // collected; a path collected later is released when the walk pops it.
    let submit = |pushed: &[WorkFrame], visited: &HashSet<PathBuf>| {
        if let Some(prefetch) = prefetch {
            prefetch.submit(pushed.iter().filter_map(|frame| match frame {
                WorkFrame::Enter(path) | WorkFrame::EnterWorkerUrl(path)
                    if !visited.contains(path) =>
                {
                    Some(path)
                }
                _ => None,
            }));
        }
    };
    let entry_canonical = entry_path.canonicalize()?;
    ctx.reexport_pruner.root(&entry_canonical);
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
                submit(&stack, visited);
            }
        }
    }
    if stack.len() == 1 {
        submit(&stack, visited);
    }
    loop {
        while let Some(frame) = stack.pop() {
            let (next_path, worker_url) = match frame {
                WorkFrame::Enter(next_path) => (next_path, false),
                WorkFrame::EnterWorkerUrl(next_path) => (next_path, true),
                WorkFrame::Finish(prepared) => {
                    let canonical = prepared.canonical.clone();
                    collect_module_finish(
                        prepared,
                        ctx,
                        visited,
                        target,
                        skip_transforms,
                        progress,
                    )?;
                    states.insert(canonical, VisitState::Done);
                    continue;
                }
            };
            let canonical = next_path
                .canonicalize()
                .map_err(|e| anyhow!("Failed to canonicalize {}: {}", next_path.display(), e))?;
            // Worker/asset/standalone roots can enter outside ordinary
            // import edges. Their full namespace must remain available.
            ctx.reexport_pruner.implicit_root(&canonical);

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
                prefetch,
                sources,
            );
            if let Some(prefetch) = prefetch {
                prefetch.release(&canonical);
            }
            let discovered = match discovered {
                Ok(discovered) => discovered,
                Err(error) if worker_url => {
                    if matches!(format, OutputFormat::Text) {
                        eprintln!(
                            "  Warning: {} is named by new URL(..., import.meta.url) \
                                     in a program that uses worker_threads, but it did not \
                                     compile, so it is not a worker entry: {error:#}",
                            canonical.display()
                        );
                    }
                    states.insert(canonical, VisitState::Done);
                    continue;
                }
                Err(error) => return Err(error),
            };

            if let Some(prepared) = discovered.finish {
                stack.push(WorkFrame::Finish(prepared));
            } else {
                states.insert(canonical, VisitState::Done);
            }
            let pushed_from = stack.len();
            for child in discovered.children.into_iter().rev() {
                stack.push(WorkFrame::Enter(child));
            }
            submit(&stack[pushed_from..], visited);
        }
        let workers = worker_url::accept_pending(ctx, visited);
        if !workers.is_empty() {
            stack.extend(workers.into_iter().rev().map(WorkFrame::EnterWorkerUrl));
            submit(&stack, visited);
            continue;
        }
        let pending = reexport_prune::settle(ctx);
        if pending.is_empty() {
            // Every CommonJS module is known: the support module they import
            // can be written, and it imports no compiled module itself.
            if let Some(support) = write_cjs_support(ctx, visited, &entry_canonical)? {
                stack.push(WorkFrame::Enter(support));
                submit(&stack, visited);
                continue;
            }
            break;
        }
        stack.extend(pending.into_iter().rev().map(WorkFrame::Enter));
        submit(&stack, visited);
    }
    worker_url::link_accepted(ctx, format);
    Ok(())
}

/// Write the CommonJS support module (`cjs_wrap::support`) once the walk has
/// collected every module that imports it, and hand back its path to walk.
/// `None` when no module imports it or it is already collected.
fn write_cjs_support(
    ctx: &mut CompilationContext,
    visited: &HashSet<PathBuf>,
    entry_canonical: &std::path::Path,
) -> Result<Option<PathBuf>> {
    let Some(path) = ctx.cjs_support_path.clone() else {
        return Ok(None);
    };
    if ctx.cjs_support_keys.is_empty() {
        return Ok(None);
    }
    let entry_key = perry_hir::source_remap::baked_path_of(entry_canonical);
    let entry_is_cjs = ctx
        .cjs_support_keys
        .iter()
        .any(|k| k.as_str() == &*entry_key);
    let source = super::super::cjs_wrap::support::source(&ctx.cjs_support_keys, entry_is_cjs);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| anyhow!("create {}: {}", dir.display(), e))?;
    }
    // Rewrite only on a change, so an unchanged graph keeps the file (and
    // the object cache keyed by it) as it was.
    if std::fs::read_to_string(&path).ok().as_deref() != Some(source.as_str()) {
        std::fs::write(&path, &source).map_err(|e| anyhow!("write {}: {}", path.display(), e))?;
    }
    let canonical = path.canonicalize()?;
    if visited.contains(&canonical) {
        return Ok(None);
    }
    Ok(Some(path))
}
