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

/// Returns true for credentials and local configuration that must never leave a
/// project accidentally. `publish.include` can opt in an individual project
/// file after the caller has made that intent explicit.
fn is_sensitive_file(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let name = name.to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    name == ".npmrc"
        || name == ".env"
        || name.starts_with(".env.")
        || matches!(
            extension.as_str(),
            "pem" | "key" | "p12" | "pfx" | "crt" | "cer" | "der" | "jks" | "keystore"
        )
        || ((name.contains("service-account") || name.contains("service_account"))
            && extension == "json")
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

fn include_matches(pattern: &str, path: &Path, project_dir: &Path) -> bool {
    exclude_matches(pattern, path, project_dir)
}

fn is_same_open_file(opened: &fs::Metadata, current: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        opened.dev() == current.dev() && opened.ino() == current.ino()
    }
    #[cfg(not(unix))]
    {
        opened.is_file()
            && current.is_file()
            && opened.len() == current.len()
            && opened.modified().ok() == current.modified().ok()
    }
}

/// Opens a regular file only after proving that its resolved location remains
/// under `root`. The handle, rather than the path, is then handed to `tar` so a
/// replacement after this check cannot change the archived content.
fn open_file_beneath(root: &Path, path: &Path) -> Option<fs::File> {
    let before = fs::symlink_metadata(path).ok()?;
    if !before.file_type().is_file() {
        return None;
    }
    let canonical_before = path.canonicalize().ok()?;
    if !canonical_before.starts_with(root) {
        return None;
    }

    let file = fs::File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file() {
        return None;
    }

    // Detect a path replacement between `symlink_metadata`, canonicalization,
    // and opening. If the path changes after this point, the already-opened
    // descriptor remains the verified file.
    let canonical_after = path.canonicalize().ok()?;
    let current = fs::metadata(path).ok()?;
    if canonical_after != canonical_before
        || !canonical_after.starts_with(root)
        || !is_same_open_file(&opened, &current)
    {
        return None;
    }
    Some(file)
}

fn append_file_beneath(
    ar: &mut tar::Builder<GzEncoder<Vec<u8>>>,
    root: &Path,
    path: &Path,
    tar_path: &Path,
) -> Result<bool> {
    let Some(mut file) = open_file_beneath(root, path) else {
        return Ok(false);
    };
    ar.append_file(tar_path, &mut file)?;
    Ok(true)
}

fn append_dir_beneath(
    ar: &mut tar::Builder<GzEncoder<Vec<u8>>>,
    root: &Path,
    path: &Path,
    tar_path: &Path,
) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(false),
    };
    if !metadata.file_type().is_dir() {
        return Ok(false);
    }
    let canonical = match path.canonicalize() {
        Ok(path) if path.starts_with(root) => path,
        _ => return Ok(false),
    };
    // `canonical` is used solely to prove containment; append_dir records
    // metadata for the directory and never reads a symlink target.
    let _ = canonical;
    ar.append_dir(tar_path, path)?;
    Ok(true)
}

