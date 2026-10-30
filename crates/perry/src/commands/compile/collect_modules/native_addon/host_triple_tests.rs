use super::binary_tests::macho_import;
use super::{collect_or_refuse_node_addon, CompilationContext};
use crate::commands::compile::host_target_triple;
use std::fs;
use std::path::Path;

fn elf_header(machine: u16) -> Vec<u8> {
    let mut bytes = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0];
    bytes.resize(16, 0);
    bytes.extend(3u16.to_le_bytes()); // ET_DYN
    bytes.extend(machine.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend([0u8; 24]); // entry, phoff, shoff
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(64u16.to_le_bytes());
    bytes.extend(56u16.to_le_bytes());
    bytes.extend([0u8; 2]); // phnum
    bytes.extend(64u16.to_le_bytes());
    bytes.extend([0u8; 4]); // shnum, shstrndx
    bytes
}

fn macho(arm: bool) -> Vec<u8> {
    let mut bytes = macho_import("_napi_create_int32", 0xfe, true, 1);
    if arm {
        bytes[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
    }
    bytes
}

fn addon_for_host() -> Vec<u8> {
    let triple = host_target_triple().expect("host triple");
    let arm = triple.starts_with("aarch64-");
    if triple.contains("-apple-") {
        macho(arm)
    } else {
        elf_header(if arm { 0xb7 } else { 0x3e })
    }
}

fn addon_for_other_target() -> Vec<u8> {
    let triple = host_target_triple().expect("host triple");
    if triple.contains("-apple-") {
        macho(!triple.starts_with("aarch64-"))
    } else {
        macho(false)
    }
}

fn collect(bytes: &[u8]) -> anyhow::Result<bool> {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let package = root.join("node_modules/demo");
    let addon = package.join("prebuilds/addon.node");
    fs::create_dir_all(addon.parent().unwrap()).unwrap();
    fs::write(&addon, bytes).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"name":"demo","version":"1"}"#,
    )
    .unwrap();
    let mut ctx = CompilationContext::new(root);
    collect_or_refuse_node_addon(&mut ctx, Path::new(&addon))
}

#[test]
fn require_of_addon_for_another_target_is_skipped_without_approval() {
    assert!(collect(&addon_for_other_target()).unwrap());
}

#[test]
fn require_of_host_addon_is_still_authorized_like_any_native_addon() {
    let err = collect(&addon_for_host()).unwrap_err().to_string();
    assert!(err.contains("perry.nativeAddons"), "{err}");
}
