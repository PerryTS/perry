//! Per-target resolution of the entry-point file and bundle id from
//! `perry.toml`. Extracted from `mod.rs` to keep it under the file-size gate.

use super::config_types::PerryToml;

/// Resolve the entry-point source file for the target platform, falling back
/// to `[app]`/`[project]` `entry` and finally a per-platform default.
pub(super) fn resolve_entry(
    config: &PerryToml,
    is_ios: bool,
    is_visionos: bool,
    is_tvos: bool,
    is_watchos: bool,
    is_android: bool,
) -> String {
    if is_android {
        config
            .android
            .as_ref()
            .and_then(|a| a.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    } else if is_ios {
        config
            .ios
            .as_ref()
            .and_then(|i| i.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_ios.ts".into())
    } else if is_visionos {
        config
            .visionos
            .as_ref()
            .and_then(|i| i.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_visionos.ts".into())
    } else if is_tvos {
        config
            .tvos
            .as_ref()
            .and_then(|t| t.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_tvos.ts".into())
    } else if is_watchos {
        config
            .watchos
            .as_ref()
            .and_then(|w| w.entry.clone())
            .or_else(|| config.app.as_ref().and_then(|a| a.entry.clone()))
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main_watchos.ts".into())
    } else {
        config
            .app
            .as_ref()
            .and_then(|a| a.entry.clone())
            .or_else(|| config.project.as_ref().and_then(|p| p.entry.clone()))
            .unwrap_or_else(|| "src/main.ts".into())
    }
}

/// Resolve the bundle id for the target platform. The `else` arm covers
/// macOS/Linux/Windows/Web.
pub(super) fn resolve_bundle_id(
    config: &PerryToml,
    app_name: &str,
    app_bundle_id: &Option<String>,
    project_bundle_id: &Option<String>,
    is_ios: bool,
    is_visionos: bool,
    is_tvos: bool,
    is_watchos: bool,
    is_android: bool,
) -> String {
    let default = || format!("com.perry.{}", app_name.to_lowercase().replace(' ', "-"));
    if is_android {
        config
            .android
            .as_ref()
            .and_then(|a| a.package_name.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .unwrap_or_else(default)
    } else if is_ios {
        config
            .ios
            .as_ref()
            .and_then(|i| i.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(default)
    } else if is_visionos {
        config
            .visionos
            .as_ref()
            .and_then(|i| i.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .or_else(|| config.macos.as_ref().and_then(|m| m.bundle_id.clone()))
            .unwrap_or_else(default)
    } else if is_tvos {
        config
            .tvos
            .as_ref()
            .and_then(|t| t.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .or_else(|| config.ios.as_ref().and_then(|i| i.bundle_id.clone()))
            .unwrap_or_else(default)
    } else if is_watchos {
        // A standalone watchOS app must have its OWN unique bundle id — do NOT
        // fall back to the iOS app's id (App Store Connect rejects duplicates).
        // The appstore/testflight preflight in mod.rs hard-requires [watchos] bundle_id.
        config
            .watchos
            .as_ref()
            .and_then(|w| w.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .unwrap_or_else(default)
    } else {
        config
            .macos
            .as_ref()
            .and_then(|m| m.bundle_id.clone())
            .or_else(|| app_bundle_id.clone())
            .or_else(|| project_bundle_id.clone())
            .unwrap_or_else(default)
    }
}
