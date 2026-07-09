//! Windows linker subsystem regression tests — split from compile.rs
//! in v0.5.1019 (file-size CI gate). Brought back in via
//! `#[cfg(test)] mod windows_link_tests;` in compile.rs.

use super::library_search::newest_mt_dir_under;
use super::link::{embed_app_manifest, linker_is_msvc_link_exe, WINDOWS_APP_MANIFEST};
use super::windows_default_output_extension;
use super::windows_pe_subsystem_flag;
use super::windows_subsystem_needs_ui;
use std::process::Command;

// Regression guard for issue #120: without an explicit subsystem flag the
// MSVC linker historically defaulted to WINDOWS (2), silently detaching
// stdout/stderr so console.log output never reached the terminal.

#[test]
fn cli_build_uses_console_subsystem() {
    assert_eq!(windows_pe_subsystem_flag(false, "10"), "/SUBSYSTEM:CONSOLE");
}

#[test]
fn ui_build_uses_windows_subsystem() {
    assert_eq!(windows_pe_subsystem_flag(true, "10"), "/SUBSYSTEM:WINDOWS");
}

// Issue #303: --min-windows-version=7 emits the ,5.1 suffix that marks
// the PE as Win7-compatible.
#[test]
fn min_windows_7_appends_5_1_suffix() {
    assert_eq!(
        windows_pe_subsystem_flag(false, "7"),
        "/SUBSYSTEM:CONSOLE,5.1"
    );
    assert_eq!(
        windows_pe_subsystem_flag(true, "7"),
        "/SUBSYSTEM:WINDOWS,5.1"
    );
}

// Issue #303: --min-windows-version=8 emits the ,6.02 suffix.
#[test]
fn min_windows_8_appends_6_02_suffix() {
    assert_eq!(
        windows_pe_subsystem_flag(false, "8"),
        "/SUBSYSTEM:CONSOLE,6.02"
    );
    assert_eq!(
        windows_pe_subsystem_flag(true, "8"),
        "/SUBSYSTEM:WINDOWS,6.02"
    );
}

// Anything other than 7/8/10 falls through to no suffix — caller-side
// CompileArgs validation rejects unknown values before reaching the
// linker, so this branch is unreachable in practice but documented.
#[test]
fn unknown_min_windows_falls_through_to_default() {
    assert_eq!(windows_pe_subsystem_flag(false, "11"), "/SUBSYSTEM:CONSOLE");
    assert_eq!(windows_pe_subsystem_flag(true, ""), "/SUBSYSTEM:WINDOWS");
}

// --windows-subsystem / [windows] subsystem override (resolved into
// ctx.windows_subsystem) folds into needs_ui before the flag is built.

// "auto" defers to the import-driven heuristic — both polarities pass through.
#[test]
fn subsystem_auto_defers_to_needs_ui() {
    assert!(!windows_subsystem_needs_ui("auto", false));
    assert!(windows_subsystem_needs_ui("auto", true));
}

// "windows" forces GUI even when nothing imported perry/ui — this is the
// Bloom-Engine-game case: no console window pops up alongside the game.
#[test]
fn subsystem_windows_forces_gui() {
    assert!(windows_subsystem_needs_ui("windows", false));
    let flag = windows_pe_subsystem_flag(windows_subsystem_needs_ui("windows", false), "10");
    assert_eq!(flag, "/SUBSYSTEM:WINDOWS");
}

// "console" forces a console even for a UI program that would auto-detect GUI.
#[test]
fn subsystem_console_forces_console() {
    assert!(!windows_subsystem_needs_ui("console", true));
    let flag = windows_pe_subsystem_flag(windows_subsystem_needs_ui("console", true), "10");
    assert_eq!(flag, "/SUBSYSTEM:CONSOLE");
}

// The override composes with the min-windows-version suffix.
#[test]
fn subsystem_override_composes_with_min_version_suffix() {
    let flag = windows_pe_subsystem_flag(windows_subsystem_needs_ui("windows", false), "7");
    assert_eq!(flag, "/SUBSYSTEM:WINDOWS,5.1");
}

// Issue #4771: the Windows output extension defaults by output type — .exe for
// executables, .dll for shared libraries, .lib for static libraries — so a
// user-supplied `-o NAME` without an extension still produces a runnable /
// linkable file on Windows.
#[test]
fn windows_output_extension_defaults_by_type() {
    assert_eq!(windows_default_output_extension(false, false), "exe");
    assert_eq!(windows_default_output_extension(true, false), "dll");
    assert_eq!(windows_default_output_extension(false, true), "lib");
}

