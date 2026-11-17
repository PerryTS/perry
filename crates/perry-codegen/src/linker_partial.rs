//! Which tool merges one module's codegen units with a relocatable link
//! (`ld -r`) on Unix hosts. Split out of `linker.rs` for the 2,000-line cap.
#[cfg(not(target_os = "windows"))]
use std::env;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
fn is_elf_object(obj: &[u8]) -> bool {
    obj.starts_with(b"\x7fELF")
}

#[cfg(not(target_os = "windows"))]
fn ndk_home_from_env() -> Option<PathBuf> {
    ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK"]
        .iter()
        .filter_map(|k| env::var_os(k))
        .find(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The relocatable linker for one module's codegen units, and whether it is
/// Apple's ld64 (which needs `-keep_private_externs`).
///
/// Chosen by the objects' format, not only by the host: a macOS host
/// cross-compiling for Android or Linux emits ELF units, and the system `ld`
/// there is ld64, which cannot read them (`ld: Missing -arch option`). ELF
/// units on a macOS host go through `ld.lld`: the NDK's when an NDK is
/// configured, otherwise the first on PATH. `PERRY_LD` still overrides the
/// tool; ld64's flag is only added for non-ELF units.
#[cfg(not(target_os = "windows"))]
fn select_unix_partial_linker(
    first_obj: &[u8],
    host_macos: bool,
    perry_ld: Option<String>,
    ndk_home: Option<PathBuf>,
) -> (String, bool) {
    let elf = is_elf_object(first_obj);
    let ld64 = host_macos && !elf;
    if let Some(tool) = perry_ld.filter(|t| !t.is_empty()) {
        return (tool, ld64);
    }
    if host_macos && elf {
        if let Some(ndk) = ndk_home {
            let lld = ndk.join("toolchains/llvm/prebuilt/darwin-x86_64/bin/ld.lld");
            if lld.is_file() {
                return (lld.to_string_lossy().into_owned(), false);
            }
        }
        return ("ld.lld".to_string(), false);
    }
    ("ld".to_string(), ld64)
}

/// [`select_unix_partial_linker`] for these units with this process's
/// environment, plus a hint for the spawn-failure message.
#[cfg(not(target_os = "windows"))]
pub(super) fn unix_partial_linker(objs: &[Vec<u8>]) -> (String, bool, &'static str) {
    let first = objs.first().map(Vec::as_slice).unwrap_or_default();
    let (tool, ld64) = select_unix_partial_linker(
        first,
        cfg!(target_os = "macos"),
        env::var("PERRY_LD").ok(),
        ndk_home_from_env(),
    );
    let hint = if is_elf_object(first) {
        " (ELF codegen units: install lld, set ANDROID_NDK_HOME, or point PERRY_LD at an ld.lld)"
    } else {
        ""
    };
    (tool, ld64, hint)
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    const ELF: &[u8] = b"\x7fELF\x02\x01\x01";
    const MACHO: &[u8] = &[0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01];

    fn pick(
        obj: &[u8],
        macos: bool,
        perry_ld: Option<&str>,
        ndk: Option<PathBuf>,
    ) -> (String, bool) {
        select_unix_partial_linker(obj, macos, perry_ld.map(str::to_string), ndk)
    }

    #[test]
    fn mach_o_units_on_macos_keep_ld64_and_its_flag() {
        assert_eq!(pick(MACHO, true, None, None), ("ld".to_string(), true));
    }

    #[test]
    fn elf_units_on_linux_keep_the_system_ld() {
        assert_eq!(pick(ELF, false, None, None), ("ld".to_string(), false));
    }

    #[test]
    fn elf_units_on_macos_never_reach_ld64() {
        // Android / Linux cross builds from a Mac: ld64 fails with
        // `ld: Missing -arch option`, and lld rejects -keep_private_externs.
        assert_eq!(pick(ELF, true, None, None), ("ld.lld".to_string(), false));
    }

    #[test]
    fn elf_units_on_macos_prefer_the_ndk_lld() {
        let ndk = env::temp_dir().join(format!("perry_ndk_probe_{}", std::process::id()));
        let bin = ndk.join("toolchains/llvm/prebuilt/darwin-x86_64/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("ld.lld"), b"").unwrap();
        let (tool, ld64) = pick(ELF, true, None, Some(ndk.clone()));
        let _ = std::fs::remove_dir_all(&ndk);
        assert!(tool.ends_with("darwin-x86_64/bin/ld.lld"), "{tool}");
        assert!(!ld64);
    }

    #[test]
    fn a_missing_ndk_falls_back_to_lld_on_path() {
        let ndk = env::temp_dir().join("perry_ndk_that_does_not_exist");
        assert_eq!(
            pick(ELF, true, None, Some(ndk)),
            ("ld.lld".to_string(), false)
        );
    }

    #[test]
    fn perry_ld_overrides_the_tool_but_not_the_format_rule() {
        assert_eq!(
            pick(ELF, true, Some("/x/ld.lld"), None),
            ("/x/ld.lld".to_string(), false)
        );
        assert_eq!(
            pick(MACHO, true, Some("/x/ld"), None),
            ("/x/ld".to_string(), true)
        );
        assert_eq!(pick(MACHO, true, Some(""), None), ("ld".to_string(), true));
    }
}
