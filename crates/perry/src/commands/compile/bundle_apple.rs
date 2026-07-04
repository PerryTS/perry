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

// Public-GM DT* (SDK / Xcode toolchain marker) fallbacks, used when neither a
// `PERRY_DT_*` env override nor the cross sysroot supplies the value (#4849).
// Verified GM toolchain on the build/sign worker (Xcode 26.3 / 17C529, SDK
// 26.2) via `xcrun --show-sdk-build-version`. DTXcode/DTXcodeBuild are one
// Xcode, so they're shared; the SDK *build* string differs per platform
// (tvOS 23K50 ≠ iOS 23C57 ≠ macOS 25C58), hence the per-platform table.
const DEFAULT_DT_XCODE: &str = "2630";
const DEFAULT_DT_XCODE_BUILD: &str = "17C529";

/// Per-platform `(sdk_version, sdk_build)` GM fallbacks. `platform_name` is the
/// DTPlatformName (`appletvos` / `iphoneos` / `macosx` / simulator variants).
fn gm_dt_defaults(platform_name: &str) -> (&'static str, &'static str) {
    match platform_name {
        "appletvos" | "appletvsimulator" => ("26.2", "23K50"),
        "macosx" => ("26.2", "25C58"),
        // iphoneos / iphonesimulator (and any future device platform) default
        // to the iOS SDK build.
        _ => ("26.2", "23C57"),
    }
}

/// Read `[project].version` and `[project].build_number` from the nearest
/// `perry.toml` (walking up to 5 parents from `input`). Defaults to
/// `("1.0.0", "1")` when absent — matching `bundle_ios.rs`. Shared by all the
/// Apple bundlers so tvOS/watchOS stop shipping a hardcoded `1.0`
/// CFBundleVersion (which made every TestFlight re-upload fail the
/// "bundle version must be higher" check). #4849.
pub(super) fn read_apple_app_version(input: &Path) -> (String, String) {
    let read = (|| -> Option<(String, String)> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if !toml_path.exists() {
                continue;
            }
            let doc: toml::Table = fs::read_to_string(&toml_path).ok()?.parse().ok()?;
            let project = doc.get("project").and_then(|v| v.as_table());
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
            return Some((version, build_number));
        }
        None
    })();
    read.unwrap_or_else(|| ("1.0.0".to_string(), "1".to_string()))
}

