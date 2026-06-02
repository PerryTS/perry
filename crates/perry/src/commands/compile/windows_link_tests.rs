//! Windows linker subsystem regression tests — split from compile.rs
//! in v0.5.1019 (file-size CI gate). Brought back in via
//! `#[cfg(test)] mod windows_link_tests;` in compile.rs.

use super::windows_pe_subsystem_flag;

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
