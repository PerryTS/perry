//! WinUI 3 runtime-asset deployment for the executable link.
//!
//! Split out of `build_and_run.rs` (2000-line-per-file cap). Pure relocation
//! of the `windows-winui` bootstrap-import-library lookup and the runtime
//! asset copy that runs after a successful link, plus their tests.

use super::*;
use anyhow::Context;

const WINUI_RUNTIME_ASSETS: [&str; 2] =
    ["Microsoft.WindowsAppRuntime.Bootstrap.dll", "resources.pri"];
const WINUI_BOOTSTRAP_IMPORT_LIBRARY: &str = "Microsoft.WindowsAppRuntime.Bootstrap.lib";

pub(super) fn winui_bootstrap_import_library(ui_library: &Path) -> Result<PathBuf> {
    let source_dir = ui_library.parent().ok_or_else(|| {
        anyhow!(
            "WinUI library has no parent directory: {}",
            ui_library.display()
        )
    })?;
    let import_library = source_dir.join(WINUI_BOOTSTRAP_IMPORT_LIBRARY);
    if !import_library.is_file() {
        return Err(anyhow!(
            "WinUI bootstrap import library {} is missing next to {}. Rebuild with: cargo build --release -p perry-ui-windows-winui",
            WINUI_BOOTSTRAP_IMPORT_LIBRARY,
            ui_library.display()
        ));
    }
    Ok(import_library)
}

fn copy_winui_runtime_assets(ui_library: &Path, exe_path: &Path) -> Result<()> {
    let source_dir = ui_library.parent().ok_or_else(|| {
        anyhow!(
            "WinUI library has no parent directory: {}",
            ui_library.display()
        )
    })?;
    let destination_dir = exe_path.parent().ok_or_else(|| {
        anyhow!(
            "WinUI executable has no parent directory: {}",
            exe_path.display()
        )
    })?;
    fs::create_dir_all(destination_dir)?;

    for asset in WINUI_RUNTIME_ASSETS {
        let source = source_dir.join(asset);
        if !source.is_file() {
            return Err(anyhow!(
                "WinUI runtime asset {} is missing next to {}. Rebuild with: cargo build --release -p perry-ui-windows-winui",
                asset,
                ui_library.display()
            ));
        }
        let destination = destination_dir.join(asset);
        if source != destination {
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "failed to deploy WinUI runtime asset {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(super) fn deploy_winui_runtime_assets(
    target: Option<&str>,
    needs_ui: bool,
    exe_path: &Path,
) -> Result<()> {
    if !needs_ui || !matches!(target, Some("windows-winui")) {
        return Ok(());
    }
    let ui_library = find_ui_library(target)
        .ok_or_else(|| anyhow!("WinUI library disappeared before runtime asset deployment"))?;
    copy_winui_runtime_assets(&ui_library, exe_path)
}

#[cfg(test)]
mod winui_asset_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("perry-winui-assets-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn copies_winui_runtime_assets_next_to_executable() {
        let root = test_root();
        let source_dir = root.join("lib");
        let output_dir = root.join("dist");
        fs::create_dir_all(&source_dir).unwrap();
        let ui_library = source_dir.join("perry_ui_windows_winui.lib");
        fs::write(&ui_library, b"archive").unwrap();
        for asset in WINUI_RUNTIME_ASSETS {
            fs::write(source_dir.join(asset), asset.as_bytes()).unwrap();
        }

        let exe_path = output_dir.join("todo.exe");
        copy_winui_runtime_assets(&ui_library, &exe_path).unwrap();

        for asset in WINUI_RUNTIME_ASSETS {
            assert_eq!(fs::read(output_dir.join(asset)).unwrap(), asset.as_bytes());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn locates_winui_bootstrap_import_library_next_to_ui_archive() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let ui_library = root.join("perry_ui_windows_winui.lib");
        let import_library = root.join(WINUI_BOOTSTRAP_IMPORT_LIBRARY);
        fs::write(&ui_library, b"archive").unwrap();
        fs::write(&import_library, b"import library").unwrap();

        assert_eq!(
            winui_bootstrap_import_library(&ui_library).unwrap(),
            import_library
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_a_missing_winui_runtime_asset() {
        let root = test_root();
        let source_dir = root.join("lib");
        fs::create_dir_all(&source_dir).unwrap();
        let ui_library = source_dir.join("perry_ui_windows_winui.lib");
        fs::write(&ui_library, b"archive").unwrap();

        let error = copy_winui_runtime_assets(&ui_library, &root.join("todo.exe"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Microsoft.WindowsAppRuntime.Bootstrap.dll"));
        fs::remove_dir_all(root).unwrap();
    }
}
