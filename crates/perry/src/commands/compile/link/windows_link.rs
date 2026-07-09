//! Windows-specific link helpers for the Win32 link step (`is_windows`
//! branch of `build_and_run_link`). Holds the system-library link line and the
//! comctl32 v6 application-manifest embed. Extracted from `link/mod.rs` to keep
//! that file under the 2000-line CI gate (`scripts/check_file_size.sh`).
//!
//! Compiled Windows UI apps bound comctl32 v5 because Perry embedded no
//! application manifest into the linked `.exe`, so every common control
//! (buttons, list views, edit boxes…) rendered in the unthemed Win95/classic
//! style regardless of the OS theme. Embedding a manifest that declares the
//! `Microsoft.Windows.Common-Controls` v6 side-by-side dependency activates
//! visual styles (the Fluent look) on Windows 10/11. See issue #4681 /
//! discussion #3486.

use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use super::super::library_search::find_windows_sdk_mt_dir;

/// Win32 application manifest embedded into UI executables. Declares the
/// comctl32 v6 (`Microsoft.Windows.Common-Controls`) side-by-side dependency
/// so common controls render with visual styles instead of the unthemed
/// classic style, plus an `asInvoker` execution level.
pub(crate) const WINDOWS_APP_MANIFEST: &str = include_str!("windows_app.manifest");

/// Append the Windows system import libraries the runtime/UI/stdlib link
/// against: the Win32 GUI + shell stack, the MSVC dynamic CRT, and the extra
/// API libs the Rust runtime pulls in. Always emitted on Windows targets (the
/// `whoami`/`winhttp`/etc. symbols are needed even by console binaries through
/// `perry-stdlib`).
pub(super) fn add_system_libs(cmd: &mut Command) {
    // Win32 GUI + shell system libraries.
    cmd.arg("user32.lib")
        .arg("gdi32.lib")
        .arg("gdiplus.lib")
        .arg("msimg32.lib")
        .arg("kernel32.lib")
        .arg("shell32.lib")
        .arg("ole32.lib")
        .arg("comctl32.lib")
        .arg("advapi32.lib")
        .arg("comdlg32.lib")
        .arg("ws2_32.lib")
        .arg("dwmapi.lib");
    // MSVC CRT (dynamic) and additional Windows API libraries needed by the Rust runtime.
    cmd.arg("msvcrt.lib")
        .arg("vcruntime.lib")
        .arg("ucrt.lib")
        .arg("bcrypt.lib")
        .arg("ntdll.lib")
        .arg("userenv.lib")
        // secur32.lib exports `GetUserNameExW`, called by the `whoami`
        // crate (transitively pulled in via `sqlx-mysql`/`sqlx-postgres`
        // through `perry-stdlib`). Without it, every doc-test that
        // touches stdlib fails on the Windows runner with
        // `LNK2019: unresolved external symbol __imp_GetUserNameExW`.
        // Closes #220.
        .arg("secur32.lib")
        .arg("oleaut32.lib")
        .arg("propsys.lib")
        .arg("runtimeobject.lib")
        .arg("iphlpapi.lib")
        // winhttp.lib — perry-ui-windows::widgets::image::fetch_url_blocking
        // uses WinHttpOpen/Connect/OpenRequest/SendRequest/ReceiveResponse
        // to fetch Image(url) bytes. The `windows` crate's `Win32_Networking_WinHttp`
        // feature emits #[link] attrs in the rlib, but those don't propagate
        // through perry-ui-windows's `staticlib` crate-type to perry's final
        // link line. Closes #732.
        .arg("winhttp.lib");
}

