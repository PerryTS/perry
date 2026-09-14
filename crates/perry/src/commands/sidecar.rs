//! Paths for files distributed alongside compiled executables.

use std::path::{Path, PathBuf};

// Keep non-code sidecars out of Contents/MacOS and outside the bundle seal.
// Attestations are written after signing; sandbox profiles can be customized.
pub(super) fn path_for_binary(binary_path: &Path, extension: &str) -> PathBuf {
    if let Some(macos) = binary_path.parent() {
        if let Some(contents) = macos.parent() {
            if let Some(app) = contents.parent() {
                if macos.file_name().is_some_and(|name| name == "MacOS")
                    && contents.file_name().is_some_and(|name| name == "Contents")
                    && app.extension().is_some_and(|ext| ext == "app")
                {
                    return app.with_extension(format!("app.{extension}"));
                }
            }
        }
    }
    binary_path.with_extension(extension)
}
