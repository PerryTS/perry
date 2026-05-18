//! `[[widget]]` build glue for `perry compile --target ios`.
//!
//! Issue #676. Walks the `perry.toml` `[[widget]]` array, invokes `swiftc`
//! per entry on the declared Swift source tree, and embeds the produced
//! mach-O binary into the host `.app/Frameworks/<Name>.appex/<Name>` with
//! a generated `Info.plist`. iOS-only for v1; `watchos_source` and
//! `glance_source` entries emit "not yet wired" warnings and skip the
//! per-platform build (`#675` and follow-up issues handle data sharing
//! and the watchOS/Android pipelines respectively).

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

/// One `[[widget]]` entry in `perry.toml`. All optional except `name` so
/// the schema is forward-compatible — a user who only filled out the
/// iOS slot today doesn't have to revisit the manifest when the
/// watchOS / Android paths come online.
#[derive(Debug, Deserialize)]
pub struct WidgetEntry {
    pub name: String,
    /// Path to the SwiftUI source directory (relative to the perry.toml
    /// project root, or absolute). Required for the iOS build path; when
    /// omitted we treat the widget as "watchOS/Android only" and warn.
    pub swift_source: Option<String>,
    /// Optional watchOS variant. Not wired up in v1.
    pub watchos_source: Option<String>,
    /// Optional Android Glance variant. Not wired up in v1.
    pub glance_source: Option<String>,
    /// Display name shown in the widget gallery. Defaults to `name`.
    pub display_name: Option<String>,
    /// One-line description. Defaults to "{display_name} widget".
    pub description: Option<String>,
    /// Optional list of AppIntent type names the widget exposes. Unused
    /// by the build glue itself — recorded in the Info.plist as
    /// `NSExtensionPrincipalClass`-adjacent metadata in a follow-up
    /// (`#675` companion).
    #[serde(default)]
    pub intents: Vec<String>,
}

/// Top-level `[[widget]]` array parsed from `perry.toml`.
#[derive(Debug, Default, Deserialize)]
struct WidgetManifest {
    #[serde(default)]
    widget: Vec<WidgetEntry>,
}

/// Read `[[widget]]` entries from the project's `perry.toml`. Returns an
/// empty `Vec` if no manifest exists or the array is absent — callers can
/// no-op without inspecting `Option`s.
pub(super) fn read_widget_entries(input: &Path) -> Vec<WidgetEntry> {
    let mut dir = match input.canonicalize() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    for _ in 0..8 {
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
        let toml_path = dir.join("perry.toml");
        if !toml_path.exists() {
            continue;
        }
        let Ok(data) = fs::read_to_string(&toml_path) else {
            continue;
        };
        let Ok(manifest) = toml::from_str::<WidgetManifest>(&data) else {
            return Vec::new();
        };
        return manifest.widget;
    }
    Vec::new()
}

/// Locate the project root that owns `input` (the directory containing
/// `perry.toml`, if any). Used to resolve `swift_source` paths that are
/// relative to the manifest, not to `input`'s parent.
fn project_root_for(input: &Path) -> Option<PathBuf> {
    let mut dir = input.canonicalize().ok()?;
    for _ in 0..8 {
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            return None;
        }
        if dir.join("perry.toml").exists() {
            return Some(dir);
        }
    }
    None
}