/// Embed the comctl32 v6 application manifest into a UI executable so common
/// controls render with visual styles (Fluent look) instead of the unthemed
/// Win95/classic style. Without the side-by-side
/// `Microsoft.Windows.Common-Controls` v6 dependency the process binds comctl32
/// v5 and every button/list/edit box looks decades old — issue #4681 /
/// discussion #3486. No-op unless `needs_ui` so console-only binaries stay
/// manifest-free.
///
/// `lld-link` embeds `/MANIFESTINPUT:` content via `/MANIFEST:EMBED` entirely
/// in-process, but MSVC `link.exe` shells out to the Windows SDK's `mt.exe` to
/// merge the manifest. Perry runs a vswhere-located `link.exe` from a plain
/// shell (no `vcvars64.bat` PATH), so `mt.exe` is normally unreachable and the
/// link died with `LNK1158: cannot run 'mt.exe'` on every UI build since this
/// embed landed (v0.5.1129 / #4683) — issue #6023. For MSVC we now put the SDK
/// bin dir holding `mt.exe` on the child's PATH, and if `mt.exe` can't be
/// found at all we skip the embed with a warning (an unthemed app that builds
/// beats a fatal LNK1158). `/MANIFESTUAC:NO` suppresses the linker's
/// auto-generated UAC fragment so it can't produce a second `trustInfo`
/// element alongside the one in our input manifest (which already declares
/// `asInvoker`).
pub(crate) fn embed_app_manifest(cmd: &mut Command, needs_ui: bool) {
    if !needs_ui {
        return;
    }
    if linker_is_msvc_link_exe(cmd) && !ensure_mt_exe_reachable(cmd) {
        eprintln!(
            "Warning: mt.exe (Windows SDK manifest tool) not found on PATH or under \
             Windows Kits\\10\\bin — linking without the comctl32 v6 application \
             manifest so the build can succeed (MSVC link.exe would otherwise fail \
             with LNK1158, issue #6023). Common controls will render in the \
             unthemed classic style. Fix: install the Windows 10/11 SDK via the \
             Visual Studio Installer, or compile from a vcvars64.bat developer \
             prompt."
        );
        return;
    }
    let manifest_path = std::env::temp_dir().join(format!(
        "perry_app_manifest_{}.manifest",
        std::process::id()
    ));
    match std::fs::write(&manifest_path, WINDOWS_APP_MANIFEST) {
        Ok(()) => {
            cmd.arg("/MANIFEST:EMBED")
                .arg("/MANIFESTUAC:NO")
                .arg(format!("/MANIFESTINPUT:{}", manifest_path.display()));
        }
        Err(e) => {
            eprintln!(
                "Warning: could not write Windows application manifest to {} ({e}); \
                 common controls will render in the unthemed classic style.",
                manifest_path.display()
            );
        }
    }
}

/// True when the link command runs MSVC `link.exe` (as opposed to `lld-link`
/// or a cc driver) — the only linker whose manifest embed depends on `mt.exe`.
/// Matches on the program's file stem so both bare (`link.exe`, cross-compile
/// fallback) and vswhere-resolved absolute paths classify correctly.
pub(crate) fn linker_is_msvc_link_exe(cmd: &Command) -> bool {
    Path::new(cmd.get_program())
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("link"))
}

/// Make sure `link.exe` will be able to spawn `mt.exe` (#6023): if it's
/// already on PATH (developer prompt) do nothing; otherwise locate the
/// Windows SDK bin dir that holds it and prepend that to the child's PATH.
/// Returns false only when `mt.exe` can't be found anywhere, in which case
/// the caller skips the manifest embed entirely.
#[cfg(target_os = "windows")]
fn ensure_mt_exe_reachable(cmd: &mut Command) -> bool {
    let on_path = Command::new("where")
        .arg("mt.exe")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if on_path {
        return true;
    }
    if let Some(dir) = find_windows_sdk_mt_dir() {
        let mut parts = vec![dir];
        if let Some(existing) = std::env::var_os("PATH") {
            parts.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(parts) {
            cmd.env("PATH", joined);
            return true;
        }
    }
    false
}

/// Non-Windows hosts can't probe a Windows SDK, and the cross-compile path
/// links with lld-link anyway (the bare `link.exe` fallback only fires after
/// a loud lld-link-missing warning) — keep the pre-#6023 behavior of always
/// embedding.
#[cfg(not(target_os = "windows"))]
fn ensure_mt_exe_reachable(_cmd: &mut Command) -> bool {
    true
}
