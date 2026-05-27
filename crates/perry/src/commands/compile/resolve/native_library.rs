use std::path::Path;

use anyhow::{anyhow, Result};

use super::{parse_native_library_functions, parse_target_native_config};

pub(crate) fn validate_native_library_manifest_value(
    package_dir: &Path,
    module_name: &str,
    native_lib: &serde_json::Value,
) -> Result<()> {
    let package_json = package_dir.join("package.json");
    parse_native_library_functions(&package_json, native_lib)?;
    let Some(targets) = native_lib.get("targets") else {
        return Ok(());
    };
    let Some(targets_obj) = targets.as_object() else {
        return Err(anyhow!(
            "native library `{}` has invalid `perry.nativeLibrary.targets`: expected object",
            module_name
        ));
    };
    for (target_key, value) in targets_obj {
        let base_target = base_target_key(target_key).ok_or_else(|| {
            anyhow!(
                "native library `{}` has unsupported target key `perry.nativeLibrary.targets.{}`. \
                 Expected macos, ios, linux, windows, android, web, harmonyos, tvos, watchos, \
                 visionos, or a supported per-arch key such as macos-arm64.",
                module_name,
                target_key
            )
        })?;
        parse_target_native_config(
            package_dir,
            module_name,
            base_target,
            &format!("perry.nativeLibrary.targets.{target_key}"),
            value,
        )?;
    }
    Ok(())
}

pub(super) fn native_manifest_target_key(target: Option<&str>) -> &'static str {
    match target {
        Some("ios-simulator") | Some("ios") => "ios",
        Some("visionos-simulator") | Some("visionos") => "visionos",
        Some("android") => "android",
        Some("tvos-simulator") | Some("tvos") => "tvos",
        Some("watchos-simulator") | Some("watchos") => "watchos",
        Some("harmonyos-simulator") | Some("harmonyos") => "harmonyos",
        Some("linux") => "linux",
        Some("windows") => "windows",
        Some("web") => "web",
        Some("macos") => "macos",
        None if cfg!(target_os = "linux") => "linux",
        None if cfg!(target_os = "windows") => "windows",
        _ => "macos",
    }
}

fn base_target_key(target_key: &str) -> Option<&str> {
    const BASES: &[&str] = &[
        "macos",
        "ios",
        "linux",
        "windows",
        "android",
        "web",
        "harmonyos",
        "tvos",
        "watchos",
        "visionos",
    ];
    if BASES.contains(&target_key) {
        return Some(target_key);
    }
    for base in BASES {
        if let Some(suffix) = target_key.strip_prefix(&format!("{base}-")) {
            if matches!(
                suffix,
                "arm64" | "aarch64" | "x64" | "x86_64" | "ia32" | "i686"
            ) {
                return Some(base);
            }
        }
    }
    None
}
