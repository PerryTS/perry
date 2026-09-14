//! Resolve conservative Worker return unions before adding module edges.
use std::collections::HashSet;
use std::path::Path;

use super::import_helpers::cached_resolve_import_with_lexical_base;
use super::{CompilationContext, OutputFormat};

pub(super) fn resolve_candidates(
    paths: &mut Vec<String>,
    entry_path: &Path,
    canonical: &Path,
    module_name: &str,
    ctx: &mut CompilationContext,
    format: OutputFormat,
) -> Result<Vec<String>, String> {
    let mut imports = Vec::new();
    let mut entries = HashSet::new();
    for path in paths.iter_mut() {
        if path.starts_with("file:") {
            *path = url::Url::parse(path)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .ok_or_else(|| {
                    format!(
                        "worker_threads Worker in module {module_name}: invalid file URL {path:?}"
                    )
                })?
                .to_string_lossy()
                .into_owned();
        }
        if let Some(resolved) =
            cached_resolve_import_with_lexical_base(path, entry_path, canonical, ctx)
        {
            // Keep spelling aliases for runtime dispatch. Module discovery
            // deduplicates these edges by canonical path, so each entry is
            // compiled only once (e.g. a --define and a relative URL fallback).
            if !imports.contains(path) {
                imports.push(path.clone());
            }
            if entries.insert(resolved.canonical_path.clone())
                && matches!(format, OutputFormat::Text)
            {
                eprintln!("  Worker entry: {}", resolved.canonical_path.display());
            }
        } else if matches!(format, OutputFormat::Text) {
            eprintln!(
                "  Warning: worker_threads Worker in module {module_name}: skipping candidate {path:?}: file not found"
            );
        }
    }
    // Missing candidates never become import edges. Retain their spellings
    // alongside valid ones so codegen cannot mistake a partial union for a
    // proven single target: choosing a missing candidate must throw at runtime.
    if imports.is_empty() {
        paths.clear();
        if matches!(format, OutputFormat::Text) {
            eprintln!(
                "  Warning: worker_threads Worker in module {module_name}: no existing candidates — this Worker will throw if constructed at runtime"
            );
        }
    }
    Ok(imports)
}
