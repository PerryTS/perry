//! `?script-string` asset imports used by TanStack Start's hydration bootstrap.

use anyhow::{anyhow, Result};
use perry_hir::{Import, ModuleKind};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::super::CompilationContext;

fn import_target(specifier: &str) -> Option<&str> {
    let (path, query) = specifier.split_once('?')?;
    query
        .split('&')
        .any(|part| part == "script-string")
        .then_some(path)
}

fn source_path(target: &str, importer_path: &Path) -> Option<PathBuf> {
    if target.starts_with('/') {
        super::super::resolve::resolve_absolute_import_paths(target).map(|path| path.canonical_path)
    } else {
        super::super::resolve::resolve_relative_import_path(target, importer_path)
    }
}

/// Materialize a string asset as a synthetic TypeScript default export.
///
/// The source is deliberately preserved byte-for-byte. It is executable text,
/// so whitespace trimming or inferred semicolon insertion can change its
/// meaning. Relative targets use the current module, not the graph entry.
pub(super) fn resolve(
    ctx: &CompilationContext,
    importer_path: &Path,
    import: &mut Import,
    pending: &mut Vec<PathBuf>,
) -> Result<bool> {
    let Some(target) = import_target(&import.source) else {
        return Ok(false);
    };
    let Some(source_path) = source_path(target, importer_path) else {
        return Ok(false);
    };
    let script = fs::read_to_string(&source_path)
        .map_err(|e| anyhow!("Failed to read {}: {}", source_path.display(), e))?;
    let literal = serde_json::to_string(&script).map_err(|e| {
        anyhow!(
            "Failed to encode script-string asset {} as a string literal: {}",
            source_path.display(),
            e
        )
    })?;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source_path.hash(&mut hasher);
    import.source.hash(&mut hasher);
    script.hash(&mut hasher);
    let filename = format!("script-string-{:016x}.ts", hasher.finish());
    let dir = ctx.cache_dir.join("synthetic-modules");
    fs::create_dir_all(&dir).map_err(|e| anyhow!("Failed to create {}: {}", dir.display(), e))?;
    let synthetic_path = dir.join(filename);
    fs::write(&synthetic_path, format!("export default {};\n", literal))
        .map_err(|e| anyhow!("Failed to write {}: {}", synthetic_path.display(), e))?;
    let canonical = synthetic_path.canonicalize().map_err(|e| {
        anyhow!(
            "Failed to canonicalize synthetic module {}: {}",
            synthetic_path.display(),
            e
        )
    })?;
    import.resolved_path = Some(canonical.to_string_lossy().into_owned());
    import.module_kind = ModuleKind::NativeCompiled;
    pending.push(synthetic_path);
    Ok(true)
}
