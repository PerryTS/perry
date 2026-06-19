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

/// Does a user `publish.exclude` pattern match `path` (within `project_dir`)?
///
/// Matching is anchored to the project root, NOT gitignore-style "match at any
/// depth". A bare name like `"jump"` excludes the project-root entry `jump`
/// (file or dir) only — it must NOT also prune a same-named directory buried in
/// the tree (e.g. `android/app/src/main/java/com/bloomengine/jump/`, which holds
/// the Android launcher Activity). That deep-match footgun silently dropped
/// source from published Android AABs (#4810). A pattern containing `/` is a
/// path relative to the project root and matches that subtree. A leading `/`
/// is accepted as an explicit root anchor (`"/jump"` == `"jump"`).
///
/// The builtin always-excluded dirs (`node_modules`, `.git`, `target`, …) are
/// handled separately and still match at any depth.
pub(super) fn exclude_matches(pattern: &str, path: &Path, project_dir: &Path) -> bool {
    let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
    if pattern.is_empty() {
        return false;
    }
    let Ok(rel) = path.strip_prefix(project_dir) else {
        return false;
    };
    if pattern.contains('/') {
        // Path-relative: matches the named subtree from the project root.
        rel.starts_with(pattern)
    } else {
        // Bare name: a single project-root entry (file or dir), root-anchored.
        rel == Path::new(pattern)
    }
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
    create_project_tarball(project_dir, extra_excludes, &[])
}

/// As [`create_project_tarball_with_excludes`], but `force_include_dirs`
/// (absolute paths) are packed verbatim — files under them bypass
/// [`should_exclude_file`]. Issue #1303: a vendored optional-framework dir
/// (e.g. the GoogleSignIn SDK declared via `perry.toml [google_auth]
/// framework_dir`) contains the static archive binary (extension-less, often
/// >1 MB) and would otherwise be dropped, leaving the worker to link the
/// > no-SDK stub.
pub(crate) fn create_project_tarball(
    project_dir: &Path,
    extra_excludes: &[String],
    force_include_dirs: &[PathBuf],
) -> Result<Vec<u8>> {
    // Force-include dirs are matched by absolute-path prefix; canonicalize
    // so a relative/`./`-prefixed input still matches the walked paths.
    let force_roots: Vec<PathBuf> = force_include_dirs
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect();
    let is_force_included = |path: &Path| -> bool {
        path.canonicalize()
            .ok()
            .map(|abs| force_roots.iter().any(|r| abs.starts_with(r)))
            .unwrap_or(false)
    };

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
        if extra_excludes
            .iter()
            .any(|ex| exclude_matches(ex, e.path(), project_dir))
        {
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
            if should_exclude_file(path) && !is_force_included(path) {
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

#[cfg(test)]
mod force_include_tests {
    use super::*;

    /// Collect the relative paths packed into a gzipped tarball.
    fn tar_entries(bytes: &[u8]) -> Vec<String> {
        let dec = flate2::read::GzDecoder::new(bytes);
        let mut ar = tar::Archive::new(dec);
        ar.entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn force_include_keeps_otherwise_excluded_framework_binary() {
        let proj = tempfile::tempdir().unwrap();
        // A vendored static framework binary: extension-less and >1 MB,
        // which `should_exclude_file` drops by default.
        let fw = proj
            .path()
            .join("vendor/google-sign-in/frameworks/GoogleSignIn.framework");
        fs::create_dir_all(&fw).unwrap();
        let binary = fw.join("GoogleSignIn");
        fs::write(&binary, vec![0u8; 2 * 1024 * 1024]).unwrap();
        // A normal source file so the tarball is never empty.
        fs::write(proj.path().join("index.ts"), "export {}\n").unwrap();

        // Without force-include: the binary is dropped.
        let plain = create_project_tarball(proj.path(), &[], &[]).unwrap();
        let plain_entries = tar_entries(&plain);
        assert!(
            !plain_entries
                .iter()
                .any(|p| p.ends_with("GoogleSignIn.framework/GoogleSignIn")),
            "binary should be excluded by default, got {plain_entries:?}"
        );

        // With force-include: the binary survives.
        let fw_dir = proj.path().join("vendor/google-sign-in/frameworks");
        let forced =
            create_project_tarball(proj.path(), &[], std::slice::from_ref(&fw_dir)).unwrap();
        let forced_entries = tar_entries(&forced);
        assert!(
            forced_entries
                .iter()
                .any(|p| p.ends_with("GoogleSignIn.framework/GoogleSignIn")),
            "force-included framework binary should be packed, got {forced_entries:?}"
        );
    }

    #[test]
    fn exclude_matches_is_root_anchored() {
        let root = Path::new("/proj");
        // Bare name matches a project-root entry (file or dir)...
        assert!(exclude_matches("jump", Path::new("/proj/jump"), root));
        assert!(exclude_matches("dist", Path::new("/proj/dist"), root));
        // ...but NOT a same-named directory deeper in the tree (the footgun).
        assert!(!exclude_matches(
            "jump",
            Path::new("/proj/android/app/src/main/java/com/bloomengine/jump"),
            root
        ));
        // A leading slash is an explicit root anchor, equivalent to the bare name.
        assert!(exclude_matches("/jump", Path::new("/proj/jump"), root));
        assert!(!exclude_matches("/jump", Path::new("/proj/a/jump"), root));
        // Path patterns match the named subtree from the project root.
        assert!(exclude_matches(
            "android/app/build",
            Path::new("/proj/android/app/build"),
            root
        ));
        assert!(exclude_matches(
            "android/app/build",
            Path::new("/proj/android/app/build/outputs/x.txt"),
            root
        ));
        assert!(!exclude_matches(
            "android/app/build",
            Path::new("/proj/android/app/src"),
            root
        ));
        // Empty / outside-root never match.
        assert!(!exclude_matches("/", Path::new("/proj/x"), root));
        assert!(!exclude_matches("jump", Path::new("/other/jump"), root));
    }

    #[test]
    fn bare_exclude_does_not_prune_deep_same_named_dir() {
        // Regression: a project that excludes its root `jump` binary must NOT
        // have a `…/com/bloomengine/jump/` source package pruned too — that
        // silently dropped the Android launcher Activity from published AABs.
        let proj = tempfile::tempdir().unwrap();
        fs::write(proj.path().join("index.ts"), "export {}\n").unwrap();
        // Root artifact the user wants gone (small, so not auto-excluded by size).
        fs::write(proj.path().join("jump"), "binary-ish").unwrap();
        // Deep source package that happens to share the name.
        let pkg = proj
            .path()
            .join("android/app/src/main/java/com/bloomengine/jump");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("BloomActivity.kt"), "class BloomActivity\n").unwrap();

        let bytes =
            create_project_tarball(proj.path(), std::slice::from_ref(&"jump".to_string()), &[])
                .unwrap();
        let entries = tar_entries(&bytes);

        assert!(
            !entries.iter().any(|p| p == "jump"),
            "root `jump` artifact should be excluded, got {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|p| p.ends_with("com/bloomengine/jump/BloomActivity.kt")),
            "deep jump/ package must be kept, got {entries:?}"
        );
    }
}
