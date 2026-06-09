//! #498 - native-archive discovery + lockfile verification for the compile path.
//!
//! Two entry points are exported back to the crate:
//!
//! - `collect_native_archives_for_lock` — pub helper used by the standalone
//!   `perry lock` subcommand to discover every `perry.nativeLibrary` archive
//!   a build at `project_root` would consume, without running the full compile
//!   pipeline.
//! - `run_lock_verify_for_compile` — invoked from `build_and_run_link` so
//!   every backend hits the same supply-chain gate before any object reaches
//!   the linker.
//!
//! The `derive_target_key` helper is the single source of truth for the
//! per-arch lockfile key used in `perry.lock`'s `sha256_per_target` map.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::OutputFormat;

use super::resolve::{has_perry_native_library, parse_native_library_manifest};
use super::CompilationContext;

/// #498 - discover every `perry.nativeLibrary` archive a build of
/// the project at `project_root` would consume. Scans every
/// `node_modules/<pkg>/package.json` for a `perry.nativeLibrary`
/// block and resolves the per-target `prebuilt:` path. Used by the
/// standalone `perry lock` subcommand for hash verification without
/// running the full compile pipeline.
pub fn collect_native_archives_for_lock(
    project_root: &Path,
    _input: Option<&Path>,
    target: Option<&str>,
    format: OutputFormat,
) -> Result<Vec<crate::commands::perry_lock::ArchiveEntry>> {
    use crate::commands::perry_lock::ArchiveEntry;

    let mut archives: Vec<ArchiveEntry> = Vec::new();
    let node_modules = project_root.join("node_modules");
    if !node_modules.is_dir() {
        if matches!(format, OutputFormat::Text) {
            println!("  (no node_modules/ present - nothing to lock)");
        }
        return Ok(Vec::new());
    }

    for_each_native_library_package(&node_modules, &mut |pkg_dir, pkg_name| {
        if let Some(manifest) = parse_native_library_manifest(pkg_dir, pkg_name, target)? {
            if let Some(tc) = manifest.target_config.as_ref() {
                if !tc.available {
                    return Ok(());
                }
                if let Some(prebuilt) = tc.prebuilt.as_ref() {
                    if prebuilt.exists() {
                        archives.push(ArchiveEntry {
                            package: manifest.module.clone(),
                            target_key: derive_target_key(target),
                            path: prebuilt.clone(),
                        });
                    }
                }
                for backend in &tc.backends {
                    if !backend.available {
                        continue;
                    }
                    if let Some(prebuilt) = backend.prebuilt.as_ref() {
                        if prebuilt.exists() {
                            archives.push(ArchiveEntry {
                                package: format!(
                                    "{}:{}",
                                    manifest.module,
                                    backend.backend.as_str()
                                ),
                                target_key: derive_target_key(target),
                                path: prebuilt.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    })?;

    Ok(archives)
}

/// Walk every immediate child of `node_modules/` (including
/// `@scope/*` sub-children), invoking `cb(pkg_dir, pkg_name)` for
/// each directory that has a `package.json` declaring a
/// `perry.nativeLibrary` block.
fn for_each_native_library_package(
    node_modules: &Path,
    cb: &mut dyn FnMut(&Path, &str) -> Result<()>,
) -> Result<()> {
    let Ok(entries) = fs::read_dir(node_modules) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() && !ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if name.starts_with('@') {
            if let Ok(scope_entries) = fs::read_dir(&path) {
                for scope_entry in scope_entries.flatten() {
                    let Ok(sft) = scope_entry.file_type() else {
                        continue;
                    };
                    if !sft.is_dir() && !sft.is_symlink() {
                        continue;
                    }
                    let sub_name = scope_entry.file_name().to_string_lossy().to_string();
                    let pkg_dir = scope_entry.path();
                    let full_name = format!("{}/{}", name, sub_name);
                    if has_perry_native_library(&pkg_dir) {
                        cb(&pkg_dir, &full_name)?;
                    }
                }
            }
        } else if has_perry_native_library(&path) {
            cb(&path, &name)?;
        }
    }
    Ok(())
}

/// Map a Perry target string into the per-arch lockfile key used in
/// `perry.lock`'s `sha256_per_target` map. Distinct from the
/// resolver-internal target_key because the lockfile is the
/// reviewer-facing supply-chain artifact: "macos" alone is
/// ambiguous between arm64 and x86_64, so we always include arch
/// for multi-arch targets.
fn derive_target_key(target: Option<&str>) -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        "x86_64" => "x86_64",
        "x86" => "i686",
        other => other,
    };
    match target {
        None => format!("{}-{}", std::env::consts::OS, arch),
        Some("macos") => format!("macos-{}", arch),
        Some("linux") => format!("linux-{}", arch),
        // musl: keep the linux-<arch> lock key (#4826).
        Some("linux-musl") | Some("linux-x86_64-musl") | Some("linux-aarch64-musl") => {
            format!("linux-{}", arch)
        }
        Some("windows") | Some("windows-winui") => format!("windows-{}", arch),
        Some("ios") => "ios".to_string(),
        Some("ios-simulator") => "ios-simulator".to_string(),
        Some("tvos") => "tvos".to_string(),
        Some("tvos-simulator") => "tvos-simulator".to_string(),
        Some("watchos") => "watchos".to_string(),
        Some("watchos-simulator") => "watchos-simulator".to_string(),
        Some("visionos") => "visionos".to_string(),
        Some("visionos-simulator") => "visionos-simulator".to_string(),
        Some("android") => "android".to_string(),
        Some("harmonyos") => "harmonyos".to_string(),
        Some("harmonyos-simulator") => "harmonyos-simulator".to_string(),
        Some("web") => "web".to_string(),
        Some(other) => other.to_string(),
    }
}

/// Verify (or write) `perry.lock` against the prebuilt archives the
/// current build is about to consume. Called from
/// `build_and_run_link` so every backend (LLVM / WASM / ArkTS / ...)
/// hits the same supply-chain gate before any object hits the
/// linker.
///
/// Mode resolution:
///
/// - `PERRY_LOCK_FROZEN=1` -> `LockMode::Frozen` (CI verification
///   only, refuses to extend `perry.lock`).
/// - `PERRY_LOCK_UPDATE=<pkg>[,<pkg2>...]` -> `LockMode::Update`.
///   Comma-separated package names. Empty string opts everything in.
/// - default -> `LockMode::Default` (write missing, verify present).
pub(crate) fn run_lock_verify_for_compile(
    ctx: &CompilationContext,
    target: Option<&str>,
) -> Result<()> {
    use crate::commands::perry_lock::{verify_or_write, ArchiveEntry, LockMode};

    let mut archives: Vec<ArchiveEntry> = Vec::new();
    for manifest in &ctx.native_libraries {
        if let Some(tc) = manifest.target_config.as_ref() {
            if !tc.available {
                continue;
            }
            if let Some(prebuilt) = tc.prebuilt.as_ref() {
                if prebuilt.exists() {
                    archives.push(ArchiveEntry {
                        package: manifest.module.clone(),
                        target_key: derive_target_key(target),
                        path: prebuilt.clone(),
                    });
                }
            }
            for backend in &tc.backends {
                if !backend.available {
                    continue;
                }
                if let Some(prebuilt) = backend.prebuilt.as_ref() {
                    if prebuilt.exists() {
                        archives.push(ArchiveEntry {
                            package: format!("{}:{}", manifest.module, backend.backend.as_str()),
                            target_key: derive_target_key(target),
                            path: prebuilt.clone(),
                        });
                    }
                }
            }
        }
    }
    if archives.is_empty() {
        return Ok(());
    }

    let mode = if std::env::var("PERRY_LOCK_FROZEN").is_ok() {
        LockMode::Frozen
    } else if let Ok(v) = std::env::var("PERRY_LOCK_UPDATE") {
        let pkgs: Vec<String> = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        LockMode::Update(pkgs)
    } else {
        LockMode::Default
    };

    verify_or_write(&ctx.project_root, &archives, &mode)?;
    Ok(())
}

#[cfg(test)]
mod lock_integration_tests {
    //! #498 - coverage for the compile-time pieces that wire the
    //! lockfile gate into the build path. Test the pure helpers
    //! here; the verification semantics themselves live in
    //! `commands::perry_lock::tests`.
    use super::*;

    #[test]
    fn derive_target_key_includes_arch_for_multi_arch_targets() {
        assert!(derive_target_key(Some("macos")).starts_with("macos-"));
        assert!(derive_target_key(Some("linux")).starts_with("linux-"));
        assert!(derive_target_key(Some("windows")).starts_with("windows-"));
    }

    #[test]
    fn derive_target_key_for_single_arch_mobile_targets() {
        assert_eq!(derive_target_key(Some("ios")), "ios");
        assert_eq!(derive_target_key(Some("ios-simulator")), "ios-simulator");
        assert_eq!(derive_target_key(Some("tvos")), "tvos");
        assert_eq!(derive_target_key(Some("watchos")), "watchos");
        assert_eq!(derive_target_key(Some("android")), "android");
    }

    #[test]
    fn derive_target_key_native_falls_back_to_host_os_and_arch() {
        let host = derive_target_key(None);
        assert!(host.contains('-'), "host key has arch suffix: {host}");
        assert!(
            host.starts_with(std::env::consts::OS),
            "host key starts with host OS: {host}"
        );
    }

    #[test]
    fn collect_archives_no_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let archives =
            collect_native_archives_for_lock(dir.path(), None, Some("macos"), OutputFormat::Json)
                .expect("succeeds with no deps");
        assert!(archives.is_empty());
    }

    /// Picks up a scoped `node_modules/@scope/pkg` with a
    /// `perry.nativeLibrary` block and a resolved `prebuilt:` path.
    #[test]
    fn collect_archives_picks_up_scoped_package() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let pkg_dir = project.join("node_modules/@bloom/engine");
        fs::create_dir_all(&pkg_dir).unwrap();
        let archive_path = pkg_dir.join("libengine.a");
        fs::write(&archive_path, b"static-archive-bytes").unwrap();
        let manifest = serde_json::json!({
            "name": "@bloom/engine",
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": { "lib": "engine", "prebuilt": "./libengine.a" }
                    }
                }
            }
        });
        fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let archives =
            collect_native_archives_for_lock(project, None, Some("macos"), OutputFormat::Json)
                .expect("scan");
        assert_eq!(archives.len(), 1);
        assert_eq!(archives[0].package, "@bloom/engine");
        // Derived from host arch so the test passes on both arm64 Mac dev
        // boxes and x86_64 Linux CI runners.
        assert_eq!(archives[0].target_key, derive_target_key(Some("macos")));
        assert_eq!(archives[0].path, archive_path);
    }

    #[test]
    fn collect_archives_includes_backend_prebuilt_packages() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let pkg_dir = project.join("node_modules/@bloom/engine");
        fs::create_dir_all(&pkg_dir).unwrap();
        let target_archive_path = pkg_dir.join("libengine.a");
        let backend_archive_path = pkg_dir.join("libengine_vulkan.a");
        fs::write(&target_archive_path, b"target-static-archive-bytes").unwrap();
        fs::write(&backend_archive_path, b"vulkan-static-archive-bytes").unwrap();
        let manifest = serde_json::json!({
            "name": "@bloom/engine",
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "linux": {
                            "lib": "engine",
                            "prebuilt": "./libengine.a",
                            "backends": {
                                "vulkan": {
                                    "prebuilt": "./libengine_vulkan.a"
                                }
                            }
                        }
                    }
                }
            }
        });
        fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let mut archives =
            collect_native_archives_for_lock(project, None, Some("linux"), OutputFormat::Json)
                .expect("scan");
        archives.sort_by(|a, b| a.package.cmp(&b.package));

        assert_eq!(archives.len(), 2);
        assert_eq!(archives[0].package, "@bloom/engine");
        assert_eq!(archives[0].target_key, derive_target_key(Some("linux")));
        assert_eq!(archives[0].path, target_archive_path);
        assert_eq!(archives[1].package, "@bloom/engine:vulkan");
        assert_eq!(archives[1].target_key, derive_target_key(Some("linux")));
        assert_eq!(archives[1].path, backend_archive_path);
    }
}
