//! A cross link takes only archives built for its target, and needs the one
//! its program's stdlib calls resolve against.

use super::{check_missing_stdlib, locate_among, searched_report};
use std::fs;
use std::path::{Path, PathBuf};

const LIB: &str = "libperry_stdlib.a";

fn elf(machine: u16) -> Vec<u8> {
    let mut object = vec![0u8; 64];
    object[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    object[16..18].copy_from_slice(&1u16.to_le_bytes()); // ET_REL
    object[18..20].copy_from_slice(&machine.to_le_bytes());
    object[20..24].copy_from_slice(&1u32.to_le_bytes());
    object[52..54].copy_from_slice(&64u16.to_le_bytes());
    object
}

fn macho(cpu: u32) -> Vec<u8> {
    [0xfeed_facf, cpu, 0, 1, 0, 0, 0, 0]
        .iter()
        .flat_map(|word: &u32| word.to_le_bytes())
        .collect()
}

fn coff(machine: u16) -> Vec<u8> {
    let mut object = vec![0u8; 64];
    object[..2].copy_from_slice(&machine.to_le_bytes());
    object
}

const ELF_ARM64: u16 = 183;
const ELF_X64: u16 = 62;
const MACHO_ARM64: u32 = 0x0100_000c;
const MACHO_X64: u32 = 0x0100_0007;
const COFF_X64: u16 = 0x8664;

/// An archive whose first member is LLVM bitcode, as in a thin-LTO build.
fn archive(dir: &Path, object: &[u8]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let mut bytes = b"!<arch>\n".to_vec();
    for (name, data) in [("bitcode.o/", &b"BC\xc0\xde"[..]), ("native.o/", object)] {
        bytes.extend(format!("{name:<16}{:<32}{:<10}`\n", 0, data.len()).as_bytes());
        bytes.extend_from_slice(data);
        assert_eq!(data.len() % 2, 0);
    }
    let path = dir.join(LIB);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_cross_target_skips_the_host_archive_for_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let host = archive(&dir.path().join("host"), &macho(MACHO_ARM64));
    let own = archive(&dir.path().join("own"), &elf(ELF_ARM64));

    let found = locate_among(vec![host.clone(), own.clone()], LIB, Some("android")).unwrap();
    assert_eq!(found.link, own);

    let searched = locate_among(vec![host.clone()], LIB, Some("android")).unwrap_err();
    assert_eq!(
        searched_report(&searched),
        format!(
            "  - {} (built for another target or unreadable)",
            host.display()
        )
    );
}

#[test]
fn a_compressed_host_archive_is_rejected_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let raw = archive(dir.path(), &macho(MACHO_ARM64));
    let compressed = zstd::stream::encode_all(&fs::read(&raw).unwrap()[..], 1).unwrap();
    fs::write(raw.with_extension("a.zst"), compressed).unwrap();
    fs::remove_file(&raw).unwrap();

    // Run with RUST_TEST_THREADS=1 like the rest of the env-touching tests.
    std::env::set_var("PERRY_LIB_CACHE_DIR", dir.path().join("cache"));
    let searched = locate_among(vec![raw.clone()], LIB, Some("android"));
    std::env::remove_var("PERRY_LIB_CACHE_DIR");
    assert_eq!(
        searched_report(&searched.unwrap_err()),
        format!(
            "  - {} (built for another target or unreadable)",
            raw.display()
        )
    );
}

#[test]
fn an_archive_is_matched_by_format_and_architecture() {
    let dir = tempfile::tempdir().unwrap();
    for (object, target, accepted) in [
        (elf(ELF_ARM64), "android", true),
        (elf(ELF_ARM64), "android-x86_64", false),
        (elf(ELF_X64), "android-x86_64", true),
        (elf(ELF_ARM64), "ios", false),
        (macho(MACHO_ARM64), "ios", true),
        (macho(MACHO_ARM64), "linux-arm64", false),
        (macho(MACHO_X64), "macos", false),
        (coff(COFF_X64), "windows", true),
        (coff(COFF_X64), "windows-aarch64", false),
        (coff(COFF_X64), "android", false),
        (b"not an object!".to_vec(), "android", true),
    ] {
        let path = archive(dir.path(), &object);
        let found = locate_among(vec![path], LIB, Some(target)).is_ok();
        assert_eq!(found, accepted, "{target}: {:x?}", &object[..8]);
    }
}

#[test]
fn a_host_link_takes_any_archive() {
    let dir = tempfile::tempdir().unwrap();
    let path = archive(dir.path(), &elf(ELF_ARM64));
    assert_eq!(
        locate_among(vec![path.clone()], LIB, None).unwrap().link,
        path
    );
}

#[test]
fn a_missing_stdlib_fails_only_a_cross_link() {
    check_missing_stdlib(None).unwrap();
    // A triple no host perry is published for.
    let error = check_missing_stdlib(Some("android"))
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with(
            "this program links `libperry_stdlib.a`, which was not found for target `android` (aarch64-linux-android)."
        ),
        "{error}"
    );
}
