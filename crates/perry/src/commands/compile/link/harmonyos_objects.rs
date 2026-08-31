//! HarmonyOS: collect the native objects `build.rs` scripts emit, split out of
//! `build_and_run.rs` to keep that file under the 2000-line gate. Behaviour is
//! unchanged — this is the block that used to sit inline behind `is_harmonyos`.

use super::*;

/// Append every `build.rs`-produced `.o` under the HarmonyOS build roots to the
/// link line.
///
/// `auto_rebuild` emits into a `perry-auto-<hash>` directory; the workspace's
/// own `target/` is the fallback for non-auto flows, and a run invoked from
/// outside the workspace still lands under the perry source tree's `target/`.
pub(super) fn push_harmonyos_native_objects(
    cmd: &mut std::process::Command,
    target: Option<&str>,
    format: crate::OutputFormat,
) {
    let triple = super::rust_target_triple(target).unwrap_or("aarch64-unknown-linux-ohos");
    let build_roots: Vec<std::path::PathBuf> = {
        let mut roots: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir("target") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("perry-auto-") || name_str == triple {
                    roots.push(entry.path());
                }
            }
        }
        if let Some(ws_root) = super::super::find_perry_workspace_root() {
            let ws_target = ws_root.join("target");
            if let Ok(entries) = std::fs::read_dir(&ws_target) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("perry-auto-") {
                        roots.push(entry.path());
                    }
                }
            }
        }
        roots
    };
    let mut native_objs: Vec<std::path::PathBuf> = Vec::new();
    for root in &build_roots {
        let build_dir = root.join(triple).join("release").join("build");
        let entries = match std::fs::read_dir(&build_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for crate_build in entries.flatten() {
            let out_dir = crate_build.path().join("out");
            // Walk the out/ dir recursively (cc-rs can nest into source-mirror
            // subdirs like c_src/mimalloc/v2/src/).
            if let Ok(walker) = walkdir::WalkDir::new(&out_dir)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
            {
                for entry in walker {
                    if entry.file_type().is_file()
                        && entry.path().extension().and_then(|e| e.to_str()) == Some("o")
                    {
                        native_objs.push(entry.path().to_path_buf());
                    }
                }
            }
        }
    }
    if !native_objs.is_empty() && matches!(format, crate::OutputFormat::Text) {
        println!(
            "  harmonyos: linking {} build.rs native object(s)",
            native_objs.len()
        );
    }
    for obj in native_objs {
        cmd.arg(obj);
    }
}
