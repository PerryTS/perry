use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{host_target_triple, rust_target_triple, CompilationContext, NativeAddonModule};

pub(super) const NODE_API_POLICY_VERSION: u32 = 1;
pub(super) const NODE_API_VERSION: u32 = 8;
pub(super) const SHIPPING_MODEL: &str = "sidecar-v1";

#[derive(Serialize)]
struct SidecarManifest {
    schema_version: u32,
    policy_version: u32,
    napi_version: u32,
    shipping_model: &'static str,
    target: String,
    allowlist: Vec<String>,
    addons: Vec<ManifestAddon>,
}

#[derive(Serialize)]
struct ManifestAddon {
    logical_id: String,
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
    hex::encode(&digest[..8])
}

/// Every package-local file is shipped. Native addons sometimes open data
/// tables at runtime and their dependent dylibs can have versioned filenames;
/// extension filtering would silently produce a loader-success/runtime-fail
/// artifact. Nested node_modules and VCS state are separate packages, not
/// part of the selected platform payload.
pub(super) fn addon_payload_files(addon: &NativeAddonModule) -> Vec<PathBuf> {
    let mut files = walkdir::WalkDir::new(&addon.package_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            name != "node_modules" && name != ".git"
        })
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.into_path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
    let bytes =
        fs::read(path).with_context(|| format!("read native addon payload {}", path.display()))?;
    Ok((hex::encode(Sha256::digest(&bytes)), bytes.len() as u64))
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
        for source in addon_payload_files(addon) {
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
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "copy Node-API payload {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
            let (sha256, size) = hash_file(&destination)?;
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
    fn macos_bundle_sidecar_is_staged_under_frameworks() {
        let executable = Path::new("Demo.app/Contents/MacOS/demo");
        assert_eq!(
            sidecar_root(executable).unwrap(),
            Path::new("Demo.app/Contents/Frameworks/demo.perry-native")
        );
    }
}
