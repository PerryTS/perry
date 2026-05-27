//! Per-target Apple `.app` bundle writers — watchOS, tvOS, visionOS.
//!
//! Extracted from `compile.rs` for issue #1105 PR 3 (directory
//! split). The orchestrator's bundle dispatch reduces to a one-line
//! call per branch; per-platform Info.plist + asset-copy + i18n
//! logic lives here.
//!
//! iOS and macOS bundle writers remain in `compile.rs` for now —
//! their bodies are still inlined and need their own extraction PR.

use crate::OutputFormat;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use super::i18n_emit::write_lproj_localized_strings;
use super::resources::{
    copy_bundle_resource_dirs, find_project_root_for_resources, stage_native_library_artifacts,
};
use super::targets::{compile_metallib_for_bundle, lookup_bundle_id_from_toml};
use super::CompilationContext;

/// Create a watchOS `.app` bundle: copy the linked binary into the
/// bundle, write `Info.plist` (CFBundleExecutable / CFBundleIdentifier
/// / WKApplication), copy project asset directories, then run the
/// shared metallib compile for any `.metal` sources.
///
/// Returns `(app_dir, bundle_id)` so the caller can wire them into
/// the final `CompileResult` (`result_app_dir`, `result_bundle_id`).
pub(super) fn bundle_for_watchos(
    exe_path: &Path,
    stem: &str,
    target: Option<&str>,
    input: &Path,
    ctx: &CompilationContext,
    format: OutputFormat,
) -> Result<(PathBuf, String)> {
    let app_dir = exe_path.with_extension("app");
    let _ = fs::create_dir_all(&app_dir);
    let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
    fs::copy(exe_path, &bundle_exe)?;
    let _ = fs::remove_file(exe_path);

    let exe_stem = exe_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(stem);
    let bundle_id = lookup_bundle_id_from_toml(input, "watchos")
        .or_else(|| lookup_bundle_id_from_toml(input, "app"))
        .unwrap_or_else(|| crate::commands::sanitize::default_perry_bundle_id(exe_stem));

    let info_plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>10.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>4</integer>
    </array>
    <key>WKApplication</key>
    <true/>
    <key>WKWatchOnly</key>
    <true/>
</dict>
</plist>"#
    );
    fs::write(app_dir.join("Info.plist"), info_plist)?;

    // Copy project resource directories into the bundle so
    // bloom_load_texture / load_sound / read_file can resolve relative
    // asset paths via [[NSBundle mainBundle] resourcePath].
    if let Some(src_dir) = input
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        let project_root = find_project_root_for_resources(&src_dir, true);
        copy_bundle_resource_dirs(&project_root, &app_dir);
    }

    compile_metallib_for_bundle(ctx, target, &app_dir, format)?;
    stage_native_library_artifacts(ctx, &app_dir, format)?;

    match format {
        OutputFormat::Text => {
            println!("Wrote watchOS app bundle: {}", app_dir.display());
            println!();
            println!("To run on Apple Watch Simulator:");
            println!("  xcrun simctl install booted {}", app_dir.display());
            println!("  xcrun simctl launch booted {}", bundle_id);
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "output": app_dir.to_string_lossy(),
                "bundle_id": bundle_id,
                "native_modules": ctx.native_modules.len(),
                "js_modules": ctx.js_modules.len(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok((app_dir, bundle_id))
}

/// Create a tvOS `.app` bundle. Same shape as `bundle_for_watchos`
/// but with the Apple TV `UIDeviceFamily` (3), `MinimumOSVersion`
/// 17.0, and a UI-thread principal class of `BloomApplication` (the
/// runtime's UIApplication subclass on tvOS). No project-resource
/// copy in this path — tvOS apps consume assets via the metallib /
/// embedded JS bundle.
pub(super) fn bundle_for_tvos(
    exe_path: &Path,
    stem: &str,
    target: Option<&str>,
    input: &Path,
    ctx: &CompilationContext,
    format: OutputFormat,
) -> Result<(PathBuf, String)> {
    let app_dir = exe_path.with_extension("app");
    let _ = fs::create_dir_all(&app_dir);
    let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
    fs::copy(exe_path, &bundle_exe)?;
    let _ = fs::remove_file(exe_path);

    let exe_stem = exe_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(stem);
    let bundle_id = lookup_bundle_id_from_toml(input, "tvos")
        .or_else(|| lookup_bundle_id_from_toml(input, "app"))
        .unwrap_or_else(|| crate::commands::sanitize::default_perry_bundle_id(exe_stem));

    let info_plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>3</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIRequiresFullScreen</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>BloomApplication</string>
</dict>
</plist>"#
    );
    fs::write(app_dir.join("Info.plist"), info_plist)?;

    compile_metallib_for_bundle(ctx, target, &app_dir, format)?;
    stage_native_library_artifacts(ctx, &app_dir, format)?;

    match format {
        OutputFormat::Text => {
            println!("Wrote tvOS app bundle: {}", app_dir.display());
            println!();
            println!("To run on Apple TV Simulator:");
            println!("  xcrun simctl install booted {}", app_dir.display());
            println!("  xcrun simctl launch booted {}", bundle_id);
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "output": app_dir.to_string_lossy(),
                "bundle_id": bundle_id,
                "native_modules": ctx.native_modules.len(),
                "js_modules": ctx.js_modules.len(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok((app_dir, bundle_id))
}

/// Walk up from the input file looking for `perry.toml` and harvest
/// visionOS-relevant metadata: project version + build number, the
/// `[visionos] deployment_target` (falling back to `minimum_version`,
/// or `"1.0"`), the optional `encryption_exempt` flag, and any
/// `[visionos.info_plist]` key/value pairs serialized into a plist
/// fragment ready to splice into the dict.
///
/// Returns sensible defaults when no `perry.toml` is found.
fn read_visionos_app_metadata(input: &Path) -> (String, String, String, Option<bool>, String) {
    let walk = (|| -> Option<(String, String, String, Option<bool>, String)> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if !toml_path.exists() {
                continue;
            }
            let data = fs::read_to_string(&toml_path).ok()?;
            let doc: toml::Table = data.parse().ok()?;
            let project = doc.get("project").and_then(|v| v.as_table());
            let visionos = doc.get("visionos").and_then(|v| v.as_table());
            let version = project
                .and_then(|p| p.get("version"))
                .and_then(|v| v.as_str())
                .unwrap_or("1.0.0")
                .to_string();
            let build_number = project
                .and_then(|p| p.get("build_number"))
                .and_then(|v| {
                    v.as_integer()
                        .map(|n| n.to_string())
                        .or_else(|| v.as_str().map(|s| s.to_string()))
                })
                .unwrap_or_else(|| "1".to_string());
            let deployment_target = visionos
                .and_then(|v| {
                    v.get("deployment_target")
                        .or_else(|| v.get("minimum_version"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("1.0")
                .to_string();
            let encryption_exempt = visionos
                .and_then(|v| v.get("encryption_exempt"))
                .and_then(|v| v.as_bool());
            let mut entries = String::new();
            if let Some(info_plist) = visionos
                .and_then(|v| v.get("info_plist"))
                .and_then(|v| v.as_table())
            {
                for (key, value) in info_plist {
                    if let Some(s) = value.as_str() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <string>{}</string>\n",
                            key, s
                        ));
                    } else if let Some(b) = value.as_bool() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <{}/>\n",
                            key,
                            if b { "true" } else { "false" }
                        ));
                    } else if let Some(i) = value.as_integer() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <integer>{}</integer>\n",
                            key, i
                        ));
                    }
                }
            }
            return Some((
                version,
                build_number,
                deployment_target,
                encryption_exempt,
                entries,
            ));
        }
        Some((
            "1.0.0".to_string(),
            "1".to_string(),
            "1.0".to_string(),
            None,
            String::new(),
        ))
    })();
    walk.unwrap_or_else(|| {
        (
            "1.0.0".to_string(),
            "1".to_string(),
            "1.0".to_string(),
            None,
            String::new(),
        )
    })
}

