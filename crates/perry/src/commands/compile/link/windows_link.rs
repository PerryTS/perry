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

use std::process::Command;

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
/// Both `link.exe` and `lld-link` embed `/MANIFESTINPUT:` content via
/// `/MANIFEST:EMBED` with no external `mt.exe`/`rc.exe`. `/MANIFESTUAC:NO`
/// suppresses the linker's auto-generated UAC fragment so it can't produce a
/// second `trustInfo` element alongside the one in our input manifest (which
/// already declares `asInvoker`).
pub(super) fn embed_app_manifest(cmd: &mut Command, needs_ui: bool) {
    if !needs_ui {
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