/// Read the human-facing app display name from `perry.toml`, used for the
/// `CFBundleDisplayName` Info.plist key — the name shown under the icon on the
/// Home Screen. Prefers the platform's own `[<platform>].display_name` (e.g.
/// `[ios]`, `[tvos]`), falling back to `[project].display_name`. Returns `None`
/// when neither is set, in which case the bundler omits CFBundleDisplayName and
/// the Home Screen falls back to CFBundleName — i.e. the sanitized executable
/// stem, e.g. `bloom-jump` instead of the intended `Bloom Jump`.
pub(super) fn read_app_display_name(input: &Path, platform: &str) -> Option<String> {
    let mut dir = input.canonicalize().ok()?;
    for _ in 0..5 {
        dir = dir.parent()?.to_path_buf();
        let toml_path = dir.join("perry.toml");
        if !toml_path.exists() {
            continue;
        }
        let doc: toml::Table = fs::read_to_string(&toml_path).ok()?.parse().ok()?;
        let from_table = |name: &str| {
            doc.get(name)
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("display_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        // First perry.toml on the walk is the project manifest; stop here
        // whether or not it carries the key (mirrors read_apple_app_version).
        return from_table(platform).or_else(|| from_table("project"));
    }
    None
}

/// Minimal XML escape for text interpolated into an Info.plist `<string>`.
/// Display names are user-controlled (perry.toml), so an `&` or `<` would
/// otherwise produce a malformed plist that the bundle tooling rejects.
pub(super) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Read the SDK marketing version (e.g. `26.2`) from the cross sysroot's
/// `SDKSettings.json` so the plist DT* version matches the SDK that actually
/// links the binary (its Mach-O `LC_BUILD_VERSION` sdk field). Returns `None`
/// if the sysroot/JSON is unavailable. #4849.
fn sdk_version_from_sysroot(sysroot_env: &str, default_sysroot: &str) -> Option<String> {
    let sysroot = std::env::var(sysroot_env).unwrap_or_else(|_| default_sysroot.to_string());
    let data = fs::read_to_string(Path::new(&sysroot).join("SDKSettings.json")).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&data).ok()?;
    doc.get("Version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Build the DT* (SDK / Xcode toolchain marker) Info.plist key block for an
/// Apple bundle. These are what App Store Connect inspects to reject builds
/// made with a beta/non-GM SDK ("Apps built with beta versions aren't
/// allowed"). Resolution order per value:
///   1. `PERRY_DT_*` env override (the build worker pins its GM toolchain),
///   2. the cross sysroot's `SDKSettings.json` (version only),
///   3. a public-GM constant fallback.
///
/// `platform_name` is the DTPlatformName / DTSDKName prefix
/// (`iphoneos` / `appletvos` / `macosx` / `xros` / `watchos`). #4849.
pub(super) fn apple_dt_plist_block(
    platform_name: &str,
    sysroot_env: &str,
    default_sysroot: &str,
) -> String {
    let (default_version, default_sdk_build) = gm_dt_defaults(platform_name);
    let version = std::env::var("PERRY_DT_PLATFORM_VERSION")
        .ok()
        .or_else(|| sdk_version_from_sysroot(sysroot_env, default_sysroot))
        .unwrap_or_else(|| default_version.to_string());
    let sdk_build =
        std::env::var("PERRY_DT_SDK_BUILD").unwrap_or_else(|_| default_sdk_build.to_string());
    // The platform build usually equals the SDK build; allow a separate
    // override but default to the SDK build.
    let platform_build =
        std::env::var("PERRY_DT_PLATFORM_BUILD").unwrap_or_else(|_| sdk_build.clone());
    let dt_xcode = std::env::var("PERRY_DT_XCODE").unwrap_or_else(|_| DEFAULT_DT_XCODE.to_string());
    let dt_xcode_build = std::env::var("PERRY_DT_XCODE_BUILD")
        .unwrap_or_else(|_| DEFAULT_DT_XCODE_BUILD.to_string());
    format!(
        r#"    <key>DTPlatformName</key>
    <string>{platform_name}</string>
    <key>DTPlatformVersion</key>
    <string>{version}</string>
    <key>DTSDKName</key>
    <string>{platform_name}{version}</string>
    <key>DTPlatformBuild</key>
    <string>{platform_build}</string>
    <key>DTSDKBuild</key>
    <string>{sdk_build}</string>
    <key>DTXcode</key>
    <string>{dt_xcode}</string>
    <key>DTXcodeBuild</key>
    <string>{dt_xcode_build}</string>
    <key>DTCompiler</key>
    <string>com.apple.compilers.llvm.clang.1_0</string>
"#
    )
}

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

    // #4849: read version/build_number from perry.toml (was hardcoded "1.0").
    let (app_version, app_build_number) = read_apple_app_version(input);

    // Device builds default to arm64-only, which requires watchOS 26 (S9+).
    // arm64_32 device builds (PERRY_WATCHOS_ARM64_32) reach pre-S9 watches and
    // use a low floor (overridable via PERRY_WATCHOS_MIN); the simulator target
    // keeps the lower floor too.
    let arm64_32 = target == Some("watchos") && std::env::var("PERRY_WATCHOS_ARM64_32").is_ok();
    let min_os_owned;
    let min_os = if target == Some("watchos-simulator") {
        "10.0"
    } else if arm64_32 {
        min_os_owned = std::env::var("PERRY_WATCHOS_MIN").unwrap_or_else(|_| "11.0".to_string());
        min_os_owned.as_str()
    } else {
        "26.0"
    };

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
    <string>{app_build_number}</string>
    <key>CFBundleShortVersionString</key>
    <string>{app_version}</string>
    <key>MinimumOSVersion</key>
    <string>{min_os}</string>
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

    // Append custom Info.plist entries from [watchos.info_plist] in
    // perry.toml (same walk as the iOS bundler) — required for usage
    // descriptions like NSMicrophoneUsageDescription, without which
    // watchOS denies the permission request outright.
    let custom_plist_entries = (|| -> Option<String> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if toml_path.exists() {
                let data = fs::read_to_string(&toml_path).ok()?;
                let doc: toml::Table = data.parse().ok()?;
                let watchos = doc.get("watchos")?.as_table()?;
                let info_plist_table = watchos.get("info_plist")?.as_table()?;
                let mut entries = String::new();
                for (key, value) in info_plist_table {
                    if let Some(s) = value.as_str() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <string>{}</string>\n",
                            xml_escape(key),
                            xml_escape(s)
                        ));
                    } else if let Some(b) = value.as_bool() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <{}/>\n",
                            xml_escape(key),
                            if b { "true" } else { "false" }
                        ));
                    }
                }
                if !entries.is_empty() {
                    return Some(entries);
                }
            }
        }
        None
    })()
    .unwrap_or_default();
    let info_plist = if !custom_plist_entries.is_empty() {
        info_plist.replace(
            "</dict>\n</plist>",
            &format!("{}</dict>\n</plist>", custom_plist_entries),
        )
    } else {
        info_plist
    };
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
/// runtime's UIApplication subclass on tvOS). Project resource
/// directories (assets/levels/sounds/…) are copied into the bundle so
/// games that read files at runtime via bloom_read_file /
/// bloom_load_texture can resolve relative paths against the bundle's
/// resourcePath — same as the watchOS/visionOS/iOS paths.
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

    // #4849: read version/build_number from perry.toml (was hardcoded "1.0",
    // so every TestFlight re-upload was rejected with "bundle version must be
    // higher than the previously uploaded version: '1.0'").
    let (app_version, app_build_number) = read_apple_app_version(input);

    // #4849: tvOS previously emitted no DT* keys at all, so App Store Connect
    // rejected it ("must be built with the latest public GM SDK"). Emit the
    // shared DT* block (SDK version from the sysroot, build strings from
    // PERRY_DT_* env / GM fallback) and the CFBundleSupportedPlatforms /
    // package-type keys the iOS bundler already carries.
    let is_sim = matches!(target, Some("tvos-simulator"));
    let plist_supported_platform = if is_sim {
        "AppleTVSimulator"
    } else {
        "AppleTVOS"
    };
    let (dt_platform_name, dt_sysroot_env, dt_default_sysroot) = if is_sim {
        (
            "appletvsimulator",
            "PERRY_TVOS_SIMULATOR_SYSROOT",
            "/opt/apple-sysroot/tvos-simulator",
        )
    } else {
        ("appletvos", "PERRY_TVOS_SYSROOT", "/opt/apple-sysroot/tvos")
    };
    let dt_block = apple_dt_plist_block(dt_platform_name, dt_sysroot_env, dt_default_sysroot);

    // CFBundleDisplayName — name shown under the icon on the tvOS Home Screen.
    // Prefers [tvos]/[project] display_name; without it tvOS falls back to
    // CFBundleName (the executable stem, e.g. "bloom-jump").
    let display_name_block = read_app_display_name(input, "tvos")
        .map(|name| {
            format!(
                "    <key>CFBundleDisplayName</key>\n    <string>{}</string>\n",
                xml_escape(&name)
            )
        })
        .unwrap_or_default();

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
{display_name_block}    <key>CFBundleVersion</key>
    <string>{app_build_number}</string>
    <key>CFBundleShortVersionString</key>
    <string>{app_version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>{plist_supported_platform}</string>
    </array>
{dt_block}    <key>UIDeviceFamily</key>
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_apple_app_version_reads_project_table() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("perry.toml"),
            "[project]\nversion = \"3.2.1\"\nbuild_number = 47\n",
        )
        .unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let input = src.join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();

        let (version, build) = read_apple_app_version(&input);
        assert_eq!(version, "3.2.1");
        // build_number may be a TOML integer — must serialize to the string.
        assert_eq!(build, "47");
    }

    #[test]
    fn read_apple_app_version_defaults_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();
        assert_eq!(
            read_apple_app_version(&input),
            ("1.0.0".to_string(), "1".to_string())
        );
    }

    #[test]
    fn dt_block_reads_version_from_sysroot_json() {
        // A sysroot whose SDKSettings.json reports 26.2 must drive
        // DTPlatformVersion + the DTSDKName suffix (so the plist matches the
        // SDK that links the binary, not a stale hardcoded version).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("SDKSettings.json"),
            r#"{"Version":"26.2","CanonicalName":"appletvos26.2"}"#,
        )
        .unwrap();
        let block = apple_dt_plist_block(
            "appletvos",
            "PERRY_DT_TEST_SYSROOT_DOES_NOT_EXIST",
            dir.path().to_str().unwrap(),
        );
        assert!(
            block.contains("<key>DTPlatformVersion</key>\n    <string>26.2</string>"),
            "expected sysroot version 26.2 in:\n{block}"
        );
        assert!(
            block.contains("<key>DTSDKName</key>\n    <string>appletvos26.2</string>"),
            "expected DTSDKName appletvos26.2 in:\n{block}"
        );
        assert!(block.contains("<key>DTPlatformName</key>\n    <string>appletvos</string>"));
    }

    #[test]
    fn dt_block_falls_back_to_gm_constants_without_sysroot() {
        // No sysroot → per-platform GM fallbacks. tvOS SDK build (23K50) must
        // differ from iOS (23C57); the shared Xcode build is 17C529.
        let tvos = apple_dt_plist_block(
            "appletvos",
            "PERRY_DT_TEST_SYSROOT_DOES_NOT_EXIST",
            "/nonexistent/sysroot/path",
        );
        assert!(tvos.contains("<key>DTPlatformVersion</key>\n    <string>26.2</string>"));
        assert!(tvos.contains("<key>DTSDKBuild</key>\n    <string>23K50</string>"));
        assert!(tvos.contains(&format!(
            "<key>DTXcodeBuild</key>\n    <string>{DEFAULT_DT_XCODE_BUILD}</string>"
        )));

        let ios = apple_dt_plist_block(
            "iphoneos",
            "PERRY_DT_TEST_SYSROOT_DOES_NOT_EXIST",
            "/nonexistent/sysroot/path",
        );
        assert!(ios.contains("<key>DTSDKBuild</key>\n    <string>23C57</string>"));
    }
}