/// Create a visionOS `.app` bundle. Reads `perry.toml` for version +
/// build number + deployment target + encryption-exemption + custom
/// Info.plist entries, copies the linked binary, writes Info.plist
/// (with XRSimulator / XROS platform tag), copies project asset
/// directories, and writes per-locale `.lproj/Localizable.strings`
/// from the i18n table. Returns `(app_dir, bundle_id)`.
pub(super) fn bundle_for_visionos(
    exe_path: &Path,
    stem: &str,
    target: Option<&str>,
    input: &Path,
    ctx: &CompilationContext,
    i18n_table: Option<&perry_transform::i18n::I18nStringTable>,
    i18n_config: Option<&perry_transform::i18n::I18nConfig>,
    format: OutputFormat,
) -> Result<(PathBuf, String)> {
    let app_dir = exe_path.with_extension("app");
    let _ = fs::create_dir_all(&app_dir);
    let bundle_exe = app_dir.join(exe_path.file_name().unwrap_or_default());
    fs::copy(exe_path, &bundle_exe)?;
    let _ = fs::remove_file(exe_path);

    let exe_stem = exe_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(stem);
    let bundle_id = lookup_bundle_id_from_toml(input, "visionos")
        .or_else(|| lookup_bundle_id_from_toml(input, "app"))
        .or_else(|| lookup_bundle_id_from_toml(input, "ios"))
        .unwrap_or_else(|| crate::commands::sanitize::default_perry_bundle_id(exe_stem));

    let (app_version, app_build_number, deployment_target, encryption_exempt, custom_plist_entries) =
        read_visionos_app_metadata(input);

    let platform_name = if target == Some("visionos-simulator") {
        "XRSimulator"
    } else {
        "XROS"
    };

    let mut info_plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{exe_stem}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{exe_stem}</string>
    <key>CFBundleVersion</key>
    <string>{app_build_number}</string>
    <key>CFBundleShortVersionString</key>
    <string>{app_version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>MinimumOSVersion</key>
    <string>{deployment_target}</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>{platform_name}</string>
    </array>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
    </array>
    <key>UIDeviceFamily</key>
    <array>
        <integer>7</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UIApplicationSupportsMultipleScenes</key>
        <true/>
        <key>UIApplicationPreferredDefaultSceneSessionRole</key>
        <string>UIWindowSceneSessionRoleApplication</string>
        <key>UISceneConfigurations</key>
        <dict/>
    </dict>
</dict>
</plist>"#
    );

    let usage_descriptions = concat!(
        "    <key>NSCameraUsageDescription</key>\n",
        "    <string>This app uses the camera to identify colors.</string>\n",
        "    <key>NSMicrophoneUsageDescription</key>\n",
        "    <string>This app uses the microphone to measure sound levels.</string>\n",
    );
    info_plist = info_plist.replace(
        "</dict>\n</plist>",
        &format!("{}</dict>\n</plist>", usage_descriptions),
    );

    if let Some(exempt) = encryption_exempt {
        let encryption_entry = format!(
            "    <key>ITSAppUsesNonExemptEncryption</key>\n    <{}/>\n",
            if exempt { "false" } else { "true" }
        );
        info_plist = info_plist.replace(
            "</dict>\n</plist>",
            &format!("{}</dict>\n</plist>", encryption_entry),
        );
    }

    if !custom_plist_entries.is_empty() {
        info_plist = info_plist.replace(
            "</dict>\n</plist>",
            &format!("{}</dict>\n</plist>", custom_plist_entries),
        );
    }

    fs::write(app_dir.join("Info.plist"), info_plist)?;

    if let Some(src_dir) = input
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        let project_root = find_project_root_for_resources(&src_dir, true);
        copy_bundle_resource_dirs(&project_root, &app_dir);
    }
    compile_metallib_for_bundle(ctx, target, &app_dir, format)?;
    stage_native_library_artifacts(ctx, &app_dir, format)?;

    write_lproj_localized_strings(&app_dir, i18n_table, i18n_config);

    match format {
        OutputFormat::Text => {
            println!("Wrote visionOS app bundle: {}", app_dir.display());
            println!();
            println!("To run on Apple Vision Pro Simulator:");
            println!("  xcrun simctl install booted {}", app_dir.display());
            println!("  xcrun simctl launch booted {}", bundle_id);
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "output": app_dir.to_string_lossy(),
                "bundle_id": bundle_id,
                "native_modules": ctx.native_modules.len(),
                "js_modules": ctx.js_modules.len(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok((app_dir, bundle_id))
}
