//! iOS `.app` bundle assembly extracted from `run_with_parse_cache`.
//!
//! Resolves the bundle ID (CLI → perry.toml `[ios].bundle_id` → package.json
//! `bundleId` → `com.perry.<stem>` fallback), copies the linker output into
//! `<exe>.app/`, generates the Info.plist (with deep links, Google Auth, and
//! perry.toml `[ios.info_plist]` overrides spliced in), packs splash assets
//! and `.lproj` localization bundles, runs `compile_metallib_for_bundle`,
//! and embeds any declared SwiftUI widgets from `[[widget]]` entries.
//!
//! Returns `(app_dir, bundle_id)` to the orchestrator.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::OutputFormat;

use super::apple_info_plist::{
    inject_ads_info_plist, inject_app_group_entitlement, inject_google_auth_info_plist,
    inject_ios_deeplinks, inject_ios_push_entitlement,
};
use super::resources::stage_native_library_artifacts;
use super::targets::compile_metallib_for_bundle;
use super::widget_build;
use super::CompilationContext;

/// Build an iOS `.app` bundle. Takes individual `args` fields rather than
/// `&CompileArgs` because the orchestrator has already partial-moved
/// `args.output` by this point — see the feedback note in the file
/// header. The call site picks out exactly the fields the bundle code
/// touches.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_ios_app_bundle(
    input: &Path,
    app_bundle_id: Option<&str>,
    ctx: &CompilationContext,
    exe_path: &Path,
    stem: &str,
    target: Option<&str>,
    compiled_features: &[String],
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
    // Precedence: --app-bundle-id CLI flag > perry.toml [ios].bundle_id / [app]
    // / [project] / top-level > package.json "bundleId" > com.perry.{name}.
    // CLI wins so callers (doc-tests harness, CI, scripts) can override the
    // embedded ID without editing manifests; without this the app installs
    // under its fallback CFBundleIdentifier and a later `simctl launch
    // <custom-id>` fails with FBSOpenApplicationServiceErrorDomain code=4.
    let bundle_id = app_bundle_id
        .map(|s| s.to_string())
        .map(|raw| {
            // #999: CLI override goes straight to codesign argv.
            crate::commands::sanitize::validate_bundle_id_or_exit(&raw, "CLI --app-bundle-id")
        })
        .or_else(|| {
            (|| -> Option<String> {
                let mut dir = input.canonicalize().ok()?;
                for _ in 0..5 {
                    dir = dir.parent()?.to_path_buf();
                    // Check perry.toml first: [ios].bundle_id, then top-level bundle_id
                    let toml_path = dir.join("perry.toml");
                    if toml_path.exists() {
                        if let Ok(data) = fs::read_to_string(&toml_path) {
                            if let Ok(doc) = data.parse::<toml::Table>() {
                                let toml_bid = doc
                                    .get("ios")
                                    .and_then(|i| i.get("bundle_id"))
                                    .or_else(|| doc.get("app").and_then(|a| a.get("bundle_id")))
                                    .or_else(|| doc.get("project").and_then(|p| p.get("bundle_id")))
                                    .or_else(|| doc.get("bundle_id"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                if let Some(raw) = toml_bid {
                                    // #999: validate before letting toml-supplied IDs reach codesign.
                                    let label =
                                        format!("perry.toml bundle_id at {}", toml_path.display());
                                    return Some(
                                        crate::commands::sanitize::validate_bundle_id_or_exit(
                                            &raw, &label,
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    // Then check package.json
                    let pkg = dir.join("package.json");
                    if pkg.exists() {
                        let data = fs::read_to_string(&pkg).ok()?;
                        let idx = data.find("\"bundleId\"")?;
                        let colon = data[idx..].find(':')?;
                        let q1 = data[idx + colon..].find('"')? + idx + colon + 1;
                        let q2 = data[q1..].find('"')? + q1;
                        // #999: same as the toml path above — validate
                        // before this raw byte-sliced string reaches
                        // codesign argv.
                        let raw = &data[q1..q2];
                        let label = format!("package.json `bundleId` at {}", pkg.display());
                        return Some(crate::commands::sanitize::validate_bundle_id_or_exit(
                            raw, &label,
                        ));
                    }
                }
                None
            })()
        })
        .unwrap_or_else(|| {
            // #998: shared helper so iOS bundle IDs match the macOS
            // path (sanitized + lowercase-canonical). `exe_stem` is
            // already transitively safe (sourced from the upstream
            // sanitized `stem`); the helper still calls the
            // sanitizer for the lowercase mapping.
            crate::commands::sanitize::default_perry_bundle_id(exe_stem)
        });

    // Read perry.toml for version + build_number via the shared Apple helper
    // (#4849: same logic the watchOS/tvOS/visionOS bundlers use).
    let (app_version, app_build_number) = super::bundle_apple::read_apple_app_version(input);

    // CFBundleDisplayName — the name shown under the icon on the Home Screen.
    // Without it iOS falls back to CFBundleName (the executable stem), so a
    // project named `bloom-jump` shows up as "bloom-jump". When perry.toml sets
    // [ios]/[project] display_name, emit it so the Home Screen reads "Bloom Jump".
    let display_name_block = super::bundle_apple::read_app_display_name(input, "ios")
        .map(|name| {
            format!(
                "<key>CFBundleDisplayName</key>\n<string>{}</string>\n",
                super::bundle_apple::xml_escape(&name)
            )
        })
        .unwrap_or_default();

    let encryption_exempt_plist = (|| -> Option<String> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if toml_path.exists() {
                let data = fs::read_to_string(toml_path).ok()?;
                let doc: toml::Table = data.parse().ok()?;
                let ios = doc.get("ios")?.as_table()?;
                let exempt = ios.get("encryption_exempt")?.as_bool()?;
                if exempt {
                    return Some(
                        "    <key>ITSAppUsesNonExemptEncryption</key>\n    <false/>".into(),
                    );
                } else {
                    return Some(
                        "    <key>ITSAppUsesNonExemptEncryption</key>\n    <true/>".into(),
                    );
                }
            }
        }
        None
    })()
    .unwrap_or_default();

    // Game-loop apps use traditional UIApplicationMain lifecycle, not SceneDelegate.
    // Including UIApplicationSceneManifest causes a black screen with game-loop.
    let scene_manifest = if compiled_features.iter().any(|f| f == "ios-game-loop") {
        String::new()
    } else {
        r#"    <key>UIApplicationSceneManifest</key>
<dict>
    <key>UIApplicationSupportsMultipleScenes</key>
    <false/>
    <key>UISceneConfigurations</key>
    <dict>
        <key>UIWindowSceneSessionRoleApplication</key>
        <array>
            <dict>
                <key>UISceneConfigurationName</key>
                <string>Default Configuration</string>
                <key>UISceneDelegateClassName</key>
                <string>PerrySceneDelegate</string>
            </dict>
        </array>
    </dict>
</dict>
"#
        .to_string()
    };

    // Simulator bundles must declare iPhoneSimulator / iphonesimulator in
    // Info.plist. Mismatch against the Mach-O LC_BUILD_VERSION (which is
    // "iphonesimulator" when the binary was built for -target
    // aarch64-apple-ios-sim) causes simctl to refuse launch with
    // `FBSOpenApplicationServiceErrorDomain code=4`.
    let is_sim = matches!(target, Some("ios-simulator"));
    let plist_supported_platform = if is_sim {
        "iPhoneSimulator"
    } else {
        "iPhoneOS"
    };
    let plist_platform_name = if is_sim {
        "iphonesimulator"
    } else {
        "iphoneos"
    };
    // #4849: emit the shared DT* block (SDK version from the sysroot, build
    // strings from PERRY_DT_* env / GM fallback) instead of hardcoded beta SDK
    // markers (was 26.4 / 23E237, which is neither the SDK that links the
    // binary nor a current public-GM build).
    let (dt_sysroot_env, dt_default_sysroot) = if is_sim {
        (
            "PERRY_IOS_SIMULATOR_SYSROOT",
            "/opt/apple-sysroot/ios-simulator",
        )
    } else {
        ("PERRY_IOS_SYSROOT", "/opt/apple-sysroot/ios")
    };
    let dt_block = super::bundle_apple::apple_dt_plist_block(
        plist_platform_name,
        dt_sysroot_env,
        dt_default_sysroot,
    );
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
{display_name_block}<key>CFBundleVersion</key>
<string>{app_build_number}</string>
<key>CFBundleShortVersionString</key>
<string>{app_version}</string>
<key>CFBundlePackageType</key>
<string>APPL</string>
<key>CFBundleInfoDictionaryVersion</key>
<string>6.0</string>
<key>CFBundleIconName</key>
<string>AppIcon</string>
<key>MinimumOSVersion</key>
<string>17.0</string>
<key>CFBundleSupportedPlatforms</key>
<array><string>{plist_supported_platform}</string></array>
{dt_block}<key>UIRequiredDeviceCapabilities</key>
<array><string>arm64</string></array>
<key>CFBundleIcons</key>
<dict>
    <key>CFBundlePrimaryIcon</key>
    <dict>
        <key>CFBundleIconFiles</key>
        <array>
            <string>AppIcon60x60</string>
        </array>
    </dict>
</dict>
<key>CFBundleIcons~ipad</key>
<dict>
    <key>CFBundlePrimaryIcon</key>
    <dict>
        <key>CFBundleIconFiles</key>
        <array>
            <string>AppIcon76x76</string>
        </array>
    </dict>
</dict>
<key>UIDeviceFamily</key>
<array>
    <integer>1</integer>
    <integer>2</integer>
</array>
<key>UILaunchScreen</key>
<dict/>
<key>UISupportedInterfaceOrientations</key>
<array>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationPortraitUpsideDown</string>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
</array>
<key>UISupportedInterfaceOrientations~ipad</key>
<array>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationPortraitUpsideDown</string>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
</array>
{scene_manifest}</dict>
</plist>"#,
    );

    // Apply orientations from perry.toml [ios].orientations
    let info_plist = (|| -> Option<String> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if toml_path.exists() {
                let data = fs::read_to_string(&toml_path).ok()?;
                let doc: toml::Table = data.parse().ok()?;
                let ios = doc.get("ios")?.as_table()?;
                let orientations = ios.get("orientations")?.as_array()?;
                let mut entries = Vec::new();
                for o in orientations {
                    let s = o.as_str()?;
                    match s {
                        "landscape" => {
                            entries.push("UIInterfaceOrientationLandscapeLeft");
                            entries.push("UIInterfaceOrientationLandscapeRight");
                        }
                        "portrait" => {
                            entries.push("UIInterfaceOrientationPortrait");
                            entries.push("UIInterfaceOrientationPortraitUpsideDown");
                        }
                        other => {
                            // Allow raw UIInterfaceOrientation* values
                            if other.starts_with("UIInterfaceOrientation") {
                                entries.push(other);
                            }
                        }
                    }
                }
                if !entries.is_empty() {
                    let xml: String = entries.iter()
                        .map(|e| format!("        <string>{}</string>", e))
                        .collect::<Vec<_>>().join("\n");
                    let all_orientations = format!(
                        "    <key>UISupportedInterfaceOrientations</key>\n    <array>\n{}\n    </array>",
                        xml
                    );
                    // Replace both iPhone and iPad orientation blocks
                    let mut plist = info_plist.clone();
                    // Replace iPhone orientations
                    if let (Some(start), Some(_)) = (
                        plist.find("<key>UISupportedInterfaceOrientations</key>"),
                        plist.find("<key>UISupportedInterfaceOrientations~ipad</key>"),
                    ) {
                        let ipad_start = plist.find("<key>UISupportedInterfaceOrientations~ipad</key>").unwrap();
                        // Find end of iPhone array
                        let _iphone_section = &plist[start..ipad_start];
                        plist = format!(
                            "{}{}\n    {}",
                            &plist[..start],
                            all_orientations,
                            &plist[ipad_start..]
                        );
                        // iPad must always have all 4 orientations for App Store validation
                        // (the app can still lock to landscape at runtime)
                    }
                    return Some(plist);
                }
            }
        }
        None
    })().unwrap_or(info_plist);

    // Append usage descriptions for camera and microphone
    let usage_descriptions = concat!(
        "    <key>NSCameraUsageDescription</key>\n",
        "    <string>This app uses the camera to identify colors.</string>\n",
        "    <key>NSMicrophoneUsageDescription</key>\n",
        "    <string>This app uses the microphone to measure sound levels.</string>",
    );
    let info_plist = info_plist.replace(
        "</dict>\n</plist>",
        &format!("{}\n</dict>\n</plist>", usage_descriptions),
    );

    // Append ITSAppUsesNonExemptEncryption if configured in perry.toml
    let info_plist = if !encryption_exempt_plist.is_empty() {
        info_plist.replace(
            "</dict>\n</plist>",
            &format!("{}\n</dict>\n</plist>", encryption_exempt_plist),
        )
    } else {
        info_plist
    };

    // Append custom Info.plist entries from [ios.info_plist] in perry.toml
    let custom_plist_entries = (|| -> Option<String> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if toml_path.exists() {
                let data = fs::read_to_string(&toml_path).ok()?;
                let doc: toml::Table = data.parse().ok()?;
                let ios = doc.get("ios")?.as_table()?;
                let info_plist_table = ios.get("info_plist")?.as_table()?;
                let mut entries = String::new();
                for (key, value) in info_plist_table {
                    if let Some(s) = value.as_str() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <string>{}</string>\n",
                            key, s
                        ));
                    } else if let Some(b) = value.as_bool() {
                        entries.push_str(&format!(
                            "    <key>{}</key>\n    <{}/>",
                            key,
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

    // Issue #583: deep links — append CFBundleURLTypes from
    // package.json `perry.deepLinks.schemes`, and emit an
    // `app.entitlements` file with `com.apple.developer.associated-
    // domains` entries from `perry.deepLinks.universalLinks.ios`.
    // The entitlements file is referenced by codesign at signing
    // time; the existing `perry publish` flow picks it up
    // automatically when present alongside the .app bundle.
    let info_plist =
        inject_ios_deeplinks(&info_plist, input, &app_dir, format).unwrap_or(info_plist);

    // #1178 — augment `app.entitlements` with the
    // `com.apple.security.application-groups` array when
    // `[ios] app_group` is set in perry.toml. Idempotent with the
    // deeplinks pass above — it only adds our key, leaving any
    // existing entitlements (associated-domains, hand-written
    // entries) intact.
    inject_app_group_entitlement(&app_dir, ctx.app_metadata.app_group.as_deref(), format);

    // #5074 — emit the `aps-environment` entitlement when
    // `[ios] push_notifications = true` is set in perry.toml. Without it
    // `registerForRemoteNotifications` always fails and no APNs token is
    // produced. Idempotent with the deeplinks / app-group passes above.
    inject_ios_push_entitlement(input, &app_dir, format);

    // #1138 — `[google_auth]` block in perry.toml feeds the
    // GoogleSignIn SDK via Info.plist keys the Swift bridge in
    // `@perryts/google-auth` reads at runtime.
    let info_plist =
        inject_google_auth_info_plist(&info_plist, input, format).unwrap_or(info_plist);

    // #867 — `[ads]` block in perry.toml feeds the Google Mobile Ads SDK
    // via `GADApplicationIdentifier` (+ ATT purpose string) the runtime
    // reads from `Bundle.main.infoDictionary` at launch. Idempotent with
    // the passes above — only adds our keys.
    let info_plist = inject_ads_info_plist(&info_plist, input, format).unwrap_or(info_plist);

    fs::write(app_dir.join("Info.plist"), info_plist)?;

    // Read splash screen config from package.json perry.splash section
    let splash_config: Option<(Option<std::path::PathBuf>, String, Option<std::path::PathBuf>)> = (|| -> Option<(Option<std::path::PathBuf>, String, Option<std::path::PathBuf>)> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let pkg = dir.join("package.json");
            if pkg.exists() {
                let data = fs::read_to_string(&pkg).ok()?;
                let pkg_val: serde_json::Value = serde_json::from_str(&data).ok()?;
                let splash = pkg_val.get("perry")?.get("splash")?;

                // Check for custom storyboard override first
                if let Some(sb_path) = splash.get("ios").and_then(|i| i.get("storyboard")).and_then(|v| v.as_str()) {
                    let abs = dir.join(sb_path);
                    if abs.exists() {
                        return Some((None, "#FFFFFF".into(), Some(abs)));
                    }
                }

                // Resolve image: splash.ios.image -> splash.image
                let image_path = splash.get("ios").and_then(|i| i.get("image")).and_then(|v| v.as_str())
                    .or_else(|| splash.get("image").and_then(|v| v.as_str()))
                    .map(|p| dir.join(p))
                    .filter(|p| p.exists());

                // Resolve background: splash.ios.background -> splash.background -> "#FFFFFF"
                let background = splash.get("ios").and_then(|i| i.get("background")).and_then(|v| v.as_str())
                    .or_else(|| splash.get("background").and_then(|v| v.as_str()))
                    .unwrap_or("#FFFFFF")
                    .to_string();

                if image_path.is_some() || background != "#FFFFFF" {
                    return Some((image_path, background, None));
                }
                return None;
            }
        }
        None
    })();

    // Write a compiled LaunchScreen storyboard — with splash image if configured,
    // otherwise a minimal blank storyboard so iPadOS treats the app as native iPad.
    let launch_sb_xml = if let Some((ref image_path, ref bg_hex, ref custom_sb)) = splash_config {
        if let Some(custom) = custom_sb {
            // Custom storyboard: copy as-is
            fs::read_to_string(custom).unwrap_or_default()
        } else {
            // Copy splash image into bundle
            if let Some(img) = image_path {
                let _ = fs::copy(img, app_dir.join("splash_image.png"));
            }

            // Parse hex color to RGB floats
            let hex = bg_hex.trim_start_matches('#');
            let (r, g, b) = if hex.len() == 6 {
                let rv = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255) as f64 / 255.0;
                let gv = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255) as f64 / 255.0;
                let bv = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255) as f64 / 255.0;
                (rv, gv, bv)
            } else {
                (1.0, 1.0, 1.0)
            };

            let image_views = if image_path.is_some() {
                r#"
                    <subviews>
                        <imageView clipsSubviews="YES" userInteractionEnabled="NO" contentMode="scaleAspectFit" image="splash_image" translatesAutoresizingMaskIntoConstraints="NO" id="img-splash-1">
                            <rect key="frame" x="132.5" y="362" width="128" height="128"/>
                            <constraints>
                                <constraint firstAttribute="width" constant="128" id="img-w-1"/>
                                <constraint firstAttribute="height" constant="128" id="img-h-1"/>
                            </constraints>
                        </imageView>
                    </subviews>
                    <constraints>
                        <constraint firstItem="img-splash-1" firstAttribute="centerX" secondItem="Ze5-6b-2t3" secondAttribute="centerX" id="cx-1"/>
                        <constraint firstItem="img-splash-1" firstAttribute="centerY" secondItem="Ze5-6b-2t3" secondAttribute="centerY" id="cy-1"/>
                    </constraints>"#.to_string()
            } else {
                String::new()
            };

            let resources = if image_path.is_some() {
                r#"
<resources>
    <image name="splash_image" width="128" height="128"/>
</resources>"#
                    .to_string()
            } else {
                String::new()
            };

            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" toolsVersion="21701" targetRuntime="iOS.CocoaTouch" propertyAccessControl="none" useAutolayout="YES" launchScreen="YES" useTraitCollections="YES" useSafeAreas="YES" colorMatched="YES" initialViewController="01J-lp-oVM">
<scenes>
    <scene sceneID="EHf-IW-A2E">
        <objects>
            <viewController id="01J-lp-oVM" sceneMemberID="viewController">
                <view key="view" contentMode="scaleToFill" id="Ze5-6b-2t3">
                    <rect key="frame" x="0.0" y="0.0" width="393" height="852"/>
                    <autoresizingMask key="autoresizingMask" widthSizable="YES" heightSizable="YES"/>
                    <color key="backgroundColor" red="{r}" green="{g}" blue="{b}" alpha="1" colorSpace="custom" customColorSpace="sRGB"/>{image_views}
                </view>
            </viewController>
            <placeholder placeholderIdentifier="IBFirstResponder" id="iYj-Kq-Ea1" userLabel="First Responder" sceneMemberID="firstResponder"/>
        </objects>
        <point key="canvasLocation" x="0" y="0"/>
    </scene>
</scenes>{resources}
</document>"#
            )
        }
    } else {
        // No splash config — minimal blank storyboard for iPadOS compatibility
        r#"<?xml version="1.0" encoding="UTF-8"?>
<document type="com.apple.InterfaceBuilder3.CocoaTouch.Storyboard.XIB" version="3.0" toolsVersion="21701" targetRuntime="iOS.CocoaTouch" propertyAccessControl="none" useAutolayout="YES" launchScreen="YES" useTraitCollections="YES" useSafeAreas="YES" colorMatched="YES" initialViewController="01J-lp-oVM">
<scenes>
    <scene sceneID="EHf-IW-A2E">
        <objects>
            <viewController id="01J-lp-oVM" sceneMemberID="viewController">
                <view key="view" contentMode="scaleToFill" id="Ze5-6b-2t3">
                    <rect key="frame" x="0.0" y="0.0" width="393" height="852"/>
                    <autoresizingMask key="autoresizingMask" widthSizable="YES" heightSizable="YES"/>
                    <color key="backgroundColor" systemColor="systemBackgroundColor"/>
                </view>
            </viewController>
            <placeholder placeholderIdentifier="IBFirstResponder" id="iYj-Kq-Ea1" userLabel="First Responder" sceneMemberID="firstResponder"/>
        </objects>
        <point key="canvasLocation" x="0" y="0"/>
    </scene>
</scenes>
</document>"#.to_string()
    };

    let sb_source = app_dir.join("_LaunchScreen.storyboard");
    fs::write(&sb_source, launch_sb_xml)?;
    let storyboardc = app_dir.join("Base.lproj").join("LaunchScreen.storyboardc");
    let _ = fs::create_dir_all(app_dir.join("Base.lproj"));
    let _ = fs::remove_dir_all(&storyboardc);
    let ibt_result = std::process::Command::new("ibtool")
        .arg("--compile")
        .arg(storyboardc.as_os_str())
        .arg(sb_source.as_os_str())
        .output();
    let _ = fs::remove_file(&sb_source);
    if ibt_result.is_err() || !ibt_result.as_ref().unwrap().status.success() {
        eprintln!("Warning: ibtool failed to compile LaunchScreen.storyboard");
    }

    // Bundle resource files: scan source for ImageFile('...') calls and copy referenced files
    // Also copy any directories named 'logo', 'assets', 'resources', 'images' from the project root
    let source_dir = input
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    if let Some(src_dir) = &source_dir {
        // Walk up to find project root (where package.json is)
        let mut project_root = src_dir.clone();
        for _ in 0..5 {
            if project_root.join("package.json").exists() {
                break;
            }
            if let Some(parent) = project_root.parent() {
                project_root = parent.to_path_buf();
            } else {
                break;
            }
        }
        // Copy common resource directories into the bundle
        for dir_name in &["logo", "assets", "resources", "images"] {
            let resource_dir = project_root.join(dir_name);
            if resource_dir.is_dir() {
                let dest = app_dir.join(dir_name);
                eprintln!(
                    "[perry] iOS asset copy: src={} -> dst={}",
                    resource_dir.display(),
                    dest.display()
                );
                fn copy_dir_recursive(
                    src: &std::path::Path,
                    dst: &std::path::Path,
                ) -> std::io::Result<()> {
                    fs::create_dir_all(dst)?;
                    for entry in fs::read_dir(src)? {
                        let entry = entry?;
                        let ty = entry.file_type()?;
                        let dest_path = dst.join(entry.file_name());
                        if ty.is_dir() {
                            copy_dir_recursive(&entry.path(), &dest_path)?;
                        } else {
                            fs::copy(entry.path(), &dest_path)?;
                        }
                    }
                    Ok(())
                }
                let _ = copy_dir_recursive(&resource_dir, &dest);
            }
        }
    }

    // --- i18n: generate .lproj bundles for iOS/macOS ---
    if let (Some(table), Some(config)) = (&i18n_table, &i18n_config) {
        if !table.keys.is_empty() {
            for (locale_idx, locale) in config.locales.iter().enumerate() {
                let lproj_dir = app_dir.join(format!("{}.lproj", locale));
                let _ = fs::create_dir_all(&lproj_dir);
                let mut strings_content = String::new();
                for (key_idx, key) in table.keys.iter().enumerate() {
                    let flat_idx = locale_idx * table.keys.len() + key_idx;
                    let value = table
                        .translations
                        .get(flat_idx)
                        .cloned()
                        .unwrap_or_else(|| key.clone());
                    // Escape for .strings format
                    let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
                    let escaped_val = value.replace('\\', "\\\\").replace('"', "\\\"");
                    strings_content
                        .push_str(&format!("\"{}\" = \"{}\";\n", escaped_key, escaped_val));
                }
                let _ = fs::write(lproj_dir.join("Localizable.strings"), &strings_content);
            }
            match format {
                OutputFormat::Text => println!(
                    "  Generated {}.lproj bundles for {} locale(s)",
                    config.locales.join(", "),
                    config.locales.len()
                ),
                OutputFormat::Json => {}
            }
        }
    }

    compile_metallib_for_bundle(ctx, target, &app_dir, format)?;
    stage_native_library_artifacts(ctx, &app_dir, format)?;

    // Issue #676: build any [[widget]] entries declared in perry.toml,
    // embedding each produced .appex into <app>.app/Frameworks/<Name>.appex/.
    // iOS-only for v1 — watchOS / Android Glance variants warn-and-skip
    // inside the helper.
    let widgets_built = {
        let is_ios_sim = matches!(target, Some("ios-simulator"));
        widget_build::build_declared_widgets_ios(input, &app_dir, is_ios_sim, &bundle_id, format)?
    };

    match format {
        OutputFormat::Text => {
            println!("Wrote iOS app bundle: {}", app_dir.display());
            if widgets_built > 0 {
                println!(
                    "  Embedded {} widget extension{} under Frameworks/",
                    widgets_built,
                    if widgets_built == 1 { "" } else { "s" }
                );
            }
            println!();
            if matches!(target, Some("ios")) {
                // Physical-device target: dev-sign the bundle when a development
                // profile + matching identity are already provisioned locally
                // (via `perry setup ios --development`), then print the on-device
                // install/launch path. A plain compile without those materials
                // leaves the bundle unsigned and just prints instructions.
                let config = crate::commands::publish::load_config();
                let signed =
                    crate::commands::run::try_sign_existing_dev_profile(&app_dir, &config, format)
                        .unwrap_or(false);
                if !signed {
                    println!(
                        "Bundle is unsigned — `perry run --target ios` dev-signs it automatically."
                    );
                    println!("First-time device provisioning: perry setup ios --development");
                    println!();
                }
                println!("To install + launch on a connected device:");
                println!("  perry run --target ios");
                println!("Or manually (find <UDID> with `xcrun devicectl list devices`):");
                println!(
                    "  xcrun devicectl device install app --device <UDID> {}",
                    app_dir.display()
                );
                println!(
                    "  xcrun devicectl device process launch --device <UDID> {}",
                    bundle_id
                );
            } else {
                println!("To run on iOS Simulator:");
                println!("  xcrun simctl install booted {}", app_dir.display());
                println!("  xcrun simctl launch booted {}", bundle_id);
            }
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "output": app_dir.to_string_lossy(),
                "bundle_id": bundle_id,
                "native_modules": ctx.native_modules.len(),
                "js_modules": ctx.js_modules.len(),
                "widgets_built": widgets_built,
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok((app_dir, bundle_id))
}