/// TAR headers always use `/` as the path separator. Keep the accompanying
/// manifest in the same form on Windows without rewriting literal backslashes
/// in valid Unix filenames.
fn inventory_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    #[cfg(windows)]
    {
        path.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path.into_owned()
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

pub(crate) fn create_project_tarball_with_filters(
    project_dir: &Path,
    extra_excludes: &[String],
    explicit_includes: &[String],
) -> Result<Vec<u8>> {
    create_project_tarball_with_includes(project_dir, extra_excludes, &[], explicit_includes)
}

/// Test helper for the optional-framework force-include behavior.
///
/// `force_include_dirs` lets files beneath those absolute paths bypass
/// [`should_exclude_file`], while the regular root and symlink checks still
/// apply. Issue #1303: a vendored optional-framework dir
/// (e.g. the GoogleSignIn SDK declared via `perry.toml [google_auth]
/// framework_dir`) contains the static archive binary (extension-less, often
/// >1 MB) and would otherwise be dropped, leaving the worker to link the
/// > no-SDK stub.
#[cfg(test)]
fn create_project_tarball(
    project_dir: &Path,
    extra_excludes: &[String],
    force_include_dirs: &[PathBuf],
) -> Result<Vec<u8>> {
    create_project_tarball_with_includes(project_dir, extra_excludes, force_include_dirs, &[])
}

/// Creates the publish archive. `explicit_includes` may restore automatic
/// exclusions (including sensitive files), but never follows symlinks and
/// never overrides a `publish.exclude` rule.
pub(super) fn create_project_tarball_with_includes(
    project_dir: &Path,
    extra_excludes: &[String],
    force_include_dirs: &[PathBuf],
    explicit_includes: &[String],
) -> Result<Vec<u8>> {
    let project_root = project_dir
        .canonicalize()
        .context("Failed to canonicalize project directory")?;
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
    ar.follow_symlinks(false);
    let mut inventory = Vec::new();

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
    for entry in WalkDir::new(&project_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
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
                .any(|ex| exclude_matches(ex, e.path(), &project_root))
            {
                return false;
            }
            if name.ends_with(".app") {
                return false;
            }
            true
        })
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(&project_root)?;

        if relative.as_os_str().is_empty() {
            continue;
        }

        if entry.file_type().is_file() {
            let explicitly_included = explicit_includes
                .iter()
                .any(|include| include_matches(include, path, &project_root));
            // Force-included framework roots are only for their binary
            // artifacts; they must not silently bypass credential filtering.
            if is_sensitive_file(path) && !explicitly_included {
                continue;
            }
            if should_exclude_file(path) && !is_force_included(path) && !explicitly_included {
                continue;
            }
            if append_file_beneath(&mut ar, &project_root, path, relative)? {
                inventory.push(inventory_path(relative));
            }
        } else if entry.file_type().is_dir() {
            append_dir_beneath(&mut ar, &project_root, path, relative)?;
        }
    }

    // Include file: dependencies under node_modules/<pkg-name>/
    let file_deps = resolve_file_deps(project_dir);
    for (pkg_name, dep_dir) in &file_deps {
        let nm_prefix = PathBuf::from("node_modules").join(pkg_name);
        // Walk the dependency directory (exclude .git, target, dist, build artifacts)
        let dep_exclude_dirs = [".git", "target", "dist", "build", "xcode", "node_modules"];
        for entry in WalkDir::new(dep_dir)
            .follow_links(false)
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

            if entry.file_type().is_file() {
                if is_sensitive_file(path) || should_exclude_file(path) {
                    continue;
                }
                if append_file_beneath(&mut ar, dep_dir, path, &tar_path)? {
                    inventory.push(inventory_path(&tar_path));
                }
            } else if entry.file_type().is_dir() {
                append_dir_beneath(&mut ar, dep_dir, path, &tar_path)?;
            }
        }
    }

    inventory.sort();
    let inventory = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "files": inventory,
    }))?;
    let mut header = tar::Header::new_gnu();
    header.set_size(inventory.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    ar.append_data(
        &mut header,
        Path::new(".perry/publish-manifest.json"),
        inventory.as_slice(),
    )?;

    ar.finish()?;
    let encoder = ar.into_inner()?;
    Ok(encoder.finish()?)
}

#[cfg(test)]
mod force_include_tests {
    use super::*;
    use std::io::Read;

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

