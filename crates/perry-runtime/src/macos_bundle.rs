//! macOS `.app` bundle working-directory fix.
//!
//! A macOS app launched from Finder/Dock starts with the working directory set
//! to `/`. Perry-compiled GUI/game apps — and the native libraries they link,
//! e.g. the Bloom engine — load assets via paths relative to the CWD, like
//! `bloom_load_texture("assets/sprites/atlas.png")` / `fopen("assets/...")`.
//! The build worker bundles those assets into `<App>.app/Contents/Resources/`,
//! so on macOS we `chdir` there at startup — before any user code or native
//! engine init runs — so the relative asset paths resolve.
//!
//! iOS already launches with the app bundle as the CWD base, so it needs no
//! equivalent; this is gated to macOS *and* to executables that actually live
//! in `…/Contents/MacOS/`, leaving plain CLI/server binaries' CWD untouched.

/// Called once from `main()`'s prelude. No-op unless this is a macOS binary
/// running inside an `.app` bundle (i.e. the executable is at
/// `<App>.app/Contents/MacOS/<bin>`), in which case it chdir's to the sibling
/// `Contents/Resources` so relative asset paths resolve.
#[no_mangle]
pub extern "C" fn perry_macos_bundle_chdir() {
    #[cfg(target_os = "macos")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(resources) = bundle_resources_dir(&exe) {
                if resources.is_dir() {
                    let _ = std::env::set_current_dir(&resources);
                }
            }
        }
    }
}

/// Map an executable path `<App>.app/Contents/MacOS/<bin>` to its sibling
/// `<App>.app/Contents/Resources`. Returns `None` when the executable is not
/// inside a `Contents/MacOS/` directory (e.g. a plain CLI binary).
#[cfg(any(target_os = "macos", test))]
fn bundle_resources_dir(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    exe.parent() // …/Contents/MacOS
        .filter(|macos| macos.file_name() == Some(std::ffi::OsStr::new("MacOS")))
        .and_then(|macos| macos.parent()) // …/Contents
        .map(|contents| contents.join("Resources"))
}

#[cfg(test)]
mod tests {
    use super::bundle_resources_dir;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_resources_inside_app_bundle() {
        assert_eq!(
            bundle_resources_dir(Path::new("/Applications/Foo.app/Contents/MacOS/Foo")),
            Some(PathBuf::from("/Applications/Foo.app/Contents/Resources"))
        );
    }

    #[test]
    fn returns_none_for_plain_binary() {
        assert_eq!(bundle_resources_dir(Path::new("/usr/local/bin/foo")), None);
        assert_eq!(bundle_resources_dir(Path::new("foo")), None);
    }
}
