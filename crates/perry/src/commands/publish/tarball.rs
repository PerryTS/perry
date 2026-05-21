use super::*;

/// Should this file be excluded from the tarball?
pub(super) fn should_exclude_file(path: &Path) -> bool {
    let exclude_extensions = [
        "o", "a", "dylib", "so", "dll", "exe", "dmg", "ipa", "apk", "aab",
    ];
    let name = path.file_name().unwrap_or_default().to_string_lossy();

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if exclude_extensions.contains(&ext) {
            return true;
        }
    }
    if name.starts_with('_')
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
    {
        return true;
    }
    if path.extension().is_none()
        && path
            .metadata()
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
    {
        return true;
    }
    if name == ".DS_Store" {
        return true;
    }
    false
}

/// Resolve `file:` dependencies from package.json and return (package_name, resolved_path) pairs.
pub(super) fn resolve_file_deps(project_dir: &Path) -> Vec<(String, PathBuf)> {
    let pkg_path = project_dir.join("package.json");
    let Ok(content) = fs::read_to_string(&pkg_path) else {
        return vec![];
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) else {
        return vec![];
    };
    let mut deps = Vec::new();
    for key in ["dependencies", "devDependencies"] {
        if let Some(obj) = pkg.get(key).and_then(|v| v.as_object()) {
            for (name, value) in obj {
                if let Some(spec) = value.as_str() {
                    if let Some(rel_path) = spec.strip_prefix("file:") {
                        let resolved = project_dir.join(rel_path).canonicalize().ok();
                        if let Some(abs_path) = resolved {
                            if abs_path.is_dir() {
                                deps.push((name.clone(), abs_path));
                            }
                        }
                    }
                }
            }
        }
    }
    deps
}

pub(crate) fn create_project_tarball_with_excludes(
    project_dir: &Path,
    extra_excludes: &[String],
) -> Result<Vec<u8>> {
    let buf = Vec::new();
    let encoder = GzEncoder::new(buf, Compression::default());
    let mut ar = tar::Builder::new(encoder);

    let builtin_exclude_dirs: Vec<&str> = vec![
        "node_modules",
        ".git",
        "dist",
        "build",
        "target",
        ".perry",
        "xcode",
    ];

    // Walk the project directory
    for entry in WalkDir::new(project_dir).into_iter().filter_entry(|e| {
        // The walk root is always kept — exclusion rules below apply to
        // children only. Without this guard, a user whose project root
        // basename happens to match a bare-name entry in
        // `publish.exclude` (typical when excluding a built binary that
        // shares a name with the project dir) would have the entire
        // tree pruned at depth 0, producing an empty tarball with no
        // CLI-side error. Tracked in #416.
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if builtin_exclude_dirs.iter().any(|ex| name == *ex) {
            return false;
        }
        if extra_excludes.iter().any(|ex| {
            if ex.contains('/') {
                // Path-based exclude: match against relative path from project root
                e.path()
                    .strip_prefix(project_dir)
                    .map(|rel| rel.starts_with(ex))
                    .unwrap_or(false)
            } else {
                name == *ex
            }
        }) {
            return false;
        }
        if name.ends_with(".app") {
            return false;
        }
        true
    }) {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(project_dir)?;

        if relative.as_os_str().is_empty() {
            continue;
        }

        if path.is_file() {
            if should_exclude_file(path) {
                continue;
            }
            ar.append_path_with_name(path, relative)?;
        } else if path.is_dir() {
            ar.append_dir(relative, path)?;
        }
    }

    // Include file: dependencies under node_modules/<pkg-name>/
    let file_deps = resolve_file_deps(project_dir);
    for (pkg_name, dep_dir) in &file_deps {
        let nm_prefix = PathBuf::from("node_modules").join(pkg_name);
        // Walk the dependency directory (exclude .git, target, dist, build artifacts)
        let dep_exclude_dirs = [".git", "target", "dist", "build", "xcode", "node_modules"];
        for entry in WalkDir::new(dep_dir)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                if dep_exclude_dirs.iter().any(|ex| name == *ex) {
                    return false;
                }
                if name.ends_with(".app") {
                    return false;
                }
                true
            })
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let relative = match path.strip_prefix(dep_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if relative.as_os_str().is_empty() {
                continue;
            }

            let tar_path = nm_prefix.join(relative);

            if path.is_file() {
                if should_exclude_file(path) {
                    continue;
                }
                ar.append_path_with_name(path, &tar_path)?;
            } else if path.is_dir() {
                ar.append_dir(&tar_path, path)?;
            }
        }
    }

    ar.finish()?;
    let encoder = ar.into_inner()?;
    Ok(encoder.finish()?)
}