    fn tar_manifest_files(bytes: &[u8]) -> Vec<String> {
        let dec = flate2::read::GzDecoder::new(bytes);
        let mut ar = tar::Archive::new(dec);
        for entry in ar.entries().unwrap().flatten() {
            if entry.path().unwrap() == Path::new(".perry/publish-manifest.json") {
                let mut contents = String::new();
                let mut entry = entry;
                entry.read_to_string(&mut contents).unwrap();
                return serde_json::from_str::<serde_json::Value>(&contents).unwrap()["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|file| file.as_str().map(String::from))
                    .collect();
            }
        }
        panic!("publish manifest missing from archive");
    }

    #[test]
    fn inventory_paths_use_tar_separators_without_rewriting_unix_names() {
        let path = Path::new(r"directory\file.ts");
        #[cfg(windows)]
        assert_eq!(inventory_path(path), "directory/file.ts");
        #[cfg(not(windows))]
        assert_eq!(inventory_path(path), r"directory\file.ts");
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

    #[cfg(unix)]
    #[test]
    fn skips_symlinks_without_reading_their_targets() {
        use std::os::unix::fs::symlink;

        let proj = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(proj.path().join("index.ts"), "export {}\n").unwrap();
        fs::write(proj.path().join("inside.txt"), "inside").unwrap();
        fs::write(outside.path().join("secret.txt"), "outside secret").unwrap();
        fs::create_dir_all(outside.path().join("external-dir")).unwrap();
        fs::write(
            outside.path().join("external-dir/nested.txt"),
            "outside nested secret",
        )
        .unwrap();
        symlink(
            outside.path().join("secret.txt"),
            proj.path().join("external-file"),
        )
        .unwrap();
        symlink(
            outside.path().join("external-dir"),
            proj.path().join("external-dir"),
        )
        .unwrap();
        symlink("inside.txt", proj.path().join("internal-file")).unwrap();
        symlink("chain-b", proj.path().join("chain-a")).unwrap();
        symlink("chain-a", proj.path().join("chain-b")).unwrap();

        let entries = tar_entries(&create_project_tarball(proj.path(), &[], &[]).unwrap());
        assert!(entries.iter().any(|p| p == "inside.txt"));
        for omitted in [
            "external-file",
            "external-dir",
            "external-dir/nested.txt",
            "internal-file",
            "chain-a",
            "chain-b",
        ] {
            assert!(
                !entries.iter().any(|p| p == omitted),
                "symlink entry {omitted} must be omitted, got {entries:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlink_escapes_from_file_dependencies() {
        use std::os::unix::fs::symlink;

        let proj = tempfile::tempdir().unwrap();
        let dep = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(proj.path().join("index.ts"), "export {}\n").unwrap();
        fs::write(
            proj.path().join("package.json"),
            format!(
                r#"{{"dependencies":{{"local-dep":"file:{}"}}}}"#,
                dep.path().display()
            ),
        )
        .unwrap();
        fs::write(dep.path().join("index.ts"), "export const dep = true\n").unwrap();
        fs::write(
            outside.path().join("secret.ts"),
            "export const secret = true\n",
        )
        .unwrap();
        symlink(
            outside.path().join("secret.ts"),
            dep.path().join("escaped.ts"),
        )
        .unwrap();

        let entries = tar_entries(&create_project_tarball(proj.path(), &[], &[]).unwrap());
        assert!(entries
            .iter()
            .any(|p| p == "node_modules/local-dep/index.ts"));
        assert!(!entries
            .iter()
            .any(|p| p == "node_modules/local-dep/escaped.ts"));
    }

    #[test]
    fn excludes_sensitive_files_unless_explicitly_included() {
        let proj = tempfile::tempdir().unwrap();
        fs::write(proj.path().join("index.ts"), "export {}\n").unwrap();
        for name in [
            ".env",
            ".env.production",
            ".npmrc",
            "tls-cert.pem",
            "signing.key",
            "service-account.json",
            "service_account.production.json",
        ] {
            fs::write(proj.path().join(name), "sensitive").unwrap();
        }

        let default_tarball = create_project_tarball(proj.path(), &[], &[]).unwrap();
        let default_entries = tar_entries(&default_tarball);
        assert!(default_entries.iter().any(|p| p == "index.ts"));
        assert!(default_entries
            .iter()
            .any(|p| p == ".perry/publish-manifest.json"));
        let manifest_files = tar_manifest_files(&default_tarball);
        assert!(manifest_files.iter().any(|p| p == "index.ts"));
        for name in [
            ".env",
            ".env.production",
            ".npmrc",
            "tls-cert.pem",
            "signing.key",
            "service-account.json",
            "service_account.production.json",
        ] {
            assert!(
                !default_entries.iter().any(|p| p == name),
                "sensitive file {name} must be excluded by default"
            );
        }

        let included = create_project_tarball_with_includes(
            proj.path(),
            &[],
            &[],
            &[".env.production".to_string()],
        )
        .unwrap();
        let included_entries = tar_entries(&included);
        assert!(included_entries.iter().any(|p| p == ".env.production"));
        assert!(!included_entries.iter().any(|p| p == ".env"));
        assert!(!included_entries.iter().any(|p| p == ".npmrc"));
    }
}
