//! `[[widget]]` build glue for `perry compile --target {ios,android}`.
//!
//! Issue #676 (iOS) + #1139 (Android Glance). Walks the `perry.toml`
//! `[[widget]]` array and dispatches per platform:
//!
//! - **iOS**: [`build_declared_widgets_ios`] — invokes `swiftc` on
//!   `swift_source` and embeds the produced mach-O binary at
//!   `<.app>/Frameworks/<Name>.appex/<Name>` with a generated
//!   `Info.plist`.
//! - **Android**: [`build_declared_widgets_android`] — copies the
//!   `glance_source` Kotlin tree into the produced Gradle project at
//!   `app/src/main/java/<package>/widgets/<name>/`, emits
//!   `res/xml/widget_info_<name>.xml`, and injects a
//!   `<receiver>` entry into the merged `AndroidManifest.xml`.
//!
//! `watchos_source` is still skipped with a deprecation warning
//! (follow-up issue #676 watchOS slice).

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
    /// Issue #1179: TS entry point (relative to the perry.toml project
    /// root, or absolute) that declares one or more `Widget({...})` calls.
    /// When set, `perry compile --target ios` / `--target android`
    /// invokes the existing `--target ios-widget` / `--target android-widget`
    /// codegen internally and uses the produced sources in place of
    /// `swift_source` / `glance_source`. Lets apps keep a single source
    /// of truth (`widgets/index.ts`) without checking in generated code.
    pub ts_source: Option<String>,
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
        let has_ts = entry.ts_source.is_some();

        // watchOS / Android variants: emit a warning and skip. We don't
        // hard-fail because a single `[[widget]]` entry can legitimately
        // declare multiple platform sources — the iOS half should still
        // build even if the Android half hasn't been wired yet.
        //
        // When `ts_source` is set we always produce SwiftUI from it for
        // the iOS path, so the "watchOS-only" / "Android-only" skips
        // don't apply.
        if entry.swift_source.is_none() && !has_ts && entry.watchos_source.is_some() {
            warn_skip(
                format,
                &entry.name,
                "watchOS",
                "watchOS widget build path not yet wired — follow-up issue #676 (watchOS).",
            );
            continue;
        }
        if entry.glance_source.is_some() && entry.swift_source.is_none() && !has_ts {
            // Pure-Android widget: skipped here, picked up by
            // `build_declared_widgets_android` when the target is android.
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
        // glance_source alongside swift_source is informational only —
        // the Android slice is handled by build_declared_widgets_android
        // on android targets, the iOS .appex is built here.

        // Issue #1179: resolve the SwiftUI source directory. Precedence
        // is explicit `swift_source` (hand-edited override) > `ts_source`
        // (cross-compiled from TS). When neither is present we surface
        // the bogus entry as a warning and skip.
        let (abs_swift_source, swift_source_display) = if let Some(swift_source) =
            entry.swift_source.as_deref()
        {
            let abs = resolve_against_project_root(project_root.as_deref(), swift_source);
            if !abs.exists() {
                return Err(anyhow!(
                    "[[widget]] `{}`: swift_source `{}` does not exist (looked at `{}`)",
                    entry.name,
                    swift_source,
                    abs.display()
                ));
            }
            (abs, swift_source.to_string())
        } else if let Some(ts_source) = entry.ts_source.as_deref() {
            let abs_ts = resolve_against_project_root(project_root.as_deref(), ts_source);
            if !abs_ts.exists() {
                return Err(anyhow!(
                    "[[widget]] `{}`: ts_source `{}` does not exist (looked at `{}`)",
                    entry.name,
                    ts_source,
                    abs_ts.display()
                ));
            }
            let dir = compile_ts_widget_to_swift(&abs_ts, &entry.name, app_bundle_id, format)?;
            (dir, ts_source.to_string())
        } else {
            match format {
                OutputFormat::Text => eprintln!(
                    "Warning: [[widget]] `{}` has neither swift_source, ts_source, nor a recognized non-iOS source — skipping.",
                    entry.name
                ),
                OutputFormat::Json => {}
            }
            continue;
        };

        let display_name = entry
            .display_name
            .clone()
            .unwrap_or_else(|| entry.name.clone());
        let widget_bundle_id = format!("{}.{}", app_bundle_id, entry.name);

        let swift_files = collect_swift_files(&abs_swift_source)?;
        if swift_files.is_empty() {
            return Err(anyhow!(
                "[[widget]] `{}`: source `{}` contained no `.swift` files",
                entry.name,
                swift_source_display
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

/// Resolve a `[[widget]]`-relative path against the project root that owns
/// `perry.toml`. Falls back to the raw path if no project root is known
/// or the joined candidate doesn't exist on disk.
fn resolve_against_project_root(project_root: Option<&Path>, source: &str) -> PathBuf {
    match project_root {
        Some(root) => {
            let candidate = root.join(source);
            if candidate.exists() {
                candidate
            } else {
                PathBuf::from(source)
            }
        }
        None => PathBuf::from(source),
    }
}

/// Issue #1179: cross-compile a `[[widget]] ts_source` to SwiftUI by
/// re-invoking the running `perry` binary with `--target ios-widget
/// --skip-swift-build`. Returns the directory of emitted `.swift` files,
/// which the iOS build path then feeds into `swiftc` as if it were the
/// user's `swift_source`. Side-effect-free at the filesystem level apart
/// from writing into a fresh temp directory under `std::env::temp_dir()`.
fn compile_ts_widget_to_swift(
    ts_source: &Path,
    widget_name: &str,
    app_bundle_id: &str,
    format: OutputFormat,
) -> Result<PathBuf> {
    let perry = std::env::current_exe()
        .map_err(|e| anyhow!("Failed to locate the running perry binary: {}", e))?;

    let tmp_dir = std::env::temp_dir().join(format!(
        "perry-ts-widget-ios-{}-{}-{}",
        widget_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("Failed to create temp dir `{}`", tmp_dir.display()))?;

    if let OutputFormat::Text = format {
        println!(
            "  Cross-compiling TS widget `{}` ({}) → {}",
            widget_name,
            ts_source.display(),
            tmp_dir.display()
        );
    }

    let mut cmd = Command::new(&perry);
    // Pass `--format` as a global flag so it propagates to the nested
    // `compile` subcommand. Inherit the parent's format so json-mode
    // builds don't get text chatter mixed into stdout.
    match format {
        OutputFormat::Text => {}
        OutputFormat::Json => {
            cmd.args(["--format", "json"]);
        }
    }
    cmd.arg("compile")
        .arg(ts_source)
        .args(["--target", "ios-widget"])
        .args(["--app-bundle-id", app_bundle_id])
        .arg("--skip-swift-build")
        .arg("-o")
        .arg(&tmp_dir);

    let status = cmd.status().map_err(|e| {
        anyhow!(
            "Failed to invoke nested `perry compile --target ios-widget` for widget `{}`: {}",
            widget_name,
            e
        )
    })?;
    if !status.success() {
        return Err(anyhow!(
            "Nested `perry compile --target ios-widget` failed for widget `{}` \
             (input `{}`, exit {}).",
            widget_name,
            ts_source.display(),
            status.code().unwrap_or(-1),
        ));
    }

    Ok(tmp_dir)
}

/// Issue #1179: cross-compile a `[[widget]] ts_source` to Kotlin Glance
/// sources by re-invoking the running `perry` binary with
/// `--target android-widget`. Returns the directory of emitted `.kt`
/// files, which the Android build path then copies into the Gradle
/// project as if it were the user's `glance_source`.
fn compile_ts_widget_to_glance(
    ts_source: &Path,
    widget_name: &str,
    app_package: &str,
    format: OutputFormat,
) -> Result<PathBuf> {
    let perry = std::env::current_exe()
        .map_err(|e| anyhow!("Failed to locate the running perry binary: {}", e))?;

    let tmp_dir = std::env::temp_dir().join(format!(
        "perry-ts-widget-android-{}-{}-{}",
        widget_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("Failed to create temp dir `{}`", tmp_dir.display()))?;

    if let OutputFormat::Text = format {
        println!(
            "  Cross-compiling TS widget `{}` ({}) → {}",
            widget_name,
            ts_source.display(),
            tmp_dir.display()
        );
    }

    let mut cmd = Command::new(&perry);
    match format {
        OutputFormat::Text => {}
        OutputFormat::Json => {
            cmd.args(["--format", "json"]);
        }
    }
    cmd.arg("compile")
        .arg(ts_source)
        .args(["--target", "android-widget"])
        .args(["--app-bundle-id", app_package])
        .arg("-o")
        .arg(&tmp_dir);

    let status = cmd.status().map_err(|e| {
        anyhow!(
            "Failed to invoke nested `perry compile --target android-widget` for widget `{}`: {}",
            widget_name,
            e
        )
    })?;
    if !status.success() {
        return Err(anyhow!(
            "Nested `perry compile --target android-widget` failed for widget `{}` \
             (input `{}`, exit {}).",
            widget_name,
            ts_source.display(),
            status.code().unwrap_or(-1),
        ));
    }

    Ok(tmp_dir)
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

/// Issue #1139 — Android Glance build path for `[[widget]]` entries
/// that declare a `glance_source` Kotlin tree. Mirrors the iOS slice:
/// walks the manifest, copies sources into the produced Gradle project,
/// emits the appwidget-provider XML, and injects a `<receiver>` entry
/// into the merged AndroidManifest.xml. The user's Kotlin code is
/// expected to extend `GlanceAppWidgetReceiver` (or `AppWidgetProvider`).
///
/// `build_dir` is the Gradle project the Android `run` flow already
/// produced (`crates/perry-ui-android/template` after `applicationId`
/// rewriting). `app_package` is the Java package name registered as the
/// Application namespace (e.g. `com.example.app`). The function is
/// best-effort: missing `perry.toml`, no `[[widget]]` entries, or no
/// `glance_source` set on any entry → no-op returning `Ok(0)`.
pub(crate) fn build_declared_widgets_android(
    input: &Path,
    build_dir: &Path,
    app_package: &str,
    format: OutputFormat,
) -> Result<usize> {
    let entries = read_widget_entries(input);
    if entries.is_empty() {
        return Ok(0);
    }
    let project_root = project_root_for(input);

    let java_root = build_dir.join("app/src/main/java");
    let xml_res_dir = build_dir.join("app/src/main/res/xml");
    let manifest_path = build_dir.join("app/src/main/AndroidManifest.xml");

    let mut built = 0usize;
    let mut receiver_blocks: Vec<String> = Vec::new();

    for entry in &entries {
        // Issue #1179: resolve the Kotlin/Glance source directory.
        // Precedence is explicit `glance_source` (hand-edited override)
        // > `ts_source` (cross-compiled from TS). Entries with neither
        // skip silently on the Android path — they may be iOS-only.
        let (abs_source, source_display) =
            if let Some(glance_source) = entry.glance_source.as_deref() {
                let abs = resolve_against_project_root(project_root.as_deref(), glance_source);
                if !abs.exists() {
                    return Err(anyhow!(
                        "[[widget]] `{}`: glance_source `{}` does not exist (looked at `{}`)",
                        entry.name,
                        glance_source,
                        abs.display()
                    ));
                }
                (abs, glance_source.to_string())
            } else if let Some(ts_source) = entry.ts_source.as_deref() {
                let abs_ts = resolve_against_project_root(project_root.as_deref(), ts_source);
                if !abs_ts.exists() {
                    return Err(anyhow!(
                        "[[widget]] `{}`: ts_source `{}` does not exist (looked at `{}`)",
                        entry.name,
                        ts_source,
                        abs_ts.display()
                    ));
                }
                let dir = compile_ts_widget_to_glance(&abs_ts, &entry.name, app_package, format)?;
                (dir, ts_source.to_string())
            } else {
                continue;
            };

        // Lay each widget under `<app_package>.widgets.<name>` so apps
        // shipping several widgets don't collide in one flat package.
        let widget_pkg_segment = format!("widgets.{}", entry.name.to_lowercase());
        let widget_pkg = format!("{}.{}", app_package, widget_pkg_segment);
        let dest_dir = java_root
            .join(app_package.replace('.', "/"))
            .join("widgets")
            .join(entry.name.to_lowercase());
        fs::create_dir_all(&dest_dir)
            .with_context(|| format!("Failed to create `{}`", dest_dir.display()))?;

        let kt_files = collect_kotlin_files(&abs_source)?;
        if kt_files.is_empty() {
            return Err(anyhow!(
                "[[widget]] `{}`: source `{}` contained no `.kt` files (resolved to `{}`)",
                entry.name,
                source_display,
                abs_source.display()
            ));
        }
        // Track the first class that looks like a Glance receiver so
        // we can wire it into the manifest. Heuristic: prefer a file
        // named `<Name>Receiver.kt` or any file declaring a class
        // ending in `Receiver`; fall back to `<Name>.kt`.
        let mut receiver_class: Option<String> = None;
        for src in &kt_files {
            let name = src
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Widget.kt");
            let dest = dest_dir.join(name);
            fs::copy(src, &dest).with_context(|| {
                format!("Failed to copy `{}` → `{}`", src.display(), dest.display())
            })?;
            if receiver_class.is_none() {
                if let Ok(body) = fs::read_to_string(&dest) {
                    if let Some(c) = find_receiver_class(&body) {
                        receiver_class = Some(c);
                    }
                }
            }
        }
        let receiver_class = receiver_class.unwrap_or_else(|| format!("{}Receiver", entry.name));

        // Emit appwidget-provider metadata. Sizes are a sensible default
        // (1x1 cell minimum, resizable horizontally + vertically). Apps
        // wanting custom sizing can ship the XML themselves under
        // `<glance_source>/widget_info.xml` and we'll prefer that.
        let provider_xml_name = format!("widget_info_{}", entry.name.to_lowercase());
        fs::create_dir_all(&xml_res_dir)
            .with_context(|| format!("Failed to create `{}`", xml_res_dir.display()))?;
        let provider_xml =
            render_appwidget_provider_xml(&entry.description.clone().unwrap_or_else(|| {
                format!(
                    "{} widget",
                    entry.display_name.as_deref().unwrap_or(&entry.name)
                )
            }));
        fs::write(
            xml_res_dir.join(format!("{}.xml", provider_xml_name)),
            provider_xml,
        )?;

        // Receiver block injected into AndroidManifest.xml's <application>.
        receiver_blocks.push(format!(
            r#"        <receiver
            android:name="{widget_pkg}.{receiver_class}"
            android:exported="true">
            <intent-filter>
                <action android:name="android.appwidget.action.APPWIDGET_UPDATE" />
            </intent-filter>
            <meta-data
                android:name="android.appwidget.provider"
                android:resource="@xml/{provider_xml_name}" />
        </receiver>
"#,
            widget_pkg = widget_pkg,
            receiver_class = receiver_class,
            provider_xml_name = provider_xml_name,
        ));

        if let OutputFormat::Text = format {
            println!(
                "  Glance widget `{}` → {} ({} kt file{})",
                entry.name,
                dest_dir.display(),
                kt_files.len(),
                if kt_files.len() == 1 { "" } else { "s" }
            );
        }
        built += 1;
    }

    if !receiver_blocks.is_empty() && manifest_path.exists() {
        let manifest = fs::read_to_string(&manifest_path)?;
        let merged_xml = receiver_blocks.join("");
        let updated = if let Some(idx) = manifest.find("</application>") {
            let (head, tail) = manifest.split_at(idx);
            format!("{}{}{}", head, merged_xml, tail)
        } else {
            // No </application> close tag — unexpected; warn and skip.
            return Err(anyhow!(
                "AndroidManifest.xml at `{}` has no </application> tag",
                manifest_path.display()
            ));
        };
        fs::write(&manifest_path, updated)?;
    }

    Ok(built)
}

/// Walk `dir` and return every `*.kt` file (one level deep, like the
/// iOS variant). Errors mirror `collect_swift_files`.
fn collect_kotlin_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let md = fs::metadata(dir).with_context(|| format!("Failed to stat `{}`", dir.display()))?;
    if !md.is_dir() {
        if dir.extension().and_then(|s| s.to_str()) == Some("kt") {
            return Ok(vec![dir.to_path_buf()]);
        }
        return Err(anyhow!(
            "glance_source must be a directory or a `.kt` file: `{}`",
            dir.display()
        ));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read `{}`", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("kt") {
            files.push(p);
        }
    }
    files.sort();
    Ok(files)
}

/// Scan a Kotlin source body for `class <Name>Receiver` /
/// `class <Name>AppWidgetReceiver` / `class <Name>GlanceAppWidgetReceiver`
/// declarations. Returns the first match. Best-effort regex-free scan
/// — good enough for the user files Perry's `[[widget]]` flow expects.
fn find_receiver_class(body: &str) -> Option<String> {
    for line in body.lines() {
        let line = line.trim();
        // `class FooReceiver : GlanceAppWidgetReceiver()` — find the
        // first identifier after `class` that ends with `Receiver`.
        let Some(rest) = line.strip_prefix("class ").or_else(|| {
            // Allow `open class`, `internal class`, etc.
            line.split_whitespace()
                .position(|t| t == "class")
                .map(|i| {
                    let after: Vec<&str> = line.split_whitespace().skip(i + 1).collect();
                    after.first().copied().unwrap_or("")
                })
                .filter(|s| !s.is_empty())
        }) else {
            continue;
        };
        let token: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if token.ends_with("Receiver") && !token.is_empty() {
            return Some(token);
        }
    }
    None
}

fn render_appwidget_provider_xml(description: &str) -> String {
    let escaped = description
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<appwidget-provider xmlns:android="http://schemas.android.com/apk/res/android"
    android:minWidth="40dp"
    android:minHeight="40dp"
    android:updatePeriodMillis="1800000"
    android:resizeMode="horizontal|vertical"
    android:widgetCategory="home_screen"
    android:description="{description}" />
"#,
        description = escaped,
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
    fn manifest_parses_ts_source_widget_entry() {
        let toml_text = r#"
[[widget]]
name = "Widgets"
ts_source = "widgets/index.ts"
"#;
        let m: WidgetManifest = toml::from_str(toml_text).unwrap();
        assert_eq!(m.widget.len(), 1);
        let w = &m.widget[0];
        assert_eq!(w.name, "Widgets");
        assert_eq!(w.ts_source.as_deref(), Some("widgets/index.ts"));
        assert!(w.swift_source.is_none());
        assert!(w.glance_source.is_none());
    }

    #[test]
    fn resolve_against_project_root_joins_when_candidate_exists() {
        let tmp = std::env::temp_dir().join(format!(
            "perry-resolve-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = tmp.join("widgets");
        fs::create_dir_all(&sub).unwrap();
        let resolved = resolve_against_project_root(Some(&tmp), "widgets");
        assert_eq!(resolved, sub);
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_against_project_root_falls_back_to_raw() {
        // Non-existent candidate under root → fall back to raw path.
        let resolved = resolve_against_project_root(
            Some(Path::new("/nonexistent/perry/root")),
            "/absolute/widgets/dir",
        );
        assert_eq!(resolved, PathBuf::from("/absolute/widgets/dir"));
    }

    #[test]
    fn info_plist_embeds_expected_keys() {
        let plist = render_info_plist("TopSites", "Top Sites", "com.example.app.TopSites");
        assert!(plist.contains("<string>TopSites</string>"));
        assert!(plist.contains("<string>com.example.app.TopSites</string>"));
        assert!(plist.contains("com.apple.widgetkit-extension"));
    }

    #[test]
    fn find_receiver_class_extracts_glance_receiver() {
        let body = r#"
package com.example.widgets

import androidx.glance.appwidget.GlanceAppWidgetReceiver

class TopSitesReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget = TopSitesWidget()
}
"#;
        assert_eq!(
            find_receiver_class(body),
            Some("TopSitesReceiver".to_string())
        );
    }

    #[test]
    fn find_receiver_class_handles_open_modifier() {
        let body = "open class FooReceiver : android.appwidget.AppWidgetProvider() {}\n";
        assert_eq!(find_receiver_class(body), Some("FooReceiver".to_string()));
    }

    #[test]
    fn find_receiver_class_returns_none_for_no_match() {
        let body = "class HelperWidget {}\nclass NotAReceiverClass {}\n";
        assert_eq!(find_receiver_class(body), None);
    }

    #[test]
    fn appwidget_provider_xml_escapes_description() {
        let xml = render_appwidget_provider_xml("Daily \"Top\" & <change>");
        assert!(xml.contains("&quot;Top&quot;"));
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&lt;change&gt;"));
    }

    #[test]
    fn collect_kotlin_files_filters_extensions() -> Result<()> {
        let tmp = std::env::temp_dir().join(format!(
            "perry-widget-kotlin-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp)?;
        fs::write(tmp.join("A.kt"), "// a")?;
        fs::write(tmp.join("B.kt"), "// b")?;
        fs::write(tmp.join("ignore.swift"), "skip")?;
        let mut files = collect_kotlin_files(&tmp)?;
        files.sort();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("A.kt"));
        assert!(files[1].ends_with("B.kt"));
        fs::remove_dir_all(&tmp).ok();
        Ok(())
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
