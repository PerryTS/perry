//! Project-asset copy helpers used by the per-target bundle writers.
//!
//! Extracted from `compile.rs` for issue #1105 PR 3 (directory split).
//! Pure file move — no behavior change. Three callers from the parent
//! orchestrator and the various `bundle_for_*` helpers all share these
//! routines for locating the project root and copying its asset
//! directories into the platform bundle.

use std::fs;
use std::path::{Path, PathBuf};

/// Walk up from `start` looking for a project anchor (`package.json`,
/// or `perry.toml` if `watch_for_perry_toml` is `true`). Bounded to 5
/// levels so a runaway walk can't traverse the filesystem. Returns
/// the deepest directory that holds an anchor; if none found within
/// the bound, returns the starting input unchanged.
pub(super) fn find_project_root_for_resources(start: &Path, watch_for_perry_toml: bool) -> PathBuf {
    let mut project_root = start.to_path_buf();
    for _ in 0..5 {
        if project_root.join("package.json").exists() {
            break;
        }
        if watch_for_perry_toml && project_root.join("perry.toml").exists() {
            break;
        }
        if let Some(parent) = project_root.parent() {
            project_root = parent.to_path_buf();
        } else {
            break;
        }
    }
    project_root
}

/// Recursive copy used by the per-target bundle writers. Mirrors the
/// inline `copy_dir_recursive_standalone` / `copy_dir_recursive`
/// helpers that lived in each bundle branch before PR 2.
pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Copy the `logo` / `assets` / `resources` / `images` directories
/// from `project_root` into `dest_dir`. Used by the bundle writers
/// so `[[NSBundle mainBundle] resourcePath]` / `resolve_asset_path`
/// can find assets at runtime.
pub(super) fn copy_bundle_resource_dirs(project_root: &Path, dest_dir: &Path) {
    for dir_name in &["logo", "assets", "resources", "images"] {
        let resource_dir = project_root.join(dir_name);
        if resource_dir.is_dir() {
            let dest = dest_dir.join(dir_name);
            let _ = copy_dir_recursive(&resource_dir, &dest);
        }
    }
}