// Issue #4681 / discussion #3486: the embedded app manifest must declare the
// comctl32 v6 side-by-side dependency, otherwise common controls bind v5 and
// render unthemed. Guards against accidental edits to windows_app.manifest.
#[test]
fn app_manifest_requests_comctl32_v6() {
    assert!(
        WINDOWS_APP_MANIFEST.starts_with("<?xml"),
        "manifest should be a well-formed XML document"
    );
    assert!(
        WINDOWS_APP_MANIFEST.contains("Microsoft.Windows.Common-Controls"),
        "manifest must reference the Common-Controls assembly"
    );
    assert!(
        WINDOWS_APP_MANIFEST.contains("version=\"6.0.0.0\""),
        "manifest must request Common-Controls v6 for visual styles"
    );
    assert!(
        WINDOWS_APP_MANIFEST.contains("6595b64144ccf1df"),
        "manifest must carry the Common-Controls public key token"
    );
}

// asInvoker keeps UI binaries out of the UAC installer-detection heuristic and
// avoids a second linker-generated trustInfo block (we pass /MANIFESTUAC:NO).
#[test]
fn app_manifest_runs_as_invoker() {
    assert!(
        WINDOWS_APP_MANIFEST.contains("level=\"asInvoker\""),
        "manifest must declare the asInvoker execution level"
    );
}

// Issue #6023: only MSVC link.exe needs the mt.exe reachability treatment —
// lld-link embeds manifests in-process. The classifier keys on the program's
// file stem so bare names and vswhere-resolved absolute paths both match.
#[test]
fn msvc_link_exe_classification() {
    assert!(linker_is_msvc_link_exe(&Command::new("link.exe")));
    assert!(linker_is_msvc_link_exe(&Command::new("LINK.EXE")));
    assert!(linker_is_msvc_link_exe(&Command::new("link")));
    // Forward-slash path form keeps the test host-independent; on Windows the
    // vswhere backslash form resolves through the same Path::file_stem.
    assert!(linker_is_msvc_link_exe(&Command::new(
        "C:/VS/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/link.exe"
    )));
    assert!(!linker_is_msvc_link_exe(&Command::new("lld-link.exe")));
    assert!(!linker_is_msvc_link_exe(&Command::new("lld-link")));
    assert!(!linker_is_msvc_link_exe(&Command::new("cc")));
}

// Console-only binaries must stay manifest-free (and thus never depend on
// mt.exe at all).
#[test]
fn console_build_adds_no_manifest_args() {
    let mut cmd = Command::new("link.exe");
    embed_app_manifest(&mut cmd, false);
    assert_eq!(cmd.get_args().count(), 0);
}

// lld-link needs no mt.exe, so a UI build through it always gets the full
// /MANIFEST:EMBED argument set regardless of any Windows SDK presence.
#[test]
fn ui_build_embeds_manifest_via_lld_link() {
    let mut cmd = Command::new("lld-link.exe");
    embed_app_manifest(&mut cmd, true);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(args.contains(&"/MANIFEST:EMBED".to_string()));
    assert!(args.contains(&"/MANIFESTUAC:NO".to_string()));
    assert!(
        args.iter().any(|a| a.starts_with("/MANIFESTINPUT:")),
        "manifest input file must be passed: {args:?}"
    );
}

// Issue #6023: the SDK-bin probe picks the newest versioned dir that actually
// contains mt.exe, preferring x64 over x86.
#[test]
fn mt_dir_probe_picks_newest_versioned_sdk() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path();
    // Older SDK with both arches, newest mt-bearing SDK also with both
    // arches (x64 must win within it), plus a newer version dir that lacks
    // mt.exe entirely (must be skipped).
    for dir in [
        "10.0.19041.0/x64",
        "10.0.19041.0/x86",
        "10.0.22621.0/x64",
        "10.0.22621.0/x86",
        "10.0.26100.0/arm64",
    ] {
        std::fs::create_dir_all(bin.join(dir)).unwrap();
    }
    for mt in [
        "10.0.19041.0/x64/mt.exe",
        "10.0.19041.0/x86/mt.exe",
        "10.0.22621.0/x64/mt.exe",
        "10.0.22621.0/x86/mt.exe",
    ] {
        std::fs::write(bin.join(mt), b"").unwrap();
    }
    assert_eq!(
        newest_mt_dir_under(bin),
        Some(bin.join("10.0.22621.0").join("x64"))
    );
}

// Pre-10.0.15063 SDKs put tools directly under bin\<arch> with no version dir.
#[test]
fn mt_dir_probe_falls_back_to_unversioned_layout() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path();
    std::fs::create_dir_all(bin.join("x86")).unwrap();
    std::fs::write(bin.join("x86").join("mt.exe"), b"").unwrap();
    assert_eq!(newest_mt_dir_under(bin), Some(bin.join("x86")));
}

// No SDK bin root / no mt.exe anywhere → None, which makes embed_app_manifest
// skip the embed instead of letting link.exe die with LNK1158.
#[test]
fn mt_dir_probe_handles_missing_root_and_empty_tree() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(newest_mt_dir_under(&root.path().join("nope")), None);
    std::fs::create_dir_all(root.path().join("10.0.22621.0").join("x64")).unwrap();
    assert_eq!(newest_mt_dir_under(root.path()), None);
}
