//! lld as the linker of a native glibc Linux executable: the default on
//! x86_64 (`PERRY_LINK_LLD=0` keeps GNU ld), opt-in elsewhere
//! (`PERRY_LINK_LLD=1`).
//!
//! lld lays the same inputs out smaller (chess -1.0%: `.rodata` string
//! merging, section padding) with the same code: no input section is
//! changed, dropped or folded, and start-up runs fewer ld.so instructions
//! (hello2 -2.4%).
//!
//! Lazy PLT slots: Rust code takes libc functions' addresses from the GOT
//! (bound at start). A C caller of the same function through the PLT gets,
//! under lld, a second JUMP_SLOT that ld.so binds lazily, about 400
//! instructions on its first call (GNU ld routes such a call through
//! `.plt.got`, the bound GOT entry). mimalloc made 16 such calls (mmap,
//! memcpy, clock_gettime, ...: chess +6K per run). On x86_64 glibc the
//! vendored libmimalloc-sys (`third_party/libmimalloc-sys`, VENDORED.md)
//! sends exactly those calls through the GOT and fails its build if one
//! stays a PLT call; with it lld runs no more instructions than GNU ld, so
//! lld is the default there.
//! A global `-fno-plt` would do the same but rewrites every call of the C
//! parts and cost 256 KiB of peak RSS. aarch64 was not measured: opt-in.
//!
//! Identical code folding (`--icf`) is never used by default:
//! * `--icf=all` folds generated JS bodies with equal code, and lld drops the
//!   folded body's `perry_fnrec` record, so a stack frame of one names the
//!   other (`error.stack`). Runtime code compares function addresses as
//!   identity (`info.code == <thunk>`), and value-stored thunks fold too.
//! * `--icf=safe` folds nothing: neither rustc nor perry's codegen emits the
//!   address-significance table it needs.
//! * `--keep-unique` resolves global symbols only; most runtime functions
//!   are local after LTO.
//!
//! Kept as GNU ld has it: the fixed base 0x400000 (lld's own default is
//! 0x200000; the base must stay above the NaN-box handle band) and the
//! separate code segment. The default hot-text order works under lld with
//! its run in its own output section (`hot_text_order`).
//!
//! `PERRY_LINK_LLD`: `0` (or `off`, `false`, `no`, `gnu`) links with GNU ld;
//! `1` (or `on`, `true`, `yes`, `lld`) with lld; unset or anything else, the
//! platform default. lld is used only when `ld.lld` 13 or newer is on PATH,
//! GNU ld otherwise. `icf-all` adds `--icf=all` for measurement only
//! (unsound, see above).
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

/// The linker without a switch: lld where the runtime archives make it
/// cost no instructions (x86_64, `perry_noplt_libc.h`), GNU ld elsewhere.
const DEFAULT_MODE: Mode = if cfg!(target_arch = "x86_64") {
    Mode::Lld
} else {
    Mode::Gnu
};

pub(super) fn mode_from(value: Option<&str>, default: Mode) -> Mode {
    match value.map(str::trim) {
        Some("0" | "off" | "false" | "no" | "gnu") => Mode::Gnu,
        Some("1" | "on" | "true" | "yes" | "lld") => Mode::Lld,
        Some("icf-all") => Mode::LldIcfAll,
        _ => default,
    }
}

fn mode() -> Mode {
    mode_from(
        std::env::var("PERRY_LINK_LLD").ok().as_deref(),
        DEFAULT_MODE,
    )
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

/// Whether the link line `args` makes the `cc` driver run lld: its last
/// `-fuse-ld=` names lld. Besides [`add_link_args`], the cross links to
/// Linux pass `-fuse-ld=lld` themselves (`platform_cmd`: x86_64 from macOS,
/// the clang fallbacks of aarch64 and musl), and `PERRY_EXTRA_LINK_ARGS` can.
/// Those links need the lld flavor of the hot-text script too: lld rejects
/// the GNU one ("unable to insert .text before .text").
pub(super) fn driver_runs_lld<'a>(args: impl IntoIterator<Item = &'a std::ffi::OsStr>) -> bool {
    args.into_iter()
        .filter_map(|arg| arg.to_str()?.strip_prefix("-fuse-ld="))
        .last()
        .is_some_and(|linker| matches!(linker, "lld" | "ld.lld") || linker.ends_with("/ld.lld"))
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
    fn the_switch_picks_either_linker_and_unset_is_the_default() {
        for default in [Mode::Gnu, Mode::Lld] {
            assert_eq!(mode_from(None, default), default);
            assert_eq!(mode_from(Some(""), default), default);
            for on in ["1", "on", "true", "yes", "lld", " 1 "] {
                assert_eq!(mode_from(Some(on), default), Mode::Lld, "{on}");
            }
            for off in ["off", "0", "false", "no", "gnu", " 0 "] {
                assert_eq!(mode_from(Some(off), default), Mode::Gnu, "{off}");
            }
            assert_eq!(mode_from(Some("icf-all"), default), Mode::LldIcfAll);
        }
        // lld by default on x86_64 only (the measured, shimmed archives).
        assert_eq!(DEFAULT_MODE == Mode::Lld, cfg!(target_arch = "x86_64"));
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
    fn a_link_line_runs_lld_when_its_last_fuse_ld_names_lld() {
        let runs = |args: &[&str]| driver_runs_lld(args.iter().map(std::ffi::OsStr::new));
        // The native lld link (`add_link_args`).
        let mut native = Command::new("cc");
        add_link_args(&mut native, Mode::Lld);
        assert!(driver_runs_lld(native.get_args()));
        // The cross links' own lines (`platform_cmd`).
        assert!(runs(&[
            "-target",
            "x86_64-unknown-linux-gnu",
            "-fuse-ld=lld"
        ]));
        assert!(runs(&[
            "-target",
            "aarch64-unknown-linux-gnu",
            "-fuse-ld=lld"
        ]));
        assert!(runs(&[
            "-target",
            "x86_64-unknown-linux-musl",
            "-fuse-ld=lld",
            "-static"
        ]));
        assert!(runs(&["-fuse-ld=/usr/lib/llvm-22/bin/ld.lld"]));
        // GNU ld: no switch, or a later one that names another linker.
        assert!(!runs(&[]));
        assert!(!runs(&["-Wl,--gc-sections", "-no-pie"]));
        assert!(!runs(&["-fuse-ld=lld", "-fuse-ld=bfd"]));
        assert!(!runs(&["-fuse-ld=gold"]));
        assert!(runs(&["-fuse-ld=bfd", "-fuse-ld=lld"]));
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
