//! The linker of a native glibc Linux executable: lld when it is installed,
//! GNU ld (the `cc` default) otherwise.
//!
//! lld lays the same inputs out smaller (chess: about 100 KB, 1.1%, mostly
//! `.rodata` merging and section padding) with the same code: no input
//! section is changed, dropped or folded. Identical code folding (`--icf`) is
//! deliberately NOT used:
//! * `--icf=all` folds generated JS bodies with equal code, and lld drops the
//!   folded body's `perry_fnrec` record, so a stack frame of one names the
//!   other (`error.stack`). Runtime code compares function addresses as
//!   identity (`info.code == <thunk>`), and value-stored thunks fold too.
//! * `--icf=safe` folds nothing: neither rustc nor perry's codegen emits the
//!   address-significance table it needs.
//! * `--keep-unique` resolves global symbols only; most runtime functions
//!   are local after LTO.
//!
//! Kept as GNU ld had it: the fixed base 0x400000 (lld's own default is
//! 0x200000; the base must stay above the NaN-box handle band) and the
//! separate code segment.
//!
//! `PERRY_LINK_LLD=off` (or `0`) keeps GNU ld. `PERRY_LINK_LLD=icf-all` adds
//! `--icf=all` for measurement only (unsound, see above).
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// What the switch asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Gnu,
    Lld,
    LldIcfAll,
}

pub(super) fn mode_from(value: Option<&str>) -> Mode {
    match value.map(str::trim) {
        Some("off" | "0" | "false" | "no" | "gnu") => Mode::Gnu,
        Some("icf-all") => Mode::LldIcfAll,
        _ => Mode::Lld,
    }
}

fn mode() -> Mode {
    mode_from(std::env::var("PERRY_LINK_LLD").ok().as_deref())
}

/// Oldest lld that accepts every flag of this link (`-z start-stop-gc`).
const MIN_LLD_MAJOR: u32 = 13;

/// The major version out of `ld.lld --version` ("Ubuntu LLD 22.1.8 (...)",
/// "LLD 18.1.3 (compatible with GNU linkers)").
pub(super) fn lld_major(version: &str) -> Option<u32> {
    let rest = &version[version.find("LLD ")? + 4..];
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// `ld.lld` as `cc -fuse-ld=lld` finds it: the first one on PATH.
fn find_on_path(path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .map(|dir| dir.join("ld.lld"))
        .find(|candidate| candidate.is_file())
}

fn usable(ld_lld: &Path) -> bool {
    Command::new(ld_lld)
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| lld_major(&String::from_utf8_lossy(&out.stdout)))
        .is_some_and(|major| major >= MIN_LLD_MAJOR)
}

/// lld is on PATH and new enough (probed once per process).
fn lld_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        find_on_path(std::env::var_os("PATH").as_deref()).is_some_and(|p| usable(&p))
    })
}

/// Whether this link uses lld. `eligible`: a native glibc Linux executable
/// built on Linux with the default function layout (a recorded
/// `--function-order` keeps its GNU ld script).
pub(super) fn use_lld(eligible: bool) -> Option<Mode> {
    if !eligible {
        return None;
    }
    match mode() {
        Mode::Gnu => None,
        mode => lld_available().then_some(mode),
    }
}

pub(super) fn add_link_args(cmd: &mut Command, mode: Mode) {
    cmd.arg("-fuse-ld=lld");
    cmd.arg("-Wl,--image-base=0x400000");
    cmd.arg("-Wl,-z,separate-code");
    if mode == Mode::LldIcfAll {
        cmd.arg("-Wl,--icf=all");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_reads_off_lld_and_icf_all() {
        assert_eq!(mode_from(None), Mode::Lld);
        assert_eq!(mode_from(Some("1")), Mode::Lld);
        for off in ["off", "0", "false", "no", "gnu", " off "] {
            assert_eq!(mode_from(Some(off)), Mode::Gnu, "{off}");
        }
        assert_eq!(mode_from(Some("icf-all")), Mode::LldIcfAll);
    }

    #[test]
    fn the_version_line_gives_the_major() {
        assert_eq!(
            lld_major("Ubuntu LLD 22.1.8 (compatible with GNU linkers)"),
            Some(22)
        );
        assert_eq!(
            lld_major("LLD 12.0.1 (compatible with GNU linkers)"),
            Some(12)
        );
        assert_eq!(lld_major("GNU ld (GNU Binutils) 2.42"), None);
    }

    #[test]
    fn lld_is_found_on_path_only() {
        let dir = std::env::temp_dir().join(format!("perry-lld-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_on_path(Some(dir.as_os_str())), None);
        std::fs::write(dir.join("ld.lld"), b"").unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent-perry"), &dir]).unwrap();
        assert_eq!(find_on_path(Some(&path)), Some(dir.join("ld.lld")));
        // Not executable as a linker: not usable.
        assert!(!usable(&dir.join("ld.lld")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