/// Build every `[[widget]]` declared in `perry.toml` for the iOS target,
/// embedding each produced `.appex` into `<app_dir>/Frameworks/<Name>.appex/`.
/// Non-iOS widget variants (`watchos_source`, `glance_source`) emit a
/// "not yet wired" warning and are skipped without failing the parent
/// compile.
///
/// `app_dir` is the `.app` bundle Perry has already written to. `is_simulator`
/// controls the SDK + target triple selection (`iphonesimulator` vs.
/// `iphoneos`). `app_bundle_id` is used to derive each widget extension's
/// bundle ID (`<app>.<name>` — same convention `perry-codegen-swiftui`
/// uses for native-IR widgets).
pub(super) fn build_declared_widgets_ios(
    input: &Path,
    app_dir: &Path,
    is_simulator: bool,
    app_bundle_id: &str,
    format: OutputFormat,
) -> Result<usize> {
    let entries = read_widget_entries(input);
    if entries.is_empty() {
        return Ok(0);
    }

    let project_root = project_root_for(input);
    let frameworks_dir = app_dir.join("Frameworks");
    fs::create_dir_all(&frameworks_dir)
        .with_context(|| format!("Failed to create `{}`", frameworks_dir.display()))?;

    let sdk = if is_simulator {
        "iphonesimulator"
    } else {
        "iphoneos"
    };
    let target_triple = if is_simulator {
        "arm64-apple-ios17.0-simulator"
    } else {
        "arm64-apple-ios17.0"
    };

    let sdk_path = resolve_sdk_path(sdk)?;

    let mut built = 0usize;
    for entry in &entries {
        // watchOS / Android variants: emit a warning and skip. We don't
        // hard-fail because a single `[[widget]]` entry can legitimately
        // declare multiple platform sources — the iOS half should still
        // build even if the Android half hasn't been wired yet.
        if entry.swift_source.is_none() && entry.watchos_source.is_some() {
            warn_skip(
                format,
                &entry.name,
                "watchOS",
                "watchOS widget build path not yet wired — follow-up issue #676 (watchOS).",
            );
            continue;
        }
        if entry.glance_source.is_some() && entry.swift_source.is_none() {
            warn_skip(
                format,
                &entry.name,
                "Android Glance",
                "Android Glance build path not yet wired — follow-up issue #676 (Glance).",
            );
            continue;
        }
        // Also warn for declared-but-skipped extras when iOS *is* present.
        if entry.watchos_source.is_some() {
            warn_skip(
                format,
                &entry.name,
                "watchOS",
                "watchos_source declared, but watchOS widget build path not yet wired in this Perry version (iOS .appex built normally).",
            );
        }
        if entry.glance_source.is_some() {
            warn_skip(
                format,
                &entry.name,
                "Android Glance",
                "glance_source declared, but Android Glance build path not yet wired in this Perry version (iOS .appex built normally).",
            );
        }

        let Some(swift_source) = entry.swift_source.as_deref() else {
            // No iOS source and no skip-warned non-iOS source above means a
            // bogus entry — surface it but don't kill the build.
            match format {
                OutputFormat::Text => eprintln!(
                    "Warning: [[widget]] `{}` has neither swift_source nor a recognized non-iOS source — skipping.",
                    entry.name
                ),
                OutputFormat::Json => {}
            }
            continue;
        };

        let abs_swift_source = match project_root.as_deref() {
            Some(root) => {
                let candidate = root.join(swift_source);
                if candidate.exists() {
                    candidate
                } else {
                    PathBuf::from(swift_source)
                }
            }
            None => PathBuf::from(swift_source),
        };

        if !abs_swift_source.exists() {
            return Err(anyhow!(
                "[[widget]] `{}`: swift_source `{}` does not exist (looked at `{}`)",
                entry.name,
                swift_source,
                abs_swift_source.display()
            ));
        }

        let display_name = entry
            .display_name
            .clone()
            .unwrap_or_else(|| entry.name.clone());
        let widget_bundle_id = format!("{}.{}", app_bundle_id, entry.name);

        let swift_files = collect_swift_files(&abs_swift_source)?;
        if swift_files.is_empty() {
            return Err(anyhow!(
                "[[widget]] `{}`: swift_source `{}` contained no `.swift` files",
                entry.name,
                abs_swift_source.display()
            ));
        }

        let appex_dir = frameworks_dir.join(format!("{}.appex", entry.name));
        fs::create_dir_all(&appex_dir)
            .with_context(|| format!("Failed to create `{}`", appex_dir.display()))?;

        let plist = render_info_plist(&entry.name, &display_name, &widget_bundle_id);
        fs::write(appex_dir.join("Info.plist"), plist).with_context(|| {
            format!(
                "Failed to write Info.plist for widget `{}` at `{}`",
                entry.name,
                appex_dir.display()
            )
        })?;

        let binary_path = appex_dir.join(&entry.name);

        match format {
            OutputFormat::Text => println!(
                "Building widget `{}` → {} ({} swift file{})",
                entry.name,
                appex_dir.display(),
                swift_files.len(),
                if swift_files.len() == 1 { "" } else { "s" }
            ),
            OutputFormat::Json => {}
        }

        let mut cmd = Command::new("xcrun");
        cmd.args(["--sdk", sdk, "swiftc"])
            .args(["-target", target_triple])
            .args(["-sdk", &sdk_path])
            .arg("-emit-executable")
            .arg("-parse-as-library")
            .arg("-O");
        for f in &swift_files {
            cmd.arg(f);
        }
        // Always link WidgetKit + SwiftUI. AppIntents is added when the
        // user wrote at least one `*Intent.swift` file alongside their
        // widget — same heuristic the scaffolder emits.
        cmd.args(["-framework", "WidgetKit"]);
        cmd.args(["-framework", "SwiftUI"]);
        if swift_files.iter().any(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.contains("Intent.swift"))
                .unwrap_or(false)
        }) {
            cmd.args(["-framework", "AppIntents"]);
        }
        cmd.arg("-o").arg(&binary_path);

        let status = cmd.status().map_err(|e| {
            anyhow!(
                "Failed to invoke swiftc for widget `{}`: {}. \
                 Install Xcode or the Command Line Tools (`xcode-select --install`).",
                entry.name,
                e
            )
        })?;
        if !status.success() {
            return Err(anyhow!(
                "swiftc failed building widget `{}` (exit {}). Sources at `{}`.",
                entry.name,
                status.code().unwrap_or(-1),
                abs_swift_source.display()
            ));
        }

        match format {
            OutputFormat::Text => {
                println!("  Embedded {}.appex at {}", entry.name, appex_dir.display())
            }
            OutputFormat::Json => {}
        }

        built += 1;

        // Echo intents list in text mode so the user can sanity-check the
        // manifest entry was parsed. Intents themselves are app-side
        // concerns; this is informational.
        if !entry.intents.is_empty() {
            if let OutputFormat::Text = format {
                println!("  Intents: {}", entry.intents.join(", "));
            }
        }
    }

    Ok(built)
}

