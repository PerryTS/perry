//! Local macOS UI application packaging (#10078).

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::bundle_apple::{read_app_display_name, xml_escape};
use super::CompilationContext;
use crate::OutputFormat;

fn app_icon_source(input: &Path) -> Option<PathBuf> {
    let source_dir = input.canonicalize().ok()?.parent()?.to_path_buf();
    let project_root = super::resources::find_project_root_for_resources(&source_dir, true);
    let icon = project_root.join("assets/AppIcon.icns");
    icon.is_file().then_some(icon)
}

fn project_name(input: &Path) -> Option<String> {
    let mut dir = input.canonicalize().ok()?.parent()?.to_path_buf();
    for _ in 0..5 {
        let toml_path = dir.join("perry.toml");
        if toml_path.exists() {
            let doc: toml::Table = fs::read_to_string(toml_path).ok()?.parse().ok()?;
            return doc
                .get("project")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

pub(super) struct MacosBundleLayout {
    pub app_dir: PathBuf,
    pub executable: PathBuf,
}

pub(super) fn layout_for_compile(
    needs_ui: bool,
    output_type: &str,
    target: Option<&str>,
    output: &Path,
) -> Result<Option<MacosBundleLayout>> {
    let macos = target == Some("macos") || (target.is_none() && cfg!(target_os = "macos"));
    if !needs_ui || output_type != "executable" || !macos {
        return Ok(None);
    }
    let explicit_bundle = output.extension().is_some_and(|ext| ext == "app");
    let executable_name = if explicit_bundle {
        output.file_stem()
    } else {
        output.file_name()
    }
    .filter(|name| !name.is_empty())
    .ok_or_else(|| anyhow!("macOS app output needs a filename: {}", output.display()))?;
    let app_dir = if explicit_bundle {
        output.to_path_buf()
    } else {
        let mut name = executable_name.to_os_string();
        name.push(".app");
        output.with_file_name(name)
    };
    let executable = app_dir.join("Contents/MacOS").join(executable_name);
    Ok(Some(MacosBundleLayout {
        app_dir,
        executable,
    }))
}

/// The embedded and bundle plists must agree on executable and app identity.
pub(super) fn info_plist(ctx: &CompilationContext, input: &Path, executable: &Path) -> String {
    let filename = executable.file_name().unwrap_or_default().to_string_lossy();
    let display_name = read_app_display_name(input, "macos")
        .or_else(|| project_name(input))
        .unwrap_or_else(|| filename.to_string());
    let icon_entry = if app_icon_source(input).is_some() {
        "    <key>CFBundleIconFile</key><string>AppIcon.icns</string>\n"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleIdentifier</key><string>{bundle_id}</string>
    <key>CFBundleName</key><string>{display_name}</string>
    <key>CFBundleDisplayName</key><string>{display_name}</string>
    <key>CFBundleExecutable</key><string>{filename}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
{icon_entry}    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{build_number}</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSCameraUsageDescription</key>
    <string>This app uses the camera for WebView video calls.</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>This app uses the microphone for WebView video calls.</string>
</dict>
</plist>
"#,
        bundle_id = xml_escape(&ctx.app_metadata.bundle_id),
        display_name = xml_escape(&display_name),
        filename = xml_escape(&filename),
        icon_entry = icon_entry,
        version = xml_escape(&ctx.app_metadata.version),
        build_number = ctx.app_metadata.build_number,
    )
}

fn write_bundle_files(layout: &MacosBundleLayout, linked_exe: &Path, plist: &str) -> Result<()> {
    fs::create_dir_all(layout.executable.parent().unwrap())?;
    fs::create_dir_all(layout.app_dir.join("Contents/Resources"))?;
    if linked_exe != layout.executable {
        fs::copy(linked_exe, &layout.executable)
            .with_context(|| format!("copy executable into {}", layout.app_dir.display()))?;
    }
    fs::write(layout.app_dir.join("Contents/Info.plist"), plist)?;
    Ok(())
}

pub(super) fn bundle_for_macos(
    layout: &MacosBundleLayout,
    linked_exe: &Path,
    input: &Path,
    ctx: &CompilationContext,
    target: Option<&str>,
    i18n_table: Option<&perry_transform::i18n::I18nStringTable>,
    i18n_config: Option<&perry_transform::i18n::I18nConfig>,
    format: OutputFormat,
) -> Result<(PathBuf, String)> {
    write_bundle_files(
        layout,
        linked_exe,
        &info_plist(ctx, input, &layout.executable),
    )?;
    if linked_exe != layout.executable {
        if let Some(parent) = linked_exe.parent() {
            super::resources::copy_standalone_resource_dirs(input, parent);
            super::resources::stage_native_library_artifacts(ctx, parent, format)?;
        }
    }
    let resources = layout.app_dir.join("Contents/Resources");
    super::resources::copy_standalone_resource_dirs(input, &resources);
    if let Some(icon) = app_icon_source(input) {
        fs::copy(&icon, resources.join("AppIcon.icns"))
            .with_context(|| format!("copy app icon from {}", icon.display()))?;
    }
    super::resources::stage_native_library_artifacts(ctx, &resources, format)?;
    super::i18n_emit::write_lproj_localized_strings(&resources, i18n_table, i18n_config);
    super::native_addon_sidecar::stage_native_addon_sidecar(ctx, &layout.executable, target)?;

    super::post_link::emit_sandbox_sidecar(ctx, &layout.executable, format);

    // Local development needs no signing identity or provisioning profile.
    // Seal the external plist and copied resources after the bundle is complete.
    if cfg!(target_os = "macos") {
        let signed = Command::new("codesign")
            .args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(&layout.app_dir)
            .output()
            .context("sign local macOS app bundle")?;
        if !signed.status.success() {
            anyhow::bail!(
                "codesign failed for {}: {}",
                layout.app_dir.display(),
                String::from_utf8_lossy(&signed.stderr)
            );
        }
    }
    let bundle_id = ctx.app_metadata.bundle_id.clone();
    match format {
        OutputFormat::Text => println!("Wrote macOS app bundle: {}", layout.app_dir.display()),
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({"success": true, "output": layout.app_dir, "bundle_id": bundle_id})
        ),
    }
    Ok((layout.app_dir.clone(), bundle_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_macos_ui_executables_are_bundled() {
        let output = Path::new("demo");
        for (ui, kind, target) in [
            (false, "executable", "macos"),
            (true, "dylib", "macos"),
            (true, "staticlib", "macos"),
            (true, "executable", "linux"),
            (true, "executable", "ios-simulator"),
        ] {
            assert!(layout_for_compile(ui, kind, Some(target), output)
                .unwrap()
                .is_none());
        }
        assert!(
            layout_for_compile(true, "executable", Some("macos"), output)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn bundle_paths_preserve_names_and_explicit_app_extension() {
        for (output, expected) in [
            ("out/My App.v2", "out/My App.v2.app"),
            ("out/My App.v2.app", "out/My App.v2.app"),
        ] {
            let layout = layout_for_compile(true, "executable", Some("macos"), Path::new(output))
                .unwrap()
                .unwrap();
            assert_eq!(layout.app_dir, Path::new(expected));
            assert_eq!(
                layout.executable,
                Path::new(expected).join("Contents/MacOS/My App.v2")
            );
        }
    }

    #[test]
    fn packaging_keeps_linked_binary_and_does_not_truncate_an_in_bundle_output() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("demo");
        fs::write(&raw, b"linked executable").unwrap();
        let layout = layout_for_compile(true, "executable", Some("macos"), &raw)
            .unwrap()
            .unwrap();
        write_bundle_files(&layout, &raw, "first plist").unwrap();
        assert_eq!(fs::read(&raw).unwrap(), b"linked executable");
        assert_eq!(fs::read(&layout.executable).unwrap(), b"linked executable");
        assert!(layout.app_dir.join("Contents/Resources").is_dir());
        write_bundle_files(&layout, &layout.executable, "updated plist").unwrap();
        assert_eq!(fs::read(&layout.executable).unwrap(), b"linked executable");
        assert_eq!(
            fs::read_to_string(layout.app_dir.join("Contents/Info.plist")).unwrap(),
            "updated plist"
        );
    }
}
