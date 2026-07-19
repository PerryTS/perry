//! The default output path for a compile — the file `perry app.ts` writes when
//! the user gave no `-o`.
//!
//! One source of truth, shared by the compile pipeline (`run_pipeline`, which
//! links to this path) and the build cache (`build_cache`, which fingerprints
//! it to decide whether a rebuild is needed). These used to be two independent
//! `default_output_path` functions that had already drifted apart; keeping them
//! in one place is what makes the Android arm below correct in both.

use std::path::PathBuf;

/// The output file a compile targets when no `-o` was given.
///
/// `stem` is the already-sanitized entry-file stem (`app.ts` → `app`).
pub(super) fn default_output_path(
    is_dylib: bool,
    is_staticlib: bool,
    target: Option<&str>,
    stem: &str,
) -> PathBuf {
    if is_dylib {
        // #4771 — keyed on the target like the rest of this file (host is
        // only the fallback when no `--target` was given), not on host cfg
        // alone: `--target windows` must yield `.dll` regardless of host.
        // Same by-output-type table as `windows_default_output_extension`,
        // which already covered the `-o NAME` path; this covers the
        // no-`-o` default, which used to fall through to `.so` on Windows.
        if is_windows_target(target) {
            PathBuf::from(format!("{}.dll", stem))
        } else if is_apple_target(target) {
            PathBuf::from(format!("{}.dylib", stem))
        } else {
            PathBuf::from(format!("{}.so", stem))
        }
    } else if is_staticlib {
        // #1088 — Windows hosts expect `.lib`; everywhere else uses
        // the Unix `lib<stem>.a` convention so the archive is reachable
        // from `-l<stem>` at the host's link step.
        if is_windows_target(target) {
            PathBuf::from(format!("{}.lib", stem))
        } else {
            PathBuf::from(format!("lib{}.a", stem))
        }
    } else if matches!(
        target,
        Some("harmonyos")
            | Some("harmonyos-simulator")
            // #5740 — Android (and Wear OS, which links identically: same NDK,
            // same triple, same cdylib shape) links with `-shared` and ships as
            // a `.so` that `PerryActivity` dlopens; there is no standalone
            // executable shipping shape. Without this arm the default output was
            // the bare stem (`app`), which fails the link outright in a stock
            // Android project — `app/` is already a directory there, so lld
            // reports `cannot open output file app: Is a directory`.
            | Some("android")
            | Some("wearos")
    ) {
        // HarmonyOS apps ship as .so loaded by the ArkTS runtime via
        // napi_module_register — there is no standalone executable
        // shipping shape. `lib` prefix matches the dlopen name used by
        // the generated ArkTS shim (`import entry from 'libapp.so'`),
        // and the Android/Wear OS jniLibs copy expects a `.so` too.
        PathBuf::from(format!("lib{}.so", stem))
    } else if is_windows_target(target) {
        PathBuf::from(format!("{}.exe", stem))
    } else {
        PathBuf::from(stem)
    }
}

/// `--target windows*`, or a native build on a Windows host.
fn is_windows_target(target: Option<&str>) -> bool {
    matches!(target, Some("windows") | Some("windows-winui"))
        || (target.is_none() && cfg!(target_os = "windows"))
}

/// `--target macos` or any embedded-Apple target, or a native build on a
/// macOS host — the platforms whose shared-library convention is `.dylib`.
fn is_apple_target(target: Option<&str>) -> bool {
    matches!(
        target,
        Some(
            "macos"
                | "ios"
                | "ios-simulator"
                | "tvos"
                | "tvos-simulator"
                | "watchos"
                | "watchos-simulator"
                | "visionos"
                | "visionos-simulator"
        )
    ) || (target.is_none() && cfg!(target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use super::default_output_path;
    use std::path::PathBuf;

    fn exe(target: Option<&str>, stem: &str) -> PathBuf {
        default_output_path(false, false, target, stem)
    }

    /// #5740 bug 4 — `--target android` used to fall through to the bare stem,
    /// so the link ran with `-o app` inside an Android project where `app/` is
    /// a directory: `ld.lld: cannot open output file app: Is a directory`.
    #[test]
    fn android_defaults_to_shared_library() {
        assert_eq!(exe(Some("android"), "app"), PathBuf::from("libapp.so"));
        assert_eq!(exe(Some("android"), "hello"), PathBuf::from("libhello.so"));
    }

    /// Wear OS links exactly like Android (same NDK, triple, cdylib + TLS
    /// model — see `link::build_and_run`'s `is_android`), so it needs the
    /// same default.
    #[test]
    fn wearos_defaults_to_shared_library() {
        assert_eq!(exe(Some("wearos"), "app"), PathBuf::from("libapp.so"));
    }

    #[test]
    fn harmonyos_still_defaults_to_shared_library() {
        assert_eq!(exe(Some("harmonyos"), "app"), PathBuf::from("libapp.so"));
        assert_eq!(
            exe(Some("harmonyos-simulator"), "app"),
            PathBuf::from("libapp.so")
        );
    }

    #[test]
    fn windows_target_defaults_to_exe() {
        assert_eq!(exe(Some("windows"), "app"), PathBuf::from("app.exe"));
        assert_eq!(exe(Some("windows-winui"), "app"), PathBuf::from("app.exe"));
    }

    /// The unix/native executable shape must stay a bare, extension-less name.
    #[test]
    fn unix_targets_default_to_bare_stem() {
        assert_eq!(exe(Some("linux"), "app"), PathBuf::from("app"));
        assert_eq!(exe(Some("macos"), "app"), PathBuf::from("app"));
        #[cfg(not(target_os = "windows"))]
        assert_eq!(exe(None, "app"), PathBuf::from("app"));
    }

    #[test]
    fn staticlib_keeps_its_conventions() {
        assert_eq!(
            default_output_path(false, true, Some("linux"), "app"),
            PathBuf::from("libapp.a")
        );
        assert_eq!(
            default_output_path(false, true, Some("windows"), "app"),
            PathBuf::from("app.lib")
        );
    }

    /// #4771 finish — an extension-less `--output-type dylib` build keys the
    /// extension on the target: `.dll` for Windows (used to fall through to
    /// `.so`), `.dylib` for the Apple family, `.so` elsewhere.
    #[test]
    fn dylib_extension_keys_on_target() {
        let dylib = |target| default_output_path(true, false, target, "app");
        assert_eq!(dylib(Some("windows")), PathBuf::from("app.dll"));
        assert_eq!(dylib(Some("windows-winui")), PathBuf::from("app.dll"));
        assert_eq!(dylib(Some("macos")), PathBuf::from("app.dylib"));
        assert_eq!(dylib(Some("ios")), PathBuf::from("app.dylib"));
        assert_eq!(dylib(Some("ios-simulator")), PathBuf::from("app.dylib"));
        assert_eq!(dylib(Some("linux")), PathBuf::from("app.so"));
    }

    /// No `--target` falls back to the host's shared-library convention.
    #[test]
    fn dylib_uses_the_host_shared_library_extension() {
        let out = default_output_path(true, false, None, "app");
        #[cfg(target_os = "macos")]
        assert_eq!(out, PathBuf::from("app.dylib"));
        #[cfg(target_os = "windows")]
        assert_eq!(out, PathBuf::from("app.dll"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(out, PathBuf::from("app.so"));
    }
}