fn warn_skip(format: OutputFormat, widget: &str, platform: &str, message: &str) {
    if let OutputFormat::Text = format {
        eprintln!("Warning: widget `{}` ({}): {}", widget, platform, message);
    }
}

fn resolve_sdk_path(sdk: &str) -> Result<String> {
    let output = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .map_err(|e| {
            anyhow!(
                "Failed to invoke `xcrun --sdk {} --show-sdk-path`: {}. \
                 Install Xcode or the Command Line Tools (`xcode-select --install`).",
                sdk,
                e
            )
        })?;
    if !output.status.success() {
        return Err(anyhow!(
            "`xcrun --sdk {} --show-sdk-path` failed: {}",
            sdk,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Walk `dir` and return every `*.swift` file (one level deep is enough
/// for v1 — the scaffolder lays everything flat). Returns the files
/// sorted so the swiftc command line is stable across invocations.
fn collect_swift_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let md = fs::metadata(dir).with_context(|| format!("Failed to stat `{}`", dir.display()))?;
    if !md.is_dir() {
        // Allow a single .swift file as well — useful for tests.
        if dir.extension().and_then(|s| s.to_str()) == Some("swift") {
            return Ok(vec![dir.to_path_buf()]);
        }
        return Err(anyhow!(
            "swift_source must be a directory or a `.swift` file: `{}`",
            dir.display()
        ));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read `{}`", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("swift") {
            files.push(p);
        }
    }
    files.sort();
    Ok(files)
}

/// Generate a minimal Info.plist for the widget `.appex` bundle. The
/// CFBundleExecutable matches the binary name we wrote (the widget's
/// `name`). NSExtensionPointIdentifier is `com.apple.widgetkit-extension`
/// — that's the contract that makes iOS recognize the embedded `.appex`
/// as a WidgetKit extension at install time.
fn render_info_plist(name: &str, display_name: &str, bundle_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>{display_name}</string>
    <key>CFBundleExecutable</key>
    <string>{name}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundlePackageType</key>
    <string>XPC!</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>NSExtension</key>
    <dict>
        <key>NSExtensionPointIdentifier</key>
        <string>com.apple.widgetkit-extension</string>
    </dict>
</dict>
</plist>
"#,
        name = name,
        display_name = display_name,
        bundle_id = bundle_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_minimal_widget_entry() {
        let toml_text = r#"
[[widget]]
name = "TopSitesWidget"
swift_source = "ios-widgets/TopSitesWidget"
"#;
        let m: WidgetManifest = toml::from_str(toml_text).unwrap();
        assert_eq!(m.widget.len(), 1);
        assert_eq!(m.widget[0].name, "TopSitesWidget");
        assert_eq!(
            m.widget[0].swift_source.as_deref(),
            Some("ios-widgets/TopSitesWidget")
        );
    }

    #[test]
    fn manifest_parses_full_widget_entry() {
        let toml_text = r#"
[[widget]]
name = "DailyChangeWidget"
swift_source = "ios-widgets/DailyChangeWidget"
glance_source = "android-widgets/DailyChangeGlance"
display_name = "Daily change"
description = "Today's delta"
intents = ["DailyIntent", "RefreshIntent"]
"#;
        let m: WidgetManifest = toml::from_str(toml_text).unwrap();
        assert_eq!(m.widget.len(), 1);
        let w = &m.widget[0];
        assert_eq!(w.display_name.as_deref(), Some("Daily change"));
        assert_eq!(w.intents, vec!["DailyIntent", "RefreshIntent"]);
        assert!(w.glance_source.is_some());
    }

    #[test]
    fn info_plist_embeds_expected_keys() {
        let plist = render_info_plist("TopSites", "Top Sites", "com.example.app.TopSites");
        assert!(plist.contains("<string>TopSites</string>"));
        assert!(plist.contains("<string>com.example.app.TopSites</string>"));
        assert!(plist.contains("com.apple.widgetkit-extension"));
    }

    #[test]
    fn collect_swift_files_filters_extensions() -> Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "perry-widget-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp)?;
        fs::write(tmp.join("A.swift"), "// a")?;
        fs::write(tmp.join("B.swift"), "// b")?;
        fs::write(tmp.join("ignore.txt"), "skip")?;
        let mut files = collect_swift_files(&tmp)?;
        files.sort();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("A.swift"));
        assert!(files[1].ends_with("B.swift"));
        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }
}
