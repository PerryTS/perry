use std::collections::HashMap;

use super::*;

/// Collect all modules to compile (transitive closure of imports).
pub(crate) fn collect_modules(
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
