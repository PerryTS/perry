use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::{host_target_triple, rust_target_triple, CompilationContext, NativeAddonModule};

pub(super) const NODE_API_POLICY_VERSION: u32 = 1;
pub(super) const NODE_API_VERSION: u32 = 10;
pub(super) const SHIPPING_MODEL: &str = "sidecar-v1";

#[derive(Serialize)]
struct SidecarManifest {
    schema_version: u32,
    policy_version: u32,
    napi_version: u32,
    shipping_model: &'static str,
    target: String,
    allowlist: Vec<String>,
    path_allowlist: Vec<String>,
    addons: Vec<ManifestAddon>,
}

#[derive(Serialize)]
struct ManifestAddon {
    logical_id: String,
    require_aliases: Vec<String>,
    package: String,
    version: String,
    entry: String,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    path: String,
    sha256: String,
    size: u64,
}

// Platform packages commonly expose their .node binary as package.json main.
// Record that exact entry at compile time; never consult build-machine package
// metadata at runtime. Packages with exports keep their exports policy.
fn package_entry_aliases(addon: &NativeAddonModule) -> Vec<String> {
    if !addon.ship_package_payload {
        return Vec::new();
    }
    let Some(manifest) = fs::read_to_string(addon.package_dir.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Vec::new();
    };
    if manifest.get("exports").is_some() {
        return Vec::new();
    }
    let Some(name) = manifest.get("name").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let main = manifest
        .get("main")
        .and_then(|v| v.as_str())
        .unwrap_or("index");
    let resolved = resolve_main_file(&addon.package_dir.join(main))
        .or_else(|| resolve_main_file(&addon.package_dir.join("index")));
    if resolved.as_ref() == Some(&addon.source_path) {
        vec![name.to_string()]
    } else {
        Vec::new()
    }
}

fn resolve_main_file(path: &Path) -> Option<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    for extension in ["js", "json", "node"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(format!(".{extension}"));
        candidates.push(PathBuf::from(candidate));
    }
    for extension in ["js", "json", "node"] {
        candidates.push(path.join(format!("index.{extension}")));
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

pub(super) fn sidecar_root(executable: &Path) -> Result<PathBuf> {
    let file_name = executable
        .file_name()
        .ok_or_else(|| anyhow!("output executable has no filename"))?
        .to_string_lossy();
    if executable
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "MacOS")
    {
        if let Some(contents) = executable
            .parent()
            .and_then(Path::parent)
            .filter(|path| path.file_name().is_some_and(|name| name == "Contents"))
        {
            return Ok(contents
                .join("Frameworks")
                .join(format!("{file_name}.perry-native")));
        }
    }
    Ok(executable.with_file_name(format!("{file_name}.perry-native")))
}

fn portable_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn payload_key(addon: &NativeAddonModule) -> String {
    let digest = Sha256::digest(addon.logical_id.as_bytes());
    perry_hex::encode(&digest[..8])
}

/// Keep runtime data and shared libraries, including versioned filenames. Only
/// recognizable development files and unselected prebuilds are omitted. A
/// project can retain unusual runtime inputs via perry.nativeAddonFiles.
pub(super) fn addon_payload_files(
    ctx: &CompilationContext,
    addon: &NativeAddonModule,
) -> Result<Vec<PathBuf>> {
    if !addon.ship_package_payload {
        return Ok(vec![addon.source_path.clone()]);
    }
    let manifest_path = ctx
        .project_root
        .ancestors()
        .map(|directory| directory.join("package.json"))
        .find(|path| path.is_file());
    let manifest: serde_json::Value = if let Some(manifest_path) = manifest_path {
        serde_json::from_slice(&fs::read(&manifest_path)?)?
    } else {
        serde_json::Value::Null
    };
    let extras = manifest.pointer("/perry/nativeAddonFiles");
    let mut retained = Vec::new();
    if let Some(extras) = extras {
        let extras = extras.as_object().ok_or_else(|| {
            anyhow!(
                "perry.nativeAddonFiles must map package names to arrays of package-relative paths"
            )
        })?;
        if let Some(paths) = extras.get(&addon.package) {
            let paths = paths.as_array().ok_or_else(|| {
                anyhow!("perry.nativeAddonFiles[{}] must be an array", addon.package)
            })?;
            for path in paths {
                let path = path.as_str().ok_or_else(|| {
                    anyhow!(
                        "perry.nativeAddonFiles[{}] paths must be strings",
                        addon.package
                    )
                })?;
                let path = Path::new(path);
                if path.as_os_str().is_empty()
                    || path.components().any(|part| {
                        !matches!(
                            part,
                            std::path::Component::Normal(_) | std::path::Component::CurDir
                        )
                    })
                {
                    anyhow::bail!(
                        "nativeAddonFiles path must remain inside {}: {}",
                        addon.package,
                        path.display()
                    );
                }
                if path.components().any(|part| matches!(part, std::path::Component::Normal(name) if name == "node_modules" || name == ".git")) {
                    anyhow::bail!("nativeAddonFiles cannot include node_modules or .git: {}", path.display());
                }
                let source = addon.package_dir.join(path);
                let canonical = source.canonicalize().with_context(|| {
                    format!("missing nativeAddonFiles input {}", source.display())
                })?;
                if !canonical.starts_with(addon.package_dir.canonicalize()?) {
                    anyhow::bail!(
                        "nativeAddonFiles path escapes {}: {}",
                        addon.package,
                        path.display()
                    );
                }
                retained.push(source);
            }
        }
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&addon.package_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            name != "node_modules" && name != ".git"
        })
    {
        let path = entry?.into_path();
        if !path.is_file() {
            continue;
        }
        let relative = path.strip_prefix(&addon.package_dir)?;
        if path == addon.source_path
            || retained.iter().any(|extra| path.starts_with(extra))
            || runtime_payload_file(relative, &addon.entry_relative)
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn runtime_payload_file(relative: &Path, entry: &Path) -> bool {
    // prebuilds/<platform>/... is node-gyp-build's selected payload. Flat
    // prebuilds/<platform>.node layouts select just the entry binary.
    if relative.starts_with("prebuilds") {
        if entry.starts_with("prebuilds") {
            let selected = entry.components().nth(1);
            if entry.components().count() > 2 && relative.components().nth(1) != selected {
                return false;
            }
        }
        if entry.starts_with("prebuilds") && entry.components().count() == 2 {
            let selected = entry
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let candidate = relative
                .components()
                .nth(1)
                .and_then(|component| component.as_os_str().to_str())
                .unwrap_or_default();
            let candidate = Path::new(candidate)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(candidate);
            let known_platform = [
                "linux-",
                "linuxmusl-",
                "darwin-",
                "win32-",
                "freebsd-",
                "openbsd-",
                "netbsd-",
                "android-",
                "sunos-",
                "aix-",
            ]
            .iter()
            .any(|prefix| candidate.starts_with(prefix));
            if known_platform
                && candidate != selected
                && !candidate.starts_with(&format!("{selected}-"))
            {
                return false;
            }
        }
        if (!entry.starts_with("prebuilds") || entry.components().count() == 2)
            && relative
                .extension()
                .is_some_and(|extension| extension == "node")
            && relative != entry
        {
            return false;
        }
    }
    if relative.starts_with(".github") {
        return false;
    }
    let name = relative.file_name().unwrap_or_default().to_string_lossy();
    // Preserve redistribution licenses and unknown data, even under src/deps.
    if name.starts_with("LICENSE") || name.starts_with("COPYING") || name.starts_with("NOTICE") {
        return true;
    }
    if matches!(
        name.as_ref(),
        "binding.gyp" | "Dockerfile" | "Dockerfile-alpine" | "Makefile" | "CMakeLists.txt"
    ) {
        return false;
    }
    !matches!(
        relative
            .extension()
            .and_then(|extension| extension.to_str()),
        Some(
            "c" | "cc"
                | "cpp"
                | "cxx"
                | "h"
                | "hh"
                | "hpp"
                | "gyp"
                | "gypi"
                | "md"
                | "markdown"
                | "ts"
                | "cts"
                | "mts"
                | "js"
                | "cjs"
                | "mjs"
                | "sh"
                | "bat"
                | "ps1"
                | "patch"
                | "map"
        )
    )
}

/// Preserve platform copying behavior (permissions, attributes and efficient
/// kernel copies), then authenticate the staged bytes with bounded memory.
fn copy_and_hash_file(source: &Path, destination: &Path) -> Result<(String, u64)> {
    fs::copy(source, destination)?;
    let mut input = fs::File::open(destination)?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let length = input.metadata()?.len();
    let mut buffer = vec![0u8; length.clamp(1, 256 * 1024) as usize];
    loop {
        let count = match input.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((perry_hex::encode(hash.finalize()), size))
}

fn target_tuple(target: Option<&str>) -> String {
    rust_target_triple(target)
        .or_else(host_target_triple)
        .unwrap_or("unknown-host")
        .to_string()
}

pub(super) fn stage_native_addon_sidecar(
    ctx: &CompilationContext,
    executable: &Path,
    target: Option<&str>,
) -> Result<Option<PathBuf>> {
    if ctx.native_addons.is_empty() {
        return Ok(None);
    }
    let root = sidecar_root(executable)?;
    let temporary = root.with_extension(format!("perry-native.tmp-{}", std::process::id()));
    if temporary.exists() {
        fs::remove_dir_all(&temporary).with_context(|| {
            format!(
                "remove stale Node-API staging directory {}",
                temporary.display()
            )
        })?;
    }
    fs::create_dir_all(&temporary)?;

    let mut manifest_addons = Vec::new();
    for addon in ctx.native_addons.values() {
        let prefix = payload_key(addon);
        let mut files = Vec::new();
        let mut copied = BTreeSet::new();
        for source in addon_payload_files(ctx, addon)? {
            let relative = source.strip_prefix(&addon.package_dir).with_context(|| {
                format!(
                    "payload {} is outside package {}",
                    source.display(),
                    addon.package_dir.display()
                )
            })?;
            let destination_relative = PathBuf::from(&prefix).join(relative);
            if !copied.insert(destination_relative.clone()) {
                continue;
            }
            let destination = temporary.join(&destination_relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let (sha256, size) = copy_and_hash_file(&source, &destination).with_context(|| {
                format!(
                    "copy Node-API payload {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
            files.push(ManifestFile {
                path: portable_path(&destination_relative),
                sha256,
                size,
            });
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let entry = PathBuf::from(&prefix).join(&addon.entry_relative);
        if !temporary.join(&entry).is_file() {
            anyhow::bail!(
                "Node-API entry {} was not included in its sidecar payload",
                addon.source_path.display()
            );
        }
        manifest_addons.push(ManifestAddon {
            logical_id: addon.logical_id.clone(),
            require_aliases: package_entry_aliases(addon),
            package: addon.package.clone(),
            version: addon.version.clone(),
            entry: portable_path(&entry),
            files,
        });
    }
    manifest_addons.sort_by(|left, right| left.logical_id.cmp(&right.logical_id));
    let manifest = SidecarManifest {
        schema_version: NODE_API_POLICY_VERSION,
        policy_version: NODE_API_POLICY_VERSION,
        napi_version: NODE_API_VERSION,
        shipping_model: SHIPPING_MODEL,
        target: target_tuple(target),
        allowlist: ctx.native_addon_packages.iter().cloned().collect(),
        path_allowlist: ctx.native_addon_paths.values().cloned().collect(),
        addons: manifest_addons,
    };
    fs::write(
        temporary.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("replace Node-API sidecar {}", root.display()))?;
    }
    fs::rename(&temporary, &root)
        .with_context(|| format!("publish Node-API sidecar {}", root.display()))?;
    Ok(Some(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::compile::NativeAddonModule;

    #[test]
    fn staged_manifest_is_relocatable_hashed_and_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("node_modules/demo");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            r#"{"name":"demo","version":"1.0.0"}"#,
        )
        .unwrap();
        let entry = package.join("demo.node");
        fs::write(&entry, b"native payload").unwrap();
        fs::write(package.join("table.dat"), b"runtime data").unwrap();
        let output = dir
            .path()
            .join(if cfg!(windows) { "app.exe" } else { "app" });
        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.native_addon_packages.insert("demo".to_string());
        ctx.native_addons.insert(
            "demo/demo.node".to_string(),
            NativeAddonModule {
                logical_id: "demo/demo.node".to_string(),
                package: "demo".to_string(),
                version: "1.0.0".to_string(),
                source_path: entry,
                package_dir: package,
                entry_relative: PathBuf::from("demo.node"),
                ship_package_payload: true,
            },
        );
        let root = stage_native_addon_sidecar(&ctx, &output, None)
            .unwrap()
            .unwrap();
        let first = fs::read(root.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(manifest["shipping_model"], SHIPPING_MODEL);
        assert_eq!(manifest["napi_version"], NODE_API_VERSION);
        assert_eq!(manifest["addons"][0]["logical_id"], "demo/demo.node");
        assert!(manifest["addons"][0]["files"].as_array().unwrap().len() >= 3);
        assert!(!String::from_utf8_lossy(&first).contains(&dir.path().display().to_string()));

        stage_native_addon_sidecar(&ctx, &output, None).unwrap();
        let second = fs::read(root.join("manifest.json")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn payload_prunes_development_and_foreign_prebuilds_but_keeps_runtime_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("node_modules/demo");
        let kept = [
            "prebuilds/linux-x64/addon.node",
            "prebuilds/linux-x64/helper.node",
            "prebuilds/linux-x64/libdependency.so.2",
            "deps/table.dat",
            "data/settings.json",
            "data/settings.yaml",
            "LICENSE",
            "package.json",
        ];
        assert!(!runtime_payload_file(
            Path::new("prebuilds/win32-x64/library.dll"),
            Path::new("prebuilds/linux-x64.node")
        ));
        assert!(!runtime_payload_file(
            Path::new("prebuilds/darwin-arm64.dylib"),
            Path::new("prebuilds/linux-x64.node")
        ));
        assert!(runtime_payload_file(
            Path::new("prebuilds/common/table.dat"),
            Path::new("prebuilds/linux-x64.node")
        ));
        assert!(runtime_payload_file(
            Path::new("prebuilds/linux-x64/libfoo.so.2"),
            Path::new("prebuilds/linux-x64.node")
        ));
        let dropped = [
            "prebuilds/darwin-arm64/addon.node",
            "prebuilds/win32-x64/lib.dll",
            "src/addon.cc",
            "deps/sqlite3.c",
            "test/suite.js",
            ".github/workflows/ci.yml",
            "binding.gyp",
            "Dockerfile",
            "README.md",
            "lib/wrapper.js",
        ];
        for path in kept.iter().chain(dropped.iter()) {
            let path = package.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"fixture").unwrap();
        }
        let addon = NativeAddonModule {
            logical_id: "demo/prebuilds/linux-x64/addon.node".into(),
            package: "demo".into(),
            version: "1".into(),
            source_path: package.join(kept[0]),
            package_dir: package.clone(),
            entry_relative: kept[0].into(),
            ship_package_payload: true,
        };
        let ctx = CompilationContext::new(dir.path().to_path_buf());
        let actual = addon_payload_files(&ctx, &addon)
            .unwrap()
            .into_iter()
            .map(|path| portable_path(path.strip_prefix(&package).unwrap()))
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, kept.iter().map(|path| path.to_string()).collect());
        fs::write(
            dir.path().join("package.json"),
            r#"{"perry":{"nativeAddonFiles":{"demo":["test/suite.js","src"]}}}"#,
        )
        .unwrap();
        let retained = addon_payload_files(&ctx, &addon).unwrap();
        assert!(retained.contains(&package.join("test/suite.js")));
        assert!(retained.contains(&package.join("src/addon.cc")));
        fs::write(
            dir.path().join("package.json"),
            r#"{"perry":{"nativeAddonFiles":{"demo":["../escape"]}}}"#,
        )
        .unwrap();
        assert!(addon_payload_files(&ctx, &addon)
            .unwrap_err()
            .to_string()
            .contains("inside"));
        fs::write(
            dir.path().join("package.json"),
            r#"{"perry":{"nativeAddonFiles":{"demo":["missing.dat"]}}}"#,
        )
        .unwrap();
        assert!(addon_payload_files(&ctx, &addon)
            .unwrap_err()
            .to_string()
            .contains("missing"));
    }

    #[test]
    fn package_entry_aliases_preserve_js_and_exports_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().canonicalize().unwrap();
        let entry = package.join("index.node");
        fs::write(&entry, "fixture").unwrap();
        let addon = NativeAddonModule {
            logical_id: "demo/index.node".into(),
            package: "demo".into(),
            version: "1".into(),
            source_path: entry,
            package_dir: package.clone(),
            entry_relative: "index.node".into(),
            ship_package_payload: true,
        };
        fs::write(package.join("package.json"), r#"{"name":"demo"}"#).unwrap();
        assert_eq!(package_entry_aliases(&addon), vec!["demo"]);
        fs::write(package.join("index.js"), "module.exports = {};").unwrap();
        assert!(package_entry_aliases(&addon).is_empty());
        fs::write(
            package.join("package.json"),
            r#"{"name":"demo","main":"index.node"}"#,
        )
        .unwrap();
        assert_eq!(package_entry_aliases(&addon), vec!["demo"]);
        fs::write(
            package.join("package.json"),
            r#"{"name":"demo","main":"index.node","exports":"./index.js"}"#,
        )
        .unwrap();
        assert!(package_entry_aliases(&addon).is_empty());
    }

    #[test]
    fn streaming_copy_hashes_all_chunks_and_preserves_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("large.dat");
        let destination = dir.path().join("copy.dat");
        let bytes = (0..(3 * 256 * 1024 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        fs::write(&source, &bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&source, fs::Permissions::from_mode(0o744)).unwrap();
        }
        #[cfg(target_os = "macos")]
        assert!(std::process::Command::new("xattr")
            .args(["-w", "com.perry.payload-test", "retained"])
            .arg(&source)
            .status()
            .unwrap()
            .success());
        let (hash, size) = copy_and_hash_file(&source, &destination).unwrap();
        assert_eq!(size, bytes.len() as u64);
        assert_eq!(hash, perry_hex::encode(Sha256::digest(&bytes)));
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
                0o744
            );
        }
        #[cfg(target_os = "macos")]
        {
            let attribute = std::process::Command::new("xattr")
                .args(["-p", "com.perry.payload-test"])
                .arg(&destination)
                .output()
                .unwrap();
            assert!(attribute.status.success());
            assert_eq!(
                String::from_utf8_lossy(&attribute.stdout).trim(),
                "retained"
            );
        }
    }

    #[test]
    fn macos_bundle_sidecar_is_staged_under_frameworks() {
        let executable = Path::new("Demo.app/Contents/MacOS/demo");
        assert_eq!(
            sidecar_root(executable).unwrap(),
            Path::new("Demo.app/Contents/Frameworks/demo.perry-native")
        );
    }
}
